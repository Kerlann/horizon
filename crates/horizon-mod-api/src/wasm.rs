//! The mod's side of the WebAssembly interface, built only for `wasm32`: the generated
//! `Guest` reaches the game through the world's imports, and the generated exports call the
//! mod's [`Hooks`] and parts. A mod writes nothing for it: `#[mod_api::horizon_mod]` names
//! its hooks, and the same source builds native or to `mod.wasm`.
use crate::Hooks;
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};

use crate::wasm_refused::Refused;

fn expect<T>(value: Result<T, Refused>) -> T {
    match value {
        Ok(value) => value,
        Err(why) => panic!("a value can't cross between the game and this mod: {why}"),
    }
}

/// One call's storage for the values its borrowed arguments point into. The values may
/// borrow each other, so they are held type-erased and dropped newest first with the store.
#[derive(Default)]
pub(crate) struct Keep {
    held: RefCell<Vec<(*mut u8, unsafe fn(*mut u8))>>,
}

impl Keep {
    fn hold<T>(&self, value: T) -> *mut T {
        unsafe fn drop_box<T>(pointer: *mut u8) {
            // SAFETY: `pointer` came from `Box::into_raw` of a `Box<T>` in `hold`, once.
            drop(unsafe { Box::from_raw(pointer.cast::<T>()) });
        }
        let pointer = Box::into_raw(Box::new(value));
        self.held.borrow_mut().push((pointer.cast(), drop_box::<T>));
        pointer
    }

    pub(crate) fn str(&self, value: String) -> &str {
        // SAFETY: the box stays allocated, unmoved and handed out once, until `self` drops.
        unsafe { &*self.hold(value) }
    }

    pub(crate) fn slice<T>(&self, value: Vec<T>) -> &[T] {
        // SAFETY: as in `str`.
        unsafe { &*self.hold(value) }
    }

    pub(crate) fn val<T>(&self, value: T) -> &T {
        // SAFETY: as in `str`.
        unsafe { &*self.hold(value) }
    }

    #[allow(clippy::mut_from_ref)]
    pub(crate) fn val_mut<T>(&self, value: T) -> &mut T {
        // SAFETY: as in `str`.
        unsafe { &mut *self.hold(value) }
    }
}

impl Drop for Keep {
    fn drop(&mut self) {
        for (pointer, drop) in self.held.get_mut().drain(..).rev() {
            // SAFETY: each entry is a live box `hold` made, dropped once here.
            unsafe { drop(pointer) };
        }
    }
}

fn array<T, const N: usize>(items: Vec<T>) -> Result<[T; N], Refused> {
    items.try_into().map_err(|_| Refused::SHAPE)
}

/// The one `&'static str` for each distinct string: ids and dimensions the game names.
fn intern(value: String) -> &'static str {
    static INTERNED: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);
    let mut interned = INTERNED.lock().unwrap_or_else(|e| e.into_inner());
    let set = interned.get_or_insert_with(HashSet::new);
    match set.get(value.as_str()) {
        Some(existing) => existing,
        None => {
            let leaked: &'static str = Box::leak(value.into_boxed_str());
            set.insert(leaked);
            leaked
        }
    }
}

/// The game's names by the index each crosses as, and back: the game gives each index once
/// (`names.name`), and an index for a name the mod gives (`names.intern`).
#[derive(Default)]
struct Names {
    names: Vec<Option<&'static str>>,
    index: std::collections::HashMap<&'static str, u32>,
}

static NAMES: Mutex<Option<Names>> = Mutex::new(None);

/// The name the game's index `id` stands for.
fn name_of(id: u32) -> &'static str {
    let mut names = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    let names = names.get_or_insert_with(Names::default);
    if let Some(Some(name)) = names.names.get(id as usize) {
        return name;
    }
    let name = intern(glue::horizon::mod_api::names::name(id));
    if names.names.len() <= id as usize {
        names.names.resize(id as usize + 1, None);
    }
    names.names[id as usize] = Some(name);
    names.index.insert(name, id);
    name
}

/// The game's index for `name`.
fn name_id(name: &str) -> u32 {
    let mut names = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    let names = names.get_or_insert_with(Names::default);
    if let Some(id) = names.index.get(name) {
        return *id;
    }
    let id = glue::horizon::mod_api::names::intern(name);
    let name = intern(name.to_string());
    if names.names.len() <= id as usize {
        names.names.resize(id as usize + 1, None);
    }
    names.names[id as usize] = Some(name);
    names.index.insert(name, id);
    id
}

unsafe extern "Rust" {
    /// The mod's hooks, defined by `#[mod_api::horizon_mod]` in the mod's crate.
    safe fn __horizon_mod_hooks() -> Box<dyn Hooks>;
}

