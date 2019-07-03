use std::io::Error as IoError;
use std::path::Path;

use futures::prelude::*;
use log::{debug, error, info, warn};
use scopeguard::defer;

use tokio::runtime::current_thread::Runtime;

use super::session::ClientSession;
use crate::net::{UnixPacket, UnixPacketListener};

pub fn handle_client(sock: UnixPacket) -> impl Future<Item = (), Error = ()> {
    info!("client connected");

    let mut buffer: Vec<u8> = Vec::with_capacity(4096);
    buffer.resize_with(4096, Default::default);

    ClientSession::start(sock.recvmsg(buffer))
        .map_err(|err| warn!("failed to handle client {:?}", err))
}

pub(crate) fn signals(
) -> Result<impl Stream<Item = i32, Error = IoError>, IoError> {
    use signal_hook as sig;
    use tokio_reactor::Handle;

    let h = Handle::default();
    let sigs = sig::iterator::Signals::new(&[sig::SIGINT, sig::SIGTERM])?;

    sig::iterator::Async::new(sigs, &h)
}

pub(crate) struct Args<'a> {
    pub server: &'a Path,
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
