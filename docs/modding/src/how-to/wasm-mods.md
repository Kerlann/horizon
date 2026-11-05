# Building a mod to `mod.wasm`

A mod written against `mod_api` builds either way from the same source: native, compiled into
the game by the launcher ([Native mods](native-mods.md)), or to `mod.wasm`, which any game with
the same mod API runs with no rebuild. Only the build step differs: the hooks, Parts,
`[dependencies]` and the manifest mean the same for both.

```rust,ignore
{{#include ../../examples/wasm-hello/src/lib.rs:mod}}
```

## Build

```sh
cargo horizon-mod build --wasm
```

It builds the crate with the pinned Rust toolchain (installed through rustup the first time, with
`rust-src`), writes `mod.wasm` beside `horizon-mod.toml`, records it in a `[wasm]` section, and
packs `target/horizon-mod/<id>.hmod`. `[wasm]` is written by the tool, never by hand:

```toml
[wasm]
sha512 = "…"                      # mod.wasm's hash, checked when the game loads it
toolchain = "1.99.1 <commit>"     # every mod linked together needs the same one
[wasm.linked]                     # the exact builds of its wasm dependencies
my_library = "…"
```

A mod is `[native]` or `[wasm]`, not both. A wasm mod's id is words that each start with a
letter (`hello_wasm`, not `mod_2`).

## Dependencies

`[dependencies]` work as for native mods. List the library mod in `horizon-mod.toml` and its
crate in `Cargo.toml`; build the library with `--wasm` first. The game links a mod with its
dependencies at load, one copy of each library shared by every mod that uses it, and refuses a
mod whose library is missing or another build than the one it was linked against, naming it on
the Mods page. An optional dependency is built in with the Cargo feature of its id, so its crate's
entry in `Cargo.toml` is `optional = true` under that id; an absent one is left out of the group at
load, and `if_installed!` keeps the mod from calling it.

## In the game

The game loads every `mod.wasm` turned on in the Mods page (or passed with `--dev-mod`) at start,
after the native mods. Each runs sandboxed with a memory limit and a time limit per call: 100 ms
for a hook or part call, 30 s for start-up and a call of the load phase (registration, a data
load, a resource reload). A mod that traps or runs past its limit stops its group, not the game.
The limit is wall-clock time, so a slow machine reaches it sooner; `--wasm-fuel` counts
instructions instead, so a mod stops at the same place on every machine, at some cost per call
(for debugging and deterministic runs). `--wasm-interpreter` runs mods interpreted, for hosts
that can't compile code at run time; it is much slower.
A mod refused at load is named on the Mods page with its reason. A `mod.wasm` built against
hooks this game doesn't have, or has with another type, names the first such import
(`horizon:mod-api/host@0.1.0#entity`), so rebuilding against this game's `mod_api` is the fix.

## Listening at points

A wasm mod listens at the game's [points](../injections.md) as a native mod does: the same
`#[mod_api::horizon_mod] impl mod_points::Points`, and one `[[native.injection]]` per place in
`horizon-mod.toml`, with no `game` or commit pins beside `[wasm]`. The sample listens at the tick:

```toml
[[native.injection]]
id = "count_ticks"
target = "horizon_client::game::Game<'_>::tick"
kind = "HEAD"
trait_method = "tick"
```

At load the game checks each place the `mod.wasm` fills against this list and against its own points:
a place the list doesn't name, a listed place the mod doesn't fill, a `target` this game has no point
at, or a point whose record changed since the mod was built refuses the mod and names the place. A
listener changes its record's fields (`cancel`, `result`, the arguments) as a native one does, and the
game takes them back when it returns. Only `trait_method` injections go beside `[wasm]`: a `helper`
needs a native mod. A point call costs about as much as a hook call (a few hundred nanoseconds on a
release build), and a place nobody listens at costs nothing.

## Calls into the game

A wasm mod reaches the game through the same `mod_api` as a native one, and each call crosses
the sandbox: a hook call into a mod costs about 1.5 µs, and a capability lookup about 2.5 µs on
top (measured on a release build). A `Capability<T>` holds no borrow and stays valid for the
tick, so a pipe or machine looks up its neighbours' capabilities once per tick and calls the
handlers it got, rather than looking them up again for every move. A mod's own handlers, and
those of mods in its group, are called directly without crossing.

`host.is_loaded("other_mod")` says whether a mod is installed, native, wasm or content, as
Fabric's `isModLoaded` does; the list is fixed at startup.
