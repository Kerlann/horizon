//! The `armor_hurt` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// Armor is about to take durability damage from a hit (NeoForge's
    /// `ArmorHurtEvent`, vanilla's `LivingEntity.doHurtEquipment`): once per
    /// equipment-wear operation, for the pieces that would wear. `entity` is the wearer's entity id,
    /// `player` its UUID (None for a mob), `kind` its type and `source` the hit's
    /// damage type (ordinary armor wear has unknown `causing` and `direct` ids;
    /// helmet wear carries the ids known to the hit). Each entry has
    /// its `slot`, the `stack`, the `damage` vanilla worked out and a
    /// `new_damage` a mod may change (0 for none; Unbreaking is applied
    /// after). Canceling means no armor damage. A server player's `feet`,
    /// `legs`, `chest` and `head`; a mob's `body` (a horse's armor).
    /// Falling-block, anvil and stalactite impacts also ask for the head alone,
    /// after blocking and before the 25% helmet reduction and the hurt cooldown.
    /// Canceling this wear does not cancel that protection or the health hit.
    /// The host gives the hit's level random and level attachments. Server-side.
    #[hook(
        side = "server",
        contract = "cancel",
        out = "cancel",
        neoforge = "ArmorHurtEvent",
        cancellable = true,
        host = "level",
        target = "horizon_client::game::native_hooks::HitLevel<'_>::native_armor_hurt"
    )]
    fn armor_hurt();
}
