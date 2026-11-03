//! Block entity handles for native server mods: reads, and the writes `/data merge block`
//! and `BlockEntity.setChanged` make. A write inside `host.transaction` is journaled
//! (`transactions.rs`) and tells the world at the root commit.
use super::{Game, transactions::After};
use horizon_client::{
    block_entity_nbt,
    containers::BlockEntity,
    mod_api::{BlockEntityRef, LevelRef},
    save,
    world::World,
};
use horizon_world::nbt::{Compound, Tag};

impl Game<'_> {
    fn native_block_entity_target(&self, entity: BlockEntityRef) -> Option<(&World, &BlockEntity)> {
        let world = self.native_level_world(entity.level)?;
        Some((world, loaded_block_entity(world, entity.position)?))
    }

    /// Find a block entity in a loaded chunk of an open server level. A block
    /// entity temporarily taken out for its tick cannot be read in that tick.
    pub(super) fn native_block_entity_ref(
        &self,
        level: LevelRef,
        pos: [i32; 3],
    ) -> Option<BlockEntityRef> {
        let entity = BlockEntityRef {
            level,
            position: pos,
        };
        self.native_block_entity_target(entity).map(|_| entity)
    }

    /// The block entity's type (`BlockEntity.getType`).
    pub(super) fn native_block_entity_kind(&self, entity: BlockEntityRef) -> Option<&str> {
        let (world, block_entity) = self.native_block_entity_target(entity)?;
        let [x, y, z] = entity.position;
        let block = &self.pack.assets.registry.state(world.block(x, y, z))?.name;
        Some(save::block_entity_id(block_entity, block))
    }

    /// The block entity's position.
    pub(super) fn native_block_entity_position(&self, entity: BlockEntityRef) -> Option<[i32; 3]> {
        self.native_block_entity_target(entity)?;
        Some(entity.position)
    }

    /// The block entity's level.
    pub(super) fn native_block_entity_level(&self, entity: BlockEntityRef) -> Option<LevelRef> {
        self.native_block_entity_target(entity)?;
        Some(entity.level)
    }

    /// The data saved without identity or position metadata
    /// (`BlockEntity.saveWithoutMetadata`).
    pub(super) fn native_block_entity_data(&self, entity: BlockEntityRef) -> Option<Compound> {
        let (world, block_entity) = self.native_block_entity_target(entity)?;
        let [x, y, z] = entity.position;
        let block = &self.pack.assets.registry.state(world.block(x, y, z))?.name;
        save_without_metadata(world, (x, y, z), block_entity, block)
    }

    /// The block entity's key and block name when a write may change it: a loaded block
    /// entity of the level in place.
    fn native_block_entity_write_target(
        &self,
        entity: BlockEntityRef,
    ) -> Option<((i32, i32, i32), String)> {
        if !self.native_level_in_place(entity.level) {
            return None;
        }
        let (world, _) = self.native_block_entity_target(entity)?;
        let [x, y, z] = entity.position;
        let block = self
            .pack
            .assets
            .registry
            .state(world.block(x, y, z))?
            .name
            .clone();
        Some(((x, y, z), block))
    }

    /// Merges `data` into the block entity's saved data and loads it, as `/data merge block`
    /// does (`BlockDataAccessor.setData`: `loadWithComponents`, `setChanged`, then
    /// `sendBlockUpdated`, so comparators re-read it and clients get the update). `id`, `x`,
    /// `y` and `z` in `data` are ignored. True when the data changed; false for a stale
    /// handle, a client, another level than the one in place, data nested too deep or that
    /// changes nothing, or a result the block entity can't load. Inside a transaction an
    /// abort puts the block entity, its name, `lock`, loot table roll and attachments back.
    pub(super) fn native_block_entity_merge_data(
        &mut self,
        entity: BlockEntityRef,
        data: Compound,
    ) -> bool {
        let Some((key, block)) = self.native_block_entity_write_target(entity) else {
            return false;
        };
        let mut nbt = data;
        for field in ["id", "x", "y", "z"] {
            nbt.remove(&field.into());
        }
        let nbt = Tag::Compound(nbt);
        if horizon_client::nbt_path::too_deep(&nbt, 0) {
            return false;
        }
        let Some(old) = self
            .world
            .block_entities
            .get(&key)
            .and_then(|current| save_with_metadata(&self.world, key, current, &block))
            .map(Tag::Compound)
        else {
            return false;
        };
        let merged = horizon_client::data_cmd::merged(&old, &nbt);
        if merged == old {
            return false;
        }
        self.journal_block_entity(key);
        if !self.load_block_entity_data(key, &merged) {
            return false;
        }
        let kept = self.world.block_entities.get(&key);
        if kept.is_some_and(|kept| {
            !matches!(kept, BlockEntity::Banner(_) | BlockEntity::Sign(_))
                && nameable(key, kept, &block)
        }) {
            match save::text(merged.get("CustomName")) {
                Some(name) => self.world.block_names.insert(key, name),
                None => self.world.block_names.remove(&key),
            };
        }
        self.after_commit(After::Updated(key));
        self.after_commit(After::Changed(key));
        true
    }

    /// `BlockEntity.setChanged`: the chunk is saved again and comparators re-read the
    /// container (`updateNeighbourForOutputSignal`). Inside a transaction it runs once at the
    /// root commit. False for a stale handle, a client or another level than the one in place.
    pub(super) fn native_block_entity_set_changed(&mut self, entity: BlockEntityRef) -> bool {
        let Some((key, _)) = self.native_block_entity_write_target(entity) else {
            return false;
        };
        self.after_commit(After::Changed(key));
        true
    }

    /// The custom name (`Nameable.getCustomName`) as plain text. None when the block entity
    /// has none or isn't nameable (chests, barrels, furnaces, hoppers, brewing stands,
    /// banners, beacons, enchanting tables, command blocks...), or for a stale handle or a
    /// client.
    pub(super) fn native_block_entity_name(&self, entity: BlockEntityRef) -> Option<String> {
        let (world, block_entity) = self.native_block_entity_target(entity)?;
        let [x, y, z] = entity.position;
        match block_entity {
            BlockEntity::Banner(banner) => banner.name.clone(),
            _ => world.block_names.get(&(x, y, z)).cloned(),
        }
    }

    /// Sets the custom name, or clears it with None, as an anvil-renamed item placed with
    /// `components` sets `CustomName`. Plain text only. Clients get a block entity update.
    /// False for a block entity that isn't nameable, a stale handle, a client or another
    /// level than the one in place. Inside a transaction an abort puts the name back.
    pub(super) fn native_block_entity_set_name(
        &mut self,
        entity: BlockEntityRef,
        name: Option<&str>,
    ) -> bool {
        let Some((key, block)) = self.native_block_entity_write_target(entity) else {
            return false;
        };
        let nameable = self
            .world
            .block_entities
            .get(&key)
            .is_some_and(|current| nameable(key, current, &block));
        if !nameable {
            return false;
        }
        self.journal_block_entity(key);
        match self.world.block_entities.get_mut(&key) {
            Some(BlockEntity::Banner(banner)) => banner.name = name.map(str::to_owned),
            _ => {
                match name {
                    Some(name) => self.world.block_names.insert(key, name.to_owned()),
                    None => self.world.block_names.remove(&key),
                };
            }
        }
        self.after_commit(After::Updated(key));
        true
    }
}

