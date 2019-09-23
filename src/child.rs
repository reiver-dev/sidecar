use crate::child_watcher::{spawn, Child};
use crate::messages::{self as msg, Files, StartMode};
use crate::raw::{Fd, RawFd};
use crate::system;
use crate::tty;
use std::io::Error as IoError;
use std::os::unix::io::FromRawFd;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub(crate) fn setup_command_streams(
    command: &mut Command,
    req: Files,
    fds: &[RawFd],
) -> usize {
    let mut i = 0;
    if req.contains(Files::IN) {
        command.stdin(unsafe { Stdio::from_raw_fd(fds[i]) });
        i += 1;
    } else {
        command.stdin(Stdio::null());
    }

    if req.contains(Files::OUT) {
        command.stdout(unsafe { Stdio::from_raw_fd(fds[i]) });
        i += 1;
    } else {
        command.stdout(Stdio::null());
    }

    if req.contains(Files::ERR) {
        command.stderr(unsafe { Stdio::from_raw_fd(fds[i]) });
        i += 1;
    } else {
        command.stderr(Stdio::null());
    }

    i
}

#[cfg(target_os = "linux")]
fn kill_self_if_parent_exits(
    parent_pid: system::Pid,
    signal: system::Signal,
) -> Result<(), IoError> {
    system::set_death_signal(signal)?;
    if system::Pid::parent() != parent_pid {
        std::process::exit(128);
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn kill_self_if_parent_exits(
    _parent_pid: system::Pid,
    _signal: system::Signal,
) -> Result<(), IoError> {
    Ok(())
}

fn prepare(req: &msg::ProcessRequest, parent: system::Pid) -> Command {
    let mut cmd = Command::new(&req.program);
    cmd.args(req.argv);

    let startup_mode: msg::StartMode = req.startup;
    let deathsig = system::Signal::from_c_int(req.deathsig).ok();
    let pgid = system::Pid::from_raw(req.pgid);

    unsafe {
        cmd.pre_exec(move || {
            if cfg!(target_os = "linux") {
                if let Some(ds) = deathsig {
                    kill_self_if_parent_exits(parent, ds)?;
                }
            }

            if startup_mode.contains(StartMode::DETACH_TERMINAL) {
                tty::disconnect_controlling_terminal()?;
            }

            if startup_mode.contains(StartMode::PROCESS_GROUP) {
                system::new_process_group(pgid)?;
            }

            if startup_mode.contains(StartMode::SESSION) {
                system::new_session()?
            }

            if startup_mode.contains(StartMode::NOHUP) {
                system::nohup()?
            }

            Ok(())
        });
    }

    if !req.env.is_empty() {
        for (k, v) in req.env {
            cmd.env(k, v);
        }
    }

    if !req.cwd.is_empty() {
        let pb: PathBuf = req.cwd.into();
        cmd.current_dir(pb);
    }

    if req.uid >= 0 {
        cmd.uid(req.uid as u32);
    }

    if req.gid >= 0 {
        cmd.gid(req.gid as u32);
    }

    cmd
}

pub(crate) fn execute_into(req: &msg::ProcessRequest) -> IoError {
    prepare(req, system::Pid::parent()).exec()
}

pub(crate) fn setup_command(
    req: &msg::ProcessRequest,
    fds: &[RawFd],
) -> Result<Child, IoError> {
    let mut cmd = prepare(req, system::Pid::this());

    let numfds = if !req.io.is_empty() {
        setup_command_streams(&mut cmd, req.io, &fds)
    } else {
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());
        0
    };

    for _ in fds.iter().skip(numfds).cloned().map(Fd::new) {
        //
    }

    spawn(cmd)
}
