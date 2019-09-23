use std::io::Result;
use std::path::Path;

use futures_util::TryFutureExt;

use log::{debug, error};

use crate::messages as msg;
use crate::runtime;
use crate::socket::Socket;

pub(crate) struct Args<'a> {
    pub connect: &'a Path,
}

async fn execute(socket: Socket) -> Result<()> {
    let mut buffer = Vec::with_capacity(16);

    {
        let request = msg::RequestInput::Stop;
        msg::encode_request(&mut buffer, &request)?;
    }

    socket.send(&buffer).map_ok(|_| ()).await
}

pub(crate) fn command(args: &Args) -> Result<i32> {
    debug!("connecting to {:?}", args.connect);
    match Socket::connect(args.connect) {
        Ok(socket) => runtime::start(execute(socket))?.map(|_| 0),
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