/// The block entity as `/data get block` shows it (`saveWithFullMetadata`), with the custom
/// name Horizon keeps beside the model.
fn save_with_metadata(
    world: &World,
    pos: (i32, i32, i32),
    entity: &BlockEntity,
    block: &str,
) -> Option<Compound> {
    let mut tag = save::block_entity_data(pos, entity, block, world.block_lock(pos));
    Game::restore_loot_table(world, pos, &mut tag);
    let Tag::Compound(mut data) = tag else {
        return None;
    };
    if let Some(name) = world.block_names.get(&pos) {
        data.insert("CustomName".into(), Tag::String(name.as_str().into()));
    }
    Some(data)
}

fn save_without_metadata(
    world: &World,
    pos: (i32, i32, i32),
    entity: &BlockEntity,
    block: &str,
) -> Option<Compound> {
    let mut data = save_with_metadata(world, pos, entity, block)?;
    for key in ["id", "x", "y", "z"] {
        data.remove(&key.into());
    }
    Some(data)
}

/// Whether the block entity keeps a `CustomName` (`Nameable`): its saved form does.
fn nameable(pos: (i32, i32, i32), entity: &BlockEntity, block: &str) -> bool {
    let mut tag = save::encode_block_entity(pos, entity, block);
    if let Tag::Compound(map) = &mut tag {
        map.insert("CustomName".into(), Tag::String("x".into()));
    }
    block_entity_nbt::canonical(&tag).is_some_and(|tag| tag.get("CustomName").is_some())
}

