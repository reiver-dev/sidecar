use std::convert::TryInto;
use std::future::Future;
use std::io::Result;
use std::mem;
use std::pin::Pin;
use std::task::{Context, Poll};

use libc::CMSG_SPACE;
use nix::sys::socket::{self, ControlMessage, ControlMessageOwned, MsgFlags};
use nix::sys::uio::IoVec;

use super::{Events, RawFd};

#[cfg(target_os = "linux")]
const RECV_FLAGS: MsgFlags = MsgFlags::MSG_CMSG_CLOEXEC;

#[cfg(not(target_os = "linux"))]
const RECV_FLAGS: MsgFlags = MsgFlags::empty();

fn cmsg_space(num_fds: usize) -> usize {
    let sz = (mem::size_of::<RawFd>() * num_fds).try_into().unwrap();
    let res = unsafe { CMSG_SPACE(sz) };
    res as usize
}

fn extract_fds(received: &socket::RecvMsg, dest: &mut [RawFd]) -> usize {
    let mut numfds = 0;
    let max = dest.len();

    'done: for r in received.cmsgs() {
        if let ControlMessageOwned::ScmRights(ref fds) = r {
            for fd in fds {
                dest[numfds] = *fd;
                numfds += 1;
                if numfds == max {
                    break 'done;
                }
            }
        }
    }

    numfds
}

pub struct SendFds<'a, 'b> {
    events: &'a Events,
    buf: &'b [u8],
    cmsg: [ControlMessage<'b>; 1],
}

impl<'a, 'b> SendFds<'a, 'b> {
    pub fn new(events: &'a Events, buf: &'b [u8], fds: &'b [RawFd]) -> Self {
        Self {
            events,
            buf,
            cmsg: [ControlMessage::ScmRights(fds); 1],
        }
    }

    pub fn do_poll(&mut self, ctx: &mut Context<'_>) -> Poll<Result<usize>> {
        let iovec = [IoVec::from_slice(self.buf); 1];
        self.events.poll_write(ctx, |fd| {
            socket::sendmsg(fd, &iovec, &self.cmsg, MsgFlags::empty(), None)
        })
    }
}

pub struct CmsgBuf<'a> {
    data: &'a mut [u8],
    fds: &'a mut [RawFd],
    inner: Vec<u8>,
}

pub struct RecvFds<'a, 'b> {
    events: &'a Events,
    buf: &'b mut CmsgBuf<'b>,
}

impl<'a> CmsgBuf<'a> {
    pub fn new(buf: &'a mut [u8], fds: &'a mut [RawFd]) -> CmsgBuf<'a> {
        let len = fds.len();
        CmsgBuf {
            data: buf,
            fds: fds,
            inner: vec![0u8; cmsg_space(len)],
        }
    }
}

impl<'a, 'b> RecvFds<'a, 'b> {
    pub fn new(events: &'a Events, buf: &'b mut CmsgBuf<'b>) -> Self {
        Self { events, buf }
    }

    pub fn do_poll(
        &mut self,
        ctx: &mut Context<'_>,
    ) -> Poll<Result<(usize, usize)>> {
        let iovec = [IoVec::from_mut_slice(self.buf.data); 1];
        let cmsg = &mut self.buf.inner;
        let fds = &mut self.buf.fds;
        match self.events.poll_write(ctx, |fd| {
            socket::recvmsg(fd, &iovec, Some(cmsg), RECV_FLAGS)
        }) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(received)) => {
                let numbytes = received.bytes;
                let numfds = extract_fds(&received, fds);
                Poll::Ready(Ok((numbytes, numfds)))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
        }
    }
}

impl<'a, 'b> Future for SendFds<'a, 'b> {
    type Output = Result<usize>;

    fn poll(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
    ) -> Poll<Self::Output> {
        self.get_mut().do_poll(ctx)
    }
}

impl<'a, 'b> Future for RecvFds<'a, 'b> {
    type Output = Result<(usize, usize)>;

    fn poll(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
    ) -> Poll<Self::Output> {
        self.get_mut().do_poll(ctx)
    }
}

pub fn sendfds<'a, 'b>(
    events: &'a Events,
    buf: &'b [u8],
    fds: &'b [RawFd],
) -> SendFds<'a, 'b> {
    SendFds::new(events, buf, fds)
}

pub fn recvfds<'a, 'b>(
    events: &'a Events,
    buf: &'b mut CmsgBuf<'b>,
) -> RecvFds<'a, 'b> {
    RecvFds::new(events, buf)
}
