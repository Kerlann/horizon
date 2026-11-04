//! Horizon: native Rust client for Minecraft Java Edition 26.3 gameplay.
#![forbid(unsafe_code)]

#[cfg(not(any(feature = "system-allocator", feature = "alloc-census")))]
#[global_allocator]
static ALLOCATOR: horizon_threads::Allocator = horizon_threads::ALLOCATOR;
#[cfg(all(feature = "alloc-census", not(feature = "system-allocator")))]
#[global_allocator]
static ALLOCATOR: horizon_threads::CensusAllocator = horizon_threads::CensusAllocator;

/// Identifies this executable as the game for launcher imports. `main` reads it so the
/// linker keeps it; it carries neither an API version nor a per-build id.
static GAME_IDENTITY: [u8; horizon_mods::game_identity::MARK.len()] =
    horizon_mods::game_identity::MARK;

/// Prototype: what the launcher's generated wrapper would do for one `[[native.injection]]` whose
/// target is a compiled-in point. The target comes from data (`HORIZON_POINT_DEMO`), so moving the
/// injection to another point of the same signature needs no build at all.
mod point_demo {
    use std::sync::atomic::{AtomicU64, Ordering};
    type TickServer = for<'h, 'g> fn(&'h mut (dyn mod_api::Host + 'g), &mut (bool,));
    static TICKS: AtomicU64 = AtomicU64::new(0);
    static FREEZE_AFTER: AtomicU64 = AtomicU64::new(u64::MAX);

    /// The mod's listener: count server ticks; after N, MODIFY_ARG `paused = true`.
    fn count<'g>(_host: &mut (dyn mod_api::Host + 'g), args: &mut (bool,)) {
        let n = TICKS.fetch_add(1, Ordering::Relaxed) + 1;
        if n % 200 == 0 {
            eprintln!("POINT_DEMO {n} server ticks seen at HEAD");
        }
        if n >= FREEZE_AFTER.load(Ordering::Relaxed) {
            args.0 = true;
        }
    }

    pub fn attach() {
        let Ok(target) = std::env::var("HORIZON_POINT_DEMO") else { return };
        if let Some(n) = std::env::var("HORIZON_POINT_DEMO_FREEZE_AFTER").ok().and_then(|v| v.parse().ok()) {
            FREEZE_AFTER.store(n, Ordering::Relaxed);
        }
        let point = horizon_client::points_generated::ALL
            .iter()
            .find(|p| p.name() == target)
            .unwrap_or_else(|| panic!("{target} is not a point: rebuild with the injection applied as source"));
        point
            .attach(Box::new(vec![count as TickServer]))
            .unwrap_or_else(|e| panic!("{target}: {e:?}"));
        eprintln!("POINT_DEMO attached to {target} at load");
    }
}

fn main() {
    std::hint::black_box(&GAME_IDENTITY);
    point_demo::attach();
    horizon_client::entry::main();
}
