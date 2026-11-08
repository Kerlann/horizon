// hidden: a module of game-side types; mods use their re-exports
#[doc(hidden)]
pub mod crafting;
mod __private {
    pub use crate::__private::ingredient_parser::{
        WalkedIngredient, dedup_keep_order, resolved_items,
    };
}

pub use crate::crafting::{
    Ingredient, IngredientKeysError, Recipe, RecipeInput, RecipeLookupError, RecipeMatch,
    RecipePropertyAddition,
};
