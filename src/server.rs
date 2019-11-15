use std::io::Result;
use std::path::Path;
use std::process::ExitStatus;

use futures::future::{select, Either, FutureExt};
use futures::stream::{select as stream_select, StreamExt};

use log::{debug, error, info, warn};
use scopeguard::defer;
use tokio::signal::unix::{signal, SignalKind};

use crate::child::setup_command;
use crate::child_watcher::{self, Child};
use crate::messages as msg;
use crate::raw::{blocking::bind, flags::set_cloexec, CmsgBuf, RawFd};
use crate::runtime;
use crate::socket::{Shutdown, Socket};
use crate::system::{self, kill, killpg, Pid, Signal};

fn pass_signal(pid: Pid, mut sigval: i32, pg_leader: bool) {
    let send_to_group = if sigval < 0 {
        sigval = -sigval;
        true
    } else {
        false
    };

    match Signal::from_c_int(sigval) {
        Ok(sig) => {
            if send_to_group && pg_leader {
                info!("process={} received group signal={}", pid, sig);
                killpg(pid, sig);
            } else {
                info!("process={} received signal={}", pid, sig);
                kill(pid, sig);
            }
        }
        Err(_) => {
            warn!(
                "process={} received invalid signal value {:?}",
                pid, sigval
            );
        }
    }
}

fn child_finished(pid: Pid, status: ExitStatus) -> msg::ProcessResult {
    match status.code() {
        Some(code) => {
            info!("process={} exited code={:?}", pid, code);
            msg::ProcessResult::Exit(code as i32)
        }
        None => {
            use std::os::unix::process::ExitStatusExt;
            match status.signal() {
                Some(sig) => {
                    info!("process={} exited signal={:?}", pid, sig);
                    msg::ProcessResult::Signal(sig)
                }
                None => {
                    warn!("process={} exited without reason", pid);
                    msg::ProcessResult::Undefined
                }
            }
        }
    }
}

async fn handle_child(
    sock: Socket,
    mut child: Child,
    mut buffer: Vec<u8>,
    killsig: system::Signal,
    process_group_leader: bool,
) -> Result<()> {
    let mut sendbuf = Vec::with_capacity(16);
    let mut signal = sock.recv(&mut buffer);
    let pid = system::Pid::from_raw(child.id() as i32);

    loop {
        let selected = select(child, signal).await;
        let (nchild, nsignal) = match selected {
            Either::Left((Err(waiterror), _signal)) => {
                warn!("process={} wait error={:?}", pid, waiterror);
                return Err(waiterror);
            }
            Either::Left((Ok(exitstatus), _signal)) => {
                let response = child_finished(pid, exitstatus);
                if let Err(err) = sock.shutdown(Shutdown::Read) {
                    warn!(
                        "process={} failed to shutdown read: {:?}",
                        pid, err
                    );
                };
                msg::encode_request(&mut sendbuf, &response)?;
                sock.send(&sendbuf).await?;
                break;
            }
            Either::Right((received, child1)) => match received {
                Err(err) => {
                    warn!(
                        "process={} client error={:?} sending SIGKILL",
                        pid, err
                    );
                    if process_group_leader {
                        system::killpg(pid, system::SIGKILL);
                    } else {
                        system::kill(pid, system::SIGKILL);
                    }
                    let _ = child1.await;
                    break;
                }
                Ok(0) => {
                    warn!(
                        "process={} client disconnected sending signal={}",
                        pid, killsig
                    );
                    if process_group_leader {
                        system::killpg(pid, killsig);
                    } else {
                        system::kill(pid, killsig);
                    }
                    let _ = child1.await;
                    break;
                }
                Ok(size) => {
                    let req: msg::Signal =
                        { msg::decode_request(&buffer[..size])? };
                    pass_signal(pid, req.0, process_group_leader);
                    (child1, sock.recv(&mut buffer))
                }
            },
        };

        child = nchild;
        signal = nsignal;
    }

    Ok(())
}

struct ChildParams {
    pub is_pg_leader: bool,
    pub connsig: Signal,
}

