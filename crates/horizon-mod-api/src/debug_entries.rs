//! Named entries a native mod adds to the F3 debug screen (NeoForge's `RegisterDebugEntriesEvent`).

/// A debug entry a native mod registers: its id and the vanilla group its lines are drawn in.
#[derive(Clone, Debug, PartialEq, Eq)]
// exhaustive: a mod builds it with a literal, an id and a group
pub struct DebugEntry {
    /// `<namespace>:<path>` in a namespace other than vanilla's, like `mymod:tick_cost`.
    pub id: String,
    /// The group its lines join, with vanilla's lines of that group.
    pub group: DebugGroup,
}

/// The vanilla groups a debug entry's lines can join (`DebugScreenDisplayer.addToGroup`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
// exhaustive: the closed set of groups vanilla's debug screen draws, matched without a wildcard
pub enum DebugGroup {
    /// `position`: the player's position, section and speed.
    Position,
    /// `light`: the light levels at the feet.
    Light,
    /// `memory`: the process's memory use.
    Memory,
    /// `looking_at_block`: the targeted block's state and tags.
    LookingAtBlock,
    /// `looking_at_entity`: the targeted entity and its tags.
    LookingAtEntity,
    /// `looking_at_fluid`: the targeted fluid's state and tags.
    LookingAtFluid,
    /// `chunk_generation`: the density of the chunk at the feet.
    ChunkGeneration,
}
