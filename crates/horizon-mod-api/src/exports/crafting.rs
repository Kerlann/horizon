// hidden: a module of game-side types; mods use their re-exports
#[doc(hidden)]
pub mod crafting;
pub use crate::crafting::{CraftResult, Ingredient, Recipe, RecipePropertyAddition};
