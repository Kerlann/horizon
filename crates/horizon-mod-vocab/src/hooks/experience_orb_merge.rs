//! The `experience_orb_merge` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0130

pub trait Hooks {
    /// The experience orb `into` (`into_value` points, `into_count` orbs) is about to
    /// take `from` (`from_value` points, `from_count` orbs) in, after
    /// `experience_orb_can_merge` said yes (`from` is 0 and `from_count` 1 for a new orb
    /// that is spawning). Set `canceled` to stop it: both orbs stay, and a spawning orb
    /// tries the next orb in reach, then spawns on its own. Left alone, `into` adds
    /// `from`'s count, as vanilla does. A mod that writes `into`'s value or count with
    /// `set_experience_orb` here makes that write the merged orb: `from` is taken in
    /// without adding its count, so `value = 11, count = 1` pays 11 once. The write is
    /// refused, with both orbs left as they were, when `from` has changed or left reach by
    /// the time the merge applies. A merge whose combined count would pass `i32::MAX`
    /// (what a save holds) is refused the same way. Server-side.
    #[hook(
        side = "server",
        contract = "cancel",
        out = "canceled",
        cancellable = true,
        version = 2,
        neoforge = "none (vanilla ExperienceOrb.tryMergeToExisting; NeoForge adds no event or extension for it)"
    )]
    fn experience_orb_merge(
        dimension: &str,
        into: u32,
        from: u32,
        into_value: i32,
        into_count: u32,
        from_value: i32,
        from_count: u32,
        canceled: &mut bool,
    );
}
