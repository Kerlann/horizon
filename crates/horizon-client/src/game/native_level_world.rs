//! A level's world settings for native server mods, through a `LevelRef`
//! (docs/modding/src/how-to/native-mods.md, "Entity and level handles", #7387): the seed, the
//! difficulty, game rules and weather. They return `None` on a client or for
//! a handle of an earlier open.
use super::{Dim, Game};
use horizon_client::command::vanilla::DIFFICULTIES;
use horizon_client::world::World;
use horizon_core::position::BlockPos;
use horizon_core::random::LegacyRandom;
use horizon_world::level::{Dimension as LevelDimension, LevelAccess};
use horizon_world::registry::BlockStateId;
use mod_api::{FeatureFlags, GameRuleValue, LevelRef};

fn weather_for_level(
    shape: horizon_client::chunk::LevelShape,
    is_end: bool,
    levels: (f32, f32),
) -> (f32, f32) {
    if shape.sky_light && !shape.has_ceiling && !is_end {
        levels
    } else {
        (0.0, 0.0)
    }
}

/// `Level.isRaining` and `isThundering` from the rain and thunder levels.
fn weather_flags((rain, thunder): (f32, f32)) -> (bool, bool) {
    (rain > 0.2, thunder > 0.9)
}

/// A read-only view of one level's column for `Level.isRainingAt`, so a level parked
/// while the server ticks another answers like the one in place.
struct RainView<'a> {
    world: &'a World,
    facts: &'a horizon_client::blocks::BlockFacts,
    biomes: &'a horizon_client::biome::Biomes,
    dimension: LevelDimension,
    raining: bool,
    /// `LevelAccess::random` asks for a generator; the read draws none.
    random: LegacyRandom,
}

impl LevelAccess for RainView<'_> {
    fn block_state(&self, pos: BlockPos) -> BlockStateId {
        self.world.block(pos.x, pos.y, pos.z)
    }
    fn set_raw(&mut self, _pos: BlockPos, _state: BlockStateId) -> Option<BlockStateId> {
        None
    }
    fn is_loaded(&self, pos: BlockPos) -> bool {
        self.world.loaded(pos.x, pos.z)
    }
    fn random(&mut self) -> &mut LegacyRandom {
        &mut self.random
    }
    fn game_time(&self) -> i64 {
        self.world.game_time
    }
    fn min_y(&self) -> i32 {
        self.world.shape.min_y
    }
    fn max_y(&self) -> i32 {
        self.world.shape.max_y()
    }
    fn sky_light(&self, pos: BlockPos) -> i32 {
        i32::from(self.world.light(pos.x, pos.y, pos.z) >> 4)
    }
    fn world_surface(&self, x: i32, z: i32) -> i32 {
        self.world.world_surface(x, z, |s| self.facts.is_air(s))
    }
    fn biome_name(&self, pos: BlockPos) -> Option<&str> {
        let chunk = self.world.chunks.get(&(pos.x >> 4, pos.z >> 4))?;
        Some(&self.biomes.get(chunk.biome(pos.x, pos.y, pos.z)).name)
    }
    fn is_raining(&self) -> bool {
        self.raining
    }
    fn dimension(&self) -> LevelDimension {
        self.dimension
    }
}

