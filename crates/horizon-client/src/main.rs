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

fn main() {
    std::hint::black_box(&GAME_IDENTITY);
    horizon_client::entry::main();
}
