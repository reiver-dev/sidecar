use log::{debug, error, warn};
use std::io::{Error as IoError, ErrorKind, Result};
use std::os::unix::io::AsRawFd;
use std::path::Path;

use nix::sys::signal::{raise, Signal};

use futures::future::{select, Either};

use crate::messages as msg;
use crate::raw::blocking::connect;
use crate::runtime;
use crate::signals;
use crate::socket::Socket;
use crate::system;

pub(crate) struct Args<'a> {
    pub connect: &'a Path,
    pub program: &'a str,
    pub args: &'a [&'a str],
    pub env: &'a [(&'a str, &'a str)],
    pub cwd: &'a str,
    pub uid: i32,
    pub gid: i32,
    pub deathsig: i32,
    pub setpgid: Option<i32>,
    pub setsid: bool,
    pub notty: bool,
}

fn handle_stop(mut sigval: i32) {
    use Signal::{SIGSTOP, SIGTSTP};

    sigval = sigval.abs();

    if let Ok(sig) = Signal::from_c_int(sigval) {
        debug!("received signal value={}", sig);
        if sig == SIGTSTP || sig == SIGSTOP {
            debug!("raising SIGSTOP");
            if let Err(err) = raise(SIGSTOP) {
                error!("signal raise error: {:?}", err);
            }
        }
    }
}

fn convert_to_group_signals(signal: Signal) -> i32 {
    use Signal::*;

    match signal {
        SIGTSTP | SIGSTOP | SIGCONT | SIGTTIN | SIGTTOU => -(signal as i32),
        _ => (signal as i32),
    }
}

async fn wait_child(
    socket: &Socket,
    signals: &signals::SignalHandler,
    mut buffer: &mut Vec<u8>,
) -> Result<i32> {
    let mut sendbuf = Vec::new();
    let mut srv = socket.recv(&mut buffer);
    let mut sig = signals.wait();

    let child_finished = |result: Result<usize>, buffer: &[u8]| {
        use msg::ProcessResult::*;
        match result {
            Ok(0) => {
                warn!("server disconnected");
                Ok(128)
            }
            Ok(bytes) => {
                let status: msg::ProcessResult;
                status = msg::decode_request(&buffer[..bytes])?;
                match status {
                    Undefined => {
                        warn!("exit reason undefined");
                        Ok(127)
                    }
                    Exit(code) => Ok(code),
                    Signal(sig) => Ok(128 + sig),
                }
            }
            Err(err) => Err(err),
        }
    };

    let exitstatus = loop {
        let selected = select(srv, sig).await;
        let (nsrv, nsig) = match selected {
            Either::Left((read, _sig1)) => {
                break child_finished(read, buffer)?;
            }
            Either::Right((sigval, srv1)) => match sigval {
                Ok(val) => {
                    let v = convert_to_group_signals(val);

                    let m = msg::Signal(v);

                    sendbuf.clear();
                    msg::encode_request(&mut sendbuf, &m)?;
                    let sel = select(srv1, socket.send(&sendbuf)).await;

                    match sel {
                        Either::Left((read, _sigsend)) => {
                            break child_finished(read, buffer)?;
                        }
                        Either::Right((delivered, srv1)) => match delivered {
                            Ok(_) => {
                                debug!("signal value sent");
                                handle_stop(v);
                                (srv1, signals.wait())
                            }
                            Err(err) => {
                                warn!("sender error");
                                return Err(err);
                            }
                        },
                    }
                }
                Err(err) => {
                    panic!(format!("signal handler error {:?}", err));
                }
            },
        };

        srv = nsrv;
        sig = nsig;
    };

    Ok(exitstatus)
}

fn prepare_request<'a>(args: &Args<'a>) -> msg::ExecRequestInput<'a> {
    let mut startup = msg::StartMode::empty();
    let pgid = match args.setpgid {
        Some(id) => {
            startup |= msg::StartMode::PROCESS_GROUP;
            id
        }
        None => 0,
    };

    if args.setsid {
        startup |= msg::StartMode::SESSION;
    }

    if args.notty {
        startup |= msg::StartMode::DETACH_TERMINAL;
    }

    let files = msg::Files::IN | msg::Files::OUT | msg::Files::ERR;

    msg::ExecRequestInput {
        program: args.program,
        argv: args.args,
        cwd: args.cwd,
        env: args.env,
        startup,
        io: files,
        pgid,
        uid: args.uid,
        gid: args.gid,
        deathsig: args.deathsig,
        connsig: system::SIGKILL as i32,
    }
}

async fn execute(
    request: &msg::ExecRequestInput<'_>,
    socket: Socket,
) -> Result<i32> {
    let mut buffer = Vec::new();
    msg::encode_request(&mut buffer, &request)?;

    {
        let mut header = Vec::new();
        msg::encode_request(
            &mut header,
            &msg::RequestInput::Exec(msg::ExecHeader {
                body_size: buffer.len(),
            }),
        )?;

        let _sent = socket.send(&header).await?;
    }

    {
        let streams = [
            std::io::stdin().as_raw_fd(),
            std::io::stdout().as_raw_fd(),
            std::io::stderr().as_raw_fd(),
        ];
        let _sent = socket.sendfds(&buffer, &streams).await?;
    }

    buffer.clear();
    buffer.resize(4096, 0);

    let received = socket.recv(&mut buffer).await?;
    debug!("response received {:?} bytes", received);

    if received > 0 {
        let ret: msg::StartedProcess =
            { msg::decode_request_ref(&buffer[..received])? };
        debug!("received {:#?}", ret);
        if ret.errno != 0 {
            Err(IoError::from_raw_os_error(ret.errno))
        } else {
            let sigsink = signals::SignalHandler::new()?;
            wait_child(&socket, &sigsink, &mut buffer).await
        }
    } else {
        warn!("server disconnected");
        Err(ErrorKind::ConnectionAborted.into())
    }
}

pub(crate) fn command(args: &Args) -> Result<i32> {
    let request = prepare_request(args);
    debug!("connecting to {:?}", args.connect);
    match connect(args.connect) {
        Ok(fd) => runtime::new()?.block_on(async {
            let ret = execute(&request, Socket::from_fd(fd)?).await?;
            debug!("finished with code {:?}", ret);
            Ok(ret)
        }),
        Err(err) => {
            error!(
                "failed to connect\n    \
                 socket: {}\n    \
                 error:  {}",
                args.connect.to_string_lossy(),
                err,
            );
            Ok(128)
        }
    }
}
