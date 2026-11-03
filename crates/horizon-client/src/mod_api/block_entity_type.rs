//! Typed declarations of native block entity types.
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::{Arc, OnceLock},
};

/// A registered block entity type and the blocks that may hold it.
#[derive(Clone, Debug)]
pub struct BlockEntityTypeKey(pub(super) Arc<BlockEntityType>);

#[derive(Debug, PartialEq, Eq, Hash)]
pub(super) struct BlockEntityType {
    pub id: String,
    pub blocks: Vec<String>,
    pub mod_id: &'static str,
}

impl BlockEntityTypeKey {
    /// The namespaced registry and save id.
    pub fn id(&self) -> &str {
        &self.0.id
    }

    pub(crate) fn blocks(&self) -> &[String] {
        &self.0.blocks
    }

    pub(super) fn named(id: &str, mod_id: &'static str) -> Self {
        Self(Arc::new(BlockEntityType {
            id: id.into(),
            blocks: Vec::new(),
            mod_id,
        }))
    }
}

impl AsRef<str> for BlockEntityTypeKey {
    fn as_ref(&self) -> &str {
        self.id()
    }
}

impl PartialEq for BlockEntityTypeKey {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}
impl Eq for BlockEntityTypeKey {}
impl Hash for BlockEntityTypeKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id().hash(state);
    }
}

static RESOLVED: OnceLock<HashMap<String, BlockEntityTypeKey>> = OnceLock::new();

/// Freeze every vanilla and mod key before a world or its network tables load.
pub(crate) fn resolve_block_entity_type_keys(
    ids: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<(), Vec<String>> {
    let keys: HashMap<_, _> = ids
        .into_iter()
        .map(|id| {
            let id = id.as_ref();
            (id.to_owned(), BlockEntityTypeKey::named(id, "registry"))
        })
        .collect();
    let refused: Vec<_> = super::part_table::requested_block_entity_types()
        .iter()
        .filter(|key| !keys.contains_key(key.id()))
        .map(|key| {
            format!(
                "mod {}: unknown block entity type {}",
                key.0.mod_id,
                key.id()
            )
        })
        .collect();
    if !refused.is_empty() {
        return Err(refused);
    }
    match RESOLVED.set(keys) {
        Ok(()) => Ok(()),
        Err(keys) if RESOLVED.get() == Some(&keys) => Ok(()),
        Err(_) => Err(vec![
            "block entity registry changed; restart the game".into(),
        ]),
    }
}

pub(crate) fn block_entity_type_key(id: &str) -> Option<BlockEntityTypeKey> {
    RESOLVED.get()?.get(id).cloned()
}

/// Builds one native block entity type during mod registration.
pub struct BlockEntityTypeBuilder<'a> {
    pub(super) registrar: &'a mut super::Registrar,
    pub(super) id: String,
    pub(super) blocks: Vec<String>,
}

impl BlockEntityTypeBuilder<'_> {
    /// The blocks which may contain this type.
    pub fn blocks(mut self, blocks: &[&str]) -> Self {
        self.blocks = blocks.iter().map(|block| (*block).to_owned()).collect();
        self
    }

    /// Register the type and return its typed key.
    pub fn build(self) -> BlockEntityTypeKey {
        self.registrar
            .register_block_entity_type(self.id, self.blocks)
    }
}
