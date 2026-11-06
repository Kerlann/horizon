// hidden: a module of game-side types; mods use their re-exports
#[doc(hidden)]
pub mod camera;
pub use crate::camera::{CameraEntity, CameraType, FovPlayer};
