# Forking: Horizon's extras and this fork

Upstream introduced a `horizon-extras` Cargo feature in
horizon-rust/horizon#12055 to group its cosmetics, emotes, presence, server-list
promotions and settings pages. This fork deliberately differs: excluded features
are removed, not compiled behind a switch.

Cosmetics, wardrobe, emotes, partner ads, sponsor cards, telemetry and Discord
presence are not part of this fork. No Cargo feature restores them. The Mods and
Horizon Settings pages remain available independently of the client's default
features; disabling audio or controllers does not hide mod management.

## Build features

The client's default features are `audio`, `controllers` and `friends`.
Play with Friends remains independently selectable. For example, a client without
audio or controllers can be built with:

```sh
cargo build --release -p horizon-client --no-default-features --features friends
```

The dedicated server uses `--no-default-features`. Neither build mode introduces
a telemetry placeholder or an official-build promotion in the options screen.

## Modding and rendering

The mod runtime, typed native and WebAssembly mod API, agent control, controllers,
HDR and shader packs remain separate from the excluded upstream features.
Renderer options and optional client settings retain their own controls and
vanilla-preserving defaults. See [Extras](extras.md) and the
[modding book](modding/src/introduction.md).
