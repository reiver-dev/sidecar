use std::io::Result;
use std::os::unix::io::AsRawFd;
use std::task::{Context, Poll};

use nix::unistd;

use crate::raw::{self, Events, Fd, RawFd};

pub struct PipeRead {
    inner: Events,
}

impl PipeRead {
    #[allow(dead_code)]
    pub fn read<'a, 'b>(&'a self, buf: &'b mut [u8]) -> raw::Read<'a, 'b> {
        raw::read(&self.inner, buf)
    }

    pub fn poll_read(
        &self,
        buf: &mut [u8],
        ctx: &mut Context<'_>,
    ) -> Poll<Result<usize>> {
        self.inner.poll_read(ctx, |fd| unistd::read(fd, buf))
    }
}

pub struct PipeWrite {
    inner: Events,
}

impl PipeWrite {
    #[allow(dead_code)]
    pub fn write<'a, 'b>(&'a self, buf: &'b [u8]) -> raw::Write<'a, 'b> {
        raw::write(&self.inner, buf)
    }
}

#[cfg(target_os = "linux")]
fn make_pipe_fds() -> Result<(Fd, Fd)> {
    use nix::fcntl::OFlag;

    unistd::pipe2(OFlag::O_CLOEXEC | OFlag::O_NONBLOCK)
        .map(|(r, w)| (Fd::new(r), Fd::new(w)))
        .map_err(raw::nixerror)
}

#[cfg(not(target_os = "linux"))]
fn make_pipe_fds() -> Result<(Fd, Fd)> {
    let (r, w) = unistd::pipe().map_err(raw::nixerror)?;
    let rd = Fd::new(r);
    let wd = Fd::new(w);
    raw::flags::set_cloexec_nonblocking(r)?;
    raw::flags::set_cloexec_nonblocking(w)?;
    Ok((rd, wd))
}

pub fn make_pipe() -> Result<(PipeRead, PipeWrite)> {
    let (r, w) = make_pipe_fds()?;

    let pread = PipeRead {
        inner: Events::from_fd(r),
    };

    let pwrite = PipeWrite {
        inner: Events::from_fd(w),
    };

    Ok((pread, pwrite))
}

impl AsRawFd for PipeRead {
    fn as_raw_fd(&self) -> RawFd {
        self.inner.as_raw_fd()
    }
}

impl AsRawFd for PipeWrite {
    fn as_raw_fd(&self) -> RawFd {
        self.inner.as_raw_fd()
    }
}
