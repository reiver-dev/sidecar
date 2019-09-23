use super::fd::Fd;
use mio::unix::EventedFd;
use mio::{Evented, Poll, PollOpt, Ready, Token};
use std::io::Result;

impl Evented for Fd {
    fn register(
        &self,
        poll: &Poll,
        token: Token,
        events: Ready,
        opts: PollOpt,
    ) -> Result<()> {
        EventedFd(&self.raw()).register(poll, token, events, opts)
    }

    fn reregister(
        &self,
        poll: &Poll,
        token: Token,
        events: Ready,
        opts: PollOpt,
    ) -> Result<()> {
        EventedFd(&self.raw()).reregister(poll, token, events, opts)
    }

    fn deregister(&self, poll: &Poll) -> Result<()> {
        EventedFd(&self.raw()).deregister(poll)
    }
}
