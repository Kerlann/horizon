//! The one list of point listeners, and the slot table the points read. A native mod's
//! `#[horizon_mod] impl Points` and the wasm runtime's proxies both [`register`] here while the game
//! loads, and raw listeners [`register_raw`]; the game then [`seal`]s the list, which
//! sets [`POINTS`]. A firing slot walks the mods subscribed to that place, API listeners first and
//! then raw ones, each in registration (dispatch) order, skipping disabled and [`stop`]ped mods.
use crate::{POINT_COUNT, PointId, Points};
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

/// Each place's slot: set once, at the seal, where a mod listens. A point that nothing listens at
/// costs one load of it.
pub static POINTS: [AtomicBool; POINT_COUNT] = [const { AtomicBool::new(false) }; POINT_COUNT];

struct Listener {
    id: &'static str,
    /// Resolved at the seal, so firing reads the owner's canonical enable flag without a scan.
    mod_index: Option<usize>,
    /// The mod's `impl Points`, when it has one.
    points: Option<&'static dyn Points>,
    /// Set when the mod stopped (a trap in its wasm group, or the game giving up on it): its
    /// listeners are skipped from then on.
    stopped: AtomicBool,
}

/// How a slot calls a raw listener.
#[derive(Clone, Copy)]
pub enum RawKind {
    /// A function of the place's listener type, cast back by the slot before calling it.
    Typed(*const ()),
    /// A runtime listener receiving the place index, receiver and raw record or return value.
    ///
    /// # Safety
    ///
    /// The caller must pass the registered place's index and valid pointers of that place's exact
    /// receiver and raw record or return types, respecting their mutability, aliasing, ownership
    /// and lifetime requirements for the call's duration. The receiver is null for a free or
    /// associated function, and at a borrowed-return place. A shared receiver must not be written
    /// to; its mutable pointer only erases the type, not its mutability.
    Erased(unsafe fn(usize, *mut (), *mut ())),
}

// Function pointers: no data behind them, so they move between threads.
unsafe impl Send for RawKind {}
unsafe impl Sync for RawKind {}

/// A raw listener in its place's list.
#[derive(Clone, Copy)]
struct Raw {
    listener: u32,
    kind: RawKind,
}

/// The list while mods load: registrations, and stops that came before the seal.
struct Open {
    listeners: Vec<Listener>,
    raw: Vec<(PointId, Raw)>,
    stopped: Vec<String>,
    /// Set once [`seal`] has taken the list: registering is refused from then on.
    sealing: bool,
}

struct Sealed {
    listeners: Box<[Listener]>,
    /// Each place's subscribed `impl Points`, by index into `listeners`, in order.
    places: Box<[Box<[u32]>]>,
    /// Each place's raw listeners, in order.
    raw: Box<[Box<[Raw]>]>,
}

/// Registering, stopping and sealing all hold this lock, so a registration either lands in the
/// sealed list or is refused, and a stop before the seal still holds after it.
static OPEN: Mutex<Open> = Mutex::new(Open {
    listeners: Vec::new(),
    raw: Vec::new(),
    stopped: Vec::new(),
    sealing: false,
});
static SEALED: OnceLock<Sealed> = OnceLock::new();

/// Why a listener wasn't registered.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RegisterError {
    /// The list was sealed: listeners register while the game loads, before its points are set.
    Sealed,
    /// The mod already registered its `impl Points`.
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

fn lock() -> std::sync::MutexGuard<'static, Open> {
    OPEN.lock().unwrap_or_else(|e| e.into_inner())
}

/// The open list, while listeners may still register.
fn open() -> Result<std::sync::MutexGuard<'static, Open>, RegisterError> {
    let open = lock();
    if open.sealing {
        return Err(RegisterError::Sealed);
    }
    Ok(open)
}

/// Mod `id`'s entry in the open list, added on its first registration.
fn listener(open: &mut Open, id: &'static str) -> usize {
    match open.listeners.iter().position(|listener| listener.id == id) {
        Some(index) => index,
        None => {
            open.listeners.push(Listener {
                id,
                mod_index: None,
                points: None,
                stopped: AtomicBool::new(false),
            });
            open.listeners.len() - 1
        }
    }
}

/// Registers mod `id`'s listeners, `points`, at the places its [`Points::subscribed`] names. One
/// route for both callers: a native mod's generated export and the wasm runtime's proxy.
pub fn register(id: &'static str, points: &'static dyn Points) -> Result<(), RegisterError> {
    let mut open = open()?;
    let index = listener(&mut open, id);
    let entry = &mut open.listeners[index].points;
    if entry.is_some() {
        return Err(RegisterError::Twice);
    }
    *entry = Some(points);
    Ok(())
}

/// A raw listener at one place: a native mod's typed function, or a runtime's erased callback.
/// Both constructors are `unsafe`: the caller guarantees the slot's pointers are used correctly.
pub struct RawListener {
    id: &'static str,
    place: PointId,
    kind: RawKind,
}

impl RawListener {
    /// Mod `id`'s listener `function` at `place`.
    ///
    /// # Safety
    ///
    /// `function` is a function of `place`'s raw listener type: `__<Point>Listener` at a head,
    /// `__<Point>ReturnListener` at a return. The place's slot casts it back to that type and calls
    /// it. The launcher's generated `main` checks the type with a typed `let` before erasing it.
    pub const unsafe fn new(id: &'static str, place: PointId, function: *const ()) -> Self {
        Self {
            id,
            place,
            kind: RawKind::Typed(function),
        }
    }

