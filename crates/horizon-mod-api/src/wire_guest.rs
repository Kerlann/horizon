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

/// What one call of the mod needs: where its borrowed returns live, until it returns.
#[derive(Default)]
struct Keep {
    held: Vec<(*mut u8, unsafe fn(*mut u8))>,
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

#[derive(Default)]
struct Runtime {
    /// The input the game is writing for the next call.
    pending: Vec<u8>,
    /// One reply per call depth: the game reads it once the call returns.
    replies: Vec<Writer>,
    keeps: Vec<Keep>,
    /// Buffers for arguments and answers, reused so a steady state allocates nothing.
    spare: Vec<Vec<u8>>,
}

impl Runtime {
    fn recycle(&mut self, buffer: Vec<u8>) {
        // Retain buffers for bounded call nesting, not the history of all calls. Large
        // arguments remain allowed, but their storage must not stay pinned in the pool.
        const MAX_SPARE_BUFFERS: usize = 64;
        const MAX_SPARE_CAPACITY: usize = 1024 * 1024;
        if self.spare.len() < MAX_SPARE_BUFFERS && buffer.capacity() <= MAX_SPARE_CAPACITY {
            self.spare.push(buffer);
        }
    }
}

thread_local! {
    static RUNTIME: RefCell<Runtime> = RefCell::new(Runtime::default());
    static HANDLERS: RefCell<Slots<Vec<Box<dyn Any>>>> = RefCell::new(Slots::default());
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
        RUNTIME.with(|rt| rt.borrow_mut().recycle(buffer));
    }
}

/// Calls the game's import `id` with `args` and returns its answer.
pub fn host(id: u32, args: Writer) -> Reply {
    let mut out = RUNTIME
        .with(|rt| rt.borrow_mut().spare.pop())
        .unwrap_or_default();
    out.clear();
    out.reserve(256);
    transport::call(id, args.bytes(), &mut out);
    RUNTIME.with(|rt| rt.borrow_mut().recycle(args.into_bytes()));
    Reply(out)
}

#[cfg(target_arch = "wasm32")]
mod transport {
    #[link(wasm_import_module = "horizon")]
    unsafe extern "C" {
        fn host(id: u32, in_ptr: *const u8, in_len: u32, out_ptr: *mut u8, out_cap: u32) -> u32;
        fn host_reply(out_ptr: *mut u8);
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

    pub(super) fn call(id: u32, input: &[u8], out: &mut Vec<u8>) {
        let transport = TRANSPORT
            .with(|t| t.borrow().clone())
            .expect("no transport to the game is set");
        out.extend_from_slice(&transport(id, input));
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
    HANDLERS.with(|h| h.borrow_mut().insert_long(views))
}

/// One view of a handler: it as the trait `T`.
struct View<T: ?Sized + 'static>(Arc<T>);

/// `handler` as a view the game's calls through `T` find.
pub fn view<T: ?Sized + 'static>(handler: Arc<T>) -> Box<dyn Any> {
    Box::new(View::<T>(handler))
}

/// The handler `id` as `T`: stale when the game let it go or never had it from this mod.
pub fn handler<T: ?Sized + 'static>(id: Id) -> Result<Arc<T>, WireError> {
    HANDLERS.with(|h| {
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
        std::panic::set_hook(Box::new(|info| {
            let mut args = begin();
            args.put_str(&info.to_string());
            host(crate::wire::HOST_LOG, args);
        }));
    });
    let _resources = resource_scope();
    let _ = hooks();
    let (input, depth, mut reply) = RUNTIME.with(|rt| {
        let mut rt = rt.borrow_mut();
        let depth = rt.keeps.len();
        rt.keeps.push(Keep::default());
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
        let keep = rt.keeps.pop();
        rt.replies[depth] = reply;
        rt.recycle(input);
        drop(rt);
        drop(keep);
    });
    (pointer, outcome)
}

/// Drops the handlers the game let go of. A handler's drop may call the game: not under the lock.
fn release(ids: Vec<Id>) {
    for id in ids {
        let views = HANDLERS.with(|h| h.borrow_mut().remove(id));
        drop(views);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

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
