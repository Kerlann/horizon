//! Read-only block entity handles for native server mods.
use super::Game;
use horizon_client::{
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
    /// API @API:block_entity_refs@.
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

    /// The block entity's type (`BlockEntity.getType`). API @API:block_entity_refs@.
    pub(super) fn native_block_entity_kind(&self, entity: BlockEntityRef) -> Option<&str> {
        let (world, block_entity) = self.native_block_entity_target(entity)?;
        let [x, y, z] = entity.position;
        let block = &self.pack.assets.registry.state(world.block(x, y, z))?.name;
        Some(save::block_entity_id(block_entity, block))
    }

    /// The block entity's position. API @API:block_entity_refs@.
    pub(super) fn native_block_entity_position(&self, entity: BlockEntityRef) -> Option<[i32; 3]> {
        self.native_block_entity_target(entity)?;
        Some(entity.position)
    }

    /// The block entity's level. API @API:block_entity_refs@.
    pub(super) fn native_block_entity_level(&self, entity: BlockEntityRef) -> Option<LevelRef> {
        self.native_block_entity_target(entity)?;
        Some(entity.level)
    }

    /// The data saved without identity or position metadata
    /// (`BlockEntity.saveWithoutMetadata`). API @API:block_entity_refs@.
    pub(super) fn native_block_entity_data(&self, entity: BlockEntityRef) -> Option<Compound> {
        let (world, block_entity) = self.native_block_entity_target(entity)?;
        let [x, y, z] = entity.position;
        let block = &self.pack.assets.registry.state(world.block(x, y, z))?.name;
        save_without_metadata(world, (x, y, z), block_entity, block)
    }
}

fn save_without_metadata(
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
    for key in ["id", "x", "y", "z"] {
        data.remove(&key.into());
    }
    Some(data)
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
}
