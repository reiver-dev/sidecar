use log::debug;
use std::io::Error as IoError;
use std::path::Path;

use tokio::prelude::*;
use tokio::runtime::current_thread::Runtime;

use crate::messages as msg;
use crate::net::UnixPacket;

pub(crate) struct Args<'a> {
    pub connect: &'a Path,
}

fn execute(socket: UnixPacket) -> impl Future<Item = (), Error = IoError> {
    let mut buffer = Vec::with_capacity(16);

    {
        let request = msg::Request::Stop;
        msg::encode_request(&mut buffer, &request);
    }

    socket.send(buffer).map(|_| ())
}

pub(crate) fn command(args: &Args) -> Result<(), IoError> {
    let mut runtime = Runtime::new()?;
    debug!("connecting to {:?}", args.connect);
    let socket = UnixPacket::connect(args.connect)?;
    runtime.block_on(execute(socket))
}
