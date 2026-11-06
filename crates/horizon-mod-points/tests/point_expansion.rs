//! `#[point]` at run time: functions with the attribute, and the slot table, records and listener
//! lists the expansion finds in `mod_points`, declared here in place of the crate's. The raw
//! records and listener types are written out as the staging pass declares them beside each
//! function.
use horizon_mod_api_macros::point;
use mod_api::ServerHost;
use std::sync::atomic::Ordering;

pub struct Counter {
    total: usize,
}

impl mod_api::Host for Counter {}
impl ServerHost for Counter {}

/// A type off `Game`: its points have no host.
pub struct Pen {
    tags: Vec<String>,
}

mod mod_points {
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

    pub struct PenLabelArgs<'a> {
        pub name: std::borrow::Cow<'a, str>,
    }

    impl<'a> PenLabelArgs<'a> {
        pub fn __new(name: &'a str) -> Self {
            Self {
                name: std::borrow::Cow::Borrowed(name),
            }
        }
    }

    pub struct DoubleArgs {
        pub x: i64,
        pub result: Option<i64>,
    }

    impl DoubleArgs {
        pub fn __new(x: i64) -> Self {
            Self { x, result: None }
        }
    }

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub enum PointId {
        Add,
        AddReturn,
        Reset,
        ResetReturn,
        Slot,
        SlotReturn,
        PenLabel,
        PenLabelReturn,
        Double,
        DoubleReturn,
    }

    /// The listener the test installs: its own `Points`.
    pub trait Points: Sync {
        fn add(&self, _: &mut dyn ServerHost, _: &mut AddArgs) {}
        fn add_return(&self, _: &mut dyn ServerHost, _: &mut AddArgs) {}
        fn reset(&self, _: &mut dyn ServerHost, _: &mut ResetArgs) {}
        fn reset_return(&self, _: &mut dyn ServerHost, _: &mut ResetArgs) {}
        fn slot(&self, _: &mut dyn ServerHost, _: &mut SlotArgs) {}
        fn pen_label(&self, _: &mut PenLabelArgs<'_>) {}
        fn pen_label_return(&self, _: &mut PenLabelArgs<'_>) {}
        fn double(&self, _: &mut DoubleArgs) {}
        fn double_return(&self, _: &mut DoubleArgs) {}
    }

    pub static LISTENER: Mutex<Option<&'static dyn Points>> = Mutex::new(None);
    pub static RAW: Mutex<Vec<(PointId, __private::RawKind)>> = Mutex::new(Vec::new());
    pub static HEARD: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

    pub mod __private {
        use super::*;
        pub use ::mod_points::__private::RawKind;

        pub static POINTS: [AtomicBool; 10] = [const { AtomicBool::new(false) }; 10];

        pub fn fire(_: PointId, name: &'static str, call: &mut dyn FnMut(&'static dyn Points)) {
            HEARD.lock().unwrap().push(name);
            let listener = *LISTENER.lock().unwrap();
            listener.into_iter().for_each(call);
        }

        pub fn fire_raw(place: PointId, name: &'static str, call: &mut dyn FnMut(RawKind)) {
            let functions: Vec<_> = RAW
                .lock()
                .unwrap()
                .iter()
                .filter(|(at, _)| *at == place)
                .map(|(_, function)| *function)
                .collect();
            if !functions.is_empty() {
                HEARD.lock().unwrap().push(name);
            }
            for function in functions {
                call(function);
            }
        }
    }
}

impl Counter {
    #[point(side = "server", raw)]
    fn add(&mut self, step: usize) -> usize {
        self.total += step;
        self.total
    }

    #[point(side = "server")]
    fn reset(&mut self) {
        self.total = 0;
    }

    /// Its value borrows the counter: a head, and a raw-only return that gets the value.
    #[point(side = "server", raw)]
    fn slot(&mut self) -> &mut usize {
        &mut self.total
    }
}

// What the staging pass declares after `impl Counter`.
pub struct __AddRaw<'r, T0, R> {
    pub step: &'r mut T0,
    pub result: &'r mut Option<R>,
}
pub type __AddListener = for<'r> fn(&'r mut Counter, &'r mut __AddRaw<'r, usize, usize>);
pub struct __SlotRaw {}
pub type __AddReturnListener = for<'r> fn(&'r mut Counter, &'r mut usize);
pub type __SlotListener = for<'r> fn(&'r mut Counter, &'r mut __SlotRaw);
pub type __SlotReturnListener = for<'r> fn(&'r mut &mut usize);

impl Pen {
    #[point(name = "pen_label", raw)]
    fn label(&self, name: &str, mut tags: Vec<String>) -> Vec<String> {
        tags.push(name.to_owned());
        tags.extend(self.tags.iter().cloned());
        tags
    }
}

