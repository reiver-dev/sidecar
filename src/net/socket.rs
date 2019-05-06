use log::trace;
use std::io;
use std::net::Shutdown;
use std::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd, RawFd};
use std::path::Path;

use mio::unix::EventedFd;
use mio::Evented;
use mio::{Poll, PollOpt, Ready, Token};

use nix::sys::socket::{
    self, AddressFamily, MsgFlags, SockAddr, SockFlag, SockType,
};
use nix::sys::uio::IoVec;

#[derive(Debug)]
pub struct UnixPacket {
    inner: RawFd,
}

fn error(err: nix::Error) -> io::Error {
    match err {
        nix::Error::Sys(val) => val.into(),
        _ => unreachable!(),
    }
}

impl Drop for UnixPacket {
    fn drop(&mut self) {
        if self.inner >= 0 {
            trace!("socket fd={:?} closing", self.inner);
            nix::unistd::close(self.inner).expect("failed to close socket");
            self.inner = -1;
        }
    }
}

impl UnixPacket {
    fn new() -> io::Result<UnixPacket> {
        socket::socket(
            AddressFamily::Unix,
            SockType::SeqPacket,
            SockFlag::SOCK_CLOEXEC | SockFlag::SOCK_NONBLOCK,
            None,
        )
        .map(|fd| UnixPacket { inner: fd })
        .map_err(error)
    }

    pub fn bind<P: AsRef<Path>>(path: P) -> io::Result<UnixPacket> {
        let fd = UnixPacket::new()?;
        let addr = SockAddr::new_unix(path.as_ref()).map_err(error)?;
        socket::bind(fd.inner, &addr).map_err(error)?;
        socket::listen(fd.inner, 0).map_err(error)?;
        Ok(fd)
    }

    pub fn accept(&self) -> io::Result<Option<UnixPacket>> {
        socket::accept4(
            self.inner,
            SockFlag::SOCK_CLOEXEC | SockFlag::SOCK_NONBLOCK,
        )
        .map(|fd| {
            if fd == 0 {
                None
            } else {
                Some(UnixPacket { inner: fd })
            }
        })
        .map_err(error)
    }

    pub fn connect<P: AsRef<Path>>(path: P) -> io::Result<UnixPacket> {
        let fd = UnixPacket::new()?;
        let addr = SockAddr::new_unix(path.as_ref()).map_err(error)?;
        socket::connect(fd.inner, &addr).map(|_| fd).map_err(error)
    }

    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        socket::getsockopt(self.inner, socket::sockopt::SocketError {})
            .map(|val| {
                if val == 0 {
                    None
                } else {
                    Some(io::Error::from_raw_os_error(val))
                }
            })
            .map_err(error)
    }

    pub fn send(&self, buf: &[u8]) -> io::Result<usize> {
        socket::send(self.inner, buf, MsgFlags::empty()).map_err(error)
    }

    pub fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        socket::recv(self.inner, buf, MsgFlags::empty()).map_err(error)
    }

    pub fn sendmsg(
        &self,
        buf: &[u8],
        cmsgs: &[socket::ControlMessage<'_>],
    ) -> io::Result<usize> {
        let iov = [IoVec::from_slice(buf); 1];
        socket::sendmsg(self.inner, &iov, cmsgs, MsgFlags::empty(), None)
            .map_err(error)
    }

    pub fn recvmsg<'a, T>(
        &self,
        buf: &'a mut [u8],
        cmsg: Option<&'a mut socket::CmsgSpace<T>>,
    ) -> io::Result<socket::RecvMsg<'a>> {
        let iov = [IoVec::from_mut_slice(buf); 1];
        socket::recvmsg(self.inner, &iov, cmsg, MsgFlags::empty())
            .map_err(error)
    }

    pub fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        let nhow: socket::Shutdown = match how {
            Shutdown::Read => socket::Shutdown::Read,
            Shutdown::Write => socket::Shutdown::Write,
            Shutdown::Both => socket::Shutdown::Both,
        };
        socket::shutdown(self.inner, nhow).map_err(error)
    }

    fn as_raw_fd(&self) -> RawFd {
        self.inner
    }
}

impl Evented for UnixPacket {
    fn register(
        &self,
        poll: &Poll,
        token: Token,
        events: Ready,
        opts: PollOpt,
    ) -> io::Result<()> {
        EventedFd(&self.as_raw_fd()).register(poll, token, events, opts)
    }

    fn reregister(
        &self,
        poll: &Poll,
        token: Token,
        events: Ready,
        opts: PollOpt,
    ) -> io::Result<()> {
        EventedFd(&self.as_raw_fd()).reregister(poll, token, events, opts)
    }

    fn deregister(&self, poll: &Poll) -> io::Result<()> {
        EventedFd(&self.as_raw_fd()).deregister(poll)
    }
}

impl AsRawFd for UnixPacket {
    fn as_raw_fd(&self) -> i32 {
        self.as_raw_fd()
    }
}

impl IntoRawFd for UnixPacket {
    fn into_raw_fd(self) -> i32 {
        self.as_raw_fd()
    }
}

impl FromRawFd for UnixPacket {
    unsafe fn from_raw_fd(fd: i32) -> UnixPacket {
        UnixPacket { inner: fd }
    }
}
