<!-- chapter: Native mods -->
<!-- separator: 1 -->

## Native mods

Behaviour that needs code (a new key, a HUD, a tool that changes how mining works) is a native mod.
It is one flat Rust project with `horizon-mod.toml`, `Cargo.toml` and `src/`, beside its `assets/` and `data/`.
Start with `cargo horizon-mod new my_mod --template native` and implement the typed hooks;
raw injections are a rare escape hatch ([Injections](../../injections.md)). How the launcher builds one, what it pins and why it is not sandboxed:
[native mods](../native-mods.md).
The typed hook API is generated automatically at build time, in debug and release builds alike; its generated
Rust files are build output, not files to regenerate or commit. Engine contributors adding a hook only run
`cargo run -p xtask -- mod-api` to fill its hand-written placeholder body
([Adding a hook](../../concepts/trait-mods/027-adding-a-hook.md#adding-a-hook)).

A native mod's config files go in the game's shared `config/` folder, beside `options.txt` (on a dedicated server,
beside `server.properties`), as Fabric and NeoForge mods' do: the mod picks its own file names, such as
`config/<mod id>.json` or a folder `config/<mod id>/`. It reads and writes them with `Host::config_files()`, which is
bound to the calling mod and refuses any other mod's files. A mod that lists its files with
`native_register_configs` gets them read for it, written from their defaults when missing, and handed over again
when edited (`native_config_loading`, `native_config_reloading`, as NeoForge's `ModConfigEvent`), and a world can
keep its own server config in `serverconfig/` ([native mods](../native-mods.md)). Settings kept with `set_setting` stay where they were, in `horizon-mods/.horizon-mod-data/<id>/`.
8c838a6b381b93150deaeed2057acd1

For a client HUD's session information, use the read-only
`ClientHost::client_session()` service and the local `PlayerRef`'s
`block_breaking` and `ambient_mood` reads rather than raw injections. Missing
sessions, external display names and memory addresses remain explicit optional
values; all counters stay local. See [client session and local-player status](../../reference/hook-notes.md#client-session-and-local-player-status)
for their exact availability and frame-time semantics.

`native_compute_fov_modifier(player, base, modifier)` runs on the client each tick for the local player's FOV scale, after vanilla effects and before zoom and camera easing. `player` contains the calculation inputs and `base` is the original scale. Non-finite modifiers or values outside 0.01..=10 fall back to `base`. It does not affect a spectator camera on a mob.

The client hook `native_render_name_tag(entity_id, kind, vanilla_visible, visible, text)` runs at player and mob, armor stand, and item frame name decisions before shared geometry. Set `visible` to override vanilla and `text` to `Some` literal replacement text (NeoForge `RenderNameTagEvent`). Distance and entity availability still apply; the local player's tag is considered only when its own camera and settings gates allow it.

Native mods are Rust projects that implement the game's typed hook API (`impl Hooks`). Start with
`cargo horizon-mod new my_mod --template native` ([Trait mods](../../concepts/trait-mods/000-trait-mods-draft.md#trait-mods-draft)).
Each project has one `horizon-mod.toml`, with exact build pins in `[native]`, and ships as one `.hmod`.
For the rare case the API does not cover, raw injections change the game's Rust source before it is compiled,
using [horizon-mixins](https://github.com/horizon-rust/horizon-mixins) (MIT, by levzzz5154, made for Horizon).
An injection is an escape hatch; request a typed hook for the behavior it needs.

Native mods are native code with the player's rights, not sandboxed. The first install shows a trust notice,
and every build waits behind a red warning; the registry lists one only after a review (its later releases are
checked by machine). They play in a game of their own, built for the instance they are installed in.

Options > Mods can pause a loaded code mod without rebuilding. Native and wasm mods use the same switch:
subsequent hooks, render hooks, Parts, slot listeners and capability-provider lookups skip the disabled owner's
registrations. Re-enabling resumes those registrations without re-registering them. This pauses dispatch, not
unloads code or undoes earlier world changes; a trapped wasm group stays stopped even if its switch is turned
back on. Content-mod selection stays frozen while a world is open, including on a browser project page. A browser
install that finishes after a world opens waits until the world closes before activating its content.

Villager handles preserve the full signed integer level (at least 1), including values above the
five vanilla trade tiers. Higher levels use the master badge and do not gain further vanilla trades.
