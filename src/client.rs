use log::{debug, error, trace, warn};
use std::io::Error as IoError;
use std::os::unix::io::AsRawFd;
use std::path::Path;

use nix::sys::signal::{raise, Signal};
use tokio::prelude::*;
use tokio::runtime::current_thread::Runtime;

use futures::future::Either;
use futures::prelude::*;
use futures::stream::Stream;

use crate::messages as msg;
use crate::net::{UnixPacket, UnixPacketFramed};

pub(crate) struct Args<'a> {
    pub connect: &'a Path,
    pub program: &'a [&'a str],
    pub env: &'a [(&'a str, &'a str)],
    pub cwd: Option<&'a str>,
    pub setpgid: bool,
    pub setsid: bool,
    pub ctty: bool,
}

fn setup_signal_handlers(
) -> Result<impl Stream<Item = Signal, Error = IoError>, IoError> {
    use nix::sys::signal::*;
    use signal_hook as hook;
    use tokio_reactor::Handle;

    let h = Handle::default();
    let sigs = hook::iterator::Signals::new(
        Signal::iterator()
            .map(|x| x as i32)
            .filter(|x| !hook::FORBIDDEN.contains(x)),
    )?;

    hook::iterator::Async::new(sigs, &h)
        .map(|stream| stream.filter_map(|sig| Signal::from_c_int(sig).ok()))
}

struct SinkAfterSent<S: Sink, F> {
    inner: S,
    cb: F,
    val: Option<S::SinkItem>,
}

impl<S: Sink, F> SinkAfterSent<S, F> {
    fn new(sink: S, callback: F) -> Self {
        Self {
            inner: sink,
            cb: callback,
            val: None,
        }
    }
}

impl<S, F> Sink for SinkAfterSent<S, F>
where
    S: Sink,
    F: Fn(S::SinkItem),
    S::SinkItem: Clone,
{
    type SinkItem = S::SinkItem;
    type SinkError = S::SinkError;

    fn start_send(
        &mut self,
        item: Self::SinkItem,
    ) -> StartSend<Self::SinkItem, Self::SinkError> {
        let value = item.clone();
        match self.inner.start_send(item) {
            Ok(AsyncSink::Ready) => {
                self.val = Some(value);
                Ok(AsyncSink::Ready)
            }
            other => other,
        }
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        match self.inner.poll_complete() {
            Ok(Async::Ready(())) => {
                if let Some(item) = self.val.take() {
                    (self.cb)(item);
                }
                Ok(Async::Ready(()))
            }
            other => other,
        }
    }
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

fn wait_child<R, W, S>(
    signals: S,
    read: R,
    write: W,
) -> impl Future<Item = i32, Error = IoError>
where
    R: Stream<Item = msg::RetCode, Error = IoError>,
    W: Sink<SinkItem = msg::Signal, SinkError = IoError>,
    S: Stream<Item = Signal, Error = IoError>,
{
    let pass_signals = signals
        .map(convert_to_group_signals)
        .map(msg::Signal)
        .forward(SinkAfterSent::new(write, |s: msg::Signal| handle_stop(s.0)));

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

fn prepare_request(args: &Args) -> msg::ExecRequest {
    let mut startup = msg::StartMode::empty();

    if args.setpgid {
        startup |= msg::StartMode::PROCESS_GROUP
    }

    if args.setsid {
        startup |= msg::StartMode::SESSION;
    }

    if args.ctty {
        startup |= msg::StartMode::CONTROLLING_TERMINAL;
    }

    msg::ExecRequest {
        argv: args
            .program
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
        startup: startup,
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
    }
}

fn execute(
    args: &Args,
    socket: UnixPacket,
) -> impl Future<Item = i32, Error = IoError> {
    let mut buffer = Vec::with_capacity(4096);

    {
        let request = msg::Request::Exec(prepare_request(args));
        debug!("sending {:#?}", request);
        msg::encode_request(&mut buffer, &request);
    }

    let streams = [
        std::io::stdin().as_raw_fd(),
        std::io::stdout().as_raw_fd(),
        std::io::stderr().as_raw_fd(),
    ];

    socket
        .sendmsg(buffer, streams)
        .and_then(|(sock, mut buf)| {
            buf.clear();
            buf.resize(4096, 0);
            sock.recv(buf)
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
            let sigsink = setup_signal_handlers()
                .expect("failed to setup signal handler");
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
    debug!("connecting to {:?}", args.connect);
    let socket = UnixPacket::connect(args.connect)?;
    let ret = runtime.block_on(execute(&args, socket));
    debug!("finished with code {:?}", ret);
    ret
}
