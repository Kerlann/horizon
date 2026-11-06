//! Raid handles for native server mods (`Raid`, `Raids`). Horizon keeps the
//! raids of current and parked server levels. Earlier opens, closed levels
//! and client reads return `None` or `false`.
use super::{Game, commands::Target};
use horizon_client::{
    lan::playerdata::ints_uuid,
    mobs::Mob,
    raid::{Raid, Raids, Status},
};
use mod_api::{EntityRef, LevelRef, MobRef, NativeRaidStatus, RaidRef};

impl Game<'_> {
    // Diverges from upstream: handles resolve the requested server level without
    // swapping it into place, including during hooks for another level.
    fn native_level_raid_collections(&self, level: LevelRef) -> Option<(&Raids, &[Mob])> {
        self.native_level_world(level)?;
        if level.dimension() == self.dim.id() {
            Some((self.raids_in_place()?, &self.mobs))
        } else {
            let parked = self.levels.parked_of(level.dimension())?;
            Some((parked.raiding.raids(level.dimension())?, &parked.mobs))
        }
    }
    fn native_level_raids_mut(&mut self, level: LevelRef) -> Option<&mut Raids> {
        self.native_level_world(level)?;
        if level.dimension() == self.dim.id() {
            self.raiding.raids_mut(level.dimension())
        } else {
            self.levels
                .parked_mut_of(level.dimension())?
                .raiding
                .raids_mut(level.dimension())
        }
    }
    fn native_raid_members(&self, raid: RaidRef) -> Option<impl Iterator<Item = &Mob>> {
        let (raids, mobs) = self.native_level_raid_collections(raid.level())?;
        raids.get(raid.id())?;
        Some(super::raids::members(mobs, raid.id()))
    }
    /// The raid a handle names now, or None for a stale or foreign one.
    fn native_raid_data(&self, raid: RaidRef) -> Option<&Raid> {
        self.native_level_raid_collections(raid.level())?
            .0
            .get(raid.id())
    }

    /// `Raids.get(id)`: the raid saved under `id` in the requested level, if
    /// `Raids` still holds it (a stopped raid goes at its next tick).
    pub(super) fn native_level_raid(&self, level: LevelRef, id: i32) -> Option<RaidRef> {
        let raid = RaidRef::new(level, id);
        self.native_raid_data(raid).map(|_| raid)
    }

    /// `ServerLevel.getRaidAt(pos)`: the closest active raid whose centre is
    /// less than 96 blocks away (`Raids.getNearbyRaid`, 9216 squared).
    pub(super) fn native_level_raid_at(&self, level: LevelRef, pos: [i32; 3]) -> Option<RaidRef> {
        let (id, _) = self.native_level_raid_collections(level)?.0.nearby(pos)?;
        Some(RaidRef::new(level, id))
    }

    /// Every raid `Raids` holds, in its iteration order, stopped ones
    /// included until their next tick. None for a closed or stale level.
    pub(super) fn native_level_raids(&self, level: LevelRef) -> Option<Vec<RaidRef>> {
        Some(
            self.native_level_raid_collections(level)?
                .0
                .ids()
                .into_iter()
                .map(|id| RaidRef::new(level, id))
                .collect(),
        )
    }

    /// `Raider.getCurrentRaid`: the raid a raider belongs to, even a stopped
    /// one. None for a mob that is no raider or in no raid.
    pub(super) fn native_mob_raid(&self, mob: MobRef) -> Option<RaidRef> {
        let Target::Mob(id) = self.native_entity_target(mob.entity())? else {
            return None;
        };
        let raider = self.mobs.iter().find(|m| m.id == id && !m.removed)?;
        let id = horizon_client::mobs::raider::state(raider)?.raid_id?;
        let level = self.native_entity_level(mob.entity())?;
        self.native_level_raid(level, id)
    }

    /// `Raid.getCenter`.
    pub(super) fn native_raid_center(&self, raid: RaidRef) -> Option<[i32; 3]> {
        Some(self.native_raid_data(raid)?.center)
    }

    /// `Raid.getRaidOmenLevel`: 0 to 5 in play, any value a mod set.
    pub(super) fn native_raid_omen_level(&self, raid: RaidRef) -> Option<i32> {
        Some(self.native_raid_data(raid)?.omen_level)
    }

    /// `Raid.setRaidOmenLevel`: any value, as vanilla assigns it (only
    /// `absorbRaidOmen` clamps, to 0 to 5). The raid is saved with it.
    pub(super) fn native_raid_set_omen_level(&mut self, raid: RaidRef, level: i32) -> bool {
        self.native_level_raids_mut(raid.level())
            .and_then(|raids| raids.set_omen_level(raid.id(), level))
            .is_some()
    }

    /// `Raid.getGroupsSpawned`: the waves spawned so far.
    pub(super) fn native_raid_groups_spawned(&self, raid: RaidRef) -> Option<i32> {
        Some(self.native_raid_data(raid)?.groups_spawned)
    }

    /// `Raid.numGroups`, set from `getNumGroups(difficulty)` when the raid
    /// began: 0, 3, 5 or 7. A raid of omen level 2 or more adds a bonus wave
    /// (`hasBonusWave`).
    pub(super) fn native_raid_num_groups(&self, raid: RaidRef) -> Option<i32> {
        Some(self.native_raid_data(raid)?.num_groups)
    }

    /// Whether the omen level is above 1, which adds a wave after the last
    /// (`Raid.hasBonusWave`).
    pub(super) fn native_raid_has_bonus_wave(&self, raid: RaidRef) -> Option<bool> {
        Some(self.native_raid_data(raid)?.omen_level > 1)
    }

    /// `Raid.isStarted`.
    pub(super) fn native_raid_is_started(&self, raid: RaidRef) -> bool {
        self.native_raid_data(raid).is_some_and(|r| r.started)
    }

    /// `Raid.isActive`: false once stopped.
    pub(super) fn native_raid_is_active(&self, raid: RaidRef) -> bool {
        self.native_raid_data(raid).is_some_and(|r| r.active)
    }

    /// `Raid.RaidStatus`.
    pub(super) fn native_raid_status(&self, raid: RaidRef) -> Option<NativeRaidStatus> {
        Some(match self.native_raid_data(raid)?.status {
            Status::Ongoing => NativeRaidStatus::Ongoing,
            Status::Victory => NativeRaidStatus::Victory,
            Status::Loss => NativeRaidStatus::Loss,
            Status::Stopped => NativeRaidStatus::Stopped,
        })
    }

    /// `Raid.getTotalHealth`: the health of every raider that joined, which
    /// the boss bar divides by.
    pub(super) fn native_raid_total_health(&self, raid: RaidRef) -> Option<f32> {
        Some(self.native_raid_data(raid)?.total_health)
    }

    /// `Raid.getHealthOfLivingRaiders`.
    pub(super) fn native_raid_living_health(&self, raid: RaidRef) -> Option<f32> {
        Some(
            self.native_raid_members(raid)?
                .fold(0.0, |a, m| a + m.health),
        )
    }

    /// The boss bar's progress, 0 to 1 (`Raid.raidEvent.getProgress`).
    pub(super) fn native_raid_bar_progress(&self, raid: RaidRef) -> Option<f32> {
        Some(self.native_raid_data(raid)?.bar.progress)
    }

    /// `Raid.getAllRaiders`: the living raiders that joined, in mob-list
    /// order, as entity handles.
    pub(super) fn native_raid_raiders(&self, raid: RaidRef) -> Option<Vec<EntityRef>> {
        Some(
            self.native_raid_members(raid)?
                .map(|m| EntityRef::new(raid.level().epoch(), m.id))
                .collect(),
        )
    }

    /// The living raiders of one wave (`groupRaiderMap.get(wave)`).
    pub(super) fn native_raid_wave_raiders(
        &self,
        raid: RaidRef,
        wave: i32,
    ) -> Option<Vec<EntityRef>> {
        Some(
            self.native_raid_members(raid)?
                .filter(|m| horizon_client::mobs::raider::state(m).is_some_and(|s| s.wave == wave))
                .map(|m| EntityRef::new(raid.level().epoch(), m.id))
                .collect(),
        )
    }

    /// `Raid.heroesOfTheVillage`: the UUIDs of the players who got Hero of
    /// the Village, online or not, in the order vanilla's set iterates them.
    pub(super) fn native_raid_heroes(&self, raid: RaidRef) -> Option<Vec<u128>> {
        Some(
            self.native_raid_data(raid)?
                .heroes()
                .into_iter()
                .map(ints_uuid)
                .collect(),
        )
    }

    /// `Raid.stop`: the raid ends as `stopped` and its boss bar leaves the
    /// players; the `raid_state_change` hook fires. False if the raid is
    /// already stopped.
    pub(super) fn native_raid_stop(&mut self, raid: RaidRef) -> bool {
        if self.native_raid_data(raid).is_none_or(|r| r.is_stopped()) {
            return false;
        }
        let Some(center) = self
            .native_level_raids_mut(raid.level())
            .and_then(|raids| raids.stop(raid.id()))
        else {
            return false;
        };
        self.native_raid_state_change(raid.id(), center, "stopped");
        true
    }
}

