use std::io::Error as IoError;
use std::mem::replace;

use futures::{try_ready, Async, Future, Poll};

use super::{ControlMessage, RawFd, UnixPacket};

enum State<T, F> {
    Sending { sock: UnixPacket, buf: T, fds: F },
    Empty,
}

pub struct Msg<T, F> {
    st: State<T, F>,
}

impl<T, F> Msg<T, F>
where
    T: AsRef<[u8]>,
    F: AsRef<[RawFd]>,
{
    pub fn new(base: UnixPacket, buf: T, fds: F) -> Msg<T, F> {
        Msg {
            st: State::Sending {
                sock: base,
                buf,
                fds,
            },
        }
    }
}

impl<T, F> Future for Msg<T, F>
where
    T: AsRef<[u8]>,
    F: AsRef<[RawFd]>,
{
    type Item = (UnixPacket, T);
    type Error = IoError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        if let State::Sending {
            ref sock,
            ref buf,
            ref fds,
        } = self.st
        {
            let cmsg: [ControlMessage; 1] =
                [ControlMessage::ScmRights(fds.as_ref())];
            try_ready!(sock.poll_sendmsg(buf.as_ref(), &cmsg));
        } else {
            panic!();
        }

        if let State::Sending {
            sock,
            buf,
            fds: _ignore,
        } = replace(&mut self.st, State::Empty)
        {
            Ok(Async::Ready((sock, buf)))
        } else {
            panic!();
        }
    }
}
