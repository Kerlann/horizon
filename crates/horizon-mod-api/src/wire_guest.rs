//! The mod's side of the boundary, built for `wasm32` (and for native tests behind the
//! `wasm-guest` feature): the transport to the game, the call arena, the handlers the mod gave
//! the game and the entry points the game calls. The generated half (`Guest`, `hook`) sits
//! beside it. See the boundary's design in `horizon-mod-wasm`.
use crate::wire::{Id, Reader, Slots, Wire, WireError, Writer};
use std::any::Any;
use std::cell::RefCell;
use std::sync::Arc;

mod generated {
    #![allow(clippy::all, unused, dead_code, non_snake_case)]
    use super::super::*;
    include!(concat!(env!("OUT_DIR"), "/wire_guest_generated.rs"));
}
pub use generated::{Guest, hook, hook_name};

unsafe extern "Rust" {
    /// The mod's hooks, defined by `#[mod_api::horizon_mod]` in the mod's crate.
    safe fn __horizon_mod_hooks() -> Box<dyn crate::Hooks>;
}

/// The mod's hooks, made once.
fn hooks() -> &'static dyn crate::Hooks {
    static HOOKS: std::sync::OnceLock<Box<dyn crate::Hooks>> = std::sync::OnceLock::new();
    &**HOOKS.get_or_init(__horizon_mod_hooks)
}

#[derive(PartialEq)]
struct MemoKey {
    import: u32,
    args: Vec<u8>,
}
struct Memo {
    key: MemoKey,
    value: Box<dyn Any>,
}
#[derive(Default)]
struct MemoFrame {
    entries: Vec<Memo>,
    active: usize,
}
impl MemoFrame {
    fn clear(&mut self) {
        self.active = 0;
    }
}
fn memo_key(import: u32, args: &[u8]) -> Option<MemoKey> {
    if args.len() > 64 {
        return None;
    }
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(args.len()).ok()?;
    bytes.extend_from_slice(args);
    Some(MemoKey {
        import,
        args: bytes,
    })
}

/// What one call of the mod needs: where its borrowed returns live, until it returns.
#[derive(Default)]
struct Keep {
    held: Vec<(*mut u8, unsafe fn(*mut u8))>,
    // None for the native harness's unscoped borrow keeper: only run() starts a memo scope.
    memo: Option<MemoFrame>,
}

impl Keep {
    fn hold<T>(&mut self, value: T) -> *mut T {
        unsafe fn drop_box<T>(pointer: *mut u8) {
            // SAFETY: `pointer` came from `Box::into_raw` of a `Box<T>` in `hold`, once.
            drop(unsafe { Box::from_raw(pointer.cast::<T>()) });
        }
        let pointer = Box::into_raw(Box::new(value));
        self.held.push((pointer.cast(), drop_box::<T>));
        pointer
    }
}

impl Drop for Keep {
    fn drop(&mut self) {
        for (pointer, drop) in self.held.drain(..).rev() {
            // SAFETY: each entry is a live box `hold` made, dropped once here, newest first.
            unsafe { drop(pointer) };
        }
    }
}

/// A request acknowledges one exact decoded ID, never another member's state. This
/// remains correct under reentry and when Runtime is member-local within a group.
#[derive(Default)]
struct StaticStrings {
    entries: Vec<Option<&'static str>>,
    last: Option<u32>,
    vector_charge: usize,
}

// Leave at least seven eighths of the group's memory limit for normal mod work.
// The shared counter charges conservative reservations, not just encoded payload bytes.
fn cache_charge(bytes: usize) -> Option<usize> {
    bytes.checked_add(32)?.checked_mul(8)
}

impl Drop for StaticStrings {
    fn drop(&mut self) {
        group_budget_release(self.vector_charge);
    }
}

impl StaticStrings {
    fn resolve(&mut self, id: usize, text: Option<&str>) -> Result<&'static str, WireError> {
        if id >= crate::wire::MAX_STATIC_STRINGS {
            return Err(WireError::Invalid("static string id outside table"));
        }
        let existing = self.entries.get(id).copied().flatten();
        if let Some(text) = text {
            if let Some(existing) = existing {
                if existing != text {
                    return Err(WireError::Invalid("changed static string definition"));
                }
            } else {
                // Intern only on first sight. This preserves canonical static references and
                // the pre-existing bounded storage; known IDs never enter the mutex/hash path.
                let value = crate::wire::intern(text)?;
                if id >= self.entries.len() {
                    let needed = id + 1;
                    if needed > self.entries.capacity() {
                        let wanted = needed
                            .max(self.entries.capacity() * 2)
                            .min(crate::wire::MAX_STATIC_STRINGS);
                        let Some(charge) =
                            cache_charge(wanted * std::mem::size_of::<Option<&'static str>>())
                        else {
                            return Ok(value);
                        };
                        if !group_budget_try_reserve(charge) {
                            return Ok(value);
                        }
                        let mut replacement = Vec::new();
                        if replacement.try_reserve_exact(wanted).is_err() {
                            group_budget_release(charge);
                            return Ok(value);
                        }
                        let Some(actual) = cache_charge(
                            replacement.capacity() * std::mem::size_of::<Option<&'static str>>(),
                        ) else {
                            group_budget_release(charge);
                            return Ok(value);
                        };
                        if actual > charge && !group_budget_try_reserve(actual - charge) {
                            group_budget_release(charge);
                            return Ok(value);
                        }
                        if actual < charge {
                            group_budget_release(charge - actual);
                        }
                        replacement.extend(self.entries.iter().copied());
                        self.entries = replacement;
                        group_budget_release(self.vector_charge);
                        self.vector_charge = actual;
                    }
                    self.entries.resize(needed, None);
                }
                self.entries[id] = Some(value);
            }
        }
        let value = self
            .entries
            .get(id)
            .copied()
            .flatten()
            .ok_or(WireError::Invalid("unknown static string id"))?;
        self.last = Some(id as u32);
        Ok(value)
    }
}

