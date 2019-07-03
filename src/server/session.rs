use futures::prelude::*;
use futures::try_ready;
use std::error::Error;
use std::io::Error as IoError;

use log::{info, trace, warn};
use state_machine_future::{transition, RentToOwn, StateMachineFuture};
use tokio_process::Child;

use super::child::setup_command;
use super::process::Handler as ProcessHandler;
use crate::debug::opt;
use crate::messages as msg;
use crate::net::{RawFd, RecvMsg, Send};
use crate::system;

#[derive(StateMachineFuture)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum ClientSession {
    #[state_machine_future(start, transitions(Ready, Respond))]
    Start { fut: RecvMsg<Vec<u8>> },

    #[state_machine_future(ready)]
    Ready(()),

    #[state_machine_future(error)]
    Fail(IoError),

    #[state_machine_future(transitions(Ready, WaitChild))]
    Respond {
        fut: Send<Vec<u8>>,
        pg: bool,
        child: Option<Child>,
    },

    #[state_machine_future(transitions(Ready))]
    WaitChild { fut: ProcessHandler },
}

impl PollClientSession for ClientSession {
    fn poll_start<'a>(
        state: &'a mut RentToOwn<'a, Start>,
    ) -> Poll<AfterStart, IoError> {
        let res = try_ready!(state.fut.poll());
        state.take();

        let (sock, mut buf, fds, received, numfds) = res;
        let req: msg::Request = msg::decode_request(&buf[..received]);
        let fds: &[RawFd] = &fds[..numfds];

        match req {
            msg::Request::Stop => {
                system::raise(system::SIGINT)
                    .expect("failed to send SIGINT to self");
                transition!(Ready(()))
            }
            msg::Request::Exec(exec_request) => {
                let is_pg_leader = exec_request
                    .startup
                    .contains(msg::StartMode::PROCESS_GROUP);
                info!(
                    "process starting\n    \
                     args={:?}\n    \
                     cwd={:?}\n    \
                     env={:?}",
                    exec_request.argv,
                    opt(&exec_request.cwd),
                    opt(&exec_request.env)
                );
                let maybe_child = setup_command(exec_request, &fds);
                let response = match &maybe_child {
                    Ok(child) => {
                        info!("process={:?} started", child.id());
                        msg::StartedProcess {
                            success: true,
                            message: Some("success".to_owned()),
                            errno: 0,
                            pid: child.id() as i32,
                        }
                    }
                    Err(error) => {
                        warn!("process failed to start {:?}", error);
                        msg::StartedProcess {
                            success: false,
                            message: Some(error.description().to_owned()),
                            errno: error.raw_os_error().unwrap_or(-1),
                            pid: -1,
                        }
                    }
                };
                buf.clear();
                msg::encode_request(&mut buf, &response);
                transition!(Respond {
                    fut: Send::new(sock, buf),
                    pg: is_pg_leader,
                    child: maybe_child.ok(),
                })
            }
        }
    }

    fn poll_respond<'a>(
        state: &'a mut RentToOwn<'a, Respond>,
    ) -> Poll<AfterRespond, IoError> {
        let (sock, _buf) = try_ready!(state.fut.poll());
        let state = state.take();
        match state.child {
            Some(child) => transition!(WaitChild {
                fut: ProcessHandler::new(sock, state.pg, child)
            }),
            None => transition!(Ready(())),
        }
    }

    fn poll_wait_child<'a>(
        state: &'a mut RentToOwn<'a, WaitChild>,
    ) -> Poll<AfterWaitChild, IoError> {
        trace!("poll_wait_child");
        try_ready!(state.fut.poll());
        trace!("poll_wait_child completed");
        state.take();
        transition!(Ready(()))
    }
}
