//! Registered, allocation-free construction of hosts implemented above the engine.
//!
//! The executable installs the provider before starting the game. The first
//! acquisition seals registration, even if no provider was installed.
use super::Game;
use mod_api::{ClientHost, Host, ServerHost};
use std::sync::{Mutex, OnceLock};

/// Reference conversions supplied by the crate implementing the host adapters.
///
/// Each conversion preserves the input borrow and does not allocate.
#[derive(Clone, Copy)]
pub struct HostProvider {
    pub read: for<'g, 'r> fn(&'g Game<'r>) -> &'g dyn Host,
    pub server: for<'g, 'r> fn(&'g mut Game<'r>) -> &'g mut dyn ServerHost,
    pub client: for<'g, 'r> fn(&'g mut Game<'r>) -> &'g mut dyn ClientHost,
}

static PENDING: Mutex<Option<&'static HostProvider>> = Mutex::new(None);
static SEALED: OnceLock<Result<&'static HostProvider, ProviderError>> = OnceLock::new();

/// Why the engine could not register or acquire a host provider.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderError {
    /// A provider has already been installed.
    AlreadyRegistered,
    /// Host acquisition has begun, so startup registration is closed.
    TooLate,
    /// The executable did not install a mod host provider before acquisition.
    NotRegistered,
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::AlreadyRegistered => "a mod host provider is already registered",
            Self::TooLate => "mod host providers must register before the game starts",
            Self::NotRegistered => "the executable did not register a mod host provider",
        })
    }
}

impl std::error::Error for ProviderError {}

/// Registers reference conversions once, before the first acquisition.
pub fn register(provider: &'static HostProvider) -> Result<(), ProviderError> {
    let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    if SEALED.get().is_some() {
        return Err(ProviderError::TooLate);
    }
    if pending.is_some() {
        return Err(ProviderError::AlreadyRegistered);
    }
    *pending = Some(provider);
    Ok(())
}

/// Returns the immutable provider table. Steady-state calls take no mutex.
pub fn acquire() -> Result<&'static HostProvider, ProviderError> {
    if let Some(provider) = SEALED.get() {
        return *provider;
    }
    let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    *SEALED.get_or_init(|| pending.take().ok_or(ProviderError::NotRegistered))
}
