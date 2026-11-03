# Mod files

A `.hmod` is one mod: content, native code, or both. The readers are `horizon_mods::Package` and
`horizon_mods::Content` (content), and `horizon_mods::index::native::Bundle` (native code).
Level: content is preview, native code experimental ([data-formats.md](data-formats.md)).

## The zip

- A zip archive. Entries are stored or deflate-compressed, the only methods the readers support.
- A path is relative and `/`-separated: no `\`, no `:`, no empty, `.` or `..` part, and it doesn't start with `/`.
  Directory entries are skipped. Native entries must be regular files, not symlinks or devices.
- Each path appears once.

## Project and archive layout

```text
my_mod.hmod
├── horizon-mod.toml
├── assets/
├── data/
├── horizon-native.json
└── native/
    ├── horizon-mixin.toml
    ├── data/
    └── rust/
        ├── Cargo.toml
        └── src/lib.rs
```

- **Content only:** `horizon-mod.toml` with `assets/` and `data/`. Nothing compiles.
- **Native code:** `horizon-native.json` and `native/`. The launcher builds it into the game.
- **Both:** the two manifests name the same mod id. Content entries are checked as content and left out of the
  native build. Any other top-level archive entry refuses the file. Native packages cannot contain `mod.wasm`.

There is no mod-id directory level inside the archive. The launcher stages the contents of `native/` into the
horizon-mixins tool's `<mods>/<id>/` directory. Content stays at the project/archive top level.

## `horizon-native.json`

A JSON object. Unknown fields refuse the file.

| Field | Type | Rule |
| --- | --- | --- |
| `schema` | number | `1` |
| `id` | string | `[a-z0-9_]{1,64}`, the same as `native/horizon-mixin.toml` `[mod] id` |
| `version` | string | not empty, the same as `horizon-mixin.toml` `[mod] version` |
| `game` | string | not empty, the Minecraft version (`"26.3"`) |
| `horizon` | string | the exact Horizon commit, 40 lowercase hex characters, matching `[compatibility] horizon_revision` |
| `mixins` | string | the exact horizon-mixins commit, 40 lowercase hex characters |

## Under `native/`

`horizon-mixin.toml` is the exact filename required by horizon-mixins. The identity table is `[mod]`.
A trait mod declares `[api] path = "rust"`; a raw mixin crate declares `[[crate]] path = "rust"`.
Only one source route is allowed: declaring both refuses the package. Every declared source root is `rust`.
The crate is in `rust/` because horizon-mixins requires a non-dot relative crate path.

Besides the manifest, native files are `.rs` and `.toml` files under `rust/`, arbitrary files under
`data/` or `rust/data/`, and `README*`, `LICENSE*`, `LICENCE*`, `COPYING*`, `NOTICE*` and `.gitignore`
in `native/` or `native/rust/`.

## Content paths

`assets/` or `data/`, then at least two parts of `[a-z0-9_.-]` (vanilla's resource-location characters).

## Limits

| What | Limit |
| --- | --- |
| A registry file, as downloaded | 64 MiB |
| Native files (everything but content entries) | 512 entries, 64 MiB unpacked in total |
| Content files | 65,536 files, 16 MiB each, 512 MiB in total |
| `horizon-mod.toml` inside a native file | 1 MiB |
