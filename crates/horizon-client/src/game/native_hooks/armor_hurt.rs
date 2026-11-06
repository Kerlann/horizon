use super::*;

/// The hook points. Each does nothing in Horizon.
impl Game<'_> {
    /// Armor is about to take durability damage from a hit (NeoForge's
    /// `ArmorHurtEvent`, vanilla's `LivingEntity.doHurtEquipment`): once per
    /// hit, for the pieces that would wear. `entity` is the wearer's entity id,
    /// `player` its UUID (None for a mob), `kind` its type and `source` the hit's
    /// damage type (`causing` and `direct` are 0: the hit does not carry them here). Each entry has
    /// its `slot`, the `stack`, the `damage` vanilla worked out and a
    /// `new_damage` a mod may change (0 for none; Unbreaking is applied
    /// after). Canceling means no armor damage. A server player's `feet`,
    /// `legs`, `chest` and `head`; a mob's `body` (a horse's armor).
    /// Falling-block helmet hits (`hurtHelmet`) are not ported yet.
    /// Server-side.
    pub(in crate::game) fn native_armor_hurt(
        &mut self,
        player: Option<u128>,
        entity: u32,
        kind: &str,
        source: &HurtSource<'_>,
        entries: &mut Vec<mod_api::NativeArmorHurt>,
        cancel: &mut bool,
    ) {
        self.fire_armor_hurt(player, entity, kind, source, entries, cancel);
    }
}
