use super::RawFd;
use libc::{
    fcntl, FD_CLOEXEC, F_GETFD, F_GETFL, F_SETFD, F_SETFL, O_NONBLOCK,
};
use nix::errno::errno;
use std::io::{Error, Result};

pub fn set_cloexec(fd: RawFd) -> Result<()> {
    unsafe {
        let previous = match fcntl(fd, F_GETFD) {
            -1 => return Err(Error::from_raw_os_error(errno())),
            other => other,
        };

        let new = previous | FD_CLOEXEC;

        if new != previous {
            if let -1 = fcntl(fd, F_SETFD, new) {
                return Err(Error::from_raw_os_error(errno()));
            }
        }

        Ok(())
    }
}

pub fn set_nonblock(fd: RawFd) -> Result<()> {
    unsafe {
        let previous = match fcntl(fd, F_GETFL) {
            -1 => return Err(Error::from_raw_os_error(errno())),
            other => other,
        };

        let new = previous | O_NONBLOCK;

        if new != previous {
            if let -1 = fcntl(fd, F_SETFL, new) {
                return Err(Error::from_raw_os_error(errno()));
            }
        }

        Ok(())
    }
}

pub fn set_block(fd: RawFd) -> Result<()> {
    unsafe {
        let previous = match fcntl(fd, F_GETFL) {
            -1 => return Err(Error::from_raw_os_error(errno())),
            other => other,
        };

        let new = previous & !O_NONBLOCK;

        if new != previous {
            if let -1 = fcntl(fd, F_SETFL, new) {
                return Err(Error::from_raw_os_error(errno()));
            }
        }

        Ok(())
    }
}

pub fn set_cloexec_nonblocking(fd: RawFd) -> Result<()> {
    set_cloexec(fd)?;
    set_nonblock(fd)
}

#[allow(dead_code)]
pub fn set_cloexec_blocking(fd: RawFd) -> Result<()> {
    set_cloexec(fd)?;
    set_block(fd)
}
