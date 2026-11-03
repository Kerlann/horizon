//! Accepted native block entity membership, frozen before resources or worlds load.
use std::{collections::BTreeMap, sync::OnceLock};

#[derive(Debug, PartialEq, Eq)]
pub struct BlockEntityTypes {
    blocks: BTreeMap<String, String>,
    /// Names in protocol order: a reload may not change numeric wire IDs either.
    wire: Vec<String>,
}

impl BlockEntityTypes {
    /// Membership has already been checked by the content plan.
    pub fn new(blocks: BTreeMap<String, String>, wire: Vec<String>) -> Self {
        Self { blocks, wire }
    }

    pub fn type_for(&self, block: &str) -> Option<&str> {
        self.blocks.get(block).map(String::as_str)
    }

    pub fn fits(&self, id: &str, block: &str) -> bool {
        self.type_for(block) == Some(id)
    }
}

static TYPES: OnceLock<BlockEntityTypes> = OnceLock::new();

/// Reading before startup installation never freezes an empty table.
pub fn type_for(block: &str) -> Option<&'static str> {
    current()?.type_for(block)
}

pub fn current() -> Option<&'static BlockEntityTypes> {
    TYPES.get()
}

/// An identical setup is harmless; changing membership or wire IDs needs a restart.
pub fn install(types: BlockEntityTypes) -> Result<(), &'static str> {
    freeze(&TYPES, types)
}

fn freeze(slot: &OnceLock<BlockEntityTypes>, types: BlockEntityTypes) -> Result<(), &'static str> {
    match slot.set(types) {
        Ok(()) => Ok(()),
        Err(types) if slot.get() == Some(&types) => Ok(()),
        Err(_) => Err("block entity types changed; restart the game to use them"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_membership_and_wire_ids_are_frozen_together() {
        let slot = OnceLock::new();
        let types = || {
            BlockEntityTypes::new(
                [
                    ("test:press".into(), "test:machine".into()),
                    ("test:mill".into(), "test:machine".into()),
                ]
                .into(),
                vec!["minecraft:sign".into(), "test:machine".into()],
            )
        };
        freeze(&slot, types()).unwrap();
        freeze(&slot, types()).unwrap();
        let accepted = slot.get().unwrap();
        for block in ["test:press", "test:mill"] {
            assert!(accepted.fits("test:machine", block));
            assert!(!accepted.fits("test:unknown", block));
        }
        for block in ["test:casing", "other:press", "minecraft:stone"] {
            assert!(!accepted.fits("test:machine", block));
        }
        let mut changed = types();
        changed.blocks.remove("test:mill");
        assert!(freeze(&slot, changed).is_err());
        let mut changed = types();
        changed.wire.swap(0, 1);
        assert!(freeze(&slot, changed).is_err());
        assert_eq!(slot.get(), Some(&types()));
    }
}
