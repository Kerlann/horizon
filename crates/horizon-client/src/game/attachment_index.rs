//! Per-dimension attachment indexes, retained independently of level swaps.

use super::{Dim, Game};
use horizon_world::attachments::index::AttachmentIndex;
use mod_api::AttachmentHolder;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

pub(super) type Indexes = HashMap<Dim, Arc<Mutex<AttachmentIndex<AttachmentHolder>>>>;

impl Game<'_> {
    /// Publish the loaded level's saved entries before listeners observe its load.
    pub(super) fn bind_level_attachments(&mut self, dim: Dim) {
        let Some(level) = self.native_level(dim.id()) else {
            return;
        };
        let index = self.attachment_indexes.entry(dim).or_default();
        self.world
            .attachments
            .bind_index(index, AttachmentHolder::Level(level));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::native_entity_refs::tests::with_game;
    use horizon_world::{
        attachments::Attachments,
        nbt::{Compound, Tag},
    };

    fn saved() -> Attachments {
        let mut names = Compound::new();
        names.insert("mana".into(), Tag::Compound(Compound::new()));
        let mut namespaces = Compound::new();
        namespaces.insert("test".into(), Tag::Compound(names));
        Attachments::from_nbt(namespaces)
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn loaded_levels_keep_independent_index_through_swaps_and_close() {
        with_game(|game| {
            let key = ("test".into(), "mana".into());
            game.world.attachments = saved();
            game.native_level_opened(Dim::Overworld);
            let overworld = game.attachment_indexes[&Dim::Overworld].clone();
            let old = overworld.lock().unwrap().snapshot(&key);
            assert_eq!(old.len(), 1);
            let old_level = game.native_level(Dim::Overworld.id()).unwrap();
            assert_eq!(old[0].holder, AttachmentHolder::Level(old_level));

            game.enter_level(Dim::Nether).unwrap();
            game.world.attachments = saved();
            game.bind_level_attachments(Dim::Nether);
            let nether = game.attachment_indexes[&Dim::Nether].clone();
            assert_eq!(nether.lock().unwrap().snapshot(&key).len(), 1);
            game.enter_level(Dim::Overworld).unwrap();
            assert!(Arc::ptr_eq(
                &overworld,
                &game.attachment_indexes[&Dim::Overworld]
            ));
            assert_eq!(overworld.lock().unwrap().snapshot(&key).len(), 1);
            game.native_levels_closing();
            assert!(game.attachment_indexes.is_empty());
            assert_eq!(old.len(), 1);
            game.native_lifecycle_open_started();
            assert!(game.attachment_indexes.is_empty());
            assert_ne!(game.native_lifecycle.epoch(), Some(old_level.epoch()));
        });
    }
}
