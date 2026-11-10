//! Engine-owned injection activity and the load-time seal entry point.
//! Listener types, ordering and dispatch belong to the provider, not this crate.
#![forbid(unsafe_code)]

use std::sync::OnceLock;

/// Point activity, populated by the listener provider when registration seals.
pub static POINTS: Slots = Slots::new();
/// Call-site activity, populated by the listener provider when registration seals.
pub static SITES: Slots = Slots::new();
static SEAL: OnceLock<fn()> = OnceLock::new();

/// An index-addressed activity table. An unpopulated or unknown slot is inactive.
/// Once published, checking a slot allocates nothing and never scans listeners.
pub struct Slots {
    flags: OnceLock<Box<[bool]>>,
}

impl Slots {
    const fn new() -> Self {
        Self {
            flags: OnceLock::new(),
        }
    }

    /// Whether the provider published an active listener at this index.
    #[inline]
    pub fn active(&self, index: usize) -> bool {
        self.flags
            .get()
            .and_then(|flags| flags.get(index))
            .copied()
            .unwrap_or(false)
    }

    /// Publish activity at seal time. The first publication is permanent.
    /// Providers finish constructing their dispatch tables before publishing flags.
    pub fn publish(&self, activity: impl IntoIterator<Item = bool>) {
        self.flags.get_or_init(|| activity.into_iter().collect());
    }
}

/// Install the provider's seal callback during load, before engine startup.
/// The first provider owns sealing for the process; repeated installation is inert.
pub fn install_seal(callback: fn()) {
    SEAL.get_or_init(|| callback);
}

/// Finish listener registration, or do nothing when no provider was loaded.
pub fn seal() {
    if let Some(callback) = SEAL.get() {
        callback();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn activity_is_inert_until_published_and_publication_is_permanent() {
        let slots = Slots::new();
        assert!(!slots.active(0));
        slots.publish([true, false, true]);
        assert!(slots.active(0));
        assert!(!slots.active(1));
        assert!(slots.active(2));
        assert!(!slots.active(usize::MAX));
        slots.publish([false]);
        assert!(slots.active(2));
    }

    #[test]
    fn engine_seal_invokes_only_the_installed_provider() {
        static CALLED: AtomicBool = AtomicBool::new(false);
        seal();
        install_seal(|| CALLED.store(true, Ordering::Release));
        install_seal(|| panic!("second provider must not replace the first"));
        seal();
        assert!(CALLED.load(Ordering::Acquire));
    }
}
