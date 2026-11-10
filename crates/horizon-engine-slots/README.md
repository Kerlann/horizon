# Engine injection activity

This dependency-free crate forbids unsafe code. It owns the game's point and call-site activity arrays and a `fn()` seal entry point. It contains no point IDs, generated constants or build script.

The host installs the provider's seal callback even when no listeners were supplied. The shared registry constructs its ordered dispatch tables, then publishes the game's activity here. Without a provider, checks return false and seal is inert. Mod-owned tables retain local atomic flags in the shared registry rather than using these game-wide arrays.

The provider owns all listener types, guards, replacement rules, dispatch adapters and unsafe operations. It depends on this crate, never the reverse. Generic provider `Table::active_point` and `active_site` route checks to the appropriate local or engine backend.

A game activity check performs an immutable table lookup, bounds check and boolean read after `OnceLock` publication, without allocating or scanning listeners. The two arrays allocate once at seal. A body-only gameplay edit cannot invalidate this crate: it has no source discovery or generation dependency.
