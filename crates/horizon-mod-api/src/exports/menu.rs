// hidden: a module of game-side types; mods use their re-exports
#[doc(hidden)]
pub mod menu;
pub use crate::menu::{DragKind, LogicalSide, MenuClick, MouseButton, StackedOnOther, SwapWith};
