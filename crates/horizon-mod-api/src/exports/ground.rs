// hidden: a module of game-side types; mods use their re-exports
#[doc(hidden)]
pub mod ground;
pub use crate::ground::{GroundLevel, GroundProvider, GroundStateProvider};