#[cfg(test)]
mod tests {
    use super::super::{execution_role::ExecutionRole, native_entity_refs::tests::with_game};
    use super::*;
    use horizon_client::mobs::{Mob, raider};
    use horizon_core::geometry::Vec3;
    use mod_api::NativeWorldEpoch;

    fn raider_mob(game: &mut Game<'_>, id: u32, raid: i32, wave: i32, health: f32) {
        let kind = game
            .res
            .mob_types()
            .get("minecraft:pillager")
            .unwrap()
            .clone();
        let mut mob = Mob::new(id, kind, Vec3::new(5.0, 70.0, 5.0), 0.0);
        mob.health = health;
        let state = raider::state_mut(&mut mob);
        state.raid_id = Some(raid);
        state.wave = wave;
        game.mobs.push(mob);
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn saved_raids_are_found_before_the_first_raid_tick() {
        with_game(|game| {
            let root = std::env::temp_dir().join(format!("raid-early-{}", std::process::id()));
            game.world_root = root.clone();
            assert!(game.raid_command_create([10, 70, 10], 2));
            game.save_raids();
            // The world opens again and reads the raids, as `finish_open_world` does.
            game.raiding = Default::default();
            game.read_raids();
            use horizon_core::random::RandomSource;
            let before = game.world.random.clone().next_long();
            let level = game.native_level(game.dim.id()).unwrap();
            let raid = game.native_level_raid_at(level, [10, 70, 10]).unwrap();
            assert_eq!(game.native_raid_center(raid), Some([10, 70, 10]));
            assert_eq!(game.native_raid_omen_level(raid), Some(2));
            // Reading drew nothing from the world random; the raid tick seeds.
            assert_eq!(game.world.random.clone().next_long(), before);
            let _ = std::fs::remove_dir_all(root);
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn raid_handles_read_a_raid_and_stop_it() {
        with_game(|game| {
            game.options.dedicated = Some(Vec::new());
            let level = game.native_level(game.dim.id()).unwrap();
            assert!(
                game.native_level_raids(level).is_none()
                    || game.native_level_raids(level).unwrap().is_empty()
            );
            assert!(game.raid_command_create([10, 70, 10], 3));
            let raid = game.native_level_raid_at(level, [12, 70, 9]).unwrap();
            assert_eq!(game.native_level_raid(level, raid.id()), Some(raid));
            assert_eq!(game.native_level_raids(level), Some(vec![raid]));
            assert_eq!(raid.level(), level);
            assert_eq!(game.native_level_raid_at(level, [1000, 70, 10]), None);
            assert_eq!(game.native_raid_center(raid), Some([10, 70, 10]));
            assert_eq!(game.native_raid_omen_level(raid), Some(3));
            assert_eq!(game.native_raid_has_bonus_wave(raid), Some(true));
            assert_eq!(game.native_raid_groups_spawned(raid), Some(0));
            assert_eq!(
                game.native_raid_num_groups(raid),
                Some(horizon_client::raid::num_groups(game.difficulty_id()))
            );
            assert!(!game.native_raid_is_started(raid));
            assert!(game.native_raid_is_active(raid));
            assert_eq!(
                game.native_raid_status(raid),
                Some(NativeRaidStatus::Ongoing)
            );
            assert_eq!(game.native_raid_total_health(raid), Some(0.0));
            assert_eq!(game.native_raid_bar_progress(raid), Some(0.0));
            assert_eq!(game.native_raid_heroes(raid), Some(Vec::new()));

            raider_mob(game, 41, raid.id(), 1, 12.5);
            raider_mob(game, 42, raid.id(), 2, 7.5);
            raider_mob(game, 43, raid.id() + 1, 1, 9.0);
            assert_eq!(game.native_raid_living_health(raid), Some(20.0));
            let first = game.native_entity(41).unwrap();
            let second = game.native_entity(42).unwrap();
            assert_eq!(game.native_raid_raiders(raid), Some(vec![first, second]));
            assert_eq!(game.native_raid_wave_raiders(raid, 2), Some(vec![second]));
            assert_eq!(game.native_raid_wave_raiders(raid, 3), Some(Vec::new()));
            let mob = game.native_mob_ref(first).unwrap();
            assert_eq!(game.native_mob_raid(mob), Some(raid));
            let other = game
                .native_mob_ref(game.native_entity(43).unwrap())
                .unwrap();
            assert_eq!(game.native_mob_raid(other), None);

            assert!(game.native_raid_set_omen_level(raid, 5));
            assert_eq!(game.native_raid_omen_level(raid), Some(5));
            assert!(game.native_raid_set_omen_level(raid, -1));
            assert_eq!(game.native_raid_omen_level(raid), Some(-1));
            assert!(game.native_raid_set_omen_level(raid, i32::MAX));
            assert_eq!(game.native_raid_omen_level(raid), Some(i32::MAX));
            assert!(game.native_raid_set_omen_level(raid, 5));
            assert_eq!(game.native_raid_omen_level(raid), Some(5));

            assert!(game.native_raid_stop(raid));
            assert_eq!(
                game.native_raid_status(raid),
                Some(NativeRaidStatus::Stopped)
            );
            assert!(!game.native_raid_is_active(raid));
            assert!(!game.native_raid_stop(raid));
            assert_eq!(game.native_level_raid_at(level, [10, 70, 10]), None);
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn parked_raid_handles_read_and_write_their_own_level() {
        with_game(|game| {
            game.options.dedicated = Some(Vec::new());
            let level = game.native_level(game.dim.id()).unwrap();
            assert!(game.raid_command_create([10, 70, 10], 3));
            let raid = level.raid_at(game, [10, 70, 10]).unwrap();
            raider_mob(game, 42, raid.id(), 2, 12.0);
            raider_mob(game, 43, raid.id(), 1, 0.0);
            raider_mob(game, 44, raid.id() + 7, 2, 18.0);
            let member = game.native_entity(42).unwrap();
            let total = raid.total_health(game);
            let groups = raid.num_groups(game);
            let progress = raid.bar_progress(game);
            let started = raid.is_started(game);

            assert!(
                game.enter_command_level(super::super::Dim::Nether)
                    .is_some()
            );
            game.read_raids();
            let current = game.native_level(game.dim.id()).unwrap();
            let (other_id, _) = game
                .raiding
                .raids_mut(current.dimension())
                .unwrap()
                .create_or_extend([100, 70, 100], 2, None, 7);
            assert_eq!(other_id, raid.id()); // Raid ids are local to a level.
            raider_mob(game, 60, other_id, 2, 19.0);
            let other = current.raid(game, other_id).unwrap();
            let other_omen = other.omen_level(game);

            assert_eq!(level.raid(game, raid.id()), Some(raid));
            assert_eq!(level.raid_at(game, [10, 70, 10]), Some(raid));
            assert_eq!(level.raids(game), Some(vec![raid]));
            assert_eq!(raid.center(game), Some([10, 70, 10]));
            assert_eq!(raid.omen_level(game), Some(3));
            assert_eq!(raid.groups_spawned(game), Some(0));
            assert_eq!(raid.num_groups(game), groups);
            assert_eq!(raid.has_bonus_wave(game), Some(true));
            assert_eq!(raid.is_started(game), started);
            assert!(raid.is_active(game));
            assert_eq!(raid.status(game), Some(NativeRaidStatus::Ongoing));
            assert_eq!(raid.total_health(game), total);
            assert_eq!(raid.living_health(game), Some(12.0));
            assert_eq!(raid.bar_progress(game), progress);
            assert_eq!(raid.raiders(game), Some(vec![member]));
            assert_eq!(raid.wave_raiders(game, 2), Some(vec![member]));
            assert_eq!(raid.wave_raiders(game, 1), Some(vec![]));
            assert_eq!(raid.heroes(game), Some(vec![]));
            game.native_level_raids_mut(level).unwrap().dirty = false;
            assert!(raid.set_omen_level(game, i32::MAX));
            assert!(game.native_level_raids_mut(level).unwrap().dirty);
            assert_eq!(raid.omen_level(game), Some(i32::MAX));
            assert_eq!(other.omen_level(game), other_omen);
            assert_eq!(other.living_health(game), Some(19.0));
            game.native_level_raids_mut(level).unwrap().dirty = false;
            assert!(raid.stop(game));
            assert!(!raid.stop(game));
            assert_eq!(raid.status(game), Some(NativeRaidStatus::Stopped));
            assert!(!raid.is_active(game));
            assert!(other.is_active(game));
            assert_eq!(game.dim, super::super::Dim::Nether);
            assert_eq!(level.raid_at(game, [10, 70, 10]), None);
            assert_eq!(level.raid(game, raid.id()), Some(raid));
            assert!(
                game.levels
                    .parked_of(level.dimension())
                    .unwrap()
                    .raiding
                    .raids(level.dimension())
                    .unwrap()
                    .dirty
            );

            assert!(
                game.enter_command_level(super::super::Dim::Overworld)
                    .is_some()
            );
            assert_eq!(raid.omen_level(game), Some(i32::MAX));
            assert_eq!(raid.status(game), Some(NativeRaidStatus::Stopped));
            assert_eq!(raid.raiders(game), Some(vec![member]));
            assert!(
                game.enter_command_level(super::super::Dim::Nether)
                    .is_some()
            );
            game.levels.clear();
            assert_eq!(raid.center(game), None);
            assert_eq!(raid.raiders(game), None);
            assert!(!raid.set_omen_level(game, 1));
            assert!(!raid.stop(game));
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn stale_closed_and_client_raid_handles_read_nothing() {
        with_game(|game| {
            game.options.dedicated = Some(Vec::new());
            let level = game.native_level(game.dim.id()).unwrap();
            assert!(game.raid_command_create([10, 70, 10], 1));
            let raid = game.native_level_raid_at(level, [10, 70, 10]).unwrap();
            let later = LevelRef::new(
                NativeWorldEpoch::new(level.epoch().get() + 1),
                level.dimension(),
            );
            let stale = RaidRef::new(later, raid.id());
            assert_eq!(game.native_raid_center(stale), None);
            assert!(!game.native_raid_is_active(stale));
            assert!(!game.native_raid_stop(stale));
            assert!(!game.native_raid_set_omen_level(stale, 2));
            assert_eq!(game.native_level_raids(later), None);
            assert_eq!(game.native_level_raid_at(later, [10, 70, 10]), None);
            let parked = LevelRef::new(level.epoch(), "minecraft:the_nether");
            assert_eq!(
                game.native_raid_status(RaidRef::new(parked, raid.id())),
                None
            );
            assert_eq!(game.native_level_raid(level, raid.id() + 7), None);
            assert_eq!(
                game.native_raid_status(raid),
                Some(NativeRaidStatus::Ongoing)
            );

            game.options.execution_role = ExecutionRole::Client;
            assert_eq!(game.native_raid_center(raid), None);
            assert_eq!(game.native_level_raid_at(level, [10, 70, 10]), None);
            assert_eq!(game.native_level_raids(level), None);
            assert!(!game.native_raid_stop(raid));
        });
    }
}
