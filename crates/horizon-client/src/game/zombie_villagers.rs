//! Zombie villagers in the world: zombies turning the villagers they kill
//! into zombie villagers (`Zombie.killedEntity`), the golden apple that starts
//! a cure (`ZombieVillager.mobInteract`), and the cured villager that
//! remembers who saved it (`ZombieVillager.finishConversion`).
use super::{Game, commands::Target};
use horizon_client::{
    advancements as adv,
    gossip::GossipType,
    lan::playerdata,
    mobs::{Mob, Species, villager, zombie_villager},
};
use horizon_core::random::RandomSource;
use horizon_world::food::Difficulty;

/// The `Zombie` subclasses that convert the villagers they kill.
fn is_zombie(s: Species) -> bool {
    matches!(
        s,
        Species::Zombie
            | Species::Husk
            | Species::Drowned
            | Species::ZombieVillager
            | Species::ZombifiedPiglin
    )
}
/// Whether a zombie turns the villager it killed: always on Hard, half the
/// time on Normal (`Zombie.killedEntity`).
fn infects(difficulty: Difficulty, coin: impl FnOnce() -> bool) -> bool {
    match difficulty {
        Difficulty::Hard => true,
        Difficulty::Normal => !coin(),
        _ => false,
    }
}

impl Game<'_> {
    /// `Zombie.killedEntity`: a villager killed by a zombie may rise as a
    /// zombie villager keeping its profession, trades and gossip.
    pub(super) fn zombie_killed(&mut self, killer: u32, victim: u32) { let (killer,victim,) = match crate::points_generated::P5842.get() { Some(__l) => { let mut __a = (killer,victim,); mod_api::point::fire(__l, |__f| __f(&mut *self, &mut __a)); __a } None => (killer,victim,) };
        let Some(k) = self.mobs.iter().position(|m| m.id == killer) else {
            return;
        };
        let Some(v) = self.mobs.iter().position(|m| m.id == victim) else {
            return;
        };
        if !is_zombie(self.mobs[k].species())
            || self.mobs[v].species() != Species::Villager
            || self.mobs[v].alive()
            || self.mobs[v].removed
        {
            return;
        }
        let difficulty = self.world.health_rules.difficulty;
        let random = &mut self.mobs[k].random;
        if !infects(difficulty, || random.next_boolean()) {
            return;
        }
        // The villager is gone without dying: no drops, no death animation.
        let old = self.mobs[v].clone();
        self.mobs[v].removed = true;
        self.mobs[v].health = 0.0;
        let at = [old.position.x, old.position.y, old.position.z];
        let Some(id) = self.spawn_mob("minecraft:zombie_villager", at, old.persistent) else {
            return;
        };
        let ensure = self.mobs.iter().position(|m| m.id == victim);
        if let Some(i) = ensure {
            self.ensure_offers(i);
        }
        let old = ensure.map_or(old, |i| self.mobs[i].clone());
        let Some(z) = self.mobs.iter_mut().find(|m| m.id == id) else {
            return;
        };
        convert_to_zombie_villager(&old, z);
        let killer_at = self.mobs[k].position;
        let pitch = (self.world.random.next_float() - self.world.random.next_float()) * 0.2 + 1.0;
        self.audio.play(
            "minecraft:entity.zombie.infect",
            Some([killer_at.x, killer_at.y, killer_at.z]),
            2.0,
            pitch,
        );
    }

    /// `ZombieVillager.mobInteract`: a golden apple on a weakened zombie
    /// villager starts the cure and is eaten; without weakness it does
    /// nothing. Returns whether the click was used.
    pub(super) fn interact_zombie_villager(&mut self, index: usize, item: &str) -> bool { let (index,) = match crate::points_generated::P5841.get() { Some(__l) => { let mut __a = (index,); mod_api::point::fire(__l, |__f| __f(&mut *self, &mut __a)); __a } None => (index,) };
        let m = &self.mobs[index];
        if m.species() != Species::ZombieVillager || item != "minecraft:golden_apple" {
            return false;
        }
        if !m.has_effect("minecraft:weakness") {
            return true;
        }
        self.consume_held();
        // `player.getUUID()`: the acting player, a server's guest included.
        let starter = Some(self.acting_player_uuid());
        let time = zombie_villager::WAIT_MIN
            + self
                .world
                .random
                .next_int_bound(zombie_villager::WAIT_SPREAD)
                .unwrap_or(0);
        let mut events = Vec::new();
        zombie_villager::start_converting(&mut self.mobs[index], starter, time, &mut events);
        let _ = self.handle_mob_events(events);
        self.swing_arm();
        true
    }

    /// `ZombieVillager.finishConversion`: the villager returns with its
    /// data, trades, experience and gossip, and the curing player earns the
    /// village's trust.
    pub(super) fn cure_zombie_villager(&mut self, id: u32) { let (id,) = match crate::points_generated::P5840.get() { Some(__l) => { let mut __a = (id,); mod_api::point::fire(__l, |__f| __f(&mut *self, &mut __a)); __a } None => (id,) };
        let Some(i) = self.mobs.iter().position(|m| m.id == id) else {
            return;
        };
        if self.mobs[i].removed || zombie_villager::cure(&self.mobs[i]).is_none() {
            return;
        }
        let old = self.mobs[i].clone();
        self.mobs[i].removed = true;
        let at = [old.position.x, old.position.y, old.position.z];
        let Some(vid) = self.spawn_mob("minecraft:villager", at, old.persistent) else {
            return;
        };
        let Some(v) = self.mobs.iter_mut().find(|m| m.id == vid) else {
            return;
        };
        convert_to_villager(&old, v);
        // `level.getPlayerByUUID(conversionStarter)`: the player who fed the
        // apple, if still in this level (the host or a server's guest).
        let starter = zombie_villager::cure(&old).and_then(|c| c.starter);
        let curer = starter.and_then(|uuid| Some((uuid, self.player_here(uuid)?)));
        // `Villager.onReputationEventFrom(ZOMBIE_VILLAGER_CURED)`.
        let Some(v) = self.mobs.iter_mut().find(|m| m.id == vid) else {
            return;
        };
        if let Some((uuid, _)) = curer {
            let g = &mut villager::ext_mut(v).gossips;
            g.add(uuid, GossipType::MajorPositive, 20);
            g.add(uuid, GossipType::MinorPositive, 25);
        }
        let cured = v.clone();
        let villager_view = self.mob_entity(&cured);
        let zombie_view = self.mob_entity(&old);
        if let Some((_, who)) = curer {
            self.for_player(who, |g| {
                g.trigger_cured_zombie_villager(&zombie_view, &villager_view)
            });
        }
        let pitch = (self.world.random.next_float() - self.world.random.next_float()) * 0.2 + 1.0;
        self.audio.play(
            "minecraft:entity.zombie_villager.converted",
            Some(at),
            2.0,
            pitch,
        );
    }

    /// `ServerLevel.getPlayerByUUID`: the player with `uuid` in this level.
    fn player_here(&self, uuid: [i32; 4]) -> Option<Target> { let (uuid,) = match crate::points_generated::P5839.get() { Some(__l) => { let mut __a = (uuid,); mod_api::point::fire(__l, |__f| __f(self, &mut __a)); __a } None => (uuid,) };
        match self.target_by_uuid(playerdata::ints_uuid(uuid))? {
            Target::Player => Some(Target::Player),
            Target::Guest(conn) => {
                let g = self.lan.as_ref()?.hub.guest(conn)?;
                (g.dimension == self.dim.id()).then_some(Target::Guest(conn))
            }
            _ => None,
        }
    }

    /// `CuredZombieVillagerTrigger`.
    fn trigger_cured_zombie_villager(
        &mut self,
        zombie: &adv::EntityView,
        villager: &adv::EntityView,
    ) { if let Some(__l) = crate::points_generated::P5838.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(&mut *self, &mut __a)); }
        let me = self.player_entity();
        let (zombie, villager) = (zombie.clone(), villager.clone());
        self.trigger("minecraft:cured_zombie_villager", move |c, w| {
            let origin = me.position;
            let ctx = |e| adv::Entities {
                this: Some(e),
                origin,
                tool: None,
                block_pos: None,
            };
            adv::conditions(&c["player"], &ctx(&me), w)
                && adv::conditions(&c["zombie"], &ctx(&zombie), w)
                && adv::conditions(&c["villager"], &ctx(&villager), w)
        });
    }
}

