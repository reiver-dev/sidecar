use std::io::Error as IoError;
use std::os::unix::io::FromRawFd;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use tokio_process::{Child, CommandExt};

use crate::messages as msg;
use crate::net::RawFd;
use crate::system;

pub(crate) fn setup_command_streams(
    command: &mut Command,
    req: &msg::Io,
    fds: &[RawFd],
) {
    let mut i = 0;
    if req.stdin >= 0 {
        command.stdin(unsafe { Stdio::from_raw_fd(fds[i]) });
        i += 1;
    } else {
        command.stdin(Stdio::null());
    }

    if req.stdout >= 0 {
        command.stdout(unsafe { Stdio::from_raw_fd(fds[i]) });
        i += 1;
    } else {
        command.stdout(Stdio::null());
    }

    if req.stderr >= 0 {
        command.stderr(unsafe { Stdio::from_raw_fd(fds[i]) });
    } else {
        command.stderr(Stdio::null());
    }
}

pub(crate) fn setup_command(
    req: msg::ExecRequest,
    fds: &[RawFd],
) -> Result<Child, IoError> {
    use std::os::unix::process::CommandExt;

    let mut cmd = Command::new(&req.argv[0]);

    let startup_mode: msg::StartMode = req.startup;

    unsafe {
        cmd.pre_exec(move || {
            if startup_mode.contains(msg::StartMode::PROCESS_GROUP) {
                system::new_process_group()?
            }

            if startup_mode.contains(msg::StartMode::SESSION) {
                system::new_session()?
            }

            if startup_mode.contains(msg::StartMode::CONTROLLING_TERMINAL) {
                system::set_controlling_terminal()?
            }

            Ok(())
        });
    }

    if req.argv.len() > 1 {
        cmd.args(&req.argv[1..]);
    }

    if let Some(ios) = &req.io {
        setup_command_streams(&mut cmd, ios, &fds);
    } else {
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());
    }

    if let Some(envs) = req.env {
        cmd.envs(envs);
    }

    if let Some(cwd) = req.cwd {
        let pb: PathBuf = cwd.into();
        cmd.current_dir(pb);
    }

    cmd.spawn_async()
}
