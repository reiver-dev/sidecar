use futures::{try_ready, Async, Future, Poll};
use std::io::Error as IoError;
use std::mem::replace;

use super::UnixPacket;

enum State<T> {
    Sending { sock: UnixPacket, buf: T },
    Empty,
}

pub struct Send<T> {
    st: State<T>,
}

impl<T> Send<T> {
    pub fn new(base: UnixPacket, buf: T) -> Send<T> {
        Send {
            st: State::Sending { sock: base, buf },
        }
    }
}

impl<T> Future for Send<T>
where
    T: AsRef<[u8]>,
{
    type Item = (UnixPacket, T);
    type Error = IoError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        if let State::Sending { ref sock, ref buf } = self.st {
            try_ready!(sock.poll_send(buf.as_ref()));
        } else {
            panic!();
        }

        if let State::Sending { sock, buf } =
            replace(&mut self.st, State::Empty)
        {
            Ok(Async::Ready((sock, buf)))
        } else {
            panic!();
        }
    }
}
