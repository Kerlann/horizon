//! Load-owned declarations and resolved mappings for registration-slot keys.
use crate::{
    BlockEntityTypeKey, BlockKey, BlockStateId, BlockStateRange, EffectKey, FluidKey, ItemKey,
};
use std::collections::HashMap;

#[derive(Clone, Default)]
pub(crate) struct Requests {
    pub(crate) blocks: Names,
    pub(crate) fluids: Names,
    pub(crate) declared_fluids: HashMap<String, &'static str>,
    pub(crate) items: Names,
    pub(crate) effects: Names,
    pub(crate) block_entity_types: Names,
}

#[derive(Clone, Default)]
pub(crate) struct Names {
    entries: Vec<Request>,
    pub(crate) slots: HashMap<String, u32>,
    utf16_slots: HashMap<Vec<u16>, u32>,
    world_fluids: Vec<Option<FluidKey>>,
}

#[derive(Clone)]
struct Request {
    id: String,
    mods: Vec<&'static str>,
}

impl Names {
    pub(crate) fn request(&mut self, mod_id: &'static str, id: &str) -> Result<u32, String> {
        // Match the registry boundary's identifier classification, including the default namespace.
        let id = if id.contains(':') {
            id.to_owned()
        } else {
            format!("minecraft:{id}")
        };
        if !horizon_mods::content::namespaced_id(&id) {
            return Err(format!("mod {mod_id}: invalid registry key {id:?}"));
        }
        let slot = *self.slots.entry(id.clone()).or_insert_with(|| {
            let slot = u32::try_from(self.entries.len()).expect("registry key slots exhausted");
            self.entries.push(Request {
                id,
                mods: Vec::new(),
            });
            slot
        });
        let mods = &mut self.entries[slot as usize].mods;
        if !mods.contains(&mod_id) {
            mods.push(mod_id);
        }
        Ok(slot)
    }

    pub(crate) fn id(&self, slot: u32) -> Option<&str> {
        self.entries
            .get(slot as usize)
            .map(|entry| entry.id.as_str())
    }

    pub(crate) fn resolve<T, E>(
        &self,
        mut resolve: impl FnMut(&str) -> Option<T>,
        mut unknown: impl FnMut(&'static str, &str) -> E,
    ) -> Result<Vec<T>, Vec<E>> {
        let mut values = Vec::with_capacity(self.entries.len());
        let mut errors = Vec::new();
        for entry in &self.entries {
            match resolve(&entry.id) {
                Some(value) => values.push(value),
                None => errors.extend(entry.mods.iter().map(|mod_id| unknown(mod_id, &entry.id))),
            }
        }
        if errors.is_empty() {
            Ok(values)
        } else {
            Err(errors)
        }
    }
}

impl BlockKey {
    /// Validate an incoming slot against declarations, not a world's resolved mappings.
    pub(crate) fn from_slot(slot: u32) -> Result<Self, &'static str> {
        crate::part_table::with_registering_keys(|registering| {
            registering
                .unwrap_or(crate::part_table::requested_keys())
                .blocks
                .id(slot)
                .ok_or("unknown BlockKey slot")?;
            Ok(Self(slot))
        })
    }
}

impl FluidKey {
    pub(crate) fn from_slot(slot: u32) -> Result<Self, &'static str> {
        crate::part_table::with_registering_keys(|registering| {
            let names = match registering {
                Some(requests) => &requests.fluids,
                None => crate::part_table::resolved_fluids()
                    .get()
                    .unwrap_or(&crate::part_table::requested_keys().fluids),
            };
            names.id(slot).ok_or("unknown FluidKey slot")?;
            Ok(Self(slot))
        })
    }
}

impl ItemKey {
    pub(crate) fn from_slot(slot: u32) -> Result<Self, &'static str> {
        crate::part_table::with_registering_keys(|registering| {
            registering
                .unwrap_or(crate::part_table::requested_keys())
                .items
                .id(slot)
                .ok_or("unknown ItemKey slot")?;
            Ok(Self(slot))
        })
    }
}

impl EffectKey {
    /// Resolved effects also expose registry entries no mod explicitly requested.
    pub(crate) fn from_slot(slot: u32) -> Result<Self, &'static str> {
        crate::part_table::with_registering_keys(|registering| {
            let names = match registering {
                Some(requests) => &requests.effects,
                None => crate::part_table::resolved_effects()
                    .get()
                    .map_or(&crate::part_table::requested_keys().effects, |effects| {
                        &effects.names
                    }),
            };
            names.id(slot).ok_or("unknown EffectKey slot")?;
            Ok(Self(slot))
        })
    }
}

