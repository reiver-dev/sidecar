use std::future::Future;
use std::io::Error as IoError;
use tokio_executor::current_thread::{CurrentThread, TaskExecutor};
use tokio_executor::{spawn as _spawn, with_default as with_executor};
use tokio_net::driver::{
    set_default as default_reactor, Handle as RHandle, Reactor,
};
use tokio_timer::clock::{with_default as with_clock, Clock};
use tokio_timer::timer::{
    set_default as default_timer, Handle as THandle, Timer,
};

type Parker = Timer<Reactor>;

pub(crate) struct Runtime {
    reactor: RHandle,
    timer: THandle,
    clock: Clock,
    executor: CurrentThread<Parker>,
}

impl Runtime {
    pub(crate) fn new() -> Result<Runtime, IoError> {
        let r = Reactor::new()?;
        let reactor = r.handle();

        let clock = Clock::new();

        let t = Timer::new_with_now(r, clock.clone());
        let timer = t.handle();

        let executor = CurrentThread::new_with_park(t);

        Ok(Runtime {
            reactor,
            timer,
            clock,
            executor,
        })
    }

    pub(crate) fn spawn<F>(&mut self, future: F) -> &mut Self
    where
        F: Future<Output = ()> + 'static,
    {
        self.executor.spawn(future);
        self
    }

    pub(crate) fn block_on<F>(&mut self, future: F) -> F::Output
    where
        F: Future,
    {
        self.enter(|entered| entered.block_on(future))
    }

    fn enter<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut CurrentThread<Parker>) -> R,
    {
        let Self {
            ref reactor,
            ref timer,
            ref clock,
            ref mut executor,
        } = *self;

        let _r = default_reactor(&reactor);
        with_clock(clock, || {
            let _t = default_timer(&timer);
            let mut current = TaskExecutor::current();
            with_executor(&mut current, || f(executor))
        })
    }
}

pub(crate) fn start<T>(fut: impl Future<Output = T>) -> Result<T, IoError> {
    Runtime::new().map(|mut rt| rt.block_on(fut))
}

pub(crate) fn spawn(f: impl Future<Output = ()> + Send + 'static) {
    _spawn(f)
}
