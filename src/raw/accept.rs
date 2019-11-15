use futures::Stream;
use std::io::Result as IoResult;
use std::pin::Pin;
use std::task::{Context, Poll};

use super::{Events, Fd, RawFd};

pub struct Accept<'a> {
    events: &'a Events,
}

#[cfg(target_os = "linux")]
fn _accept(fd: RawFd) -> Result<Option<Fd>, nix::Error> {
    use nix::sys::socket::{accept4, SockFlag};
    let flags = SockFlag::SOCK_CLOEXEC | SockFlag::SOCK_NONBLOCK;
    accept4(fd, flags).map(
        |val| {
            if val == 0 {
                None
            } else {
                Some(Fd::new(val))
            }
        },
    )
}

#[cfg(not(target_os = "linux"))]
fn _accept(fd: RawFd) -> Result<Option<Fd>, nix::Error> {
    use nix::sys::socket::accept;
    accept(fd).map(|val| if val == 0 { None } else { Some(Fd::new(val)) })
}

impl<'a> Accept<'a> {
    pub fn new(events: &'a Events) -> Self {
        Accept { events }
    }

    #[cfg(not(target_os = "linux"))]
    fn do_next(&self, ctx: &mut Context<'_>) -> Poll<Option<IoResult<Fd>>> {
        use super::flags::{set_cloexec, set_nonblock};
        match self.events.poll_read_maybe(ctx, |fd| _accept(fd)) {
            Poll::Ready(val) => Poll::Ready(Some(val.and_then(|fd| {
                set_cloexec(fd.raw()).and_then(|| set_nonblock(fd.raw()))
            }))),
            Poll::Pending => Poll::Pending,
        }
    }

    #[cfg(target_os = "linux")]
    fn do_next(&self, ctx: &mut Context<'_>) -> Poll<Option<IoResult<Fd>>> {
        match self.events.poll_read_maybe(ctx, |fd| _accept(fd)) {
            Poll::Ready(val) => Poll::Ready(Some(val)),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<'a> Stream for Accept<'a> {
    type Item = IoResult<Fd>;

    fn poll_next(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        self.get_mut().do_next(ctx)
    }
}

pub fn accept(events: &Events) -> Accept {
    Accept::new(events)
}
