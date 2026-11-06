//! The `register_debug_entries` hooks: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// The F3 debug entries a mod adds (NeoForge's `RegisterDebugEntriesEvent`): push a `DebugEntry`
    /// per entry, its id `<mod id>:<path>` and the vanilla group its lines join. Asked once at start,
    /// on the client. An id in vanilla's namespace, not of that form, or registered already is logged
    /// and left out. Each entry is listed and switched on or off with vanilla's, and `debug_entry_lines`
    /// fills it while it is on.
    #[hook(
        side = "client",
        contract = "collect",
        out = "entries",
        context = "registration",
        neoforge = "RegisterDebugEntriesEvent"
    )]
    fn register_debug_entries();
}
