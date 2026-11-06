use super::*;

/// Helpers for native mods, kept as stable as the hook points.
// Native mods use these; nothing in Horizon does.
#[allow(dead_code)]
impl Game<'_> {
    /// Registered enchantments, in registry order.
    pub(in crate::game) fn native_enchantments(&self) -> Vec<EnchantmentInfo> {
        let enchantments = &self.stations().enchantments;
        enchantments
            .list
            .iter()
            .map(|e| {
                mod_api::EnchantmentInfo::new(
                    e.id.to_string(),
                    e.max_level,
                    enchantments.is_curse(&e.id),
                )
            })
            .collect()
    }
    /// `Enchantment.getMinCost(level)`: the enchanting power needed, or None for an unknown id.
    pub(in crate::game) fn native_enchantment_min_cost(&self, id: &str, level: u32) -> Option<i32> {
        self.stations()
            .enchantments
            .get(id)
            .map(|e| e.min_cost.calculate(level))
    }
    /// Whether enchantment `id` can go on `item` (`Enchantment.isSupportedItem`).
    pub(in crate::game) fn native_enchantment_supports(&self, id: &str, item: &str) -> bool {
        self.stations()
            .enchantments
            .can_enchant(self.items(), id, item)
    }
}