#[derive(Default)]
struct Runtime {
    /// The input the game is writing for the next call.
    pending: Vec<u8>,
    /// One reply per call depth: the game reads it once the call returns.
    replies: Vec<Writer>,
    keeps: Vec<Keep>,
    /// Buffers for arguments and answers, reused so a steady state allocates nothing.
    spare: Vec<Vec<u8>>,
    #[cfg(feature = "wire-pool-profile")]
    pool_counts: [u64; 5], // kept, dropped (full), dropped (large), reply drops, exports
    strings: StaticStrings,
    memo_spare: Vec<MemoFrame>,
}

impl Runtime {
    fn start_keep(&mut self) {
        let memo = self.memo_spare.pop().unwrap_or_default();
        self.keeps.push(Keep {
            memo: Some(memo),
            held: Vec::new(),
        });
    }
    fn finish_keep(&mut self) -> Option<Keep> {
        let mut keep = self.keeps.pop();
        if let Some(mut memo) = keep.as_mut().and_then(|keep| keep.memo.take()) {
            memo.clear();
            if self.memo_spare.len() < 64 {
                self.memo_spare.push(memo);
            }
        }
        keep
    }
    fn recycle(&mut self, buffer: Vec<u8>) {
        // Retain buffers for bounded call nesting, not the history of all calls. Large
        // arguments remain allowed, but their storage must not stay pinned in the pool.
        const MAX_SPARE_BUFFERS: usize = 64;
        const MAX_SPARE_CAPACITY: usize = 1024 * 1024;
        if self.spare.len() < MAX_SPARE_BUFFERS && buffer.capacity() <= MAX_SPARE_CAPACITY {
            #[cfg(feature = "wire-pool-profile")]
            {
                self.pool_counts[0] += 1;
            }
            self.spare.push(buffer);
        } else {
            #[cfg(feature = "wire-pool-profile")]
            {
                self.pool_counts[if self.spare.len() >= MAX_SPARE_BUFFERS {
                    1
                } else {
                    2
                }] += 1;
            }
        }
    }
}

thread_local! {
    static RUNTIME: RefCell<Runtime> = RefCell::new(Runtime::default());
}

type Handlers = RefCell<Slots<Vec<Box<dyn Any>>>>;

#[cfg(not(target_arch = "wasm32"))]
thread_local! {
    static HANDLERS: Handlers = RefCell::new(Slots::default());
}

/// Words in the group's shared state: the std member exports this array as `horizon_group_state`,
/// zeroed, for every mod of the group to read (one memory, one std, one state). A new word is taken
/// from the end of this list; the array's length is part of the ABI.
const GROUP_WORDS: usize = 8;
const WORD_HANDLERS: usize = 0;
const WORD_BUDGET: usize = 1;

/// What the mods of one group may hold at once in caches the host asked them to keep, in bytes.
pub const GROUP_BUDGET_BYTES: usize = 256 << 20;

#[cfg(target_arch = "wasm32")]
unsafe extern "C" {
    /// The group's shared state, defined by the std member.
    #[link_name = "horizon_group_state"]
    static GROUP_STATE: [std::sync::atomic::AtomicUsize; GROUP_WORDS];
}

#[cfg(not(target_arch = "wasm32"))]
static GROUP_STATE: [std::sync::atomic::AtomicUsize; GROUP_WORDS] =
    [const { std::sync::atomic::AtomicUsize::new(0) }; GROUP_WORDS];

/// Takes `bytes` from the group's budget, or answers `false` and takes nothing when that would pass
/// [`GROUP_BUDGET_BYTES`]. Every member counts against the same budget.
pub fn group_budget_try_reserve(bytes: usize) -> bool {
    reserve(group_state(), bytes, GROUP_BUDGET_BYTES)
}

/// Gives `bytes` back to the group's budget; never below zero.
pub fn group_budget_release(bytes: usize) {
    release_budget(group_state(), bytes)
}

fn group_state() -> &'static [std::sync::atomic::AtomicUsize; GROUP_WORDS] {
    // SAFETY: the std member defines the array; atomics need no exclusive access.
    #[cfg(target_arch = "wasm32")]
    return unsafe { &GROUP_STATE };
    #[cfg(not(target_arch = "wasm32"))]
    &GROUP_STATE
}