async fn client_session(sock: Socket) -> Result<()> {
    let mut buffer = vec![0u8; 4096];

    let req: msg::RequestOutput = {
        let received = sock.recv(&mut buffer).await?;
        debug!("request received: {} bytes", received);
        msg::decode_request(&buffer[..received])?
    };

    buffer.clear();

    match req {
        msg::RequestOutput::Stop => {
            debug!("requested `stop`");
            system::raise(Signal::SIGINT)
                .expect("failed to send SIGINT to self");
            return Ok(());
        }
        msg::RequestOutput::Exec(header) => {
            debug!("requested `exec`");
            debug!("exec header size: {}", header.body_size);
            let (child, params) = {
                let mut fdbuf = [-1 as RawFd; 3];
                let exec_request: msg::ExecRequestOutput;
                let fds: &[RawFd];
                buffer.resize_with(header.body_size, Default::default);
                {
                    let (data_len, fds_len) = sock
                        .recvfds(&mut CmsgBuf::new(&mut buffer, &mut fdbuf))
                        .await?;

                    debug!("received exec data={} fds={}", data_len, fds_len);

                    exec_request =
                        msg::decode_request_ref(&buffer[..data_len])?;

                    fds = &fdbuf[..fds_len]
                }

                let is_pg_leader = exec_request.startup.contains(
                    msg::StartMode::PROCESS_GROUP | msg::StartMode::SESSION,
                );

                let connsig = Signal::from_c_int(exec_request.connsig)
                    .unwrap_or(Signal::SIGKILL);

                let child = {
                    let proc_request: msg::ProcessRequest =
                        { (&exec_request).into() };
                    debug!("fds: {:?} -- request: {:#?}", fds, proc_request);
                    setup_command(&proc_request, fds)
                };

                (
                    child,
                    ChildParams {
                        is_pg_leader,
                        connsig,
                    },
                )
            };

            match child {
                Ok(child) => {
                    debug!("process={} started", child.id());
                    let response = msg::StartedProcess {
                        success: true,
                        message: "",
                        errno: 0,
                        pid: child.id() as i32,
                    };
                    buffer.clear();
                    msg::encode_request(&mut buffer, &response)?;
                    sock.send(&buffer).await?;
                    handle_child(
                        sock,
                        child,
                        buffer,
                        params.connsig,
                        params.is_pg_leader,
                    )
                    .await
                }
                Err(error) => {
                    debug!("process failed");
                    let message = format!("{}", error);
                    let response = msg::StartedProcess {
                        success: false,
                        message: &message,
                        errno: error.raw_os_error().unwrap_or(-1),
                        pid: -1,
                    };
                    buffer.clear();
                    msg::encode_request(&mut buffer, &response)?;
                    sock.send(&buffer).await.map(drop)
                }
            }
        }
    }
}

async fn handle_client(sock: Socket) {
    if let Err(err) = client_session(sock).await {
        error!("error during connection: {:?}", err);
    }
}

async fn listen(socket: Socket) {
    let mut incoming = socket.accept();
    while let (Some(res), incoming1) = incoming.into_future().await {
        incoming = incoming1;
        match res {
            Ok(sock) => {
                info!("client connected");
                runtime::spawn(Box::pin(handle_client(
                    Socket::from_fd(sock).unwrap(),
                )));
            }
            Err(err) => {
                error!("failed to accept connection {:?}", err);
            }
        }
    }
}

pub(crate) struct Args<'a> {
    pub server: &'a Path,
}

fn first_invalid_fd(fr: i32, to: i32) -> i32 {
    for i in fr..to {
        if !system::is_valid_fd(i) {
            return i;
        }
    }
    return to;
}

pub(crate) fn command(args: &Args) -> Result<i32> {
    let mut runtime = {
        // FIXME:
        // mio 0.6 does not set wakup pipes as CLOEXEC
        let lvfd_before = first_invalid_fd(0, 16);
        let runtime = runtime::new()?;
        let lvfd_after = first_invalid_fd(lvfd_before, 16);
        for fd in lvfd_before..lvfd_after {
            set_cloexec(fd)?;
        }

        runtime
    };

    info!("server starting at {:?}", args.server);
    let fd = bind(args.server)?;
    defer!({
        debug!("removing server socket at {:?}", args.server);
        std::fs::remove_file(args.server).unwrap_or_else(|err| {
            error!("failed to remove socket file {:?}", err)
        })
    });

    let res = runtime.block_on(async {
        let sock = Socket::from_fd(fd)?;
        info!("server started");

        let sigint = signal(SignalKind::interrupt())?;
        let sigterm = signal(SignalKind::terminate())?;
        let sigchld = child_watcher::signal_queue()?;

        runtime::spawn(child_watcher::listen(sigchld));
        runtime::spawn(listen(sock));

        match stream_select(
            sigint.map(|()| Signal::SIGINT),
            sigterm.map(|()| Signal::SIGTERM),
        )
        .into_future()
        .map(|(item, _rest)| item)
        .await
        {
            Some(sig) => {
                info!("received signal {:?}", sig);
                Ok(0)
            }
            None => {
                warn!("received no signal");
                Ok(0)
            }
        }
    });

    info!("server shutdown");
    res
}
