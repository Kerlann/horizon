//! Gameplay and startup, above the reusable client library.
#![forbid(unsafe_code)]
extern crate self as horizon_client;

#[cfg(feature = "dylib-spike")]
#[global_allocator]
static ALLOCATOR: horizon_threads::Allocator = horizon_threads::Allocator;
pub use horizon_client_lib::*;
pub mod entry;
pub mod game;
pub mod mod_api;
