//! Shared equipment durability mutation.
use super::Game;
use horizon_client::items::ItemStack;
use horizon_world::item_durability::{self, DurabilityStack, DurabilityWorld};
use std::convert::Infallible;

struct World<'a, 'g> {
    game: &'a mut Game<'g>,
    original: &'a ItemStack,
    player: bool,
    broken: Option<ItemStack>,
}

impl DurabilityWorld for World<'_, '_> {
    type Error = Infallible;
    type Stack = DurabilityStack;

    fn storage_error(error: Infallible) -> Infallible {
        error
    }
    fn infinite_materials(&mut self, _: &mut DurabilityStack) -> Result<bool, Infallible> {
        Ok(self.game.creative)
    }
    fn process_enchantments(
        &mut self,
        _: &mut DurabilityStack,
        amount: i32,
    ) -> Result<i32, Infallible> {
        let amount = amount as u32;
        let loss = if self.player {
            self.game.modify_wear(self.original, amount)
        } else {
            self.game.durability_loss(self.original, amount)
        };
        Ok(loss.min(i32::MAX as u32) as i32)
    }
    fn durability_changed(
        &mut self,
        _: &mut DurabilityStack,
        damage: i32,
    ) -> Result<(), Infallible> {
        self.game.trigger_item_durability(self.original, damage);
        Ok(())
    }
    fn broken(&mut self, _: &mut DurabilityStack, copy: DurabilityStack) -> Result<(), Infallible> {
        let mut stack = self.original.clone();
        stack.damage = copy.damage.unwrap_or(0) as u32;
        self.broken = Some(stack);
        Ok(())
    }
}

impl Game<'_> {
    /// Compute the server durability update without firing equip effects.
    pub(super) fn damage_stack(
        &mut self,
        stack: &ItemStack,
        amount: i32,
        player: bool,
    ) -> (Option<ItemStack>, Option<ItemStack>) {
        let mut projected =
            horizon_client::health::durability_projection(Some(self.items()), stack);
        let mut world = World {
            game: self,
            original: stack,
            player,
            broken: None,
        };
        item_durability::hurt_and_break(&mut projected, &mut world, amount, player).unwrap();
        let mut updated = stack.clone();
        updated.count = projected.count.max(0) as u32;
        if let Some(damage) = projected.damage {
            updated.damage = damage as u32;
        }
        ((updated.count > 0).then_some(updated), world.broken)
    }
}
