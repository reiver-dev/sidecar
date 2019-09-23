use std::future::Future;
use std::io::Result;
use std::mem;
use std::os::unix::io::AsRawFd;
use std::pin::Pin;
use std::task::{Context, Poll};

use libc::siginfo_t;
use nix::sys::signal::Signal::{self, *};
use nix::unistd;
use signal_hook_registry::{register_sigaction, unregister, SigId};

use crate::pipe;
use crate::raw::{invalid_argument, nixerror, RawFd};

type SigVal = libc::c_int;
const SIGSZ: usize = mem::size_of::<SigVal>();

fn sig_to_buf(val: SigVal) -> [u8; SIGSZ] {
    unsafe { mem::transmute_copy(&val) }
}

fn buf_to_sig(val: [u8; SIGSZ]) -> SigVal {
    unsafe { mem::transmute_copy(&val) }
}

fn make_callback(wraw: RawFd) -> impl Fn(&siginfo_t) + Send + Sync {
    move |info: &siginfo_t| {
        let _ = unistd::write(wraw, &sig_to_buf(info.si_signo));
    }
}

pub struct SignalHandler {
    actions: Vec<SigId>,
    _read: pipe::PipeRead,
    _write: pipe::PipeWrite,
}

impl SignalHandler {
    pub fn new() -> Result<Self> {
        let (r, w) = pipe::make_pipe()?;
        let mut actions = Vec::new();

        for sig in Signal::iterator() {
            match sig {
                SIGKILL | SIGSTOP | SIGILL | SIGFPE | SIGSEGV => continue,
                value => {
                    let sigval = value as libc::c_int;
                    let callback = make_callback(w.as_raw_fd());
                    let sigid =
                        unsafe { register_sigaction(sigval, callback) }?;
                    actions.push(sigid);
                }
            }
        }

        Ok(SignalHandler {
            actions,
            _read: r,
            _write: w,
        })
    }

    pub fn wait(&self) -> WaitSignal {
        WaitSignal::new(&self._read)
    }
}

pub struct WaitSignal<'a> {
    inner: &'a pipe::PipeRead,
}

impl<'a> WaitSignal<'a> {
    fn new(pipe: &'a pipe::PipeRead) -> Self {
        Self { inner: pipe }
    }

    fn do_poll(&mut self, ctx: &mut Context<'_>) -> Poll<Result<Signal>> {
        let mut buf = [0u8; mem::size_of::<libc::c_int>()];
        match self.inner.poll_read(&mut buf, ctx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(val) => Poll::Ready(match val {
                Ok(SIGSZ) => {
                    Signal::from_c_int(buf_to_sig(buf)).map_err(nixerror)
                }
                Ok(_) => Err(invalid_argument()),
                Err(err) => Err(err),
            }),
        }
    }
}

impl<'a> Future for WaitSignal<'a> {
    type Output = Result<Signal>;

    fn poll(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
    ) -> Poll<Self::Output> {
        self.get_mut().do_poll(ctx)
    }
}

impl Drop for SignalHandler {
    fn drop(&mut self) {
        self.actions.iter().for_each(|s| {
            unregister(*s);
        });
    }
}
