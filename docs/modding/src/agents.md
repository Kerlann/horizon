# For agents

Rules for a coding agent making a Horizon mod. They are short on purpose; each links to its page.

1. **Choose the shape.** A content mod has no `Cargo.toml`. A code mod has `Cargo.toml` and `src/` at the top and
   implements `Hooks` with `#[mod_api::horizon_mod]`. Never write raw injections first.
2. **Use a hook or a Part method** when one exists (search the rustdoc of `horizon-mod-api`). A per-block,
   per-item or per-entity moment is a Part method ([Parts](concepts/trait-mods.md#parts)).
3. **Otherwise follow the [injection ladder](injections.md#for-agents):** a `trait_method` point injection
   (`impl mod_points::Points`, from `horizon-mod-points`), and a `helper` bound to `Game` only as a last resort. Every injection comes
   with a [hook request](injections.md#ask-for-the-hook).
4. **Names:** NeoForge 26.3 names for hooks that have a counterpart; no `on_` prefix; the event is `&mut`;
   handles come from the event; writes take `host`.
5. **Layout:** `horizon-mod.toml`, `assets/`, `data/`, `Cargo.toml` and `src/` at the top. The id and version come
   only from `horizon-mod.toml`; ids are `[a-z0-9_]`. No `native/` folder, no `horizon-native.json`.
6. **Manifest fields:** `id`, `name`, `version`, `description`, `authors`, `api = "0.2"`, `side`,
   `capabilities = ["registries"]`, `[dependencies]`, `[native]` (`game`, `horizon`, `mixins`, `injection`) or
   `[wasm]`, `[[jar]]`. One of `[native]` or `[wasm]`, never both. Unknown fields are refused
   ([manifest](reference/manifest.md)).
7. **Error codes:** `E_HOOK_UNKNOWN` (use a catalog name), `E_HOOK_VERSION` (use a listed version).
8. **Data and assets** are read through a reload hook, once per reload, never on the tick.
9. **Missing hook?** File a [hook request](injections.md#ask-for-the-hook) on arismoko/horizon.
