use futures::{try_ready, Async, Poll, Stream};
use mio::Ready;
use std::io;
use std::path::Path;
use tokio_reactor::PollEvented;

use super::socket;
use super::{AsRawFd, RawFd, Recv, RecvMsg, Send, SendMsg};
use nix::sys::socket as nix_socket;

#[derive(Debug)]
pub struct UnixPacket {
    io: PollEvented<socket::UnixPacket>,
}

#[derive(Debug)]
pub struct UnixPacketListener {
    io: PollEvented<socket::UnixPacket>,
}

#[derive(Debug)]
pub struct Incoming {
    inner: UnixPacketListener,
}

impl Incoming {
    pub fn new(listener: UnixPacketListener) -> Incoming {
        Incoming { inner: listener }
    }
}

impl Stream for Incoming {
    type Item = UnixPacket;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, io::Error> {
        Ok(Some(try_ready!(self.inner.poll_accept())).into())
    }
}

impl UnixPacket {
    pub fn new(fd: socket::UnixPacket) -> UnixPacket {
        UnixPacket {
            io: PollEvented::new(fd),
        }
    }
}

impl UnixPacketListener {
    pub fn bind<P: AsRef<Path>>(path: P) -> io::Result<UnixPacketListener> {
        Ok(UnixPacketListener {
            io: PollEvented::new(socket::UnixPacket::bind(path)?),
        })
    }

    pub fn poll_read_ready(&self, ready: Ready) -> Poll<Ready, io::Error> {
        self.io.poll_read_ready(ready)
    }

    pub fn poll_accept(&self) -> Poll<UnixPacket, io::Error> {
        try_ready!(self.poll_read_ready(Ready::readable()));

        match self.io.get_ref().accept() {
            Ok(None) => {
                self.io.clear_read_ready(Ready::readable())?;
                Ok(Async::NotReady)
            }
            Ok(Some(sock)) => Ok(Async::Ready(UnixPacket::new(sock))),
            Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                self.io.clear_read_ready(Ready::readable())?;
                Ok(Async::NotReady)
            }
            Err(err) => Err(err),
        }
    }

    pub fn incoming(self) -> Incoming {
        Incoming::new(self)
    }
}

impl UnixPacket {
    pub fn connect<P: AsRef<Path>>(path: P) -> io::Result<UnixPacket> {
        Ok(UnixPacket {
            io: PollEvented::new(socket::UnixPacket::connect(&path)?),
        })
    }

    pub fn poll_read_ready(&self, ready: Ready) -> Poll<Ready, io::Error> {
        self.io.poll_read_ready(ready)
    }

    pub fn poll_write_ready(&self) -> Poll<Ready, io::Error> {
        self.io.poll_write_ready()
    }

    pub fn poll_recv(&self, buf: &mut [u8]) -> Poll<usize, io::Error> {
        try_ready!(self.poll_read_ready(Ready::readable()));

        match self.io.get_ref().recv(buf) {
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                self.io.clear_read_ready(Ready::readable())?;
                Ok(Async::NotReady)
            }
            Err(err) => Err(err),
            Ok(received) => Ok(received.into()),
        }
    }

    pub fn poll_send(&self, buf: &[u8]) -> Poll<usize, io::Error> {
        try_ready!(self.poll_write_ready());

        match self.io.get_ref().send(buf) {
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                self.io.clear_write_ready()?;
                Ok(Async::NotReady)
            }
            Err(err) => Err(err),
            Ok(sent) => Ok(sent.into()),
        }
    }

    pub fn poll_recvmsg<'a, T>(
        &self,
        buf: &'a mut [u8],
        cmsg: Option<&'a mut nix_socket::CmsgSpace<T>>,
    ) -> Poll<nix_socket::RecvMsg<'a>, io::Error> {
        try_ready!(self.poll_read_ready(Ready::readable()));

        match self.io.get_ref().recvmsg(buf, cmsg) {
            Ok(ret) => Ok(ret.into()),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                self.io.clear_read_ready(Ready::readable())?;
                Ok(Async::NotReady)
            }
            Err(e) => Err(e),
        }
    }

    pub fn poll_sendmsg<'a>(
        &self,
        buf: &'a [u8],
        cmsg: &[nix_socket::ControlMessage<'a>],
    ) -> Poll<usize, io::Error> {
        try_ready!(self.poll_write_ready());

        match self.io.get_ref().sendmsg(buf, cmsg) {
            Ok(ret) => Ok(ret.into()),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                self.io.clear_write_ready()?;
                Ok(Async::NotReady)
            }
            Err(e) => Err(e),
        }
    }

    pub fn recv<T: AsMut<[u8]>>(self, buf: T) -> Recv<T> {
        Recv::new(self, buf)
    }

    pub fn send<T: AsRef<[u8]>>(self, buf: T) -> Send<T> {
        Send::new(self, buf)
    }

    pub fn recvmsg<T: AsMut<[u8]>>(self, buf: T) -> RecvMsg<T> {
        RecvMsg::new(self, buf)
    }

    pub fn sendmsg<T, F>(self, buf: T, fds: F) -> SendMsg<T, F>
    where
        T: AsRef<[u8]>,
        F: AsRef<[RawFd]>,
    {
        SendMsg::new(self, buf, fds)
    }
}

impl AsRawFd for UnixPacket {
    fn as_raw_fd(&self) -> i32 {
        self.io.get_ref().as_raw_fd()
    }
}