impl Game<'_> {
    /// The enabled feature flags as this side knows them: a client's are the server's, sent at
    /// join; a server's are its world's `enabled_features`.
    pub(super) fn enabled_feature_ids(&self) -> &[String] { if let Some(__l) = crate::points_generated::P3848.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(self, &mut __a)); }
        self.remote
            .as_ref()
            .map_or(&self.level.enabled_features, |remote| &remote.features)
    }
    /// The world's seed (`WorldOptions.seed`), as `/seed` shows it.
    pub(super) fn native_level_seed(&self, level: LevelRef) -> Option<i64> { if let Some(__l) = crate::points_generated::P3847.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(self, &mut __a)); }
        self.native_level_world(level)?;
        Some(self.level.seed)
    }
    /// The difficulty (`Level.getDifficulty`): `peaceful`, `easy`, `normal`
    /// or `hard`, as `/difficulty` names it.
    pub(super) fn native_level_difficulty(&self, level: LevelRef) -> Option<&'static str> { if let Some(__l) = crate::points_generated::P3846.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(self, &mut __a)); }
        self.native_level_world(level)?;
        DIFFICULTIES.get(usize::from(self.ui.difficulty)).copied()
    }
    /// Game rule `rule` (`minecraft:keep_inventory` or `keep_inventory`), as
    /// `/gamerule` queries it: None for an unknown rule.
    pub(super) fn native_level_game_rule(
        &self,
        level: LevelRef,
        rule: &str,
    ) -> Option<GameRuleValue> { if let Some(__l) = crate::points_generated::P3845.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(self, &mut __a)); }
        self.native_level_world(level)?;
        let r = horizon_client::gamerules::rule(rule)?;
        self.ui.rules.get(r.name)
    }
    /// The rain and thunder levels, each 0.0 to 1.0 (`Level.getRainLevel`,
    /// `getThunderLevel`): both 0 in a level with no weather, as the Nether
    /// and the End.
    pub(super) fn native_level_weather(&self, level: LevelRef) -> Option<(f32, f32)> { if let Some(__l) = crate::points_generated::P3844.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(self, &mut __a)); }
        let world = self.native_level_world(level)?;
        Some(weather_for_level(
            world.shape,
            level.dimension() == super::Dim::End.id(),
            world.weather.levels(1.0),
        ))
    }
    /// Whether it rains (`Level.isRaining`: a rain level above 0.2).
    pub(super) fn native_level_is_raining(&self, level: LevelRef) -> Option<bool> { if let Some(__l) = crate::points_generated::P3843.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(self, &mut __a)); }
        Some(weather_flags(self.native_level_weather(level)?).0)
    }
    /// `Level.isRainingAt(pos)` in the level a handle names, the one in place or a parked one:
    /// it rains there and `pos` is under open sky in a biome that rains. None for a stale
    /// handle, on a client, and where `pos` is not loaded or outside the world's height; a
    /// read never loads a chunk.
    pub(super) fn native_level_is_raining_at(
        &self,
        level: LevelRef,
        pos: [i32; 3],
    ) -> Option<bool> { let (pos,) = match crate::points_generated::P3842.get() { Some(__l) => { let mut __a = (pos,); mod_api::point::fire(__l, |__f| __f(self, &mut __a)); __a } None => (pos,) };
        let world = self.native_level_world(level)?;
        if !world.is_loaded_at(pos[0], pos[1], pos[2]) {
            return None;
        }
        let view = RainView {
            world,
            facts: self.res.facts(),
            biomes: world_biomes!(self),
            dimension: Dim::from_id(level.dimension()).level(),
            raining: self.native_level_is_raining(level)?,
            random: LegacyRandom::new(0),
        };
        Some(
            self.res
                .behaviours()
                .is_raining_at(&view, BlockPos::new(pos[0], pos[1], pos[2])),
        )
    }
    /// `LevelReader.enabledFeatures` of the level a handle names. Every level of a world shares
    /// the world's flags. None for a stale handle and on a client.
    pub(super) fn native_level_enabled_features(
        &self,
        level: LevelRef,
    ) -> Option<FeatureFlags<'_>> { if let Some(__l) = crate::points_generated::P3841.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(self, &mut __a)); }
        self.native_level_world(level)?;
        Some(FeatureFlags::new(self.enabled_feature_ids()))
    }
    /// Whether it thunders (`Level.isThundering`: a thunder level above 0.9).
    pub(super) fn native_level_is_thundering(&self, level: LevelRef) -> Option<bool> { if let Some(__l) = crate::points_generated::P3840.get() { let mut __a = (); mod_api::point::fire(__l, |__f| __f(self, &mut __a)); }
        Some(weather_flags(self.native_level_weather(level)?).1)
    }
}

#[cfg(test)]
mod tests {
    use super::super::native_entity_refs::tests::with_game;
    use horizon_client::chunk::Chunk;
    use mod_api::{FeatureFlags, LevelHost};
    use std::sync::Arc;

