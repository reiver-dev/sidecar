use std::ffi::CStr;
use std::io::Error as IoError;
use std::os::unix::io::RawFd;

use libc::{self, ioctl};
use nix::fcntl::{open, OFlag};
use nix::sys::stat::Mode;

use crate::raw::{nixerror as error, Fd};

#[cfg(target_os = "linux")]
mod private {
    use nix::sys::ioctl::ioctl_num_type;
    pub(super) const TIOCNOTTY: ioctl_num_type = 0x5422;
    pub(super) use libc::TIOCSCTTY;
}

#[cfg(target_os = "macos")]
mod private {
    pub(super) const TIOCSCTTY: u64 = 0x20007461;
    pub(super) const TIOCNOTTY: u64 = 0x20007471;
}

#[cfg(target_os = "bsd")]
mod private {
    pub(super) use libc::{TIOCNOTTY, TIOCSCTTY};
}

use private::{TIOCNOTTY, TIOCSCTTY};

#[allow(dead_code)]
pub(crate) fn set_controlling_terminal(fd: RawFd) -> Result<(), IoError> {
    if unsafe { ioctl(fd, TIOCSCTTY, 1) } != 0 {
        Err(IoError::last_os_error())
    } else {
        Ok(())
    }
}

fn tty_open(flags: OFlag) -> Result<Fd, IoError> {
    let path = unsafe { CStr::from_ptr("/dev/tty\0".as_ptr() as *const _) };
    open(path, flags, Mode::empty()).map(Fd::new).map_err(error)
}

pub(crate) fn detach_controlling_terminal(fd: RawFd) -> Result<(), IoError> {
    if unsafe { ioctl(fd, TIOCNOTTY, 0) } != 0 {
        Err(IoError::last_os_error())
    } else {
        Ok(())
    }
}

pub(crate) fn disconnect_controlling_terminal() -> Result<(), IoError> {
    match tty_open(OFlag::O_RDWR | OFlag::O_NOCTTY) {
        Ok(fd) => detach_controlling_terminal(fd.raw()),
        Err(_) => Ok(()),
    }
}

#[allow(dead_code)]
pub(crate) fn ttyfd() -> Result<Fd, IoError> {
    tty_open(OFlag::O_RDWR)
}
