//! Runtime state preserved when loading NBT into an existing actual Mob.
use super::Game;
use horizon_client::mobs::Mob;
use horizon_core::nbt::Tag;

impl Game<'_> {
    pub(in crate::game) fn reload_mob_runtime(
        slot: &mut Mob,
        mut new: Mob,
        tag: &Tag,
        game_time: i64,
    ) {
        // LivingEntity.readAdditionalSaveData only replaces a decoded
        // Brain. Keep the Allay's modeled LIKED_PLAYER when the
        // required root memory map cannot load; valid empty maps clear it.
        let keep_liked_player = slot.species() == horizon_client::mobs::Species::Allay
            && !matches!(
                tag.get("Brain").and_then(|brain| brain.get("memories")),
                Some(Tag::Compound(_))
            );
        // Runtime identity and lifetime stay on the resident owner.
        new.join = slot.join;
        new.random = slot.random.clone();
        let old = horizon_client::join_seq::store_back(slot, new);
        // Runtime-only state (AI, animation) carries over.
        if keep_liked_player {
            slot.extra.tame = old.extra.tame;
            slot.extra.owner = old.extra.owner;
            if let (Some(st), Some(was)) = (slot.extra.allay.as_mut(), old.extra.allay.as_ref()) {
                st.liked_player_ttl = was.liked_player_ttl;
            }
        }
        // LivingEntity's damage cooldown and previous-hit amount
        // are runtime state. Entity's separate immunity window is
        // loaded from the saved `invulnerable_time` key above.
        slot.invulnerable = old.invulnerable;
        slot.last_hurt = old.last_hurt;
        // Entity.isInPowderSnow isn't saved data either: a skeleton in
        // powder snow stays afflicted through a merge.
        slot.in_powder_snow = old.in_powder_snow;
        // Entity.load updates saved data on the existing entity;
        // lastTradedPlayer is transient and survives a live NoAI edit.
        if slot.species() == horizon_client::mobs::Species::Villager {
            horizon_client::mobs::villager::ext_mut(slot).last_traded_player =
                horizon_client::mobs::villager::ext(&old).and_then(|ext| ext.last_traded_player);
        }
        if slot.species() == horizon_client::mobs::Species::Shulker {
            horizon_client::mobs::shulker::keep_eased_peek(slot, &old);
        }
        slot.ai = old.ai;
        // Mob.readAdditionalSaveData updates persistent home fields,
        // while retaining the rest of the live AI/runtime state.
        horizon_client::entity_nbt::read_mob_home(slot, tag);
        slot.extra.armor_attributes = old.extra.armor_attributes;
        // LivingEntity.lastEquipmentItems is runtime state Entity.load
        // keeps: the next tick swaps the archetype modifiers.
        slot.extra.adopted_body = old.extra.adopted_body;
        slot.age = old.age;
        slot.previous = slot.position;
        // Wolf.readAdditionalSaveData initializes persistent anger at
        // the live level clock, just as load_mob does on admission.
        if slot.species() == horizon_client::mobs::Species::Wolf {
            horizon_client::mobs::adopt_anger_end(slot, game_time);
        } else {
            slot.extra.anger_stamp = Some((slot.extra.anger, slot.age));
        }
    }
}
