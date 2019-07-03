use std::io::Error as IoError;
use std::mem::replace;

use log::{debug, info, warn};

use futures::prelude::*;
use tokio_process::Child;

use crate::messages as msg;
use crate::net::{Ready as EvReady, Shutdown, UnixPacket};
use crate::system;

fn pass_signal(pid: u32, mut sigval: i32, pg_leader: bool) {
    let send_to_group = if sigval < 0 {
        sigval = -sigval;
        true
    } else {
        false
    };

    match system::Signal::from_c_int(sigval) {
        Ok(sig) => {
            if send_to_group && pg_leader {
                debug!("process={:?} received group signal={:?}", pid, sig);
                system::killpg(pid, sig);
            } else {
                debug!("process={:?} received signal={:?}", pid, sig);
                system::kill(pid, sig);
            }
        }
        Err(_) => {
            warn!("invalid signal value {:?}", sigval);
            return;
        }
    }
}

struct Waiting {
    sock: UnixPacket,
    child: Child,
}

struct Reaping {
    child: Child,
}

struct SendingResult {
    sock: UnixPacket,
    data: [u8; 4],
}

#[allow(clippy::large_enum_variant)]
enum State {
    Done,
    Waiting(Waiting),
    Reaping(Reaping),
    SendingResult(SendingResult),
}

impl State {
    pub fn name(&self) -> &'static str {
        use State::*;
        match self {
            Done => "Done",
            Waiting(_) => "Waiting",
            Reaping(_) => "Reaping",
            SendingResult(_) => "SendingResult",
        }
    }
}

macro_rules! into_enum {
    ($name:ident, $member:ident) => {
        impl Into<$name> for $member {
            fn into(self) -> $name {
                $name::$member(self)
            }
        }
    };
}

into_enum!(State, Waiting);
into_enum!(State, Reaping);
into_enum!(State, SendingResult);

#[allow(clippy::large_enum_variant)]
enum Res {
    Ready(State),
    NotReady(State),
    Fail(IoError),
}

use Res::*;

pub(crate) struct Handler {
    pid: u32,
    pg: bool,
    state: State,
}

impl Handler {
    pub fn new(sock: UnixPacket, pg_leader: bool, child: Child) -> Handler {
        Handler {
            pid: child.id(),
            pg: pg_leader,
            state: Waiting { sock, child }.into(),
        }
    }

    fn poll_reaping(mut state: Reaping) -> Res {
        match state.child.poll() {
            Ok(Async::Ready(_)) => Ready(State::Done),
            Ok(Async::NotReady) => NotReady(state.into()),
            Err(err) => Fail(err),
        }
    }

    fn poll_send_result(state: SendingResult) -> Res {
        match state.sock.poll_send(&state.data) {
            Ok(Async::Ready(_)) => Ready(State::Done),
            Ok(Async::NotReady) => NotReady(state.into()),
            Err(err) => Fail(err),
        }
    }

    fn poll_waiting(mut state: Waiting, pg_leader: bool) -> Res {
        let pid = state.child.id();
        let mut buf = [0; 4];
        let slc = &mut buf;
        match state.sock.poll_recv(slc) {
            Err(err) => {
                warn!("process={:?} client error={:?}", pid, err);
                if pg_leader {
                    system::killpg(pid, system::SIGKILL);
                } else {
                    system::kill(pid, system::SIGKILL);
                }
                Ready(Reaping { child: state.child }.into())
            }
            Ok(Async::Ready(0)) => {
                warn!("process={:?} client disconnected", pid);
                if pg_leader {
                    system::killpg(pid, system::SIGKILL);
                } else {
                    system::kill(pid, system::SIGKILL);
                }
                Ready(Reaping { child: state.child }.into())
            }
            Ok(Async::Ready(size)) => {
                let req: msg::Signal = msg::decode_request(&buf[..size]);
                pass_signal(pid, req.0, pg_leader);
                if let Err(err) =
                    state.sock.clear_read_ready(EvReady::readable())
                {
                    return Fail(err);
                }
                Ready(
                    Waiting {
                        sock: state.sock,
                        child: state.child,
                    }
                    .into(),
                )
            }
            Ok(Async::NotReady) => match state.child.poll() {
                Err(err) => {
                    warn!("process={:?} wait error={:?}", pid, err);
                    Fail(err)
                }
                Ok(Async::Ready(status)) => {
                    let code = status.code().unwrap_or(0);
                    info!("process={:?} exited code={:?}", pid, code);
                    if let Err(err) = state.sock.shutdown(Shutdown::Read) {
                        warn!(
                            "process={:?} failed to shutdown read: {:?}",
                            pid, err
                        );
                    };
                    let mut buf = [0; 4];
                    let mut sl: &mut [u8] = &mut buf;
                    msg::encode_request(&mut sl, &msg::RetCode(code));
                    Ready(
                        SendingResult {
                            sock: state.sock,
                            data: buf,
                        }
                        .into(),
                    )
                }
                Ok(Async::NotReady) => NotReady(state.into()),
            },
        }
    }
}

impl Future for Handler {
    type Item = ();
    type Error = IoError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        use State::*;
        debug!("process={:?} state={:?}", self.pid, self.state.name());

        let state = replace(&mut self.state, Done);

        let res = match state {
            Done => panic!("ProcessHandler future called twice!"),
            Waiting(state) => Handler::poll_waiting(state, self.pg),
            Reaping(state) => Handler::poll_reaping(state),
            SendingResult(state) => Handler::poll_send_result(state),
        };

        match res {
            Ready(Done) | NotReady(Done) => Ok(Async::Ready(())),
            Ready(state) => {
                debug!("process={:?} transition={:?}", self.pid, state.name());
                self.state = state;
                Ok(Async::NotReady)
            }
            NotReady(state) => {
                debug!(
                    "process={:?} transition={:?} (not ready)",
                    self.pid,
                    state.name()
                );
                self.state = state;
                Ok(Async::NotReady)
            }
            Fail(err) => Err(err),
        }
    }
}