    /// A chunk of air with `biome` in every cell: full sky light everywhere.
    fn chunk_in(game: &super::Game<'_>, x: i32, biome: &str) -> Arc<Chunk> {
        let id = game.res.biomes().id(biome);
        let mut chunk = Chunk::empty(x, 0);
        for qy in 0..chunk.sections.len() as i32 * 4 {
            for (qx, qz) in (0..4).flat_map(|qx| (0..4).map(move |qz| (qx, qz))) {
                chunk.set_biome_cell(qx, qy, qz, id);
            }
        }
        Arc::new(chunk)
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn rain_at_a_position_checks_sky_roof_and_biome() {
        with_game(|game| {
            game.install_live_features();
            for (x, biome) in [
                (0, "minecraft:plains"),
                (1, "minecraft:desert"),
                (2, "minecraft:snowy_plains"),
            ] {
                let chunk = chunk_in(game, x, biome);
                game.world.chunks.insert((x, 0), chunk);
            }
            let stone = game
                .pack
                .assets
                .registry
                .default_state("minecraft:stone")
                .unwrap();
            game.world.set(3, 100, 3, stone).unwrap();
            let level = game.native_level(game.dim.id()).unwrap();
            let chunks = game.world.chunks.len();
            let plains = [1, 70, 1];
            let roofed = [3, 70, 3];
            let desert = [17, 70, 1];
            let snowy = [33, 70, 1];
            let unloaded = [16 * 9, 70, 1];

            game.world.weather.rain_level = (1.0, 1.0);
            for (pos, rains) in [
                (plains, true),
                (roofed, false),
                (desert, false),
                (snowy, false),
            ] {
                assert_eq!(
                    game.native_level_is_raining_at(level, pos),
                    Some(rains),
                    "{pos:?}"
                );
            }
            assert_eq!(game.native_level_is_raining_at(level, unloaded), None);
            assert_eq!(game.native_level_is_raining_at(level, [1, 4000, 1]), None);
            game.with_level(|_, host, _| {
                assert!(host.is_raining_at(plains));
                assert!(!host.is_raining_at(roofed));
                assert!(!host.is_raining_at(desert));
                assert!(!host.is_raining_at(snowy));
                assert!(!host.is_raining_at(unloaded));
            });

            game.world.weather.rain_level = (0.0, 0.0);
            assert_eq!(game.native_level_is_raining_at(level, plains), Some(false));
            game.with_level(|_, host, _| assert!(!host.is_raining_at(plains)));
            assert_eq!(game.world.chunks.len(), chunks, "nothing was loaded");
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn enabled_features_default_to_vanilla_and_follow_the_world() {
        with_game(|game| {
            let level = game.native_level(game.dim.id()).unwrap();
            let flags = game.native_level_enabled_features(level).unwrap();
            assert!(flags.contains("minecraft:vanilla"));
            assert!(flags.contains("vanilla"));
            assert!(!flags.contains("minecraft:redstone_experiments"));
            game.level.enabled_features = vec![
                "minecraft:vanilla".into(),
                "minecraft:redstone_experiments".into(),
            ];
            let flags = game.native_level_enabled_features(level).unwrap();
            assert!(flags.contains("minecraft:redstone_experiments"));
            assert_eq!(flags.iter().count(), 2);
            assert_eq!(
                FeatureFlags::new(&[]).iter().collect::<Vec<_>>(),
                ["minecraft:vanilla"]
            );
            let ids: Vec<String> = flags.iter().map(str::to_owned).collect();
            game.with_level(|_, host, _| {
                assert_eq!(host.enabled_features().iter().collect::<Vec<_>>(), ids);
            });
        });
    }
    #[test]
    fn weather_requires_skylight_no_ceiling_and_not_end() {
        use horizon_client::chunk::LevelShape;
        let rain = (1.0, 0.5);
        assert_eq!(
            super::weather_for_level(LevelShape::OVERWORLD, false, rain),
            rain
        );
        assert_eq!(
            super::weather_for_level(LevelShape::NETHER, false, rain),
            (0.0, 0.0)
        );
        assert_eq!(
            super::weather_for_level(LevelShape::END, true, rain),
            (0.0, 0.0)
        );
        let mut skyless = LevelShape::OVERWORLD;
        skyless.sky_light = false;
        assert_eq!(super::weather_for_level(skyless, false, rain), (0.0, 0.0));
        assert_eq!(
            super::weather_for_level(LevelShape::OVERWORLD.with_ceiling(true), false, rain),
            (0.0, 0.0)
        );
    }

    #[test]
    fn rain_and_thunder_start_above_vanillas_thresholds() {
        assert_eq!(super::weather_flags((0.2, 0.0)), (false, false));
        assert_eq!(super::weather_flags((0.21, 0.9)), (true, false));
        assert_eq!(super::weather_flags((1.0, 0.95)), (true, true));
    }
}
