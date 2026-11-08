// hidden: the wasm boundary's encoding, used by generated code, not a mod's API
#[doc(hidden)]
pub mod wire;
// hidden: the guest side of the wasm boundary, used by generated code, not a mod's API
#[cfg(any(target_arch = "wasm32", feature = "wasm-guest"))]
#[doc(hidden)]
pub mod wire_guest;
mod wire_slots;
mod wire_types;
