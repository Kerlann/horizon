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

A mod carries native code or wasm code, not both. A wasm mod may keep declarations under
`[native]` (`internals` and `injection`), but never native sources or build pins there.
A wasm mod's id is words that each start with a
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
game takes them back when it returns. API-typed points use `trait_method`; a raw point uses the
generated internals view below. A `helper` needs a native mod. A point call costs about as much as a hook call (a few hundred nanoseconds on a
release build), and a place nobody listens at costs nothing.

## Declared internals at raw points

For internals that the typed API does not yet cover, add `horizon-mod-internals` (library name
`mod_internals`) to the mod's Cargo dependencies. Declare each internal with
`cargo horizon-mod internals add <path>` and use `raw = "<function>"` instead of `trait_method`
on its injection. `cargo horizon-mod build --wasm` fetches the newest prebuilt game's
`internals.json` and generates `mod_internals::view` for exactly those declarations and the
types they reach. The generated code stays in Cargo's build directory. Opaque types and
functions that cannot cross the sandbox are refused at build, naming the declaration.

Call `mod_internals::raw_listeners!(first, second, ...)` once, listing the functions in the
manifest's raw-injection order. Each takes its generated raw record and an optional receiver:
`fn(view::HandleChatInputRaw, Option<view::Game>)`, for example. A free function has no receiver
and takes `Option<()>` instead. This compiled test mod shows the complete pattern:

```rust,ignore
{{#include ../../../../crates/horizon-mod-wasm/tests/internals/counter/src/lib.rs}}
```

A copy field has a getter and `set_<field>`, both returning `Result`; a handle field gives its
typed node. `Seq<T>` and `Map<K, V>` have `len` and `get`; `Opt<T>` has `case` and `some`.
Guest copies own their data: a native borrowed or shared copy is represented by `Box<T>`.
Small named copies are plain structs or enums; their node form, when used as a receiver, has
`Node` appended to its name. Same-named types retain their fully qualified generated names;
an unambiguous type also gets a short alias such as `Game` unless it conflicts with the view's
runtime types. A function's `&mut T` copy argument is also `&mut T` in the guest view: its value
crosses into a separate host local, and the final value is written back into the guest argument
after the call, including when the game returns an error. For example, `mutate(&mut count,
&mut small)?` updates both guest values without making either a node or aliasing game state.
Mutable node arguments still require the separation described below. Sets are opaque, not indexed
sequences. A returned node may not borrow a decoded copy local; such signatures are refused.
Elided return borrows on methods still resolve to `self`, not to copied arguments.

A function returning `Result<T, E>` (including a written `Result<T>` alias) can expose a copyable
`T` even when its error is opaque. Its view method returns `Result<Result<T, GameError>, Refusal>`:
`GameError` means the game function failed, without copying its native error into the mod.
A function returning `Option<&T>` or `Option<&mut T>` for handle-typed state returns an optional
call-scoped node. Like a directly returned reference, it expires after the next declared function
that actually runs, even when that function returns an error. With no pending writes it can be
that next function's receiver. Pending writes apply first and expire returned references before
the function's receiver and arguments are resolved.

A function that writes through a node requires that node to be separate from every other node
argument, including a shared one. Distinct struct fields or Vec/array indices can prove that
only when the entire shared path uses raw-pointer steps. Map keys, enum/Option variants and
VecDeque indices never prove separation; an object and its child cannot either. A returned-reference root is conservatively treated as overlapping
any other root. Unproved separation returns `NotHere("arguments overlap")` before applying writes.
Calls into the game cannot reenter a listener in the same wasm group. A native callback that needs
that synchronous reentry must stay native rather than silently deferring its work.

The listener sees its own journaled writes immediately. A declared function applies that journal
before running, and listener return applies the remaining writes. A trap drops writes since the
last function call and stops the dependency group; it does not undo calls that already ran.
Nodes are valid for one listener call only. A changed layout or missing declared internal refuses
the package at load, rather than guessing a new field number. No game rebuild is needed when
the declared layouts and functions are unchanged.

Native methods that collide with a field accessor use `call_<name>` (for example `call_set_mode`), determined from the full catalog field list, not the mod's declarations.

### Batched fields

Each copy field has an `x_field()` descriptor beside `x()` and `set_x()`.
Use `get_many((game.copy_field(), other.value_field()))` to read a tuple of up to twelve
fields in one crossing. It returns the matching tuple of typed `Result`s; a refused entry
does not stop the others. Use `set_many((game.copy_field().write(7), other.value_field().write(11)))`
to write a tuple. Both helpers are in `mod_internals::view`.

`set_many` writes join the listener's journal like single writes and are applied on listener return
or before a declared function call, not immediately at `set_many`. Reads see that journal,
including the last successful write to a field in a batch. Batches pay the same per-entry
base, path, journal scan and value fuel costs as single calls, including refused entries.
Raw batches over twelve entries trap before dispatch; valid inputs are prepaid and read outputs are charged before return.

### Internals budgets and trust boundary

Each listener call can retain at most 4,096 live nodes and journal 4,096 writes totaling 65,536
value cells (including writes flushed before a thunk). Retained journal payloads and keys together
are capped at 1 MiB per frame, and live-node paths have a separate 1 MiB cap; sequence storage
counts toward these byte limits too. A value can contain at most 1 MiB of string or byte data.
A write is checked against its field's codec before entering the journal. Exceeding these limits returns
`NotHere("internals budget")`. With fuel enabled, every import costs 1 plus its input and output
cells; a native thunk call also costs a fixed 1,000 fuel units.

**Native work inside a thunk is not interruptible.** Fuel and deadlines limit the wasm listener,
not the duration of a native function it calls. Daily prebuilts contain thunks only for paths in
committed, reviewed `wasm-internals/<mod>.txt` files. Reviewers must put unbounded functions
(for example, loops whose work scales with a guest argument) in `native-only.txt`. Building an
instance from source instead exposes the installed mods' declarations: choosing that build
trusts those functions to finish. A wasm-only instance can use this source-build route without
installing a native mod. A native panic is contained and stops the calling dependency group;
already-applied writes and native side effects are not rolled back.

## Calls into the game

A wasm mod reaches the game through the same `mod_api` as a native one, and each call crosses
the sandbox: a hook call into a mod costs about 1.5 µs, and a capability lookup about 2.5 µs on
top (measured on a release build). A `Capability<T>` holds no borrow and stays valid for the
tick, so a pipe or machine looks up its neighbours' capabilities once per tick and calls the
handlers it got, rather than looking them up again for every move. A mod's own handlers, and
those of mods in its group, are called directly without crossing.

`host.is_loaded("other_mod")` says whether a mod is installed, native, wasm or content, as
Fabric's `isModLoaded` does; the list is fixed at startup.
