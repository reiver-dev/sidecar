use std::io::Result;
pub use std::net::Shutdown;
use std::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd, RawFd};
use std::path::Path;

use crate::raw;
use nix::sys::socket::{
    self, AddressFamily, MsgFlags, SockAddr, SockFlag, SockType,
};

#[derive(Debug)]
pub struct Socket {
    inner: raw::Events,
}

impl Socket {
    pub fn from_fd(fd: raw::Fd) -> Self {
        Self {
            inner: raw::Events::from_fd(fd),
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn new(blocking: bool) -> Result<Socket> {
        let fd = socket::socket(
            AddressFamily::Unix,
            SockType::SeqPacket,
            SockFlag::empty(),
            None,
        )
        .map_err(raw::nixerror)?;
        let fd1 = raw::Fd::new(fd);
        if blocking {
            raw::flags::set_cloexec_blocking(fd)?;
        } else {
            raw::flags::set_cloexec_nonblocking(fd)?;
        }
        Ok(Self::from_fd(fd1))
    }

    #[cfg(target_os = "linux")]
    fn new(blocking: bool) -> Result<Socket> {
        let flags = if blocking {
            SockFlag::SOCK_CLOEXEC
        } else {
            SockFlag::SOCK_CLOEXEC | SockFlag::SOCK_NONBLOCK
        };

        socket::socket(AddressFamily::Unix, SockType::SeqPacket, flags, None)
            .map(|fd| unsafe { Socket::from_raw_fd(fd) })
            .map_err(raw::nixerror)
    }

    pub fn bind<P: AsRef<Path>>(path: P) -> Result<Socket> {
        SockAddr::new_unix(path.as_ref())
            .map_err(raw::nixerror)
            .and_then(|addr| Socket::new(false).map(|fd| (fd, addr)))
            .and_then(|(fd, addr)| {
                socket::bind(fd.as_raw_fd(), &addr)
                    .map(|_| fd)
                    .map_err(raw::nixerror)
            })
            .and_then(|fd| {
                socket::listen(fd.as_raw_fd(), 0)
                    .map(|_| fd)
                    .map_err(raw::nixerror)
            })
    }

    pub fn connect<P: AsRef<Path>>(path: P) -> Result<Socket> {
        SockAddr::new_unix(path.as_ref())
            .map_err(raw::nixerror)
            .and_then(|addr| Socket::new(true).map(|fd| (fd, addr)))
            .and_then(|(fd, addr)| {
                socket::connect(fd.inner.as_raw_fd(), &addr)
                    .map(|_| fd)
                    .map_err(raw::nixerror)
            })
            .and_then(|fd| {
                raw::flags::set_cloexec_nonblocking(fd.as_raw_fd()).map(|_| fd)
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

impl FromRawFd for Socket {
    unsafe fn from_raw_fd(fd: i32) -> Socket {
        Socket {
            inner: raw::Events::from_raw_fd(fd),
        }
    }
}