impl BlockEntityTypeKey {
    pub(crate) fn from_slot(slot: u32) -> Result<Self, &'static str> {
        crate::part_table::with_registering_keys(|registering| {
            let names = match registering {
                Some(requests) => &requests.block_entity_types,
                None => crate::part_table::resolved_block_entity_types()
                    .get()
                    .unwrap_or(&crate::part_table::requested_keys().block_entity_types),
            };
            names.id(slot).ok_or("unknown BlockEntityTypeKey slot")?;
            Ok(Self(slot))
        })
    }
}

/// A fluid requested by a mod but absent from the completed registry.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct UnknownFluid {
    /// Mod that requested the fluid.
    pub mod_id: &'static str,
    /// Canonical fluid identifier.
    pub id: String,
}

impl std::fmt::Display for UnknownFluid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "mod {} requested unknown fluid {}", self.mod_id, self.id)
    }
}

/// Resolve fluid requests against the loaded behavior registry.
pub fn resolve_fluids(ids: &[&str]) -> Result<(), Vec<UnknownFluid>> {
    let requests = crate::part_table::requested_keys();
    let mut names = requests.fluids.clone();
    let mut errors = Vec::new();
    for entry in &names.entries {
        if !ids.contains(&entry.id.as_str()) && !requests.declared_fluids.contains_key(&entry.id) {
            errors.extend(entry.mods.iter().map(|mod_id| UnknownFluid {
                mod_id,
                id: entry.id.clone(),
            }));
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    for id in ids {
        names
            .request("horizon", id)
            .expect("registry owns valid ids");
    }
    for (slot, entry) in names.entries.iter().enumerate() {
        names
            .utf16_slots
            .insert(entry.id.encode_utf16().collect(), slot as u32);
        if let Some(path) = entry.id.strip_prefix("minecraft:") {
            names
                .utf16_slots
                .insert(path.encode_utf16().collect(), slot as u32);
        }
    }
    names.world_fluids = horizon_world::fluid::FluidKind::ALL
        .iter()
        .map(|kind| names.slots.get(kind.name()).copied().map(FluidKey))
        .collect();
    let _ = crate::part_table::resolved_fluids().set(names);
    Ok(())
}

/// Resolve an engine fluid kind through the load-owned mapping.
pub fn world_fluid_key(kind: horizon_world::fluid::FluidKind) -> Option<FluidKey> {
    crate::part_table::resolved_fluids()
        .get()?
        .world_fluids
        .get(kind as usize)
        .copied()
        .flatten()
}

/// Find a fluid key at a data-loading boundary.
pub fn fluid_key(id: &str) -> Option<FluidKey> {
    let names = crate::part_table::resolved_fluids().get()?;
    let slot = if id.contains(':') {
        names.slots.get(id)
    } else {
        names.slots.get(&format!("minecraft:{id}"))
    };
    Some(FluidKey(*slot?))
}

/// Resolve an NBT UTF-16 fluid name without allocating.
pub fn fluid_key_utf16(id: &[u16]) -> Option<FluidKey> {
    Some(FluidKey(
        *crate::part_table::resolved_fluids()
            .get()?
            .utf16_slots
            .get(id)?,
    ))
}

/// The canonical registry name of a resolved fluid key.
pub fn fluid_id(key: FluidKey) -> Option<&'static str> {
    crate::part_table::resolved_fluids().get()?.id(key.0)
}

/// Resolve all block entity types after registration and content loading.
pub fn resolve_block_entity_types(
    types: &horizon_client_core::block_entity_types::BlockEntityTypes,
) -> Result<(), Vec<String>> {
    let mut names = crate::part_table::requested_keys()
        .block_entity_types
        .clone();
    names.resolve(
        |id| types.key(id),
        |mod_id, id| format!("mod {mod_id} requested unknown block entity type {id}"),
    )?;
    for id in types.names() {
        names
            .request("horizon", id)
            .expect("registry owns valid ids");
    }
    let _ = crate::part_table::resolved_block_entity_types().set(names);
    Ok(())
}

/// Lookup an engine-owned name after the registry is resolved.
pub fn block_entity_type_key(id: &str) -> Option<BlockEntityTypeKey> {
    Some(BlockEntityTypeKey(
        *crate::part_table::resolved_block_entity_types()
            .get()?
            .slots
            .get(id)?,
    ))
}

/// Canonical name of a load-owned block entity type slot.
pub fn block_entity_type_name(key: BlockEntityTypeKey) -> Option<&'static str> {
    crate::part_table::resolved_block_entity_types()
        .get()
        .unwrap_or(&crate::part_table::requested_keys().block_entity_types)
        .id(key.0)
}

