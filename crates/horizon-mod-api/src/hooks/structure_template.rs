//! Structure template lookup.

pub trait Helpers {
    /// `ServerHost::structure_template(id, at, rotation, mirror)` reads template `id` as
    /// `StructureTemplateManager.get` does: the world's saved structures first, then the game's
    /// and the mods' own. Returns what `/place template` would
    /// place at `at`: its size and blocks, each with its position, transformed state and block
    /// entity data, in vanilla's placement order. Nothing is placed. A builder can use
    /// `LevelRef::set_block` with a `ServerHost` to place blocks over successive ticks.
    #[helper(
        side = "server",
        target = "horizon_client::game::native_structures::Game<'_>::native_structure_template"
    )]
    fn structure_template();
}
