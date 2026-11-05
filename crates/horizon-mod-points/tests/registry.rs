//! The registry's lifecycle, registering, sealing and stopping, on the real points: an integration
//! test, so the generated points compile once for the tests.
use mod_points::__private::{POINTS, RawKind, RawListener, register_raw, seal};
use mod_points::{PointId, Points, RegisterError, register, stop};
use std::sync::atomic::Ordering;

struct Tick;

impl Points for Tick {
    fn subscribed(&self) -> &'static [Option<PointId>] {
        &[mod_points::ids::tick, mod_points::ids::tick_return]
    }
}

struct Quiet;

#[mod_api::__subscriptions]
impl mod_api::Hooks for Quiet {}

impl Points for Quiet {
    fn subscribed(&self) -> &'static [Option<PointId>] {
        &[]
    }
}

fn raw_listener() {}

unsafe fn erased_listener(place: usize, receiver: *mut (), raw: *mut ()) {
    assert_eq!(place, PointId::Tick as usize);
    assert!(receiver.is_null() && raw.is_null());
}

/// `raw_listener` at `place`: the test's slots never call it.
fn raw_at(id: &'static str, place: PointId) -> RawListener {
    // SAFETY: nothing fires a raw place here with a call into the function.
    unsafe { RawListener::new(id, place, raw_listener as *const ()) }
}

/// Stops its own mod, and tries to register another, from inside the seal.
struct SelfStop;

impl Points for SelfStop {
    fn subscribed(&self) -> &'static [Option<PointId>] {
        stop("self_stopping");
        assert_eq!(register("during", &Quiet), Err(RegisterError::Sealed));
        &[mod_points::ids::tick]
    }
}

#[test]
fn one_list_registered_sealed_and_stopped() {
    mod_api::__private::install(
        ["tick_mod", "raw_mod", "stopped_early", "self_stopping"]
            .into_iter()
            .map(|id| mod_api::ModEntry::new(id, Box::new(Quiet)))
            .collect(),
    );
    assert!(mod_api::__private::set_mod_on("tick_mod", false));
    assert!(mod_api::__private::set_mod_on("raw_mod", false));
    assert_eq!(register("tick_mod", &Tick), Ok(()));
    assert_eq!(register("tick_mod", &Quiet), Err(RegisterError::Twice));
    assert_eq!(register("quiet_mod", &Quiet), Ok(()));
    // A stop before the seal holds after it.
    assert_eq!(register("stopped_early", &Tick), Ok(()));
    stop("stopped_early");
    assert_eq!(register("self_stopping", &SelfStop), Ok(()));
    let raw = PointId::Tick as usize + 2;
    let place = PointId::Tick;
    assert_eq!(register_raw(&raw_at("raw_mod", place)), Ok(()));
    // SAFETY: the callback checks its place and never dereferences the pointers.
    let erased = unsafe { RawListener::erased("raw_mod", place, erased_listener) };
    assert_eq!(register_raw(&erased), Ok(()));
    assert_eq!(register_raw(&raw_at("raw_mod", place)), Ok(()));
    assert!(
        !POINTS[PointId::Tick as usize].load(Ordering::Acquire),
        "nothing listens before the seal"
    );
    seal();
    assert_eq!(register("late", &Quiet), Err(RegisterError::Sealed));
    assert_eq!(
        register_raw(&raw_at("late", place)),
        Err(RegisterError::Sealed)
    );
    assert!(POINTS[PointId::Tick as usize].load(Ordering::Acquire));
    assert!(POINTS[PointId::TickReturn as usize].load(Ordering::Acquire));
    assert!(!POINTS[raw].load(Ordering::Acquire));
    let (mut heard, mut raw_heard) = (0, 0);
    mod_points::__private::fire(PointId::Tick, "tick", &mut |_| heard += 1);
    mod_points::__private::fire_raw(PointId::Tick, "tick", &mut |_| raw_heard += 1);
    assert_eq!((heard, raw_heard), (0, 0), "disabled before sealing");
    assert!(mod_api::__private::set_mod_on("tick_mod", true));
    assert!(mod_api::__private::set_mod_on("raw_mod", true));
    mod_points::__private::fire(PointId::Tick, "tick", &mut |_| heard += 1);
    let mut order = Vec::new();
    mod_points::__private::fire_raw(PointId::Tick, "tick", &mut |kind| {
        match kind {
            RawKind::Typed(function) => {
                assert_eq!(function, raw_listener as *const ());
                order.push("typed");
            }
            RawKind::Erased(function) => {
                assert_eq!(function as usize, erased_listener as *const () as usize);
                order.push("erased");
            }
        }
        raw_heard += 1;
    });
    assert_eq!(order, ["typed", "erased", "typed"]);
    assert_eq!((heard, raw_heard), (1, 3));
    assert!(mod_api::__private::set_mod_on("tick_mod", false));
    mod_points::__private::fire(PointId::Tick, "tick", &mut |_| heard += 1);
    mod_points::__private::fire_raw(PointId::Tick, "tick", &mut |_| raw_heard += 1);
    assert_eq!(
        (heard, raw_heard),
        (1, 6),
        "only the API listener is disabled"
    );
    assert!(mod_api::__private::set_mod_on("raw_mod", false));
    mod_points::__private::fire(PointId::Tick, "tick", &mut |_| heard += 1);
    mod_points::__private::fire_raw(PointId::Tick, "tick", &mut |_| raw_heard += 1);
    assert_eq!(
        (heard, raw_heard),
        (1, 6),
        "all raw listener kinds are disabled"
    );
    assert!(mod_api::__private::set_mod_on("tick_mod", true));
    assert!(mod_api::__private::set_mod_on("raw_mod", true));
    mod_points::__private::fire(PointId::Tick, "tick", &mut |_| heard += 1);
    mod_points::__private::fire_raw(PointId::Tick, "tick", &mut |_| raw_heard += 1);
    assert_eq!(
        (heard, raw_heard),
        (2, 9),
        "listeners resume without resealing"
    );
    stop("tick_mod");
    stop("raw_mod");
    for id in ["tick_mod", "raw_mod", "stopped_early", "self_stopping"] {
        assert!(mod_api::__private::set_mod_on(id, false));
        assert!(mod_api::__private::set_mod_on(id, true));
    }
    mod_points::__private::fire(PointId::Tick, "tick", &mut |_| heard += 1);
    mod_points::__private::fire_raw(PointId::Tick, "tick", &mut |_| raw_heard += 1);
    assert_eq!(
        (heard, raw_heard),
        (2, 9),
        "an enable switch cannot undo a stop"
    );
}

#[test]
fn lists_finds_a_name_at_compile_time() {
    use mod_points::__private::lists;
    const LIST: &[&str] = &["tick", "tick_return"];
    const _: () = assert!(lists(LIST, "tick_return"));
    assert!(!lists(LIST, "tic") && !lists(LIST, "ticks") && !lists(&[], "tick"));
}
