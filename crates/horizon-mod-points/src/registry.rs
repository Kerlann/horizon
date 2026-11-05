//! The one list of point listeners. A native mod's `#[horizon_mod] impl Points` and the wasm
//! runtime's proxies both [`register`] here while the game loads; the game then [`seal`]s it and
//! sets its slot table from [`listened_at`]. A firing slot walks [`listeners`]: the mods
//! subscribed to that place, in registration order (dispatch order), skipping any [`stop`]ped.
use crate::{POINT_COUNT, PointId, Points};
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

struct Listener {
    id: &'static str,
    points: &'static dyn Points,
    /// Set when the mod stopped (a trap in its wasm group, or the game giving up on it): its
    /// listeners are skipped from then on.
    stopped: AtomicBool,
}

struct Sealed {
    listeners: Box<[Listener]>,
    /// Each place's subscribed listeners, by index into `listeners`, in order.
    places: Box<[Box<[u32]>]>,
}

/// The list while mods load: registrations, and stops that came before the seal.
struct Open {
    listeners: Vec<Listener>,
    stopped: Vec<String>,
    /// Set once [`seal`] has taken the list: registering is refused from then on.
    sealing: bool,
}

/// Registering, stopping and sealing all hold this lock, so a registration either lands in the
/// sealed list or is refused, and a stop before the seal still holds after it.
static OPEN: Mutex<Open> = Mutex::new(Open {
    listeners: Vec::new(),
    stopped: Vec::new(),
    sealing: false,
});
static SEALED: OnceLock<Sealed> = OnceLock::new();

fn open() -> std::sync::MutexGuard<'static, Open> {
    OPEN.lock().unwrap_or_else(|e| e.into_inner())
}

/// Why a listener wasn't registered.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RegisterError {
    /// The list was sealed: listeners register while the game loads, before its points are set.
    Sealed,
    /// The mod already registered its listeners.
    Twice,
}

impl std::fmt::Display for RegisterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Sealed => "the game's points were already set",
            Self::Twice => "the mod already listens at points",
        })
    }
}

impl std::error::Error for RegisterError {}

/// Registers mod `id`'s listeners, `points`, at the places its [`Points::subscribed`] names. One
/// route for both callers: a native mod's generated export and the wasm runtime's proxy.
pub fn register(id: &'static str, points: &'static dyn Points) -> Result<(), RegisterError> {
    let mut open = open();
    if open.sealing {
        return Err(RegisterError::Sealed);
    }
    if open.listeners.iter().any(|listener| listener.id == id) {
        return Err(RegisterError::Twice);
    }
    open.listeners.push(Listener {
        id,
        points,
        stopped: AtomicBool::new(false),
    });
    Ok(())
}

/// Stops mod `id`'s listeners: none of them runs again, whether the list is sealed yet or not. A
/// trap that stops a wasm group and the game giving up on a native mod both end here.
pub fn stop(id: &str) {
    let mut open = open();
    match SEALED.get() {
        Some(sealed) => {
            for listener in sealed.listeners.iter().filter(|listener| listener.id == id) {
                listener.stopped.store(true, Ordering::Release);
            }
        }
        None => open.stopped.push(id.to_owned()),
    }
}

/// Seals the list: no listener registers after it. The game calls it once, after the mods load
/// and before it reads [`listened_at`].
pub fn seal() {
    // Takes the list under the lock, then asks each mod where it listens without it: a mod's
    // `subscribed` may call `stop` (or `register`, refused) without deadlocking.
    let listeners: Box<[Listener]> = {
        let mut open = open();
        if open.sealing {
            return;
        }
        open.sealing = true;
        std::mem::take(&mut open.listeners).into()
    };
    let mut places = vec![Vec::new(); POINT_COUNT];
    for (index, listener) in listeners.iter().enumerate() {
        for place in listener.points.subscribed().iter().flatten() {
            places[*place as usize].push(index as u32);
        }
    }
    let open = open();
    // Every stop so far, before the seal or while it ran, is in `stopped`; the next ones see
    // `SEALED`, set while the lock is held.
    for listener in listeners.iter() {
        if open.stopped.iter().any(|id| id == listener.id) {
            listener.stopped.store(true, Ordering::Release);
        }
    }
    let _ = SEALED.set(Sealed {
        listeners,
        places: places.into_iter().map(Vec::into_boxed_slice).collect(),
    });
}

/// Whether a registered mod listens at the place with this `PointId` index.
pub fn listened_at(index: usize) -> bool {
    SEALED.get().is_some_and(|sealed| {
        sealed
            .places
            .get(index)
            .is_some_and(|place| !place.is_empty())
    })
}

/// The listeners at `place`, in dispatch order, each marked as running `name` while it runs so a
/// panic in it names the mod; stopped mods are skipped.
pub fn listeners(place: PointId, name: &'static str) -> impl Iterator<Item = &'static dyn Points> {
    let sealed = SEALED.get();
    let indices = sealed.map_or(&[][..], |sealed| &sealed.places[place as usize][..]);
    let mut guard = None;
    indices.iter().filter_map(move |&index| {
        drop(guard.take());
        let listener = &sealed?.listeners[index as usize];
        if listener.stopped.load(Ordering::Acquire) {
            return None;
        }
        guard = Some(mod_api::__private::enter(listener.id, name));
        Some(listener.points)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tick;

    impl Points for Tick {
        fn subscribed(&self) -> &'static [Option<PointId>] {
            &[crate::ids::tick, crate::ids::tick_return]
        }
    }

    struct Quiet;

    impl Points for Quiet {
        fn subscribed(&self) -> &'static [Option<PointId>] {
            &[]
        }
    }

    /// Stops its own mod, and tries to register another, from inside the seal.
    struct SelfStop;

    impl Points for SelfStop {
        fn subscribed(&self) -> &'static [Option<PointId>] {
            stop("self_stopping");
            assert_eq!(register("during", &Quiet), Err(RegisterError::Sealed));
            &[crate::ids::tick]
        }
    }

    #[test]
    fn one_list_registered_sealed_and_stopped() {
        assert_eq!(register("tick_mod", &Tick), Ok(()));
        assert_eq!(register("tick_mod", &Quiet), Err(RegisterError::Twice));
        assert_eq!(register("quiet_mod", &Quiet), Ok(()));
        // A stop before the seal holds after it.
        assert_eq!(register("stopped_early", &Tick), Ok(()));
        stop("stopped_early");
        assert_eq!(register("self_stopping", &SelfStop), Ok(()));
        assert!(
            !listened_at(PointId::Tick as usize),
            "nothing listens before the seal"
        );
        seal();
        assert_eq!(register("late", &Quiet), Err(RegisterError::Sealed));
        assert!(listened_at(PointId::Tick as usize) && listened_at(PointId::TickReturn as usize));
        assert!(!listened_at(PointId::Tick as usize + 2));
        assert_eq!(
            listeners(PointId::Tick, "tick").count(),
            1,
            "the mods stopped before and during the seal stay stopped"
        );
        stop("tick_mod");
        assert_eq!(listeners(PointId::Tick, "tick").count(), 0);
    }
}
