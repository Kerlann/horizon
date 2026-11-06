use super::*;

/// Helpers for native mods, kept as stable as the hook points.
// Native mods use these; nothing in Horizon does.
#[allow(dead_code)]
impl Game<'_> {
    /// Whether `stack` fits `ingredient` (`Ingredient.test`), matched with this
    /// game's loop context.
    pub(in crate::game) fn native_recipe_ingredient_test(
        &self,
        ingredient: &Ingredient,
        stack: &ItemStack,
    ) -> bool {
        ingredient.test_in(&self.loop_context, stack)
    }
}