/// `Mob.convertTo` (`ConversionType.SINGLE`, keeping equipment and the
/// pick-up-loot flag): what any converted mob carries over.
fn carry_over(from: &Mob, to: &mut Mob) {
    to.position = from.position;
    to.previous = from.position;
    to.velocity = from.velocity;
    to.yaw = from.yaw;
    to.previous_yaw = from.yaw;
    to.body_yaw = from.body_yaw;
    to.previous_body_yaw = from.body_yaw;
    to.head_yaw = from.head_yaw;
    to.previous_head_yaw = from.head_yaw;
    to.head_pitch = from.head_pitch;
    to.previous_head_pitch = from.head_pitch;
    to.fall_distance = from.fall_distance;
    to.hurt_time = from.hurt_time;
    to.on_ground = from.on_ground;
    to.fire_ticks = from.fire_ticks;
    to.persistent = from.persistent;
    to.custom_name = from.custom_name.clone();
    to.custom_name_text = from.custom_name_text.clone();
    to.name_visible = from.name_visible;
    to.invisible = from.invisible;
    to.extra.pick_up_loot = from.extra.pick_up_loot;
    for slot in horizon_items::equipment::MAINHAND..=horizon_items::equipment::BODY {
        to.extra
            .set_slot_stack(slot, from.extra.slot_stack(slot).cloned());
    }
    to.extra.gear = from.extra.gear.clone();
    to.retain_effects(|_| false);
    for &(effect, ticks, amplifier) in &from.extra.effects {
        to.load_effect(effect, ticks, amplifier);
    }
    // VillagerData and the trades, experience and gossip.
    to.extra.variant = from.extra.variant;
    to.extra.variant2 = from.extra.variant2;
    to.extra.variant3 = from.extra.variant3.max(1);
    to.extra.trade = from.extra.trade.clone();
    if let Some(g) = villager::ext(from).map(|e| e.gossips.clone()) {
        villager::ext_mut(to).gossips = g;
    }
}
/// A villager rises as a zombie villager (never as an adult-age baby: a baby
/// stays a baby for good).
pub(super) fn convert_to_zombie_villager(from: &Mob, to: &mut Mob) {
    carry_over(from, to);
    to.extra.age = if from.is_baby() { i32::MIN / 2 } else { 0 };
}
/// A cured zombie villager becomes a villager again, queasy for 10 seconds.
pub(super) fn convert_to_villager(from: &Mob, to: &mut Mob) {
    carry_over(from, to);
    to.extra.age = if from.is_baby() { -24000 } else { 0 };
    to.add_effect("minecraft:nausea", 200, 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs prepared resources"]
    fn conversions_preserve_body_equipment() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client")
            .join(horizon_core::VANILLA_VERSION);
        let types = horizon_client::mobs::MobTypes::load(&root).unwrap();
        let mob = |id, name: &str| {
            Mob::new(
                id,
                std::sync::Arc::new(types.types[name].clone()),
                horizon_core::geometry::Vec3::ZERO,
                0.0,
            )
        };
        let mut source = mob(1, "minecraft:villager");
        let mut armor = horizon_client::items::ItemStack::new("minecraft:wolf_armor", 1);
        armor.damage = 7;
        armor.data_mut().custom_name = Some(horizon_world::nbt::Tag::String("kept".into()));
        source.extra.set_body_stack(Some(armor));
        let mut zombie = mob(2, "minecraft:zombie_villager");
        convert_to_zombie_villager(&source, &mut zombie);
        assert_eq!(zombie.extra.body_id(), Some("minecraft:wolf_armor"));
        assert_eq!(zombie.extra.body_damage(), 7);
        assert_eq!(zombie.extra.body_stack(), source.extra.body_stack());
        let mut cured = mob(3, "minecraft:villager");
        convert_to_villager(&zombie, &mut cured);
        assert_eq!(cured.extra.body_id(), Some("minecraft:wolf_armor"));
        assert_eq!(cured.extra.body_damage(), 7);
        assert_eq!(cured.extra.body_stack(), source.extra.body_stack());
    }

    #[test]
    fn zombies_convert_by_difficulty() {
        assert!(infects(Difficulty::Hard, || unreachable!()));
        assert!(!infects(Difficulty::Easy, || false));
        assert!(!infects(Difficulty::Peaceful, || false));
        // Normal: a coin toss that spares the villager when it is true.
        assert!(infects(Difficulty::Normal, || false));
        assert!(!infects(Difficulty::Normal, || true));
    }
    #[test]
    fn every_zombie_kind_infects() {
        for s in [
            Species::Zombie,
            Species::Husk,
            Species::Drowned,
            Species::ZombieVillager,
            Species::ZombifiedPiglin,
        ] {
            assert!(is_zombie(s));
        }
        assert!(!is_zombie(Species::Skeleton) && !is_zombie(Species::Villager));
    }
}
