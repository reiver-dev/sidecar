use log::{error, warn};
use nix::errno::Errno;
use nix::sys::signal::{kill as _kill, killpg as _killpg, raise as _raise};
use nix::unistd::{setpgid, setsid};
use nix::Error as NixError;
use std::io::Error as IoError;

use crate::raw;
use crate::raw::nixerror as error;
pub(crate) use nix::sys::signal::Signal::{self, *};
pub(crate) use nix::unistd::Pid;

#[cfg(target_os = "linux")]
fn prctl(
    option: libc::c_int,
    arg2: libc::c_ulong,
    arg3: libc::c_ulong,
    arg4: libc::c_ulong,
    arg5: libc::c_ulong,
) -> Result<(), IoError> {
    let res = unsafe { libc::prctl(option, arg2, arg3, arg4, arg5) };
    Errno::result(res).map(drop).map_err(error)
}

#[cfg(target_os = "linux")]
pub(crate) fn set_death_signal(sig: Signal) -> Result<(), IoError> {
    const PR_SET_PDEATHSIG: libc::c_int = 1;
    let sigval = sig as libc::c_ulong;
    prctl(PR_SET_PDEATHSIG, sigval, 0, 0, 0)
}

pub(crate) fn signal_from_str(text: &str) -> Result<Signal, IoError> {
    match text.parse::<u32>() {
        Ok(signum) => Signal::from_c_int(signum as libc::c_int),
        Err(_) => text.parse::<Signal>(),
    }
    .map_err(error)
}

pub(crate) fn raise(sig: Signal) -> Result<(), IoError> {
    _raise(sig).map_err(error)
}

pub(crate) fn new_process_group(id: Pid) -> Result<(), IoError> {
    setpgid(Pid::from_raw(0), id).map_err(error)
}

pub(crate) fn new_session() -> Result<(), IoError> {
    setsid().map(|_| ()).map_err(error)
}

pub(crate) fn nohup() -> Result<(), IoError> {
    match unsafe { libc::signal(libc::SIGHUP, libc::SIG_IGN) } {
        libc::SIG_ERR => Err(IoError::last_os_error()),
        _ => Ok(()),
    }
}

pub(crate) fn kill(child: Pid, signal: Signal) {
    if let Err(err) = _kill(child, signal) {
        error!(
            "failed to send signal to process={:?} err={:?}",
            child.as_raw(),
            err
        )
    }
}

pub(crate) fn killpg(child: Pid, signal: Signal) {
    match _killpg(child, signal) {
        Err(NixError::Sys(Errno::ESRCH)) => {
            warn!(
                "failed to send signal to group process={:?} no such group",
                child.as_raw()
            );
            if let Err(err) = _kill(child, signal) {
                error!(
                    "failed to send signal to process={:?} err={:?}",
                    child.as_raw(),
                    err
                );
            }
        }
        Err(err) => {
            warn!(
                "failed to send signal to group process={:?} err={}",
                child.as_raw(),
                err
            );
        }
        _ => (),
    };
}

pub(crate) fn is_valid_fd(fd: raw::RawFd) -> bool {
    let ret = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    ret != -1 || nix::errno::errno() != libc::EBADF
}

pub(crate) fn disable_inherit_stdio() -> Result<(), IoError> {
    for fd in &[libc::STDIN_FILENO, libc::STDOUT_FILENO, libc::STDERR_FILENO] {
        raw::flags::set_cloexec(*fd)?;
    }
    Ok(())
}