fn reserve(
    state: &[std::sync::atomic::AtomicUsize; GROUP_WORDS],
    bytes: usize,
    limit: usize,
) -> bool {
    use std::sync::atomic::Ordering::Relaxed;
    state[WORD_BUDGET]
        .fetch_update(Relaxed, Relaxed, |used| {
            used.checked_add(bytes).filter(|total| *total <= limit)
        })
        .is_ok()
}

fn release_budget(state: &[std::sync::atomic::AtomicUsize; GROUP_WORDS], bytes: usize) {
    use std::sync::atomic::Ordering::Relaxed;
    let _ =
        state[WORD_BUDGET].fetch_update(Relaxed, Relaxed, |used| Some(used.saturating_sub(bytes)));
}

/// Runs `f` on the handlers the game holds for this mod's group. Members of a group share one
/// memory and one std, so they share one table: a handler a member gave the game is found by
/// another member of its group when the game returns its id. The table is made by the first
/// member to need it and lives as long as the group's memory.
fn with_handlers<R>(f: impl FnOnce(&Handlers) -> R) -> R {
    #[cfg(not(target_arch = "wasm32"))]
    {
        HANDLERS.with(f)
    }
    #[cfg(target_arch = "wasm32")]
    {
        use std::sync::atomic::Ordering::Relaxed;
        // The handler word is zero until a member makes the table; a wasm group runs on one thread.
        let word = &group_state()[WORD_HANDLERS];
        let mut table = word.load(Relaxed) as *const Handlers;
        if table.is_null() {
            table = Box::into_raw(Box::new(Handlers::default()));
            word.store(table as usize, Relaxed);
        }
        // SAFETY: made above or by another member of this group from the same type, never freed.
        f(unsafe { &*table })
    }
}

#[cfg(feature = "wire-roundtrip-profile")]
thread_local! { static ROUNDTRIPS: std::cell::Cell<[u64; 3]> = const { std::cell::Cell::new([0; 3]) }; }
#[cfg(feature = "wire-roundtrip-profile")]
pub(crate) fn diagnostic_roundtrip(before: &[u8], after: &[u8], valid: bool) {
    ROUNDTRIPS.with(|counts| {
        let mut next = counts.get();
        next[if !valid {
            2
        } else if before == after {
            0
        } else {
            1
        }] += 1;
        counts.set(next);
    });
}
#[cfg(feature = "wire-roundtrip-profile")]
pub(crate) fn diagnostic_roundtrip_counts() -> [u64; 3] {
    ROUNDTRIPS.with(|counts| counts.get())
}

/// One exact static reply ID decoded by this instance, or the reserved no-ID marker.
pub fn static_ack() -> u32 {
    RUNTIME.with(|rt| rt.borrow().strings.last.unwrap_or(u32::MAX))
}

/// Model a fresh wasm instance in a native transport fixture.
#[cfg(not(target_arch = "wasm32"))]
pub fn reset_static_strings() {
    RUNTIME.with(|rt| rt.borrow_mut().strings = StaticStrings::default());
}

/// A buffer to write a call's arguments into.
pub fn begin() -> Writer {
    let mut writer = Writer::new();
    if let Some(buffer) = RUNTIME.with(|rt| rt.borrow_mut().spare.pop()) {
        writer.replace(buffer);
    }
    writer
}

/// The game's answer to a call, read once; its buffer goes back to the pool when it drops.
pub struct Reply(Vec<u8>);

impl Reply {
    /// Decode a static string table reply and its scalar writebacks atomically.
    pub fn take_static<T: Wire>(&self) -> Result<(&'static str, T), WireError> {
        enum Value<'a> {
            Known(u32),
            Definition(u32, &'a str),
            Uncached(&'a str),
        }
        fn text<'a>(reader: &mut Reader<'a>) -> Result<&'a str, WireError> {
            let len = reader.take_len(1)?;
            std::str::from_utf8(reader.take_bytes(len)?).map_err(|_| WireError::Utf8)
        }
        let mut reader = Reader::new(&self.0);
        let tag: u8 = reader.take()?;
        let id: u32 = reader.take()?;
        let value = match tag {
            0 => Value::Known(id),
            1 => Value::Definition(id, text(&mut reader)?),
            2 if id == u32::MAX => Value::Uncached(text(&mut reader)?),
            tag => return Err(WireError::Tag(u32::from(tag))),
        };
        let tail = reader.take()?;
        if !reader.rest().is_empty() {
            return Err(WireError::Trailing);
        }
        let string = match value {
            Value::Known(id) => {
                RUNTIME.with(|rt| rt.borrow_mut().strings.resolve(id as usize, None))?
            }
            Value::Definition(id, text) => {
                RUNTIME.with(|rt| rt.borrow_mut().strings.resolve(id as usize, Some(text)))?
            }
            Value::Uncached(text) => crate::wire::intern(text)?,
        };
        Ok((string, tail))
    }

    /// The value of type `T` the game answered with.
    pub fn take<T: Wire>(&self) -> Result<T, WireError> {
        let mut reader = Reader::new(&self.0);
        let value = reader.take()?;
        match reader.rest().is_empty() {
            true => Ok(value),
            false => Err(WireError::Trailing),
        }
    }
}

