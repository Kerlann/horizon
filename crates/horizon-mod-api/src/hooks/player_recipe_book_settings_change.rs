//! The `player_recipe_book_settings_change` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0305

pub trait Hooks {
    /// A server player's recipe book open or filter setting changed. `book`
    /// is the book type's protocol id. The guest's state is not swapped in.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event; Bukkit's PlayerRecipeBookSettingsChange)",
        paper = "PlayerRecipeBookSettingsChangeEvent"
    )]
    fn player_recipe_book_settings_change();
}
