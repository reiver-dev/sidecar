use std::io::Result;
pub use std::net::Shutdown;
use std::os::unix::io::{AsRawFd, IntoRawFd, RawFd};

use crate::raw;
use nix::sys::socket::{self, MsgFlags};

#[derive(Debug)]
pub struct Socket {
    inner: raw::Events,
}

impl Socket {
    pub fn from_fd(fd: raw::Fd) -> Result<Self> {
        Ok(Self {
            inner: raw::Events::from_fd(fd)?,
        })
    }

    #[cfg(target_os = "linux")]
    pub fn accept(&self) -> raw::Accept<'_> {
        raw::accept(self.as_events())
    }

    #[cfg(not(target_os = "linux"))]
    pub fn accept(&self) -> raw::Accept<'_> {
        raw::accept(self.as_events())
    }

    pub fn send<'a, 'b>(&'a self, buf: &'b [u8]) -> raw::Send<'a, 'b> {
        raw::send(self.as_events(), buf, MsgFlags::empty())
    }

    pub fn recv<'a, 'b>(&'a self, buf: &'b mut [u8]) -> raw::Recv<'a, 'b> {
        raw::recv(self.as_events(), buf, MsgFlags::empty())
    }

    pub fn sendfds<'a, 'b>(
        &'a self,
        buf: &'b [u8],
        fds: &'b [RawFd],
    ) -> raw::SendFds<'a, 'b> {
        raw::sendfds(self.as_events(), buf, fds)
    }

    pub fn recvfds<'a, 'b>(
        &'a self,
        buf: &'b mut raw::CmsgBuf<'b>,
    ) -> raw::RecvFds<'a, 'b> {
        raw::recvfds(self.as_events(), buf)
    }

    #[allow(dead_code)]
    pub fn take_error(&self) -> Result<i32> {
        socket::getsockopt(self.as_raw_fd(), socket::sockopt::SocketError {})
            .map_err(raw::nixerror)
    }

    pub fn as_raw_fd(&self) -> RawFd {
        self.inner.as_raw_fd()
    }

    pub fn as_events(&self) -> &raw::Events {
        &self.inner
    }

    pub fn shutdown(&self, how: Shutdown) -> Result<()> {
        let nhow: socket::Shutdown = match how {
            Shutdown::Read => socket::Shutdown::Read,
            Shutdown::Write => socket::Shutdown::Write,
            Shutdown::Both => socket::Shutdown::Both,
        };
        socket::shutdown(self.as_raw_fd(), nhow).map_err(raw::nixerror)
    }
}

impl AsRawFd for Socket {
    fn as_raw_fd(&self) -> i32 {
        self.as_raw_fd()
    }
}

impl IntoRawFd for Socket {
    fn into_raw_fd(self) -> i32 {
        self.as_raw_fd()
    }
}