impl Drop for Reply {
    fn drop(&mut self) {
        let buffer = std::mem::take(&mut self.0);
        RUNTIME.with(|rt| {
            let mut rt = rt.borrow_mut();
            #[cfg(feature = "wire-pool-profile")]
            {
                rt.pool_counts[3] += 1;
            }
            rt.recycle(buffer);
        });
    }
}

/// Diagnostic marker for generated decode / mod / encode phase boundaries.
#[inline]
pub fn diagnostic_phase(id: u32, phase: u32) {
    #[cfg(feature = "wire-phase-profile")]
    {
        let mut bytes = [0; 8];
        bytes[..4].copy_from_slice(&id.to_le_bytes());
        bytes[4..].copy_from_slice(&phase.to_le_bytes());
        transport::call(0xFFFF_FE00, &bytes, &mut Vec::new());
    }
    #[cfg(not(feature = "wire-phase-profile"))]
    let _ = (id, phase);
}

/// Calls the game's import `id` with `args` and returns its answer.
pub fn host(id: u32, args: Writer) -> Reply {
    // Unknown imports may mutate context even through &self (settings do).
    RUNTIME.with(|rt| {
        for keep in &mut rt.borrow_mut().keeps {
            if let Some(memo) = &mut keep.memo {
                memo.clear();
            }
        }
    });
    host_preserving(id, args)
}

// Only generated imports whose contract preserves the current dimension use this path.
fn host_preserving(id: u32, args: Writer) -> Reply {
    let mut out = RUNTIME
        .with(|rt| rt.borrow_mut().spare.pop())
        .unwrap_or_default();
    out.clear();
    out.reserve(256);
    transport::call(id, args.bytes(), &mut out);
    RUNTIME.with(|rt| rt.borrow_mut().recycle(args.into_bytes()));
    Reply(out)
}

// Memoize checked reply bytes, never an unvalidated result or a borrow into a pooled buffer.
// Bounded keys/results merely miss; spare vectors retain capacity, not values across root calls.
fn memoized<T: Copy + 'static>(
    id: u32,
    key: Option<MemoKey>,
    args: Writer,
    decode: impl FnOnce(&Reply) -> Result<T, WireError>,
) -> Result<T, WireError> {
    let key = key.filter(|_| args.error().is_none() && std::mem::size_of::<T>() <= 512);
    let mut args = Some(args);
    let cached = key.as_ref().and_then(|key| {
        RUNTIME.with(|rt| {
            let mut rt = rt.borrow_mut();
            let memo = rt.keeps.last()?.memo.as_ref()?;
            let entry = memo.entries[..memo.active]
                .iter()
                .find(|entry| &entry.key == key)?;
            let value = *entry.value.downcast_ref::<T>()?;
            rt.recycle(
                args.take()
                    .expect("memo hit owns the argument buffer")
                    .into_bytes(),
            );
            Some(value)
        })
    });
    if let Some(value) = cached {
        return Ok(value);
    }
    let reply = host_preserving(id, args.expect("memo miss retains its argument buffer"));
    let value = decode(&reply)?;
    if let Some(key) = key.filter(|_| reply.0.len() <= 512) {
        RUNTIME.with(|rt| {
            let mut rt = rt.borrow_mut();
            let Some(memo) = rt.keeps.last_mut().and_then(|keep| keep.memo.as_mut()) else {
                return;
            };
            if memo.active == 4 {
                return;
            }
            if memo.active == memo.entries.len() {
                if memo.entries.try_reserve(1).is_err() {
                    return;
                }
                memo.entries.push(Memo {
                    key,
                    value: Box::new(value),
                });
            } else {
                let entry = &mut memo.entries[memo.active];
                entry.key = key;
                if let Some(old) = entry.value.downcast_mut::<T>() {
                    *old = value;
                } else {
                    entry.value = Box::new(value);
                }
            }
            memo.active += 1;
        });
    }
    Ok(value)
}

#[cfg(target_arch = "wasm32")]
mod transport {
    #[link(wasm_import_module = "horizon")]
    unsafe extern "C" {
        fn host(id: u32, in_ptr: *const u8, in_len: u32, out_ptr: *mut u8, out_cap: u32) -> u32;
        fn host_reply(out_ptr: *mut u8);
    }

    pub(super) fn try_call(id: u32, input: &[u8], out: &mut Vec<u8>) -> bool {
        call(id, input, out);
        true
    }
    pub(super) fn call(id: u32, input: &[u8], out: &mut Vec<u8>) {
        let cap = out.capacity();
        // SAFETY: the game writes at most `cap` bytes at `out`'s spare capacity, which it owns;
        // a longer answer is kept by the game and copied by `host_reply` into the grown buffer.
        unsafe {
            let len = host(
                id,
                input.as_ptr(),
                input.len() as u32,
                out.as_mut_ptr(),
                cap as u32,
            ) as usize;
            if len > cap {
                out.reserve(len);
                host_reply(out.as_mut_ptr());
            }
            out.set_len(len);
        }
    }
}

/// On a native build the game's side is the test's: set it with [`set_transport`].
#[cfg(not(target_arch = "wasm32"))]
mod transport {
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Serves import `id` for `input`, returning its answer.
    pub type Transport = Rc<dyn Fn(u32, &[u8]) -> Vec<u8>>;

