# Building a mod to `mod.wasm`

## Boundary benchmark

After building the shared std and the test fixture, run the release benchmark with their paths:

```sh
HORIZON_WASM_STD="$PWD/target/horizon-wasm-std.wasm" HORIZON_WASM_FIXTURE="$PWD/crates/horizon-mod-wasm/tests/fixture/target/horizon-mod/wasm_fixture.hmod" cargo test --release -p horizon-mod-wasm --test bench block_entity_ticks -- --ignored --nocapture
```

It measures 1,000 block entity ticks against an in-memory host, both without and with two host reads per tick; it is not a full game-world benchmark. The `wasm-mods` workflow runs it only on manual dispatch.

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

A mod carries native code or wasm code, not both. A wasm mod has no `[native]` table.

A mod with `[[inject]]` listeners builds against the game's staged point catalog. Pass
`--catalog path/to/points.json`, or set `HORIZON_WASM_STD` to the toolchain's std member
with `points.json` beside it. Mods without listeners need no catalog. When packaging the
wasm toolchain, `cargo horizon-mod wasm-std <out.wasm> --catalog <staged-points.json>`
copies that catalog beside the std member.

## Dependencies

`[dependencies]` work as for native mods. List the library mod in `horizon-mod.toml` and its
crate in `Cargo.toml`; build the library with `--wasm` first. The game links a mod with its
dependencies at load, one copy of each library shared by every mod that uses it, and refuses a
mod whose library is missing or another build than the one it was linked against, naming it on
the Mods page. An optional dependency is built in with the Cargo feature of its id, so its crate's
entry in `Cargo.toml` is `optional = true` under that id; an absent one is left out of the group at
load, and `if_installed!` keeps the mod from calling it.

## ABI reference

A dependency group consists of core wasm shared modules with `dylink.0`, not components.
The loader provides one linear memory, function table and standard-library member per group,
plus the `env` base globals and the `GOT.mem` and `GOT.func` relocations used by the members.
The loader supplies linker globals such as `GOT.mem.__heap_base` and `GOT.mem.__data_end`;
member constructors run after linking. Groups do not share memory with one another.

Pointers and lengths below are wasm32 `u32` values. The game provides these imports in the
`horizon` module:

- `host(id, in_ptr, in_len, out_ptr, out_cap) -> u32` invokes a generated import ID. Arguments
  are Wire values in order. It writes the reply if it fits and returns its length; if the length
  exceeds `out_cap`, call `host_reply(out_ptr)` after allocating that many bytes to copy the
  retained reply. Host errors trap the call.
- `host_reply(out_ptr)` copies that retained reply.

Each guest exports:

- `horizon_layout() -> u64`: the generated layout hash. A mismatch is refused.
- `horizon_in(len) -> ptr`: the buffer into which the game writes the next call's input.
- `horizon_hook(id) -> u64`: runs the generated export ID after the input is written. The high
  32 bits are the reply pointer and the low 32 bits its length; `u32::MAX` as the length denotes
  an error, with a Wire string at the pointer. The reply remains valid until the next call at
  the same depth. A handler call starts its input with the handler's `u64` ID.

The reserved IDs are `INIT = 0xFFFF_FF00` (guest package-list initialization),
`RELEASE = 0xFFFF_FF01` (guest handler IDs to release), and `HOST_RELEASE = 0xFFFF_FF02`
(host object ID to release). Other IDs index the generated import or export table. A hook reply
contains its return value followed by writable arguments in field order.

Wire values use little-endian numbers; lengths and collection counts are `u32`, strings are a
byte length followed by UTF-8 bytes, options have a byte tag (`0` or `1`), enums have a `u32`
case followed by its fields, and records contain their fields in declaration order. Readers reject
invalid tags, invalid UTF-8, trailing data, lengths that cannot fit in the remaining bytes, and
nesting deeper than 512. Interned static strings are limited to 256 bytes and 4096 distinct
strings per process. Execution, memory and table limits are enforced separately by the loader.

## In the game

The game's wasm boundary is being rebuilt. Each dependency group uses shared memory and one copy
of the standard library; its members are core wasm shared modules, not components. A layout mismatch
or an unresolved dependency is refused and named on the Mods page. Until the new loader lands, the
game refuses `mod.wasm`.

The game limits execution time, memory and table usage. A trap or limit hit disables the affected
group. APIs marked `Not on wasm` in their Rust documentation cannot be used by wasm mods.
