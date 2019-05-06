use std::error::Error;
use std::io::Error as IoError;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use std::os::unix::io::FromRawFd;

use log::{debug, error, info, trace, warn};
use scopeguard::defer;

use futures::future::FutureResult;
use futures::future::{self, Either};
use futures::prelude::*;

use tokio::runtime::current_thread::Runtime;
use tokio_process::{Child, CommandExt};

use crate::messages as msg;
use crate::net::{RawFd, UnixPacket, UnixPacketFramed, UnixPacketListener};

pub(crate) struct Args<'a> {
    pub server: &'a Path,
}

fn kill(child: u32, sigval: i32) {
    use nix::sys::signal;
    use nix::unistd::Pid;

    let pid = Pid::from_raw(child as i32);
    let ss = match signal::Signal::from_c_int(sigval) {
        Ok(sig) => sig,
        Err(_) => {
            warn!("invalid signal value {:?}", sigval);
            return;
        }
    };

    if let Err(err) = signal::kill(pid, ss) {
        error!("failed to send signal to process={:?} err={:?}", pid, err)
    }
}

fn handle_process<R, W>(
    signals: R,
    retcode: W,
    child: Child,
) -> impl Future<Item = (), Error = IoError>
where
    R: Stream<Item = msg::Signal, Error = IoError>,
    W: Sink<SinkItem = msg::RetCode, SinkError = IoError>,
{
    let pid = child.id();

    let ret = child
        .map(move |exit| msg::RetCode(exit.code().unwrap_or(0)))
        .and_then(|rc| {
            let rcval = rc.0;
            retcode.send(rc).map(move |_| rcval)
        });

    let sigs = signals.map(|sig| sig.0).fold((), move |_acc, sig| {
        if sig > 0 {
            debug!("process={:?} received signal={:?}", pid, sig);
            kill(pid, sig);
        }
        let res: FutureResult<(), IoError> = future::ok(());
        res
    });

    sigs.select2(ret)
        .map(move |res| match res {
            Either::A((_sig, _ret)) => {
                warn!("process={:?} client disconnected", pid);
            }
            Either::B((ret, _sig)) => {
                info!("process={:?} exited code={:?}", pid, ret);
            }
        })
        .map_err(|err| -> IoError {
            match err {
                Either::A((err, _)) => err,
                Either::B((err, _)) => err,
            }
        })
}

fn setup_command_streams(command: &mut Command, req: &msg::Io, fds: &[RawFd]) {
    let mut i = 0;
    if req.stdin >= 0 {
        command.stdin(unsafe { Stdio::from_raw_fd(fds[i]) });
        i += 1;
    }

    if req.stdout >= 0 {
        command.stdout(unsafe { Stdio::from_raw_fd(fds[i]) });
        i += 1;
    }

    if req.stderr >= 0 {
        command.stderr(unsafe { Stdio::from_raw_fd(fds[i]) });
    }
}

fn setup_command(req: msg::Request, fds: &[RawFd]) -> Command {
    let mut cmd = Command::new(&req.argv[0]);

    if req.argv.len() > 1 {
        cmd.args(&req.argv[1..]);
    }

    if let Some(ios) = &req.io {
        setup_command_streams(&mut cmd, ios, &fds);
    }

    if let Some(envs) = req.env {
        cmd.envs(envs);
    }

    if let Some(cwd) = req.cwd {
        let pb: PathBuf = cwd.into();
        cmd.current_dir(pb);
    }

    cmd
}

fn handle_client(sock: UnixPacket) -> impl Future<Item = (), Error = ()> {
    info!("client connected");

    let mut buffer: Vec<u8> = Vec::with_capacity(4096);
    buffer.resize_with(4096, Default::default);

    sock.recvmsg(buffer)
        .and_then(|(sock, mut buf, fds, received, numfds)| {
            let req: msg::Request = msg::decode_request(&buf[..received]);
            let fds: &[RawFd] = &fds[..numfds];

            info!("process spawing with {:#?} FD({:?})", req, fds);
            let maybe_child = setup_command(req, fds).spawn_async();

            buf.clear();

            let response = match &maybe_child {
                Ok(child) => {
                    info!("process={:?} started", child.id());
                    msg::StartedProcess {
                        success: true,
                        message: Some("success".to_owned()),
                        errno: 0,
                        pid: child.id() as i32,
                    }
                }
                Err(error) => {
                    warn!("process failed to start {:?}", error);
                    msg::StartedProcess {
                        success: false,
                        message: Some(error.description().to_owned()),
                        errno: error.raw_os_error().unwrap_or(-1),
                        pid: -1,
                    }
                }
            };

            debug!("sending {:#?}", response);
            msg::encode_request(&mut buf, &response);
            trace!("response data: {:?}", msg::bytes(&buf));

            sock.send(buf)
                .and_then(|(sock, _buf)| {
                    let result = match maybe_child {
                        Ok(child) => {
                            let (w, r) = UnixPacketFramed::new(
                                sock,
                                msg::Codec::<msg::Signal, msg::RetCode>::new(),
                            )
                            .split();
                            Ok((w, r, child))
                        }
                        Err(error) => Err(error),
                    };
                    future::result(result)
                })
                .and_then(|(w, r, child)| {
                    debug!(
                        "process={:?} waiting for exit and signals",
                        child.id()
                    );
                    handle_process(r, w, child)
                })
        })
        .map_err(|err| warn!("failed to handle client {:?}", err))
}

fn signals() -> Result<impl Stream<Item = i32, Error = IoError>, IoError> {
    use signal_hook as sig;
    use tokio_reactor::Handle;

    let h = Handle::default();
    let sigs = sig::iterator::Signals::new(&[sig::SIGINT, sig::SIGTERM])?;

    sig::iterator::Async::new(sigs, &h)
}

pub(crate) fn command(args: &Args) -> Result<i32, IoError> {
    let mut runtime = Runtime::new()?;

    info!("server starting at {:?}", args.server);
    let sock = UnixPacketListener::bind(args.server)?;
    info!("server started");

    defer!({
        debug!("removing server socket at {:?}", args.server);
        std::fs::remove_file(args.server).unwrap_or_else(|err| {
            error!("failed to remove socket file {:?}", err)
        })
    });

    let signals = signals()?;
    runtime.spawn(
        sock.incoming()
            .map_err(|err| error!("failed to accept connection {:?}", err))
            .for_each(|client_socket| {
                tokio::spawn(handle_client(client_socket))
            }),
    );
    let res = match runtime.block_on(signals.into_future()) {
        Ok((sig, _stream)) => {
            if let Some(val) = sig {
                debug!("received signal {:?}", val);
            }
            Ok(0)
        }
        Err((err, _stream)) => Err(err),
    };
    info!("server shutdown");
    res
}
