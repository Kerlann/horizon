<!-- chapter: Native entry point and diagnostics -->
<!-- separator: 1 -->

The generated executable uses the installed game's `crates/horizon-client/src/native_main.rs.template`.
The launcher supplies only ordered literal mod IDs and hook factory paths. The template owns entry
construction, the allocator and the canonical game identity. Source without this template is refused
before staging trait mods: update the game first.

A panic in a trait mod's hook adds `in native mod <id>, hook <hook>` to the panic's message, in the crash report
and in the server thread's log line, with the `horizon-mod.toml` id. Each entry passed to
`entry::main_with` must bind its id and hooks together: `ModEntry::new("example", example::hooks())`.
The ordered list is `Mods = Arc<[ModEntry]>`; there is no separate naming step or unnamed entry.
Before installation, `mod_api::mods()` still returns an empty list. Parts retain their registering mod's
id for startup errors and hook panics. The game stops. A point listener's panic names its mod and
method; render hooks are not named yet.

A code mod may listen at the game's injection points as a rare escape hatch, through a crate-root function
named by `[[inject]].with`, taking a `mod_points` record; the form
and its limits are in [Injections](../../injections.md).
