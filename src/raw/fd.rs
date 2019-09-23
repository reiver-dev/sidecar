use std::io::{Error, Result};
use std::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd, RawFd};

#[derive(Debug)]
#[repr(transparent)]
pub struct Fd(RawFd);

impl Fd {
    pub fn new(fd: RawFd) -> Fd {
        Fd(fd)
    }

    pub fn raw(&self) -> RawFd {
        self.0
    }

    pub fn into_raw(self) -> RawFd {
        self.0
    }

    #[allow(dead_code)]
    pub fn forget(self) {
        //
    }

    #[allow(dead_code)]
    pub fn close(self) -> Result<()> {
        match unsafe { libc::close(self.into_raw()) } {
            0 => Ok(()),
            _ => Err(Error::last_os_error()),
        }
    }
}

impl Drop for Fd {
    fn drop(&mut self) {
        let _ = unsafe { libc::close(self.0) };
    }
}

impl AsRawFd for Fd {
    fn as_raw_fd(&self) -> RawFd {
        self.raw()
    }
}

impl FromRawFd for Fd {
    unsafe fn from_raw_fd(fd: RawFd) -> Self {
        Self::new(fd)
    }
}

impl IntoRawFd for Fd {
    fn into_raw_fd(self) -> RawFd {
        self.into_raw()
    }
}
