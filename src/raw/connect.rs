use std::future::Future;
use std::io::Result;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::ready;
use nix::errno::Errno;
use nix::sys::socket::{connect as _connect, getsockopt, sockopt, SockAddr};
use nix::Error as NixError;

use super::{nixerror, Events};

enum State {
    Init,
    Connecting,
}

pub struct Connect<'a, 'b> {
    events: &'a Events,
    addr: &'b SockAddr,
    state: State,
}

impl<'a, 'b> Connect<'a, 'b> {
    pub fn new(events: &'a Events, addr: &'b SockAddr) -> Self {
        Connect {
            events,
            addr,
            state: State::Init,
        }
    }

    fn do_poll(&self, ctx: &mut Context<'_>) -> Poll<Result<()>> {
        match self.state {
            State::Init => match _connect(self.events.as_raw_fd(), self.addr) {
                Err(NixError::Sys(Errno::EINPROGRESS)) => {
                    self.events.clear_write_ready(ctx)?;
                    Poll::Pending
                }
                Err(other) => Poll::Ready(Err(nixerror(other))),
                Ok(()) => Poll::Ready(Ok(())),
            },
            State::Connecting => {
                ready!(self.events.poll_write_ready(ctx))?;
                match getsockopt(
                    self.events.as_raw_fd(),
                    sockopt::SocketError {},
                ) {
                    Ok(0) => Poll::Ready(Ok(())),
                    Ok(err) => Poll::Ready(Err(Errno::from_i32(err).into())),
                    Err(err) => Poll::Ready(Err(nixerror(err))),
                }
            }
        }
    }
}

impl<'a, 'b> Future for Connect<'a, 'b> {
    type Output = Result<()>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        self.get_mut().do_poll(ctx)
    }
}

fn connect<'a, 'b>(events: &'a Events, addr: &'b SockAddr) -> Connect<'a, 'b> {
    Connect::new(events, addr)
}
