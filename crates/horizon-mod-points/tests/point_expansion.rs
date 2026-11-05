//! `#[point]` at run time: methods with the attribute, and the slot table, records and listener
//! list the expansion finds in `crate::game::points`, declared here in place of the game's.
use horizon_mod_api_macros::point;
use mod_api::ServerHost;
use std::sync::atomic::Ordering;

struct Counter {
    total: usize,
}

impl mod_api::Host for Counter {}
impl ServerHost for Counter {}

mod game {
    pub mod points {
        use mod_api::ServerHost;
        use std::sync::Mutex;
        use std::sync::atomic::AtomicBool;

        /// A native width crosses as 64 bits, as a wasm guest sees it.
        pub struct AddArgs {
            pub step: u64,
            pub result: Option<u64>,
        }

        impl AddArgs {
            pub fn __new(step: usize) -> Self {
                Self {
                    step: step as u64,
                    result: None,
                }
            }
        }

        pub struct ResetArgs {
            pub cancel: bool,
        }

        impl ResetArgs {
            pub fn __new() -> Self {
                Self { cancel: false }
            }
        }

        pub struct SlotArgs {}

        impl SlotArgs {
            pub fn __new() -> Self {
                Self {}
            }
        }

        #[derive(Clone, Copy)]
        pub enum PointId {
            Add,
            AddReturn,
            Reset,
            ResetReturn,
            Slot,
        }

        pub static POINTS: [AtomicBool; 5] = [const { AtomicBool::new(false) }; 5];

        /// The listener the test installs: its own `Points`.
        pub trait Points: Sync {
            fn add(&self, _: &mut dyn ServerHost, _: &mut AddArgs) {}
            fn add_return(&self, _: &mut dyn ServerHost, _: &mut AddArgs) {}
            fn reset(&self, _: &mut dyn ServerHost, _: &mut ResetArgs) {}
            fn reset_return(&self, _: &mut dyn ServerHost, _: &mut ResetArgs) {}
            fn slot(&self, _: &mut dyn ServerHost, _: &mut SlotArgs) {}
        }

        pub static LISTENER: Mutex<Option<&'static dyn Points>> = Mutex::new(None);
        pub static HEARD: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

        pub fn fire(_: PointId, name: &'static str, call: &mut dyn FnMut(&'static dyn Points)) {
            HEARD.lock().unwrap().push(name);
            let listener = *LISTENER.lock().unwrap();
            listener.into_iter().for_each(call);
        }
    }
}

impl Counter {
    #[point(side = "server")]
    fn add(&mut self, step: usize) -> usize {
        self.total += step;
        self.total
    }

    #[point(side = "server")]
    fn reset(&mut self) {
        self.total = 0;
    }

    /// Its value borrows the counter: a head and no return.
    #[point(side = "server")]
    fn slot(&mut self) -> &mut usize {
        &mut self.total
    }
}

struct Listener;

impl game::points::Points for Listener {
    // Doubles the step, or answers 7 for a step of 100.
    fn add(&self, _: &mut dyn ServerHost, args: &mut game::points::AddArgs) {
        match args.step {
            100 => args.result = Some(7),
            _ => args.step *= 2,
        }
    }
    fn add_return(&self, _: &mut dyn ServerHost, args: &mut game::points::AddArgs) {
        args.result = args.result.map(|value| value + 1000);
    }
    fn reset(&self, _: &mut dyn ServerHost, args: &mut game::points::ResetArgs) {
        args.cancel = true;
    }
}

#[test]
fn slots_change_arguments_stop_the_function_and_change_its_value() {
    use game::points::{HEARD, LISTENER, POINTS, PointId};
    let mut counter = Counter { total: 0 };
    // Empty slots: the function runs as written, and no listener list is asked for.
    assert_eq!(counter.add(1), 1);
    counter.reset();
    *counter.slot() = 0;
    assert!(HEARD.lock().unwrap().is_empty());

    *LISTENER.lock().unwrap() = Some(&Listener);
    for id in [
        PointId::Add,
        PointId::AddReturn,
        PointId::Reset,
        PointId::Slot,
    ] {
        POINTS[id as usize].store(true, Ordering::Release);
    }
    // The doubled step reaches the body, and the return listener changes the value.
    assert_eq!(counter.add(3), 1006);
    assert_eq!(counter.total, 6);
    // A head result returns at once: no body, no return place.
    assert_eq!(counter.add(100), 7);
    assert_eq!(counter.total, 6);
    // A head cancel skips the body; its unset return slot isn't asked.
    counter.reset();
    assert_eq!(counter.total, 6);
    *counter.slot() += 1;
    assert_eq!(counter.total, 7);
    assert_eq!(
        *HEARD.lock().unwrap(),
        ["add", "add_return", "add", "reset", "slot"]
    );
}