    thread_local!(static TRANSPORT: RefCell<Option<Transport>> = const { RefCell::new(None) });

    pub fn set(transport: Option<Transport>) {
        TRANSPORT.with(|t| *t.borrow_mut() = transport);
    }

    pub(super) fn try_call(id: u32, input: &[u8], out: &mut Vec<u8>) -> bool {
        let transport = TRANSPORT
            .try_with(|t| t.try_borrow().ok().and_then(|t| t.clone()))
            .ok()
            .flatten();
        let Some(transport) = transport else {
            return false;
        };
        out.extend_from_slice(&transport(id, input));
        true
    }
    pub(super) fn call(id: u32, input: &[u8], out: &mut Vec<u8>) {
        assert!(try_call(id, input, out), "no transport to the game is set");
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use transport::{Transport, set as set_transport};

/// `value`, kept until the call in progress returns, for a method that answers with a borrow.
///
/// # Safety
/// The result may be used only while that call runs: the one lifetime `'a` the caller is
/// given by its own `&self`, which is local to the hook's dispatch.
pub unsafe fn kept<'a, T>(value: T) -> &'a T {
    RUNTIME.with(|rt| {
        let mut rt = rt.borrow_mut();
        let keep = match rt.keeps.last_mut() {
            Some(keep) => keep,
            None => {
                rt.keeps.push(Keep::default());
                rt.keeps.last_mut().expect("just pushed")
            }
        };
        // SAFETY: the box stays allocated and unmoved until the call's `Keep` drops.
        unsafe { &*keep.hold(value) }
    })
}

/// Keeps the mod's handler `views` for the game's calls and gives its id.
pub fn add_handler(views: Vec<Box<dyn Any>>) -> Id {
    with_handlers(|h| h.borrow_mut().insert_long(views))
}

/// One view of a handler: it as the trait `T`.
struct View<T: ?Sized + 'static>(Arc<T>);

/// `handler` as a view the game's calls through `T` find.
pub fn view<T: ?Sized + 'static>(handler: Arc<T>) -> Box<dyn Any> {
    Box::new(View::<T>(handler))
}

/// The handler `id` as `T`: stale when the game let it go or never had it from this mod.
pub fn handler<T: ?Sized + 'static>(id: Id) -> Result<Arc<T>, WireError> {
    with_handlers(|h| {
        let handlers = h.borrow();
        let views = handlers
            .get(id)
            .map_err(|_| WireError::Invalid("a handler the game no longer holds"))?;
        views
            .iter()
            .find_map(|v| v.downcast_ref::<View<T>>())
            .map(|v| v.0.clone())
            .ok_or(WireError::Invalid("a handler of another kind"))
    })
}

/// A call the boundary does not carry yet: it traps with what was called.
pub fn unsupported(what: &str) -> ! {
    panic!("{what} is not available to wasm mods yet")
}

/// The game's object the mod holds by `id`: each of its methods is a call to the game, and it
/// is released when the mod drops it.
#[derive(Debug)]
pub struct Held(pub Id);

impl Drop for Held {
    fn drop(&mut self) {
        let mut args = begin();
        args.put(&self.0);
        host(crate::wire::HOST_RELEASE, args);
    }
}

/// The game's buffer for the call about to run, `len` bytes.
pub fn input(len: usize) -> *mut u8 {
    RUNTIME.with(|rt| {
        let mut rt = rt.borrow_mut();
        if rt.pending.capacity() == 0 {
            rt.pending = rt.spare.pop().unwrap_or_default();
        }
        rt.pending.clear();
        rt.pending.resize(len, 0);
        rt.pending.as_mut_ptr()
    })
}

