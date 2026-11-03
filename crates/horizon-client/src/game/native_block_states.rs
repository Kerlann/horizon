//! Block state ids and names, and item tags, for native mods (API @API:block_state_ids@):
//! registry queries with no level, so they answer anywhere a host is given.
use super::Game;
use horizon_client::command::{Reader, args::Registries, block};
use horizon_client::items::ItemStack;
use horizon_world::chunk::BlockState;
use horizon_world::registry::{BlockRegistry, BlockStateId};
use std::sync::{Arc, Mutex};

impl Game<'_> {
    /// The state `name` names, read as `/setblock` reads a block
    /// (`BlockStateParser.parseForBlock`): `minecraft:oak_stairs[facing=east]`,
    /// `stone` for `minecraft:stone`, the properties left out at their
    /// default. Its id is vanilla's `Block.getId`. None for an unknown block,
    /// a property it doesn't have or a value it can't take, or block NBT
    /// (`{...}`); and, as `/setblock`'s argument, for anything after the
    /// block. API @API:block_state_ids@.
    pub(super) fn native_block_state_id(&self, name: &str) -> Option<BlockStateId> {
        let registry = self.pack.assets.registry;
        parse(&block_registries(registry), registry, name)
    }
    /// The state `state` as `/setblock` takes it back
    /// (`BlockStateParser.serialize`): its block, then each property in
    /// name order, `minecraft:oak_stairs[facing=east,half=bottom,...]`, or
    /// the block alone for one with no properties. None for an id no state
    /// has (`Block.stateById` gives air for one). API @API:block_state_ids@.
    pub(super) fn native_block_state_string(&self, state: BlockStateId) -> Option<String> {
        self.pack.assets.registry.state(state).map(serialize)
    }
    /// The default state of `block` (`minecraft:oak_stairs`, or `oak_stairs`):
    /// `Block.defaultBlockState`, the one a block takes when nothing else
    /// says. None for an unknown block. API @API:block_state_ids@.
    pub(super) fn native_block_state_default(&self, block: &str) -> Option<BlockStateId> {
        default_state(self.pack.assets.registry, block)
    }
    /// Whether `stack` is in the item tag `tag` (`minecraft:logs` or `logs`,
    /// no `#`):
    /// `ItemStack.is(TagKey)`, over the world's merged tags, its data packs
    /// and content mods included. An empty stack tests `minecraft:air`, an
    /// unknown tag is false. None on a server's client or before the world
    /// has loaded its tags. API @API:block_state_ids@.
    pub(super) fn native_item_stack_in_tag(&self, stack: &ItemStack, tag: &str) -> Option<bool> {
        let tag = horizon_client::command::args::normalize(tag);
        self.native_server_item_in_tag(stack, &tag).ok()
    }
}

/// The command registries' block table of `registry` alone, built once: what
/// the parser checks properties against, whether a world is open or not.
fn block_registries(registry: &'static BlockRegistry) -> Arc<Registries> {
    static CACHE: Mutex<Vec<(usize, Arc<Registries>)>> = Mutex::new(Vec::new());
    let key = std::ptr::from_ref(registry) as usize;
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, reg)) = cache.iter().find(|(cached, _)| *cached == key) {
        return reg.clone();
    }
    let reg = Arc::new(Registries {
        blocks: (*super::chat::block_properties(registry)).clone(),
        ..Registries::default()
    });
    cache.push((key, reg.clone()));
    reg
}

/// `BlockStateParser.parseForBlock` of the whole of `name`, NBT included, as
/// `/setblock` reads it; a state with NBT has no id.
fn parse(reg: &Registries, registry: &BlockRegistry, name: &str) -> Option<BlockStateId> {
    let mut reader = Reader::new(name);
    let input = block::parse(&mut reader, reg, false).ok()?;
    if reader.can_read() || input.nbt.is_some() {
        return None;
    }
    let state = BlockState {
        name: input.id,
        properties: input.properties.into_iter().collect(),
    };
    registry.resolve(&state).ok()
}

