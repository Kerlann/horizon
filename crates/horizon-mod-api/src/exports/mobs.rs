// hidden: a module of game-side types; mods use their re-exports
#[doc(hidden)]
pub mod mobs;
pub use crate::mobs::{PathComputationType, PathType};
