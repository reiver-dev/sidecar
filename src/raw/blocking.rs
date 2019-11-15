use std::io::Result;
use std::path::Path;

use super::flags;
use super::{nixerror, Fd};

use nix::sys::socket::{self, AddressFamily, SockAddr, SockFlag, SockType};

#[cfg(not(target_os = "linux"))]
fn new() -> Result<Fd> {
    let fd = socket::socket(
        AddressFamily::Unix,
        SockType::SeqPacket,
        SockFlag::empty(),
        None,
    )
    .map_err(nixerror)
    .map(Fd::new)?;
    flags::set_cloexec(fd.raw())?;
    Ok(fd)
}

#[cfg(target_os = "linux")]
fn new() -> Result<Fd> {
    socket::socket(
        AddressFamily::Unix,
        SockType::SeqPacket,
        SockFlag::SOCK_CLOEXEC,
        None,
    )
    .map_err(nixerror)
    .map(Fd::new)
}

pub fn bind(path: &Path) -> Result<Fd> {
    let addr = SockAddr::new_unix(path).map_err(nixerror)?;
    let fd = new()?;
    socket::bind(fd.raw(), &addr).map_err(nixerror)?;
    socket::listen(fd.raw(), 0).map_err(nixerror)?;
    flags::set_nonblock(fd.raw())?;
    Ok(fd)
}

pub fn connect(path: &Path) -> Result<Fd> {
    let addr = SockAddr::new_unix(path).map_err(nixerror)?;
    let fd = new()?;
    socket::connect(fd.raw(), &addr).map_err(nixerror)?;
    flags::set_nonblock(fd.raw())?;
    Ok(fd)
}