pub(crate) fn block_entity_type_display_name(key: BlockEntityTypeKey) -> String {
    crate::part_table::with_registering_keys(|registering| {
        registering
            .and_then(|r| r.block_entity_types.id(key.0))
            .or_else(|| block_entity_type_name(key))
            .unwrap_or("<invalid>")
            .to_owned()
    })
}

/// A block slot's mappings in one game's current tables.
#[derive(Clone, Copy, Debug)]
// exhaustive: the game builds this mapping at load; mods receive only BlockKey
pub struct ResolvedBlock {
    /// The block's default state.
    pub default: BlockStateId,
    /// Its consecutive state ids.
    pub states: BlockStateRange,
    /// Its index in the behaviour table.
    pub block: horizon_world::level::states::BlockId,
}

thread_local! {
    static BLOCKS: std::cell::RefCell<Option<Vec<ResolvedBlock>>> = const { std::cell::RefCell::new(None) };
}

/// Rebind this game's slots at the owning load/reload boundary. A refused reload clears the
/// mapping, so no operation can accidentally use states from the previous tables.
pub fn resolve_blocks(
    mut resolve: impl FnMut(&str) -> Option<ResolvedBlock>,
) -> Result<(), Vec<String>> {
    let result = crate::part_table::requested_keys()
        .blocks
        .resolve(&mut resolve, |mod_id, id| {
            format!("mod {mod_id} requested unknown block {id}")
        });
    BLOCKS.with(|blocks| {
        *blocks.borrow_mut() = None;
        match result {
            Ok(values) => {
                *blocks.borrow_mut() = Some(values);
                Ok(())
            }
            Err(errors) => Err(errors),
        }
    })
}

/// The current numeric mapping for a trusted key. None means this thread has no resolved
/// world tables (startup, another owning thread, or a refused reload), not an unknown slot.
pub fn block(key: BlockKey) -> Option<ResolvedBlock> {
    BLOCKS.with(|blocks| {
        blocks
            .borrow()
            .as_ref()
            .map(|values| values[key.0 as usize])
    })
}

/// The resolved effect slots. Effects remain fixed for the process, as does their registry.
#[derive(Default)]
pub(crate) struct Effects {
    pub(crate) names: Names,
    pub(crate) indices: Vec<usize>,
    keys: Vec<EffectKey>,
}

/// Resolve requested effects once, after every mod's content has been registered.
pub fn resolve_effects() -> Result<(), Vec<String>> {
    use horizon_client_core::potions;
    let installed = crate::part_table::requested_keys();
    let all: Vec<_> = potions::all_effects().collect();
    let mut names = installed.effects.clone();
    // Hooks and effect enumeration may expose entries no mod explicitly requested.
    for def in &all {
        names
            .request("horizon", potions::namespaced(def.id))
            .expect("registry owns valid ids");
    }
    let indices = names.resolve(
        |id| all.iter().position(|def| potions::namespaced(def.id) == id),
        |mod_id, id| format!("mod {mod_id} requested unknown effect {id}"),
    )?;
    let mut keys = vec![EffectKey(u32::MAX); all.len()];
    for (slot, &index) in indices.iter().enumerate() {
        keys[index] = EffectKey(slot as u32);
    }
    let _ = crate::part_table::resolved_effects().set(Effects {
        names,
        indices,
        keys,
    });
    Ok(())
}

/// Resolve an effect operation by numeric slot, never by a fresh name scan.
pub fn effect(key: EffectKey) -> Option<&'static horizon_client_core::potions::EffectDef> {
    let index = crate::part_table::resolved_effects().get()?.indices[key.0 as usize];
    let vanilla = horizon_client_core::potions::EFFECTS;
    if index < vanilla.len() {
        Some(&vanilla[index])
    } else {
        Some(&horizon_client_core::potions::mod_effects()[index - vanilla.len()])
    }
}

/// Canonical diagnostic name, unavailable until the process effect registry is resolved.
pub fn effect_id(key: EffectKey) -> Option<&'static str> {
    Some(&crate::part_table::resolved_effects().get()?.names.entries[key.0 as usize].id)
}

/// Canonical diagnostic name of a trusted, load-owned slot, independent of world resolution.
pub fn block_id(key: BlockKey) -> Option<&'static str> {
    Some(&crate::part_table::requested_keys().blocks.entries[key.0 as usize].id)
}

/// Canonical diagnostic name retained by an item registration slot.
pub fn item_id(key: ItemKey) -> Option<&'static str> {
    crate::part_table::requested_keys().items.id(key.0)
}

/// The API key for an engine registry index, also for entries not explicitly requested by mods.
pub fn effect_key(index: usize) -> Option<EffectKey> {
    crate::part_table::resolved_effects()
        .get()?
        .keys
        .get(index)
        .copied()
}