/// A generated export table sharing the hook transport and call arena.
pub type Extension = fn(u32, &mut Reader<'_>, &mut Writer) -> Result<(), WireError>;

thread_local! {
    static EXTENSIONS: RefCell<Vec<(u32, u32, Extension)>> = const { RefCell::new(Vec::new()) };
}

/// Installs a generated export table for ids matching `(id & mask) == base`.
/// Called while the guest loads, before any of its listeners run.
pub fn extend(base: u32, mask: u32, dispatch: Extension) {
    EXTENSIONS.with(|tables| {
        let mut tables = tables.borrow_mut();
        if let Some(table) = tables.iter_mut().find(|(b, m, _)| *b == base && *m == mask) {
            table.2 = dispatch;
        } else {
            tables.push((base, mask, dispatch));
        }
    });
}

fn dispatch(id: u32, input: &mut Reader<'_>, out: &mut Writer) -> Result<(), WireError> {
    let extension = EXTENSIONS.with(|tables| {
        tables
            .borrow()
            .iter()
            .find(|(base, mask, _)| id & mask == *base)
            .map(|(_, _, f)| *f)
    });
    match extension {
        Some(call) => call(id, input, out),
        None => hook(id, input, out),
    }
}

/// Installs the guest's resource registry for a synchronous wire operation.
#[doc(hidden)]
pub fn resource_scope() -> crate::wire::ResourceScope {
    generated::resource_scope()
}

/// Runs export `id` on the input the game wrote and returns where its reply is: the pointer in
/// the high half, the length in the low. A refused call answers `u32::MAX` as the length and
/// its reason, as a string, at the pointer.
pub fn run(id: u32) -> (*const u8, u32) {
    static HOOK: std::sync::Once = std::sync::Once::new();
    // A trap says nothing about why: the game gets the panic's text first.
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // A panic can originate while the pool is borrowed, or outside a guest
            // call in a native harness. Logging must not borrow it or require a transport.
            let mut args = Writer::new();
            args.put_str(&info.to_string());
            if !transport::try_call(crate::wire::HOST_LOG, args.bytes(), &mut Vec::new()) {
                previous(info);
            }
        }));
    });
    let _resources = resource_scope();
    let _ = hooks();
    let (input, depth, mut reply) = RUNTIME.with(|rt| {
        let mut rt = rt.borrow_mut();
        let depth = rt.keeps.len();
        rt.start_keep();
        if rt.replies.len() <= depth {
            rt.replies.push(Writer::new());
        }
        let mut reply = std::mem::take(&mut rt.replies[depth]);
        reply.clear();
        (std::mem::take(&mut rt.pending), depth, reply)
    });
    let mut reader = Reader::new(&input);
    let result = match id {
        crate::wire::INIT => reader
            .take::<Vec<crate::PackageInfo>>()
            .map(crate::phase::install_guest_packages),
        crate::wire::RELEASE => reader.take::<Vec<Id>>().map(release),
        id => dispatch(id, &mut reader, &mut reply),
    };
    let result = result.and_then(|()| reply.error().map_or(Ok(()), Err));
    let result = result.and_then(|()| match reader.rest().is_empty() {
        true => Ok(()),
        false => Err(WireError::Trailing),
    });
    let outcome = match result {
        Ok(()) => reply.bytes().len() as u32,
        Err(error) => {
            reply.clear();
            reply.put_str(&error.to_string());
            u32::MAX
        }
    };
    let pointer = reply.bytes().as_ptr();
    RUNTIME.with(|rt| {
        let mut rt = rt.borrow_mut();
        // The borrows of this call end with it, newest first, outside the runtime's borrow.
        let keep = rt.finish_keep();
        rt.replies[depth] = reply;
        rt.recycle(input);
        drop(rt);
        drop(keep);
    });
    #[cfg(feature = "wire-pool-profile")]
    {
        let counts = RUNTIME.with(|rt| {
            let mut rt = rt.borrow_mut();
            rt.pool_counts[4] += 1;
            (rt.pool_counts[4] == 200_000).then_some((rt.pool_counts, rt.spare.len()))
        });
        if let Some(counts) = counts {
            unsafe extern "C" {
                fn __horizon_diag_alloc_counter(index: u32) -> u64;
            }
            let allocs: [u64; 6] =
                std::array::from_fn(|index| unsafe { __horizon_diag_alloc_counter(index as u32) });
            let values = [
                counts.0[0],
                counts.0[1],
                counts.0[2],
                counts.0[3],
                counts.0[4],
                counts.1 as u64,
                allocs[0],
                allocs[1],
                allocs[2],
                allocs[3],
                allocs[4],
                allocs[5],
            ];
            let mut bytes = [0; 100];
            bytes[..4].copy_from_slice(&id.to_le_bytes());
            for (chunk, value) in bytes[4..].chunks_exact_mut(8).zip(values) {
                chunk.copy_from_slice(&value.to_le_bytes());
            }
            transport::try_call(0xFFFF_FE01, &bytes, &mut Vec::new());
            #[cfg(feature = "wire-roundtrip-profile")]
            {
                let mut args = Writer::new();
                args.put_str(&format!(
                    "actual guest entity roundtrip (equal, different, invalid)={:?}",
                    diagnostic_roundtrip_counts()
                ));
                transport::try_call(crate::wire::HOST_LOG, args.bytes(), &mut Vec::new());
            }
        }
    }
    (pointer, outcome)
}