/// `Block.defaultBlockState` of the block `block` names (`Identifier.parse`).
fn default_state(registry: &BlockRegistry, block: &str) -> Option<BlockStateId> {
    registry.default_state(&horizon_client::command::args::normalize(block))
}

/// `BlockStateParser.serialize`: the block, then `[key=value,...]` in the
/// state definition's (name) order.
pub(super) fn serialize(state: &BlockState) -> String {
    if state.properties.is_empty() {
        return state.name.clone();
    }
    let properties: Vec<String> = state
        .properties
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect();
    format!("{}[{}]", state.name, properties.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Air, a stair with two properties (its default `facing=north,half=bottom`)
    /// and wheat with an integer `age`, as `parse` reads them.
    fn blocks() -> (Arc<Registries>, &'static BlockRegistry) {
        let facts = br#"{"defaults":{"minecraft:air":0,"minecraft:oak_stairs":1,"minecraft:wheat":5},"states":[
            ["air",true,false,"","minecraft:air"],
            ["s",false,false,"",{"id":"minecraft:oak_stairs","properties":{"facing":"north","half":"bottom"}}],
            ["s",false,false,"",{"id":"minecraft:oak_stairs","properties":{"facing":"north","half":"top"}}],
            ["s",false,false,"",{"id":"minecraft:oak_stairs","properties":{"facing":"east","half":"bottom"}}],
            ["s",false,false,"",{"id":"minecraft:oak_stairs","properties":{"facing":"east","half":"top"}}],
            ["w",false,false,"",{"id":"minecraft:wheat","properties":{"age":"0"}}],
            ["w",false,false,"",{"id":"minecraft:wheat","properties":{"age":"1"}}]
        ]}"#;
        let registry = Box::leak(Box::new(
            BlockRegistry::from_facts_json(&facts[..]).unwrap(),
        ));
        (block_registries(registry), registry)
    }

    #[test]
    fn a_state_name_reads_as_setblock_reads_it_and_round_trips() {
        let (reg, registry) = blocks();
        let id = |name: &str| parse(&reg, registry, name);
        assert_eq!(id("minecraft:air"), Some(BlockStateId(0)));
        assert_eq!(id("oak_stairs"), Some(BlockStateId(1)));
        assert_eq!(
            id("oak_stairs[half=top,facing=east]"),
            Some(BlockStateId(4))
        );
        assert_eq!(
            id("minecraft:oak_stairs[facing=east]"),
            Some(BlockStateId(3))
        );
        // IntegerProperty.getValue: Integer.parseInt.
        assert_eq!(id("wheat[age=01]"), Some(BlockStateId(6)));
        for state in 0..7 {
            let text = serialize(registry.state(BlockStateId(state)).unwrap());
            assert_eq!(id(&text), Some(BlockStateId(state)), "{text}");
        }
        assert_eq!(
            serialize(registry.state(BlockStateId(4)).unwrap()),
            "minecraft:oak_stairs[facing=east,half=top]"
        );
        assert_eq!(
            serialize(registry.state(BlockStateId(0)).unwrap()),
            "minecraft:air"
        );
    }

    #[test]
    fn a_state_name_that_does_not_read_has_no_id() {
        let (reg, registry) = blocks();
        for name in [
            "minecraft:nothing",
            "oak_stairs[shape=straight]",
            "oak_stairs[facing=up]",
            "oak_stairs[facing=east,facing=north]",
            "wheat[age=2]",
            "oak_stairs{}",
            "oak_stairs extra",
            "#minecraft:stairs",
            "",
        ] {
            assert_eq!(parse(&reg, registry, name), None, "{name:?}");
        }
    }

    #[test]
    fn a_block_has_its_default_state() {
        let (_, registry) = blocks();
        assert_eq!(default_state(registry, "oak_stairs"), Some(BlockStateId(1)));
        assert_eq!(
            default_state(registry, "minecraft:wheat"),
            Some(BlockStateId(5))
        );
        for block in [
            "minecraft:nothing",
            "oak_stairs[half=top]",
            "minecraft:",
            "",
        ] {
            assert_eq!(default_state(registry, block), None, "{block:?}");
        }
    }
}