// What the staging pass declares after `impl Pen`.
pub struct __PenLabelRaw<'r, T0, T1, R> {
    pub name: &'r mut T0,
    pub tags: &'r mut T1,
    pub result: &'r mut Option<R>,
}
pub type __PenLabelListener = for<'r> fn(
    &'r Pen,
    &'r mut __PenLabelRaw<'r, std::borrow::Cow<'_, str>, Vec<String>, Vec<String>>,
);
pub type __PenLabelReturnListener = for<'r> fn(&'r Pen, &'r mut Vec<String>);

#[point(name = "double", raw, free)]
fn double(x: i64) -> i64 {
    x * 2
}

// What the staging pass declares after `fn double`.
pub struct __DoubleRaw<'r, T0, R> {
    pub x: &'r mut T0,
    pub result: &'r mut Option<R>,
}
pub type __DoubleListener = for<'r> fn(&'r mut __DoubleRaw<'r, i64, i64>);
pub type __DoubleReturnListener = for<'r> fn(&'r mut i64);

struct Listener;

impl mod_points::Points for Listener {
    // Doubles the step, or answers 7 for a step of 100.
    fn add(&self, _: &mut dyn ServerHost, args: &mut mod_points::AddArgs) {
        match args.step {
            100 => args.result = Some(7),
            _ => args.step *= 2,
        }
    }
    fn add_return(&self, _: &mut dyn ServerHost, args: &mut mod_points::AddArgs) {
        args.result = args.result.map(|value| value + 1000);
    }
    fn reset(&self, _: &mut dyn ServerHost, args: &mut mod_points::ResetArgs) {
        args.cancel = true;
    }
    fn pen_label(&self, args: &mut mod_points::PenLabelArgs<'_>) {
        args.name = format!("{}!", args.name).into();
    }
}

/// A raw listener after the API one: it sees the API's change and reads the receiver.
fn raw_add(counter: &mut Counter, raw: &mut __AddRaw<'_, usize, usize>) {
    *raw.step += counter.total;
}

fn raw_add_return(_: &mut Counter, value: &mut usize) {
    *value += 10;
}

/// The borrowed value itself: the listener writes through it.
fn raw_slot_return(slot: &mut &mut usize) {
    **slot += 50;
}

/// An internal-typed argument changed at the head.
fn raw_label(
    pen: &Pen,
    raw: &mut __PenLabelRaw<'_, std::borrow::Cow<'_, str>, Vec<String>, Vec<String>>,
) {
    raw.tags.push(format!("{} tags", pen.tags.len()));
}

/// A value the API can't carry, changed at the return.
fn raw_label_return(_: &Pen, value: &mut Vec<String>) {
    value.push("returned".into());
}

/// A free function's head: answers at once for a negative argument.
fn raw_double(raw: &mut __DoubleRaw<'_, i64, i64>) {
    if *raw.x < 0 {
        *raw.result = Some(0);
    }
}

/// Erased callbacks interpret the pointers with the exact types for their registered place,
/// use them only during the call, and keep shared receivers read-only.
unsafe fn erased_add(place: usize, receiver: *mut (), raw: *mut ()) {
    assert_eq!(place, mod_points::PointId::Add as usize);
    // SAFETY: this callback is registered only at Add, whose receiver is mutable.
    let (counter, raw) = unsafe {
        (
            &mut *receiver.cast::<Counter>(),
            &mut *raw.cast::<__AddRaw<'_, usize, usize>>(),
        )
    };
    assert!(counter.total == 2 || counter.total == 18);
    *raw.step *= 2;
}

unsafe fn erased_add_return(place: usize, receiver: *mut (), value: *mut ()) {
    assert_eq!(place, mod_points::PointId::AddReturn as usize);
    // SAFETY: this callback is registered only at AddReturn with these mutable types.
    let (counter, value) = unsafe {
        (
            &mut *receiver.cast::<Counter>(),
            &mut *value.cast::<usize>(),
        )
    };
    assert_eq!(counter.total, 18);
    *value *= 2;
}

unsafe fn erased_slot_return(place: usize, receiver: *mut (), value: *mut ()) {
    assert_eq!(place, mod_points::PointId::SlotReturn as usize);
    assert!(receiver.is_null(), "the value still borrows the receiver");
    // SAFETY: SlotReturn passes a mutable place holding its returned &mut usize.
    let value = unsafe { &mut *value.cast::<&mut usize>() };
    **value += 5;
}