/// Drops the handlers the game let go of. A handler's drop may call the game: not under the lock.
fn release(ids: Vec<Id>) {
    for id in ids {
        let views = with_handlers(|h| h.borrow_mut().remove(id));
        drop(views);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    trait Probe {
        fn answer(&self) -> u32;
    }
    struct Seven;
    impl Probe for Seven {
        fn answer(&self) -> u32 {
            7
        }
    }

    fn open_memo_scope() {
        RUNTIME.with(|rt| rt.borrow_mut().start_keep());
    }
    fn close_memo_scope() {
        drop(RUNTIME.with(|rt| rt.borrow_mut().finish_keep()));
    }
    fn memo_request(id: u32, bytes: &[u8]) -> Result<u32, WireError> {
        let mut args = begin();
        args.put_bytes(bytes);
        memoized(id, memo_key(id, bytes), args, Reply::take::<u32>)
    }

    #[test]
    fn memo_scopes_key_arguments_and_invalidate_ancestors_without_crossing_roots() {
        let calls = std::rc::Rc::new(std::cell::Cell::new(0u32));
        let count = calls.clone();
        set_transport(Some(std::rc::Rc::new(move |_, _| {
            count.set(count.get() + 1);
            count.get().to_le_bytes().to_vec()
        })));
        open_memo_scope();
        assert_eq!(memo_request(7, &[1]).unwrap(), 1);
        assert_eq!(memo_request(7, &[1]).unwrap(), 1);
        assert_eq!(memo_request(8, &[1]).unwrap(), 2);
        assert_eq!(memo_request(7, &[2]).unwrap(), 3);
        open_memo_scope();
        assert_eq!(memo_request(7, &[1]).unwrap(), 4);
        drop(host(999, begin())); // Unknown import may mutate the outer context too.
        assert_eq!(memo_request(7, &[1]).unwrap(), 6);
        close_memo_scope();
        assert_eq!(memo_request(7, &[1]).unwrap(), 7);
        close_memo_scope();
        open_memo_scope(); // Reuses the vector, never its old replies.
        assert_eq!(memo_request(7, &[1]).unwrap(), 8);
        close_memo_scope();
        // A native harness borrow keeper is not a root scope.
        unsafe {
            assert_eq!(*kept(3u32), 3);
        }
        assert_eq!(memo_request(7, &[1]).unwrap(), 9);
        assert_eq!(memo_request(7, &[1]).unwrap(), 10);
        set_transport(None);
    }

    #[test]
    fn memo_capacity_and_invalid_reply_are_misses_not_transport_failures() {
        let calls = std::rc::Rc::new(std::cell::Cell::new(0u32));
        let count = calls.clone();
        let bad = std::rc::Rc::new(std::cell::Cell::new(true));
        set_transport(Some(std::rc::Rc::new(move |id, _| {
            count.set(count.get() + 1);
            if id == 88 && bad.replace(false) {
                return vec![0];
            }
            if id == 77 {
                let mut out = Writer::new();
                out.put(&vec![4u8; 512]);
                return out.into_bytes();
            }
            count.get().to_le_bytes().to_vec()
        })));
        open_memo_scope();
        for id in 0..4 {
            memo_request(id, &[]).unwrap();
        }
        assert_eq!(memo_request(4, &[]).unwrap(), 5);
        assert_eq!(memo_request(4, &[]).unwrap(), 6);
        close_memo_scope();
        open_memo_scope();
        assert_eq!(memo_request(7, &[1; 65]).unwrap(), 7);
        assert_eq!(memo_request(7, &[1; 65]).unwrap(), 8);
        for _ in 0..2 {
            let value = memoized(77, memo_key(77, &[]), begin(), |r| {
                let bytes = r.take::<Vec<u8>>()?;
                assert_eq!(bytes, vec![4; 512]);
                Ok(bytes.len())
            })
            .unwrap();
            assert_eq!(value, 512);
        }
        assert_eq!(calls.get(), 10);
        assert!(memo_request(88, &[]).is_err());
        assert_eq!(memo_request(88, &[]).unwrap(), 12);
        assert_eq!(memo_request(88, &[]).unwrap(), 12);
        assert_eq!(calls.get(), 12);
        close_memo_scope();
        set_transport(None);
    }

    #[test]
    fn dimension_memo_preserves_volatile_getters_but_invalidates_on_settings() {
        use crate::host::{config::ConfigHost, items::ItemsHost};
        let dims = std::rc::Rc::new(std::cell::Cell::new(0));
        let maxes = std::rc::Rc::new(std::cell::Cell::new(0u32));
        let d = dims.clone();
        let m = maxes.clone();
        let dimension_id = std::cell::Cell::new(None);
        let changed = std::cell::Cell::new(false);
        set_transport(Some(std::rc::Rc::new(move |id, bytes| {
            let dim_id = dimension_id.get().unwrap_or_else(|| {
                dimension_id.set(Some(id));
                id
            });
            let mut out = Writer::new();
            if id == dim_id {
                d.set(d.get() + 1);
                out.put(&1u8);
                out.put(&u32::from(changed.get()));
                out.put_str(if changed.get() {
                    "memo:after"
                } else {
                    "memo:before"
                });
            } else if bytes.len() == 4 {
                m.set(m.get() + 1);
                out.put(&Some(m.get()));
            } else {
                changed.set(true);
            }
            out.into_bytes()
        })));
        let guest = Guest;
        open_memo_scope();
        assert_eq!(guest.dimension(), "memo:before");
        assert_eq!(guest.dimension(), "memo:before"); // Even though the acknowledgement changed.
        assert_eq!(dims.get(), 1);
        assert_eq!(guest.max_stack_size(crate::ItemKey(7)), Some(1));
        assert_eq!(guest.max_stack_size(crate::ItemKey(7)), Some(2));
        assert_eq!(guest.dimension(), "memo:before");
        assert_eq!(dims.get(), 1);
        guest.set_setting("memo", "dimension", "after"); // &self is not purity.
        assert_eq!(guest.dimension(), "memo:after");
        assert_eq!(guest.dimension(), "memo:after");
        assert_eq!(dims.get(), 2);
        close_memo_scope();
        open_memo_scope();
        assert_eq!(guest.dimension(), "memo:after");
        assert_eq!(dims.get(), 3);
        close_memo_scope();
        assert_eq!(guest.dimension(), "memo:after"); // Outside a root, never cache.
        assert_eq!(guest.dimension(), "memo:after");
        assert_eq!(dims.get(), 5);
        set_transport(None);
    }

    #[test]
    fn group_budget_is_one_counter_for_every_member_and_never_overdraws() {
        use std::sync::atomic::AtomicUsize;
        let state = [const { AtomicUsize::new(0) }; GROUP_WORDS];
        // Two members reach the same words, so their reservations add up.
        assert!(reserve(&state, 60, 100));
        assert!(reserve(&state, 40, 100));
        assert!(!reserve(&state, 1, 100), "the limit is exact");
        assert!(!reserve(&state, usize::MAX, 100), "overflow takes nothing");
        release_budget(&state, 40);
        assert!(
            reserve(&state, 40, 100),
            "released bytes are available again"
        );
        release_budget(&state, 1000);
        assert_eq!(
            state[WORD_BUDGET].load(std::sync::atomic::Ordering::Relaxed),
            0
        );
        assert_eq!(
            state[WORD_HANDLERS].load(std::sync::atomic::Ordering::Relaxed),
            0
        );
        assert!(group_budget_try_reserve(1));
        group_budget_release(1);
    }

    #[test]
    fn handlers_resolve_until_released_and_forged_ids_are_refused() {
        let provider: Arc<dyn Probe> = Arc::new(Seven);
        let id = add_handler(vec![view(provider)]);
        assert_eq!(handler::<dyn Probe>(id).unwrap().answer(), 7);
        // Another kind, an unissued slot and a stale generation answer with errors, never a panic.
        assert!(matches!(handler::<dyn Any>(id), Err(WireError::Invalid(_))));
        assert!(handler::<dyn Probe>(Id((1 << 32) | 0xffff)).is_err());
        assert!(handler::<dyn Probe>(Id(0)).is_err());
        release(vec![id]);
        assert!(matches!(
            handler::<dyn Probe>(id),
            Err(WireError::Invalid("a handler the game no longer holds"))
        ));
    }

    #[test]
    fn guest_pool_bounds_returned_leases_and_discards_large_buffers() {
        let mut runtime = Runtime::default();
        let leases: Vec<_> = (0..100).map(|_| Vec::with_capacity(32)).collect();
        for buffer in leases {
            runtime.recycle(buffer);
        }
        assert_eq!(runtime.spare.len(), 64);
        let mut runtime = Runtime::default();
        runtime.recycle(Vec::with_capacity(1024 * 1024 + 1));
        assert!(runtime.spare.is_empty());
    }

    #[test]
    fn guest_pool_returns_nested_import_arguments_and_replies() {
        set_transport(Some(std::rc::Rc::new(|id, bytes| {
            if id == 0 {
                assert_eq!(Reader::decode::<u32>(bytes).unwrap(), 5);
                let mut arguments = begin();
                arguments.put(&37u32);
                assert_eq!(host(1, arguments).take::<u32>().unwrap(), 99);
            } else {
                assert_eq!(Reader::decode::<u32>(bytes).unwrap(), 37);
            }
            99u32.to_le_bytes().to_vec()
        })));
        let mut arguments = begin();
        arguments.put(&5u32);
        let reply = host(0, arguments);
        assert_eq!(reply.take::<u32>().unwrap(), 99);
        // The nested arguments and reply, plus outer arguments, have returned. The
        // outer answer still belongs to Reply and cannot be leased by another call.
        RUNTIME.with(|rt| assert_eq!(rt.borrow().spare.len(), 3));
        drop(reply);
        RUNTIME.with(|rt| assert_eq!(rt.borrow().spare.len(), 4));
        set_transport(None);
    }

    #[test]
    fn guest_pool_does_not_cache_large_import_arguments_or_answers() {
        set_transport(Some(std::rc::Rc::new(|_, bytes| {
            assert!(bytes.len() > 1024 * 1024);
            vec![7]
        })));
        let mut arguments = begin();
        arguments.put_str(&"x".repeat(1024 * 1024 + 1));
        let reply = host(0, arguments);
        assert_eq!(reply.0, [7]);
        RUNTIME.with(|rt| assert!(rt.borrow().spare.is_empty()));
        drop(reply);
        RUNTIME.with(|rt| assert_eq!(rt.borrow().spare.len(), 1));
        set_transport(Some(std::rc::Rc::new(|_, _| vec![7; 1024 * 1024 + 1])));
        let mut arguments = begin();
        arguments.put(&5u32);
        let reply = host(0, arguments);
        assert_eq!(reply.0.len(), 1024 * 1024 + 1);
        drop(reply);
        RUNTIME.with(|rt| {
            let runtime = rt.borrow();
            assert_eq!(runtime.spare.len(), 1);
            assert!(runtime.spare[0].capacity() <= 1024 * 1024);
        });
        set_transport(None);
    }
}

#[cfg(target_arch = "wasm32")]
mod entry {
    use super::*;

    /// The layout of every value that crosses, which the game compares with its own.
    #[unsafe(no_mangle)]
    pub extern "C" fn horizon_layout() -> u64 {
        crate::wire::LAYOUT
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn horizon_in(len: u32) -> *mut u8 {
        input(len as usize)
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn horizon_hook(id: u32) -> u64 {
        let (pointer, len) = run(id);
        (pointer as usize as u64) << 32 | u64::from(len)
    }
}
