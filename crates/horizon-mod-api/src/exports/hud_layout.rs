// hidden: a module of game-side types; mods use their re-exports
#[doc(hidden)]
pub mod hud_layout;
pub use crate::hud_layout::{GuiDraw, HealthInputs};
