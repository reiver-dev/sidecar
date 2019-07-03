use log::{error, warn};
use nix::libc::{ioctl, STDIN_FILENO, TIOCSCTTY};
use nix::sys::signal::{kill as _kill, raise as _raise};
use nix::unistd::{setpgid, setsid, Pid};
use nix::Error as NixError;
use std::io::{Error, Result};

pub(crate) use nix::sys::signal::Signal::{self, *};

fn error(err: NixError) -> Error {
    match err {
        NixError::Sys(val) => val.into(),
        _ => unreachable!(),
    }
}

pub(crate) fn raise(sig: Signal) -> Result<()> {
    _raise(sig).map_err(error)
}

pub(crate) fn set_controlling_terminal() -> Result<()> {
    if unsafe { ioctl(STDIN_FILENO, TIOCSCTTY, 0) } != 0 {
        Err(Error::last_os_error())
    } else {
        Ok(())
    }
}

pub(crate) fn new_process_group() -> Result<()> {
    setpgid(Pid::from_raw(0), Pid::from_raw(0)).map_err(error)
}

pub(crate) fn new_session() -> Result<()> {
    setsid().map(|_| ()).map_err(error)
}

pub(crate) fn kill(child: u32, signal: Signal) {
    let pid = Pid::from_raw(child as i32);
    if let Err(err) = _kill(pid, signal) {
        error!("failed to send signal to process={:?} err={:?}", pid, err)
    }
}

pub(crate) fn killpg(child: u32, signal: Signal) {
    use nix::{errno::Errno, Error::Sys};

    let pid = Pid::from_raw(child as i32);
    // On Linux, killpg() is implemented as a library function that makes
    // the call kill(-pgrp, sig).
    let pg = Pid::from_raw(-(child as i32));

    match _kill(pg, signal) {
        Err(Sys(Errno::ESRCH)) => {
            warn!(
                "failed to send signal to group process={:?} no such group",
                pid
            );
            if let Err(err) = _kill(pid, signal) {
                error!(
                    "failed to send signal to process={:?} err={:?}",
                    pid, err
                );
            }
        }
        Err(err) => {
            warn!(
                "failed to send signal to group process={:?} err={:?}",
                pid, err
            );
        }
        _ => (),
    };
}
