use futures::{try_ready, Async, Future, Poll};
use std::io::Error as IoError;
use std::mem::replace;

use super::UnixPacket;

enum State<T> {
    Receiving { sock: UnixPacket, buf: T },
    Empty,
}

pub struct Recv<T> {
    st: State<T>,
}

impl<T> Recv<T> {
    pub fn new(base: UnixPacket, buf: T) -> Recv<T> {
        Recv {
            st: State::Receiving { sock: base, buf },
        }
    }
}

impl<T> Future for Recv<T>
where
    T: AsMut<[u8]>,
{
    type Item = (UnixPacket, T, usize);
    type Error = IoError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let received = if let State::Receiving {
            ref mut sock,
            ref mut buf,
        } = self.st
        {
            try_ready!(sock.poll_recv(buf.as_mut()))
        } else {
            panic!();
        };

        if let State::Receiving { sock, buf } =
            replace(&mut self.st, State::Empty)
        {
            Ok(Async::Ready((sock, buf, received)))
        } else {
            panic!();
        }
    }
}
