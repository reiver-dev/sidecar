use libc;
use std::mem::MaybeUninit;

const PTHREAD_CANCEL_DISABLE: i32 = 1;

extern "C" {
    fn pthread_setcancelstate(
        state: libc::c_int,
        oldstate: *mut libc::c_int,
    ) -> libc::c_int;
}

struct ThreadCancelGuard {
    cancel_state: i32,
}

impl ThreadCancelGuard {
    fn new() -> Self {
        let mut cancel_state: i32 = 0;
        unsafe {
            pthread_setcancelstate(PTHREAD_CANCEL_DISABLE, &mut cancel_state);
        }
        ThreadCancelGuard { cancel_state }
    }

    #[allow(dead_code)]
    fn state(&self) -> i32 {
        self.cancel_state
    }
}

impl Drop for ThreadCancelGuard {
    fn drop(&mut self) {
        unsafe {
            pthread_setcancelstate(self.cancel_state, std::ptr::null_mut());
        }
    }
}

struct ThreadSignalGuard {
    sigmask: libc::sigset_t,
}

impl ThreadSignalGuard {
    fn new() -> Self {
        unsafe {
            let mut sigmask: libc::sigset_t =
                { MaybeUninit::zeroed().assume_init() };

            let sigall: libc::sigset_t = {
                let mut mask = MaybeUninit::uninit();
                libc::sigfillset(mask.as_mut_ptr());
                mask.assume_init()
            };

            libc::pthread_sigmask(libc::SIG_BLOCK, &sigall, &mut sigmask);

            ThreadSignalGuard { sigmask }
        }
    }

    #[allow(dead_code)]
    pub fn mask(&self) -> libc::sigset_t {
        self.sigmask
    }
}

impl Drop for ThreadSignalGuard {
    fn drop(&mut self) {
        unsafe {
            libc::pthread_sigmask(
                libc::SIG_SETMASK,
                &self.sigmask,
                std::ptr::null_mut(),
            );
        }
    }
}

pub struct ThreadGuard {
    cancel_state: ThreadCancelGuard,
    sigmask: ThreadSignalGuard,
}

impl ThreadGuard {
    pub fn new() -> Self {
        ThreadGuard {
            cancel_state: ThreadCancelGuard::new(),
            sigmask: ThreadSignalGuard::new(),
        }
    }

    #[allow(dead_code)]
    pub fn cancel_state(&self) -> i32 {
        self.cancel_state.state()
    }

    #[allow(dead_code)]
    pub fn signal_mask(&self) -> libc::sigset_t {
        self.sigmask.mask()
    }
}
