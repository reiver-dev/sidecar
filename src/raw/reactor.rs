use std::io::Result;
pub use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::task::{Context, Poll};

use futures::ready;
use mio::Ready;
use nix::errno::EWOULDBLOCK;
use nix::{Error as NixError, Result as NixResult};
use tokio::io::PollEvented;

use super::nixerror;
use super::Fd;

#[derive(Debug)]
pub struct Events {
    io: PollEvented<Fd>,
}

impl AsRawFd for Events {
    fn as_raw_fd(&self) -> RawFd {
        self.io.get_ref().as_raw_fd()
    }
}

impl Events {
    pub fn from_fd(fd: Fd) -> Result<Self> {
        Ok(Self {
            io: PollEvented::new(fd)?,
        })
    }

    pub fn poll_read_ready(
        &self,
        ctx: &mut Context<'_>,
    ) -> Poll<Result<Ready>> {
        self.io.poll_read_ready(ctx, Ready::readable())
    }

    pub fn poll_write_ready(
        &self,
        ctx: &mut Context<'_>,
    ) -> Poll<Result<Ready>> {
        self.io.poll_write_ready(ctx)
    }

    pub fn clear_read_ready(&self, ctx: &mut Context<'_>) -> Result<()> {
        self.io.clear_read_ready(ctx, Ready::readable())
    }

    pub fn clear_write_ready(&self, ctx: &mut Context<'_>) -> Result<()> {
        self.io.clear_write_ready(ctx)
    }

    pub fn poll_read<F, R>(
        &self,
        ctx: &mut Context<'_>,
        fun: F,
    ) -> Poll<Result<R>>
    where
        F: FnOnce(RawFd) -> NixResult<R>,
    {
        ready!(self.poll_read_ready(ctx))?;

        match fun(self.io.get_ref().as_raw_fd()) {
            Err(NixError::Sys(EWOULDBLOCK)) => {
                self.clear_read_ready(ctx)?;
                Poll::Pending
            }
            Err(other) => Poll::Ready(Err(nixerror(other))),
            Ok(value) => Poll::Ready(Ok(value)),
        }
    }

    pub fn poll_read_maybe<F, R>(
        &self,
        ctx: &mut Context<'_>,
        fun: F,
    ) -> Poll<Result<R>>
    where
        F: FnOnce(RawFd) -> NixResult<Option<R>>,
    {
        ready!(self.poll_read_ready(ctx))?;

        match fun(self.as_raw_fd()) {
            Err(NixError::Sys(EWOULDBLOCK)) => {
                self.clear_read_ready(ctx)?;
                Poll::Pending
            }
            Err(other) => Poll::Ready(Err(nixerror(other))),
            Ok(None) => {
                self.clear_read_ready(ctx)?;
                Poll::Pending
            }
            Ok(Some(value)) => Poll::Ready(Ok(value)),
        }
    }

    pub fn poll_write<F, R>(
        &self,
        ctx: &mut Context<'_>,
        fun: F,
    ) -> Poll<Result<R>>
    where
        F: FnOnce(RawFd) -> NixResult<R>,
    {
        ready!(self.poll_write_ready(ctx))?;
        match fun(self.as_raw_fd()) {
            Err(NixError::Sys(EWOULDBLOCK)) => {
                self.clear_write_ready(ctx)?;
                Poll::Pending
            }
            Err(other) => Poll::Ready(Err(nixerror(other))),
            Ok(value) => Poll::Ready(Ok(value)),
        }
    }
}
