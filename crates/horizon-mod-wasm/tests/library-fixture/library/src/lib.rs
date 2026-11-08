use mod_api::Hooks;

#[derive(Default)]
pub struct Mod;

#[mod_api::horizon_mod]
impl Hooks for Mod {}

/// Exported across the wasm group.
#[inline(never)]
pub fn shared_value() -> i32 {
    37
}
