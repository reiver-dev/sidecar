mod accept;
mod events;
mod fd;
mod fdtransfer;
pub mod flags;
mod ops;
mod reactor;

pub use nix::sys::uio::IoVec;
pub use std::io::Result;
pub use std::os::unix::io::RawFd;

pub use accept::{accept, Accept};
pub use fd::Fd;
pub use fdtransfer::{recvfds, sendfds, CmsgBuf, RecvFds, SendFds};
pub use ops::{read, recv, send, write, Read, Recv, Send, Write};
pub use reactor::Events;

pub fn invalid_argument() -> std::io::Error {
    std::io::Error::from_raw_os_error(nix::errno::Errno::EINVAL as i32)
}

pub fn nixerror(value: nix::Error) -> std::io::Error {
    value.as_errno().unwrap_or(nix::errno::Errno::EINVAL).into()
}
