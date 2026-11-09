//! Trait mods implement `Hooks` and link into the game executable.
//! The executable passes the ordered `Mods` to `entry::main_with`.
//! Trait mods are process-wide, like mixin module statics.
//! Hooks use `&self + Send + Sync` for shared and reentrant calls; see `docs/modding/src/how-to/native-mods.md`.
//! Generate the trait and dispatch with `cargo run -p xtask -- mod-api`.
#![deny(missing_docs)]
// The generated `impl_*_areas!` macros are exported for test doubles outside the crate, and this crate's
// own test doubles use them by path too.
#![allow(macro_expanded_macro_exports_accessed_by_absolute_paths)]
extern crate self as mod_api;

use std::sync::{Arc, OnceLock};

use serde_json::{Map, Value};
use std::time::Duration;

mod generated_host {
    include!(concat!(env!("OUT_DIR"), "/mod_api_generated_host.rs"));
}

// no wire: type alias inherits the Wire policy of its tuple or Result target.
/// Result of accessing or decoding an attachment.
pub type AttachmentResult<T> = std::result::Result<T, AttachmentError>;

/// A host is the store of its holders' attachments, so `TYPE.get(host, holder)` reads one.
macro_rules! host_attachment_store {
    ($($host:ident),+) => { $(
        impl AttachmentStore for dyn $host + '_ {
            fn holds(&mut self, holder: Holder, namespace: &str, name: &str) -> AttachmentResult<bool> {
                self.attachments().holds(holder, namespace, name)
            }
            fn decoded(&mut self, holder: Holder, namespace: &str, name: &str) -> AttachmentResult<Option<AttachmentValue>> {
                self.attachments().decoded(holder, namespace, name)
            }
            fn saved(&mut self, holder: Holder, namespace: &str, name: &str) -> AttachmentResult<Option<Compound>> {
                self.attachments().saved(holder, namespace, name)
            }
            fn store(&mut self, holder: Holder, namespace: &str, name: &str, value: AttachmentValue, form: AttachmentForm) -> AttachmentResult<()> {
                self.attachments().store(holder, namespace, name, value, form)
            }
            fn discard(&mut self, holder: Holder, namespace: &str, name: &str, sync: bool) -> AttachmentResult<()> {
                self.attachments().discard(holder, namespace, name, sync)
            }
            fn queue_sync(&mut self, holder: Holder, namespace: &str, name: &str) -> AttachmentResult<()> {
                self.attachments().queue_sync(holder, namespace, name)
            }
        }
    )+ };
}
host_attachment_store!(
    Host,
    ServerHost,
    ClientHost,
    LevelRandomHost,
    LevelWriteHost
);

macro_rules! level_random_draws {
    ($($host:ident),+) => { $(
        impl dyn $host + '_ {
            /// Draw from the level's own random stream, in vanilla's order: `draw` gets the
            /// stream and the level keeps it after. None on a non-authoritative level.
            pub fn with_random<T>(&mut self, draw: impl FnOnce(&mut LegacyRandom) -> T) -> Option<T> {
                let mut random = self.random()?;
                let drawn = draw(&mut random);
                self.set_random(random);
                Some(drawn)
            }
        }
    )+ };
}
level_random_draws!(LevelRandomHost, LevelWriteHost);

mod generated {
    include!(concat!(env!("OUT_DIR"), "/mod_api_generated.rs"));
}

include!(concat!(env!("OUT_DIR"), "/mod_api_exports_root.rs"));
include!(concat!(env!("OUT_DIR"), "/wire_generated.rs"));

#[cfg(test)]
mod brewing_dispatch_tests {
    include!("item/brewing_dispatch_tests.rs");
}

#[cfg(test)]
mod item_model_dispatch_tests {
    include!("item/item_model_dispatch_tests.rs");
}