fn hooks() -> &'static dyn Hooks {
    static HOOKS: OnceLock<Box<dyn Hooks>> = OnceLock::new();
    &**HOOKS.get_or_init(__horizon_mod_hooks)
}

/// The mod's side of the exports other crates add to its world (the points crate's listeners):
/// its hooks, the value its other faces are on, and the hosts of the call running now.
pub mod exported {
    use super::glue::Guest;

    /// The mod's hooks.
    pub fn hooks() -> &'static dyn crate::Hooks {
        super::hooks()
    }
    /// The call's reading host.
    pub fn host() -> &'static dyn crate::Host {
        &Guest
    }
    /// The call's server host.
    pub fn server_host() -> &'static mut dyn crate::ServerHost {
        // A zero-sized value: leaking it allocates nothing.
        Box::leak(Box::new(Guest))
    }
    /// The call's client host.
    pub fn client_host() -> &'static mut dyn crate::ClientHost {
        Box::leak(Box::new(Guest))
    }
}

/// The parts and handlers the mod handed the game, by the index the game holds each by: every
/// trait the game may call it through (a block entity part is a block part too); none once the
/// game let it go.
static HANDLERS: Mutex<Handlers> = Mutex::new(Handlers {
    views: Vec::new(),
    free: Vec::new(),
});

struct Handlers {
    views: Vec<Vec<Box<dyn Any + Send>>>,
    free: Vec<u32>,
}

/// Keeps a part or handler the mod hands the game, by its views, for the game's calls; gives
/// its index.
fn add_handler(views: Vec<Box<dyn Any + Send>>) -> u32 {
    let mut handlers = HANDLERS.lock().unwrap_or_else(|e| e.into_inner());
    match handlers.free.pop() {
        Some(index) => {
            handlers.views[index as usize] = views;
            index
        }
        None => {
            handlers.views.push(views);
            (handlers.views.len() - 1) as u32
        }
    }
}

/// Drops the handlers the game let go of.
fn release_handlers(released: Vec<u32>) {
    let mut handlers = HANDLERS.lock().unwrap_or_else(|e| e.into_inner());
    for index in released {
        if let Some(views) = handlers.views.get_mut(index as usize) {
            let views = std::mem::take(views);
            handlers.free.push(index);
            // A handler's drop may call the game; not under the lock.
            drop(handlers);
            drop(views);
            handlers = HANDLERS.lock().unwrap_or_else(|e| e.into_inner());
        }
    }
}

/// One view of a handler: it as the trait `T`.
struct Handler<T: ?Sized + 'static>(Arc<T>);
// SAFETY: wasm32 mods run on one thread.
unsafe impl<T: ?Sized> Send for Handler<T> {}

fn view<T: ?Sized + 'static>(handler: Arc<T>) -> Box<dyn Any + Send> {
    Box::new(Handler::<T>(handler))
}

fn handler<T: ?Sized + 'static>(index: u32) -> Arc<T> {
    let handlers = HANDLERS.lock().unwrap_or_else(|e| e.into_inner());
    match handlers
        .views
        .get(index as usize)
        .and_then(|views| views.iter().find_map(|v| v.downcast_ref::<Handler<T>>()))
    {
        Some(handler) => handler.0.clone(),
        None => panic!("the game called a handler the mod didn't register"),
    }
}

/// One of the game's objects the mod holds (a capability's handler, another mod's provider),
/// by the index the game keeps it under; each call goes to the game.
struct Held(u32);

impl Drop for Held {
    fn drop(&mut self) {
        glue::horizon::mod_api::held_handles::release(self.0);
    }
}

/// An erased provider (`dyn Any`), handed to the game by its address, where any mod of the
/// group (sharing this memory and its table) reads it back. It lives as long as the mod.
fn erased(provider: Arc<dyn Any + Send + Sync>) -> u32 {
    Box::into_raw(Box::new(provider)) as usize as u32
}

/// The erased provider at `address`, handed over by a mod of this group.
fn erased_at(address: u32) -> Arc<dyn Any + Send + Sync> {
    // SAFETY: the game hands a mod only an address `erased` gave in its own group, and those
    // boxes are never freed. Its mods are linked with one memory and one table, so the
    // pointer and its vtable are this module's too.
    unsafe { (*(address as usize as *const Arc<dyn Any + Send + Sync>)).clone() }
}

/// One of the mod's own handlers the game handed back, boxed again.
struct Shared<T: ?Sized>(Arc<T>);

#[allow(
    clippy::all,
    unreachable_code,
    unused_imports,
    unused_variables,
    unused_parens,
    dead_code,
    non_snake_case
)]
mod glue {
    use super::{array, expect, intern};
    include!(concat!(env!("OUT_DIR"), "/wasm_guest_generated.rs"));
}
