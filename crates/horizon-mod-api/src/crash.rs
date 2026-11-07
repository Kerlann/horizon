//! Moved here from the game so mods can name it.
#[allow(unused_imports)]
use crate::*;
use std::cell::Cell;
use std::sync::Mutex;

thread_local! {
    /// The native mod hook this thread is inside, for the panic hook to name.
    static IN_NATIVE_MOD: Cell<Option<NativeModCall>> = const { Cell::new(None) };
}

/// The native mod hook this thread is inside, if any.
pub fn current_native_call() -> Option<NativeModCall> {
    IN_NATIVE_MOD.with(Cell::get)
}
/// What the game is doing, set as it goes: graphics adapter, settings, packs, world.
static DETAILS: Mutex<Vec<(&'static str, String)>> = Mutex::new(Vec::new());
/// A call into one hook of a native mod.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
// exhaustive: the game builds it with a literal; mods only read it
pub struct NativeModCall {
    /// The mod's `[mod] id`.
    pub id: &'static str,
    pub hook: &'static str,
    /// The part's type (`std::any::type_name`), when the call is to one of the mod's parts.
    pub part: Option<&'static str>,
}
/// Marks this thread as inside `call` until the guard drops, which also happens while a panic
/// unwinds. A hook called from a hook is named by the inner call.
pub fn enter_native_mod(call: NativeModCall) -> NativeModGuard {
    NativeModGuard {
        outer: IN_NATIVE_MOD.with(|c| c.replace(Some(call))),
        _timing: crate::__private::profiler::enter(call),
    }
}
/// The `[mod] id` of the native mod this thread is inside a hook of, if any.
pub fn current_native_mod() -> Option<&'static str> {
    IN_NATIVE_MOD.with(Cell::get).map(|call| call.id)
}
/// Puts back the call that `enter_native_mod` found.
#[must_use]
// exhaustive: its fields are private, so mods cannot build or destructure it
pub struct NativeModGuard {
    outer: Option<NativeModCall>,
    _timing: Option<crate::__private::profiler::Guard>,
}
impl Drop for NativeModGuard {
    fn drop(&mut self) {
        IN_NATIVE_MOD.with(|c| c.set(self.outer));
    }
}
/// Sets one line of the report's details (replacing the last value under `key`).
pub fn set_detail(key: &'static str, value: impl Into<String>) {
    let mut details = DETAILS.lock().unwrap_or_else(|e| e.into_inner());
    let value = value.into();
    match details.iter_mut().find(|(k, _)| *k == key) {
        Some(entry) => entry.1 = value,
        None => details.push((key, value)),
    }
}
pub fn clear_detail(key: &'static str) {
    DETAILS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|(k, _)| *k != key);
}
pub fn details() -> Vec<(&'static str, String)> {
    DETAILS.lock().unwrap_or_else(|e| e.into_inner()).clone()
}