fn loaded_block_entity(world: &World, [x, y, z]: [i32; 3]) -> Option<&BlockEntity> {
    if !world.loaded(x, z) {
        return None;
    }
    world
        .block_entities
        .get(&(x, y, z))
        .filter(|entity| !save::block_entity_placeholder(entity))
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_client::chunk::Chunk;
    use horizon_world::registry::BlockStateId;
    use std::sync::Arc;

    #[test]
    fn saved_data_omits_metadata_but_keeps_contents() {
        let world = World::new(BlockStateId(0), 0);
        let entity = BlockEntity::Chest(vec![None; 27]);
        let data = save_without_metadata(&world, (1, 64, 2), &entity, "minecraft:chest").unwrap();
        for key in ["id", "x", "y", "z"] {
            assert!(!data.contains_key(&key.into()));
        }
        assert!(data.contains_key(&"Items".into()));
    }

    #[test]
    fn lookup_requires_loaded_chunk_and_tracks_removal_and_replacement() {
        let mut world = World::new(BlockStateId(0), 0);
        let pos = [-1, 64, 0];
        let key = (-1, 64, 0);
        world.block_entities.insert(key, BlockEntity::Chest(vec![]));
        assert!(loaded_block_entity(&world, pos).is_none());
        world.chunks.insert((-1, 0), Arc::new(Chunk::empty(-1, 0)));
        assert!(matches!(
            loaded_block_entity(&world, pos),
            Some(BlockEntity::Chest(_))
        ));
        world.block_entities.remove(&key);
        assert!(loaded_block_entity(&world, pos).is_none());
        world
            .block_entities
            .insert(key, BlockEntity::DaylightDetector);
        assert!(matches!(
            loaded_block_entity(&world, pos),
            Some(BlockEntity::DaylightDetector)
        ));
        world
            .block_entities
            .insert(key, BlockEntity::Mod(Box::default()));
        assert!(loaded_block_entity(&world, pos).is_none());
    }

    fn compound(snbt: &str) -> Compound {
        match horizon_world::nbt::parse_snbt(snbt).unwrap().0 {
            Tag::Compound(c) => c,
            _ => panic!("not a compound"),
        }
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn writes_merge_data_name_and_roll_back_with_a_transaction() {
        use super::super::native_entity_refs::tests::with_game;
        use horizon_client::mod_api::{Host, NativeBlockUpdateFlags};
        with_game(|game| {
            game.options.dedicated = Some(Vec::new());
            game.world
                .chunks
                .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
            let level = game.native_level("minecraft:overworld").unwrap();
            let flags = NativeBlockUpdateFlags::NEIGHBORS;
            let states = ["minecraft:chest", "minecraft:daylight_detector"];
            for (i, name) in states.into_iter().enumerate() {
                let state = game.pack.assets.registry.default_state(name).unwrap();
                assert!(game.native_level_set_block(level, [i as i32 * 2, 64, 0], state, flags));
            }
            let chest = game.native_block_entity_ref(level, [0, 64, 0]).unwrap();
            let sensor = game.native_block_entity_ref(level, [2, 64, 0]).unwrap();
            let stick = "{Items:[{Slot:3b,id:\"minecraft:stick\",count:3}]}";
            let slot3 = |game: &Game<'_>| match game.world.block_entities.get(&(0, 64, 0)) {
                Some(BlockEntity::Chest(items)) => items[3].as_ref().map(|s| s.count),
                _ => panic!("chest block entity missing"),
            };

            // A merge loads the data, ignores the identity keys and keeps the rest.
            assert!(chest.merge_data(
                game,
                compound(&format!("{{id:\"minecraft:furnace\",x:9,{}", &stick[1..]))
            ));
            assert_eq!(slot3(game), Some(3));
            assert_eq!(chest.kind(game), Some("minecraft:chest"));
            assert_eq!(chest.position(game), Some([0, 64, 0]));
            assert!(!chest.merge_data(game, compound(stick)), "no change");
            assert!(chest.merge_data(game, compound("{Items:[]}")));
            assert_eq!(slot3(game), None);

            // Names: a chest has one, a daylight detector does not.
            assert_eq!(chest.name(game), None);
            assert!(chest.set_name(game, Some("Loot")));
            assert_eq!(chest.name(game), Some("Loot".to_owned()));
            let data = chest.data(game).unwrap();
            assert_eq!(
                data.get(&"CustomName".into()),
                Some(&Tag::String("Loot".into()))
            );
            assert!(!sensor.set_name(game, Some("Nope")));
            assert_eq!(sensor.name(game), None);
            assert!(chest.set_name(game, None));
            assert_eq!(chest.name(game), None);

            // An abort puts back data and name; a commit keeps them.
            let host: &mut dyn Host = game;
            let aborted: Result<(), ()> = host.transaction(|host, _| {
                assert!(chest.merge_data(host, compound(stick)));
                assert!(chest.set_name(host, Some("Gone")));
                assert!(chest.set_changed(host));
                Err(())
            });
            assert_eq!(aborted, Err(()));
            assert_eq!((slot3(game), chest.name(game)), (None, None));
            assert!(game.transactions.is_empty());
            let host: &mut dyn Host = game;
            let committed: Result<(), ()> = host.transaction(|host, _| {
                assert!(chest.merge_data(host, compound(stick)));
                assert!(chest.set_name(host, Some("Kept")));
                Ok(())
            });
            assert_eq!(committed, Ok(()));
            assert_eq!(
                (slot3(game), chest.name(game)),
                (Some(3), Some("Kept".to_owned()))
            );

            // Clients are told: the block log names the position, and the chunk is dirty.
            game.world.block_log = Some(Vec::new());
            game.world.modified.clear();
            assert!(chest.set_name(game, Some("Told")));
            assert_eq!(game.world.block_log, Some(vec![(0, 64, 0)]));
            assert!(game.world.modified.contains(&(0, 0)));
            game.world.modified.clear();
            assert!(chest.set_changed(game));
            assert!(game.world.modified.contains(&(0, 0)));

            // A removed block entity is a stale handle.
            game.world.block_entities.remove(&(0, 64, 0));
            assert!(!chest.set_changed(game));
            assert!(!chest.merge_data(game, compound(stick)));
            assert!(!chest.set_name(game, Some("x")));
            assert_eq!(chest.name(game), None);
        });
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn an_abort_puts_back_the_lock_and_the_loot_table_roll() {
        use super::super::native_entity_refs::tests::with_game;
        use horizon_client::mod_api::{Host, NativeBlockUpdateFlags};
        with_game(|game| {
            game.options.dedicated = Some(Vec::new());
            game.world
                .chunks
                .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
            let level = game.native_level("minecraft:overworld").unwrap();
            let chest = game.pack.assets.registry.default_state("minecraft:chest");
            let flags = NativeBlockUpdateFlags::NEIGHBORS;
            assert!(game.native_level_set_block(level, [0, 64, 0], chest.unwrap(), flags));
            let chest = game.native_block_entity_ref(level, [0, 64, 0]).unwrap();
            let key = (0, 64, 0);
            let lock = compound("{lock:{items:\"minecraft:stick\"}}");
            let table = compound("{LootTable:\"minecraft:chests/simple_dungeon\"}");
            let before = game.world.block_entity_extras(key);

            let host: &mut dyn Host = game;
            let aborted: Result<(), ()> = host.transaction(|host, _| {
                assert!(chest.merge_data(host, lock.clone()));
                assert!(chest.merge_data(host, table.clone()));
                Err(())
            });
            assert_eq!(aborted, Err(()));
            assert_eq!(game.world.block_entity_extras(key), before);
            assert!(game.world.block_lock(key).is_none());

            let host: &mut dyn Host = game;
            let committed: Result<(), ()> = host.transaction(|host, _| {
                assert!(chest.merge_data(host, lock.clone()));
                assert!(chest.merge_data(host, table.clone()));
                Ok(())
            });
            assert_eq!(committed, Ok(()));
            assert!(game.world.block_lock(key).is_some());
            assert_ne!(game.world.block_entity_extras(key), before);
        });
    }
}
