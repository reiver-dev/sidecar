use std::collections::HashMap;
use std::future::Future;
use std::io::{Result, Write};
use std::mem;
use std::os::unix::process::ExitStatusExt;
use std::pin::Pin;
use std::process::{Command, ExitStatus};
use std::sync::Mutex;
use std::task::{Context, Poll};

use lazy_static::lazy_static;
use mio_uds::UnixStream;
use tokio::io::AsyncRead;
use tokio::io::PollEvented;
use tokio::signal::unix::{signal, Signal, SignalKind};

use nix::errno::Errno;

use crate::guards::ThreadGuard;

pub fn signal_queue() -> Result<Signal> {
    signal(SignalKind::child())
}

struct WatchData {
    storage: HashMap<i32, UnixStream>,
}

struct Watchers {
    inner: Mutex<WatchData>,
}

impl Watchers {
    fn new() -> Watchers {
        Watchers {
            inner: Mutex::new(WatchData {
                storage: HashMap::new(),
            }),
        }
    }

    pub fn register(&self, pid: i32) -> UnixStream {
        let mut dt = self.inner.lock().unwrap();
        let (receiver, sender) =
            UnixStream::pair().expect("failed to create UnixStream");
        dt.storage.insert(pid, sender);
        receiver
    }

    pub fn notify(&self, pid: i32, status: i32) {
        let mut dt = self.inner.lock().unwrap();
        if let Some(fd) = dt.storage.remove(&pid) {
            send(fd, status);
        }
    }
}

pub struct Child {
    pid: i32,
    event: PollEvented<UnixStream>,
}

impl Child {
    pub fn from_id(pid: i32) -> Child {
        Child {
            pid: pid,
            event: (PollEvented::new(watchers().register(pid)).unwrap()),
        }
    }

    pub fn id(&self) -> i32 {
        self.pid
    }
}

const MSG_SIZE: usize = mem::size_of::<i32>();

impl Future for Child {
    type Output = Result<ExitStatus>;

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Self::Output> {
        let mut data = [0; 128];

        match Pin::new(&mut self.event).poll_read(cx, &mut data) {
            Poll::Ready(Ok(0)) => panic!("EOF on self-pipe"),
            Poll::Ready(Ok(MSG_SIZE)) => {
                let status: i32 = unsafe {
                    let mut d: [u8; MSG_SIZE] = [0, 0, 0, 0];
                    d.copy_from_slice(&data[..MSG_SIZE]);
                    mem::transmute(d)
                };
                Poll::Ready(Ok(ExitStatus::from_raw(status)))
            }
            Poll::Ready(Ok(sz)) => panic!(
                "Unexpected self-pipe message received: {:?}",
                &data[..sz]
            ),
            Poll::Ready(Err(e)) => panic!("Bad read on self-pipe: {}", e),
            Poll::Pending => Poll::Pending,
        }
    }
}

fn send(mut stream: UnixStream, status: i32) {
    let data: [u8; MSG_SIZE] = unsafe { mem::transmute(status) };
    drop(stream.write(&data));
}

fn watchers() -> Pin<&'static Watchers> {
    lazy_static! {
        static ref GLOBALS: Pin<Box<Watchers>> =
            Pin::new(Box::new(Watchers::new()));
    }

    GLOBALS.as_ref()
}

pub fn spawn(mut command: Command) -> Result<Child> {
    let _ = ThreadGuard::new();
    let child = command.spawn()?;
    Ok(Child::from_id(child.id() as i32))
}

pub async fn listen(mut sig: Signal) {
    let w = watchers();

    while let Some(()) = sig.recv().await {
        loop {
            let mut status: libc::c_int = 0;

            let res = unsafe {
                libc::waitpid(
                    -1,
                    &mut status as *mut libc::c_int,
                    libc::WNOHANG,
                )
            };

            match res {
                -1 => match Errno::last() {
                    Errno::EINTR => continue,
                    Errno::ECHILD => break,
                    err => panic!("waitpid failed: {}", err),
                },
                0 => break,
                pid => w.as_ref().notify(pid, status),
            }
        }
    }
}
