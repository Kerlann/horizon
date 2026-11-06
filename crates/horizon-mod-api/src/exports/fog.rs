// hidden: a module of game-side types; mods use their re-exports
#[doc(hidden)]
pub mod fog;
pub use crate::fog::{FogData, FogType};
