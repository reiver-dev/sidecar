use std::future::Future;
use std::io::Result;
use std::pin::Pin;
use std::task::{Context, Poll};

use nix::sys::socket::{self, MsgFlags};
use nix::unistd;

use super::reactor::Events;

pub struct Read<'a, 'b> {
    events: &'a Events,
    buf: &'b mut [u8],
}

impl<'a, 'b> Read<'a, 'b> {
    pub fn new(events: &'a Events, buf: &'b mut [u8]) -> Self {
        Self { events, buf }
    }

    pub fn do_poll(&mut self, ctx: &mut Context<'_>) -> Poll<Result<usize>> {
        self.events.poll_read(ctx, |fd| unistd::read(fd, self.buf))
    }
}

pub struct Write<'a, 'b> {
    events: &'a Events,
    buf: &'b [u8],
}

impl<'a, 'b> Write<'a, 'b> {
    pub fn new(events: &'a Events, buf: &'b [u8]) -> Self {
        Self { events, buf }
    }

    pub fn do_poll(&mut self, ctx: &mut Context<'_>) -> Poll<Result<usize>> {
        self.events
            .poll_write(ctx, |fd| unistd::write(fd, self.buf))
    }
}

pub struct Recv<'a, 'b> {
    events: &'a Events,
    buf: &'b mut [u8],
    flags: MsgFlags,
}

impl<'a, 'b> Recv<'a, 'b> {
    pub fn new(
        events: &'a Events,
        buf: &'b mut [u8],
        flags: MsgFlags,
    ) -> Self {
        Self { events, buf, flags }
    }

    pub fn do_poll(&mut self, ctx: &mut Context<'_>) -> Poll<Result<usize>> {
        self.events
            .poll_read(ctx, |fd| socket::recv(fd, self.buf, self.flags))
    }
}

pub struct Send<'a, 'b> {
    events: &'a Events,
    buf: &'b [u8],
    flags: MsgFlags,
}

impl<'a, 'b> Send<'a, 'b> {
    pub fn new(events: &'a Events, buf: &'b [u8], flags: MsgFlags) -> Self {
        Self { events, buf, flags }
    }

    pub fn do_poll(&mut self, ctx: &mut Context<'_>) -> Poll<Result<usize>> {
        self.events
            .poll_write(ctx, |fd| socket::send(fd, self.buf, self.flags))
    }
}

impl<'a, 'b> Future for Read<'a, 'b> {
    type Output = Result<usize>;

    fn poll(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
    ) -> Poll<Self::Output> {
        self.get_mut().do_poll(ctx)
    }
}

impl<'a, 'b> Future for Recv<'a, 'b> {
    type Output = Result<usize>;

    fn poll(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
    ) -> Poll<Self::Output> {
        self.get_mut().do_poll(ctx)
    }
}

impl<'a, 'b> Future for Write<'a, 'b> {
    type Output = Result<usize>;

    fn poll(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
    ) -> Poll<Self::Output> {
        self.get_mut().do_poll(ctx)
    }
}

impl<'a, 'b> Future for Send<'a, 'b> {
    type Output = Result<usize>;

    fn poll(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
    ) -> Poll<Self::Output> {
        self.get_mut().do_poll(ctx)
    }
}

pub fn read<'a, 'b>(events: &'a Events, buf: &'b mut [u8]) -> Read<'a, 'b> {
    Read::new(events, buf)
}

pub fn write<'a, 'b>(events: &'a Events, buf: &'b [u8]) -> Write<'a, 'b> {
    Write::new(events, buf)
}

pub fn recv<'a, 'b>(
    events: &'a Events,
    buf: &'b mut [u8],
    flags: MsgFlags,
) -> Recv<'a, 'b> {
    Recv::new(events, buf, flags)
}

pub fn send<'a, 'b>(
    events: &'a Events,
    buf: &'b [u8],
    flags: MsgFlags,
) -> Send<'a, 'b> {
    Send::new(events, buf, flags)
}
