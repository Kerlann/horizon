<!-- chapter: Adding a hook -->
<!-- separator: 1 -->

### Adding a hook

A new hook needs its declaration in the area file of `crates/horizon-mod-api/src/hooks/` (`///` lines and
`#[hook(side = "server", contract = "cancel", out = "cancel")]` on `fn name();`; the argument list is in that folder's `mod.rs`), its `native_*` function with the placeholder body
`let _ = (every, parameter);` (all its parameters, in order), and a run of `cargo run -p xtask -- mod-api`.
Put that function in `crates/horizon-client/src/game/native_hooks/<area>.rs`, matching the declaration's
area filename. The build discovers these modules automatically; do not add a module list entry. Shared
support lives in `shared.rs`; `fire.rs` includes generated dispatch entry points, not hook implementations.
That command only replaces the placeholder in its owning area file with the `dispatch_<hook>` call; commit
that source change and the declaration. Fills are idempotent; rerun the command after a failed write. The `horizon-mod-api-gen` build dependency generates the hook traits,
`Host` methods and dispatch code into Cargo's `OUT_DIR` when the game and renderer build, including release
builds. These generated Rust files are not kept in the source tree or committed, and no manual regeneration
is needed after a merge. Every hook needs a `mod_api` entry: the build fails naming any hook without one. A hook that stands in for a
method a vanilla block, block entity, item or menu overrides also gets a `scope` and a `key`
(`{"contract": "collect", "out": "output", "scope": "block", "key": "state"}`): the key is a
`BlockState` (routed by its prepared block key for block parts), an `&ItemStack`, a `ModBlockEntity` or an id `&str`, and
the generator also puts the method on that scope's part trait ([Parts](006-parts.md#parts)). A body that is neither the
placeholder nor already calls the dispatch is an error, and the generator never changes it. Hooks that are
free functions outside `Game` are wired by hand: those crates can't see the mod list, so each calls a callback
the executable installs once, such as `NativeLoadHooks` in `horizon-world` and `NativeClientHooks` in the
client library (`menu_slot_rules`, `living_breathe`, `ingredient`). A build reports a placeholder still to
fill instead of changing hand-written source; run `cargo run -p xtask -- mod-api` to fill it. Mixin injections
go at the head of the body, before the dispatch, so they still run first.