unsafe fn erased_label(place: usize, receiver: *mut (), raw: *mut ()) {
    assert_eq!(place, mod_points::PointId::PenLabel as usize);
    // SAFETY: PenLabel has a shared Pen receiver and this raw record; neither escapes the call.
    let (pen, raw) = unsafe {
        (
            &*receiver.cast::<Pen>(),
            &mut *raw
                .cast::<__PenLabelRaw<'_, std::borrow::Cow<'_, str>, Vec<String>, Vec<String>>>(),
        )
    };
    raw.tags.push(format!("{} erased tags", pen.tags.len()));
}

unsafe fn erased_double(place: usize, receiver: *mut (), raw: *mut ()) {
    assert_eq!(place, mod_points::PointId::Double as usize);
    assert!(receiver.is_null(), "a free function has no receiver");
    // SAFETY: Double passes its mutable raw record.
    let raw = unsafe { &mut *raw.cast::<__DoubleRaw<'_, i64, i64>>() };
    *raw.x += 1;
}

#[test]
fn slots_change_arguments_stop_the_function_and_change_its_value() {
    use mod_points::__private::{POINTS, RawKind};
    use mod_points::{HEARD, LISTENER, PointId, RAW};
    let mut counter = Counter { total: 0 };
    let pen = Pen {
        tags: vec!["own".into()],
    };
    // Empty slots: the function runs as written, and no listener list is asked for.
    assert_eq!(counter.add(1), 1);
    counter.reset();
    *counter.slot() = 0;
    assert_eq!(pen.label("a", Vec::new()), ["a", "own"]);
    assert_eq!(double(4), 8);
    assert!(HEARD.lock().unwrap().is_empty());

    counter.total = 2;
    *LISTENER.lock().unwrap() = Some(&Listener);
    // As the generated `main` registers raw listeners: a typed `let`, then erased.
    let add: __AddListener = raw_add;
    let add_return: __AddReturnListener = raw_add_return;
    let slot: __SlotReturnListener = raw_slot_return;
    let label: __PenLabelListener = raw_label;
    let label_return: __PenLabelReturnListener = raw_label_return;
    let head: __DoubleListener = raw_double;
    RAW.lock().unwrap().extend([
        (PointId::Add, RawKind::Typed(add as *const ())),
        (PointId::Add, RawKind::Erased(erased_add)),
        (PointId::AddReturn, RawKind::Typed(add_return as *const ())),
        (PointId::AddReturn, RawKind::Erased(erased_add_return)),
        (PointId::SlotReturn, RawKind::Typed(slot as *const ())),
        (PointId::SlotReturn, RawKind::Erased(erased_slot_return)),
        (PointId::PenLabel, RawKind::Typed(label as *const ())),
        (PointId::PenLabel, RawKind::Erased(erased_label)),
        (
            PointId::PenLabelReturn,
            RawKind::Typed(label_return as *const ()),
        ),
        (PointId::Double, RawKind::Typed(head as *const ())),
        (PointId::Double, RawKind::Erased(erased_double)),
    ]);
    for id in [
        PointId::Add,
        PointId::AddReturn,
        PointId::Reset,
        PointId::Slot,
        PointId::SlotReturn,
        PointId::PenLabel,
        PointId::PenLabelReturn,
        PointId::Double,
    ] {
        POINTS[id as usize].store(true, Ordering::Release);
    }
    // The API doubles the step to 6, the typed raw listener adds the total (2), and the erased
    // one doubles it to 16. The body returns 18; API, typed raw and erased returns then make it
    // (18 + 1000 + 10) * 2. Swapping either pair of raw listeners would change the answer.
    assert_eq!(counter.add(3), 2056);
    assert_eq!(counter.total, 18);
    // A head result returns at once: no body, no return place.
    assert_eq!(counter.add(100), 7);
    assert_eq!(counter.total, 18);
    // A head cancel skips the body; its unset return slot isn't asked.
    counter.reset();
    assert_eq!(counter.total, 18);
    // The borrowed return reaches its raw listener as itself.
    *counter.slot() += 1;
    assert_eq!(counter.total, 74);
    // Off `Game`: the API listener changes the text, the raw one an internal argument at the
    // head and the value the API can't carry at the return.
    assert_eq!(
        pen.label("b", Vec::new()),
        ["1 tags", "1 erased tags", "b!", "own", "returned"]
    );
    // A free function: a raw head result skips the body.
    assert_eq!(double(-5), 0);
    assert_eq!(double(5), 12);
    assert_eq!(
        *HEARD.lock().unwrap(),
        [
            "add",
            "add",
            "add_return",
            "add_return",
            "add",
            "add",
            "reset",
            "slot",
            "slot_return",
            "pen_label",
            "pen_label",
            "pen_label_return",
            "pen_label_return",
            "double",
            "double",
            "double",
            "double",
        ]
    );
}