    /// Mod `id`'s erased runtime listener `function` at `place`.
    ///
    /// The callback gets the place's index, the receiver and the raw record at a head or the
    /// return value's place at a return (including `()` for a function with no explicit return
    /// type). The receiver is null for a free or associated function. At a borrowed-return place
    /// it is also null: only the value's place is passed, since the value may borrow the receiver.
    /// A shared receiver is passed as a mutable pointer only to erase its type, not its mutability.
    ///
    /// # Safety
    ///
    /// Given the pointers guaranteed by [`RawKind::Erased`], `function` must interpret them using
    /// `place`'s exact receiver and raw record or return types, respecting their mutability,
    /// aliasing, ownership and lifetime requirements. The pointers and any references derived
    /// from them must not escape the callback. In particular, a shared receiver must not be
    /// written to.
    pub const unsafe fn erased(
        id: &'static str,
        place: PointId,
        function: unsafe fn(usize, *mut (), *mut ()),
    ) -> Self {
        Self {
            id,
            place,
            kind: RawKind::Erased(function),
        }
    }

    /// The mod's id.
    pub fn id(&self) -> &'static str {
        self.id
    }
}

/// Registers a raw listener, in the same order for native functions and erased callbacks.
pub fn register_raw(raw: &RawListener) -> Result<(), RegisterError> {
    let mut open = open()?;
    let listener = listener(&mut open, raw.id) as u32;
    open.raw.push((
        raw.place,
        Raw {
            listener,
            kind: raw.kind,
        },
    ));
    Ok(())
}

/// Stops mod `id`'s listeners: none of them runs again, whether the list is sealed yet or not. A
/// trap that stops a wasm group and the game giving up on a native mod both end here.
pub fn stop(id: &str) {
    let mut open = lock();
    match SEALED.get() {
        Some(sealed) => {
            for listener in sealed.listeners.iter().filter(|listener| listener.id == id) {
                listener.stopped.store(true, Ordering::Release);
            }
        }
        None => open.stopped.push(id.to_owned()),
    }
}

/// Seals the list, so no listener registers after it, and sets each slot a mod listens at. The game
/// calls it once, after the mods load.
pub fn seal() {
    // Takes the list under the lock, then asks each mod where it listens without it: a mod's
    // `subscribed` may call `stop` (or `register`, refused) without deadlocking.
    let (mut listeners, raw_list) = {
        let mut open = lock();
        if open.sealing {
            return;
        }
        open.sealing = true;
        (
            std::mem::take(&mut open.listeners),
            std::mem::take(&mut open.raw),
        )
    };
    let mut places = vec![Vec::new(); POINT_COUNT];
    for (index, listener) in listeners.iter_mut().enumerate() {
        listener.mod_index = mod_api::__private::mod_index(listener.id);
        for place in listener
            .points
            .iter()
            .flat_map(|points| points.subscribed())
            .flatten()
        {
            places[*place as usize].push(index as u32);
        }
    }
    let mut raw = vec![Vec::new(); POINT_COUNT];
    for (place, function) in raw_list {
        raw[place as usize].push(function);
    }
    let open = lock();
    // Every stop so far, before the seal or while it ran, is in `stopped`; the next ones see
    // `SEALED`, set while the lock is held.
    for listener in &listeners {
        if open.stopped.iter().any(|id| id == listener.id) {
            listener.stopped.store(true, Ordering::Release);
        }
    }
    let sealed = SEALED.get_or_init(|| Sealed {
        listeners: listeners.into(),
        places: places.into_iter().map(Vec::into_boxed_slice).collect(),
        raw: raw.into_iter().map(Vec::into_boxed_slice).collect(),
    });
    for (index, slot) in POINTS.iter().enumerate() {
        let listened = !sealed.places[index].is_empty() || !sealed.raw[index].is_empty();
        slot.store(listened, Ordering::Release);
    }
}

/// Runs `call` for the mod `listener` while it runs `name`, so a panic in it names the mod; a
/// disabled or stopped mod is skipped.
fn run(sealed: &Sealed, listener: u32, name: &'static str, call: impl FnOnce()) {
    let listener = &sealed.listeners[listener as usize];
    if !listener.stopped.load(Ordering::Acquire)
        && listener
            .mod_index
            .is_none_or(mod_api::__private::mod_index_on)
    {
        let _guard = mod_api::__private::enter(listener.id, name);
        call();
    }
}

/// Runs the `impl Points` listeners at `place` (the `Points` method `name`), each through `call`.
pub(crate) fn fire(place: PointId, name: &'static str, call: &mut dyn FnMut(&'static dyn Points)) {
    let Some(sealed) = SEALED.get() else { return };
    for &index in &sealed.places[place as usize] {
        let points = sealed.listeners[index as usize].points;
        run(sealed, index, name, || {
            call(points.expect("a subscribed mod has its Points"))
        });
    }
}

/// Runs the raw listeners at `place`, each through `call` with its kind and function.
pub(crate) fn fire_raw(place: PointId, name: &'static str, call: &mut dyn FnMut(RawKind)) {
    let Some(sealed) = SEALED.get() else { return };
    for raw in &sealed.raw[place as usize] {
        run(sealed, raw.listener, name, || call(raw.kind));
    }
}
