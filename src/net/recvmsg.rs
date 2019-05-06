use std::io::Error as IoError;
use std::mem::replace;

use futures::{try_ready, Async, Future, Poll};
use nix::sys::socket;
use nix::sys::socket::ControlMessage::ScmRights;

use super::{RawFd, UnixPacket};

type Fds = [RawFd; 16];
type CmsgSpace = socket::CmsgSpace<Fds>;

enum State<T> {
    Receiving {
        sock: UnixPacket,
        buf: T,
        cmsg: CmsgSpace,
    },
    Empty,
}

pub struct Msg<T> {
    st: State<T>,
}

impl<T> Msg<T>
where
    T: AsMut<[u8]>,
{
    pub fn new(base: UnixPacket, buf: T) -> Msg<T> {
        Msg {
            st: State::Receiving {
                sock: base,
                buf,
                cmsg: CmsgSpace::new(),
            },
        }
    }
}

impl<T> Future for Msg<T>
where
    T: AsMut<[u8]>,
{
    type Item = (UnixPacket, T, Fds, usize, usize);
    type Error = IoError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let received: socket::RecvMsg;

        if let State::Receiving {
            ref mut sock,
            ref mut buf,
            ref mut cmsg,
        } = self.st
        {
            received = try_ready!(sock.poll_recvmsg(buf.as_mut(), Some(cmsg)));
        } else {
            panic!();
        }

        let msgs = received
            .cmsgs()
            .filter_map(|val| match val {
                ScmRights(fds) => Some(fds),
                _ => None,
            })
            .flat_map(|x| x)
            .cloned()
            .take(16);

        let mut fds: Fds = [-1; 16];
        let bytes = received.bytes;
        let mut num_fds = 0;

        for (i, fd) in msgs.enumerate() {
            fds[i] = fd;
            num_fds += 1;
        }

        if let State::Receiving { sock, buf, .. } =
            replace(&mut self.st, State::Empty)
        {
            Ok(Async::Ready((sock, buf, fds, bytes, num_fds)))
        } else {
            panic!();
        }
    }
}
