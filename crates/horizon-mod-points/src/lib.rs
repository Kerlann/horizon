//! The game's injection points (#589), for mods that listen at them. Every `Game` method the
//! prebuilt game's staging pass admits is a point: a mod listens at its head (before the body
//! runs) and at its return (after) through [`Points`], with no game rebuild. Each listener gets the
//! point's record: the arguments the API carries, and `cancel` or `result` where the function's
//! return allows. A mod names each method in a `[[native.injection]]` whose `target` is the
//! point's function.
//!
//! Depend on this crate only to listen at points: it is large (one record per point), and every
//! crate that depends on it compiles a few seconds slower. Hooks, handles and parts are in
//! `mod_api`.
#![deny(missing_docs)]

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
    pub use crate::registry::{listened_at, seal};
    #[cfg(any(target_arch = "wasm32", feature = "wasm-host"))]
    pub use crate::wire;
    use crate::{PointId, Points};

    /// Runs the listeners at `place` (the `Points` method `name`), each through `call`: one copy of
    /// the loop for every point, out of the pointed functions' own code.
    #[cold]
    #[inline(never)]
    pub fn fire(place: PointId, name: &'static str, call: &mut dyn FnMut(&'static dyn Points)) {
        for points in crate::registry::listeners(place, name) {
            call(points);
        }
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

    #[cfg(test)]
    mod tests {
        use super::lists;

        #[test]
        fn lists_finds_a_name_at_compile_time() {
            const LIST: &[&str] = &["tick", "tick_return"];
            const _: () = assert!(lists(LIST, "tick_return"));
            assert!(!lists(LIST, "tic") && !lists(LIST, "ticks") && !lists(&[], "tick"));
        }
    }
}
