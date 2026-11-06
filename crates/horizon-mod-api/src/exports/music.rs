// hidden: a module of game-side types; mods use their re-exports
#[doc(hidden)]
pub mod music;
pub use crate::music::{Music, MusicLayer};
