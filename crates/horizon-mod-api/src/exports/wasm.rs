#[cfg(target_arch = "wasm32")]
mod wasm;
mod __private {
    /// The mod's side of the exports other crates add to its world (the points crate's listeners).
    #[cfg(target_arch = "wasm32")]
    pub use crate::wasm::exported as wasm_guest;
}
