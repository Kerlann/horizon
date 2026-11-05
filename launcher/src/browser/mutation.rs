//! One process-wide lease for browser mutations, including recovery and enabled-list writes.
//! Take it before reading transaction state, not just around the final list save. Nested calls
//! on the owning thread share the lease; a guard cannot move to another thread.
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::{Condvar, Mutex, OnceLock};
use std::thread::{self, ThreadId};

// Deliberately covers all homes: aliases of the same home cannot bypass serialization, and
// installing a pack can call the ordinary installer for its profile without a second lock order.
#[derive(Default)]
struct State {
    owner: Option<ThreadId>,
    depth: usize,
}

fn shared() -> &'static (Mutex<State>, Condvar) {
    static SHARED: OnceLock<(Mutex<State>, Condvar)> = OnceLock::new();
    SHARED.get_or_init(|| (Mutex::new(State::default()), Condvar::new()))
}

pub(crate) struct Lease(PhantomData<Rc<()>>);

pub(crate) fn acquire() -> Lease {
    let me = thread::current().id();
    let (state, changed) = shared();
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    while state.owner.is_some_and(|owner| owner != me) {
        #[cfg(test)]
        WAITING.with(|callback| {
            if let Some(callback) = callback.borrow_mut().take() {
                callback();
            }
        });
        state = changed.wait(state).unwrap_or_else(|e| e.into_inner());
    }
    state.owner = Some(me);
    state.depth += 1;
    Lease(PhantomData)
}

impl Drop for Lease {
    fn drop(&mut self) {
        let (state, changed) = shared();
        let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
        state.depth -= 1;
        if state.depth == 0 {
            state.owner = None;
            changed.notify_all();
        }
    }
}

#[cfg(test)]
thread_local! {
    static WAITING: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = Default::default();
}

/// Lets interleaving tests observe actual contention, without sleeps or scheduler guesses.
#[cfg(test)]
pub(crate) fn on_wait(callback: impl FnOnce() + 'static) {
    WAITING.with(|waiting| *waiting.borrow_mut() = Some(Box::new(callback)));
}
