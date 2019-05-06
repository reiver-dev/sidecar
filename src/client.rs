use log::{debug, trace, warn};
use std::io::Error as IoError;
use std::os::unix::io::AsRawFd;
use std::path::Path;

use tokio::prelude::*;
use tokio::runtime::current_thread::Runtime;

use futures::future::Either;
use futures::stream::Stream;

use crate::messages as msg;
use crate::net::{UnixPacket, UnixPacketFramed};

pub(crate) struct Args<'a> {
    pub connect: &'a Path,
    pub program: &'a [&'a str],
    pub env: &'a [(&'a str, &'a str)],
    pub cwd: Option<&'a str>,
}

fn signals() -> Result<impl Stream<Item = i32, Error = IoError>, IoError> {
    use signal_hook as sig;
    use tokio_reactor::Handle;

    let h = Handle::default();
    let sigs = sig::iterator::Signals::new(&[
        sig::SIGABRT,
        sig::SIGALRM,
        sig::SIGBUS,
        // SIGCHLD,
        // SIGCONT,
        // SIGFPE,
        sig::SIGHUP,
        // SIGILL,
        sig::SIGINT,
        sig::SIGIO,
        // SIGKILL,
        sig::SIGPIPE,
        sig::SIGPROF,
        sig::SIGQUIT,
        // SIGSEGV,
        // SIGSTOP,
        // SIGTSTP
        sig::SIGSYS,
        sig::SIGTERM,
        sig::SIGTRAP,
        sig::SIGUSR1,
        sig::SIGUSR2,
        // SIGWINCH,
    ])?;

    sig::iterator::Async::new(sigs, &h)
}

fn wait_child<R, W, S>(
    signals: S,
    read: R,
    write: W,
) -> impl Future<Item = i32, Error = IoError>
where
    R: Stream<Item = msg::RetCode, Error = IoError>,
    W: Sink<SinkItem = msg::Signal, SinkError = IoError>,
    S: Stream<Item = i32, Error = IoError>,
{
    let pass_signals = signals.map(msg::Signal).forward(write);

    debug!("waiting process to complete");

    read.into_future()
        .select2(pass_signals)
        .map(|res| match res {
            Either::A(((result, _sink), _signals)) => match result {
                Some(ret) => ret.0,
                None => {
                    warn!("server disconnected");
                    128
                }
            },
            Either::B(_any) => {
                panic!("signal handler completed");
            }
        })
        .map_err(|err| match err {
            Either::A(((err, _sink), _other)) => {
                warn!("receiver error");
                err
            }
            Either::B((err, _other)) => {
                warn!("sender error");
                err
            }
        })
}

fn execute(
    request: &msg::Request,
    socket: UnixPacket,
) -> impl Future<Item = i32, Error = IoError> {
    let mut buffer = Vec::with_capacity(4096);
    msg::encode_request(&mut buffer, &request);

    let streams = [
        std::io::stdin().as_raw_fd(),
        std::io::stdout().as_raw_fd(),
        std::io::stderr().as_raw_fd(),
    ];

    debug!("sending {:#?}", request);
    socket
        .sendmsg(buffer, streams)
        .and_then(|(sock, mut buf)| {
            trace!("request sent");

            buf.clear();
            buf.resize(4096, 0);

            let fut = sock.recv(buf);
            trace!("wait for response");
            fut
        })
        .and_then(|(sock, mut buf, received)| {
            trace!("response received {:?} bytes", received);
            if received > 0 {
                let ret: msg::StartedProcess =
                    msg::decode_request(&buf[..received]);
                debug!("received {:#?}", ret);
                buf.clear();
            } else {
                warn!("server disconnected");
            }
            buf.resize_with(4096, Default::default);
            let sigsink = signals().expect("failed to setup signal handler");
            let framed = UnixPacketFramed::new(
                sock,
                msg::Codec::<msg::RetCode, msg::Signal>::new(),
            );
            let (write, read) = framed.split();
            wait_child(sigsink, read, write)
        })
}

pub(crate) fn command(args: &Args) -> Result<i32, IoError> {
    let mut runtime = Runtime::new()?;
    let request = msg::Request {
        argv: args
            .program
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
        cwd: match &args.cwd {
            Some("") => None,
            None => None,
            Some(s) => Some(s.to_string()),
        },
        env: if !args.env.is_empty() {
            Some(
                args.env
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
            )
        } else {
            None
        },
        io: Some(msg::Io {
            stdin: std::io::stdin().as_raw_fd(),
            stdout: std::io::stdout().as_raw_fd(),
            stderr: std::io::stderr().as_raw_fd(),
        }),
    };
    debug!("connecting to {:?}", args.connect);
    let socket = UnixPacket::connect(args.connect)?;
    let ret = runtime.block_on(execute(&request, socket));
    debug!("finished with code {:?}", ret);
    ret
}
