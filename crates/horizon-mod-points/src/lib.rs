//! The game's injection points (#589), for mods that listen at them. Every function of the game
//! module the prebuilt game's staging pass admits is a point, and so is each listed function of the
//! crates below it: a mod listens at its head (before the body runs) and at its return (after)
//! through [`Points`], with no game rebuild. Each listener gets the
//! point's record: the arguments the API carries, and `cancel` or `result` where the function's
//! return allows. A mod names each method in a `[[native.injection]]` whose `target` is the
//! point's function.
//!
//! Depend on this crate only to listen at points: it is large (one record per point), and every
//! crate that depends on it compiles a few seconds slower. Hooks, handles and parts are in
//! `mod_api`.
#![deny(missing_docs)]
// Its tests are integration tests, so the unit-test build (`cargo test --lib`) is empty rather than
// a second compile of the generated points.
#![cfg(not(test))]

// The guest glue names this crate by its path, here as in the mods it is expanded into.
#[cfg(target_arch = "wasm32")]
extern crate self as mod_points;

include!(concat!(env!("OUT_DIR"), "/points_generated.rs"));

mod registry;
// hidden: how records cross to wasm mods, the runtime's and the guest glue's, not the API's
#[cfg(any(target_arch = "wasm32", feature = "wasm-host"))]
#[doc(hidden)]
pub mod wire;

pub use registry::{RegisterError, register, stop};

// hidden: the game's side of the points, not part of the API a mod is written against
#[doc(hidden)]
pub mod __private {
    pub use crate::registry::{POINTS, RawKind, RawListener, register_raw, seal};
    #[cfg(any(target_arch = "wasm32", feature = "wasm-host"))]
    pub use crate::wire;
    use crate::{PointId, Points};

    /// Runs the `impl Points` listeners at `place` (the `Points` method `name`), each through
    /// `call`: one copy of the loop for every point, out of the pointed functions' own code.
    #[cold]
    #[inline(never)]
    pub fn fire(place: PointId, name: &'static str, call: &mut dyn FnMut(&'static dyn Points)) {
        crate::registry::fire(place, name, call);
    }

    /// Runs the raw listeners at `place`, each through `call` with its kind and function. The
    /// slot casts a typed function back to the place's listener type, or calls an erased one
    /// with the place index and its receiver and raw record or return value pointers.
    ///
    /// Extracting an erased callback does not make it safe to call with arbitrary pointers:
    ///
    /// ```compile_fail,E0133
    /// use mod_points::{PointId, __private::{fire_raw, RawKind}};
    /// let place = PointId::Tick;
    /// fire_raw(place, "tick", &mut |kind| {
    ///     if let RawKind::Erased(function) = kind {
    ///         function(place as usize, std::ptr::null_mut(), std::ptr::null_mut());
    ///     }
    /// });
    /// ```
    ///
    /// A caller that provides the slot's pointer guarantees can invoke it explicitly:
    ///
    /// ```
    /// use mod_points::{PointId, __private::{fire_raw, RawKind}};
    /// // SAFETY: the caller must supply this place's exact receiver and record pointers,
    /// // valid for the callback's duration, as required by RawKind::Erased.
    /// unsafe fn forward(place: PointId, receiver: *mut (), raw: *mut ()) {
    ///     fire_raw(place, "slot", &mut |kind| {
    ///         if let RawKind::Erased(function) = kind {
    ///             // SAFETY: the caller supplies the slot's pointer guarantees.
    ///             unsafe { function(place as usize, receiver, raw) };
    ///         }
    ///     });
    /// }
    /// ```
    #[cold]
    #[inline(never)]
    pub fn fire_raw(place: PointId, name: &'static str, call: &mut dyn FnMut(RawKind)) {
        crate::registry::fire_raw(place, name, call);
    }

    /// Whether `list` holds `name`, at compile time: the launcher's generated `main` checks each
    /// point injection's method against the mod's `__HORIZON_POINTS` with it.
    pub const fn lists(list: &[&str], name: &str) -> bool {
        let mut i = 0;
        while i < list.len() {
            let (a, b) = (list[i].as_bytes(), name.as_bytes());
            if a.len() == b.len() {
                let mut j = 0;
                while j < a.len() && a[j] == b[j] {
                    j += 1;
                }
                if j == a.len() {
                    return true;
                }
            }
            i += 1;
        }
        false
    }
}
