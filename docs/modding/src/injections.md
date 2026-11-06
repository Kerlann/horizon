# Injections

An injection is a `[[native.injection]]` table in `horizon-mod.toml`. It is an escape hatch for a moment
the mod API has no hook for, and **every injection is a request for a hook**: write it so it can become one
by renaming its target, then [ask for the hook](#ask-for-the-hook).

It comes in two forms:

- A **point injection** (`trait_method`) listens at an *injection point*. Every `Game` method is one,
  apart from the few its shape rules out ([below](#which-methods-are-points)): the prebuilt game has a slot
  at each one's head and return, which the game fills at load with the listeners of the mods that name it.
  It changes no game source, so it builds against the prebuilt game in seconds.
- A **helper injection** (`helper`) splices your Rust into the game's source with
  [horizon-mixins](https://github.com/horizon-rust/horizon-mixins) before it is compiled. The game is
  rebuilt from source.

A `mod.wasm` listens at points too: its `horizon-mod.toml` lists the same point injections, with no
`[native]` pins, and the game fills those places from the `mod.wasm` at load, with no game rebuild and no
compiler ([Building a mod to `mod.wasm`](how-to/wasm-mods.md#listening-at-points)). Helper injections are
native-only: a mod that needs one cannot be a wasm mod until the hook exists.

## For agents

Apply these rules in order and stop at the first that fits.

1. A hook or a Part method exists for the moment: implement it. **No injection.**
2. No hook: a point injection. `target` (the `Game` method), `kind = "HEAD"` or `"RETURN"`, and
   `trait_method = "<point>"` (head) or `"<point>_return"` (return), where `<point>` is the method's name.
   Add `horizon-mod-points` to `Cargo.toml` beside `horizon-mod-api` (only a mod that listens at points
   does). Implement that method in `#[mod_api::horizon_mod] impl mod_points::Points` on the same type as the
   mod's `Hooks`, at the crate root. The body uses only `mod_api` types. This is the default injection.
3. You need private game internals, another kind, or another crate: `helper = "src/<file>.rs"` bound to
   `&mut Game`. Last resort. It breaks on game updates, rebuilds the game and never runs as `mod.wasm`. File a
   hook request with it.

Exact fields (unknown fields are refused):

| Field | Rule |
| --- | --- |
| `id` | unique, nonempty |
| `kind` | `HEAD` or `RETURN` for `trait_method`; any horizon-mixins kind for `helper` |
| `hook` + `hook_version` **or** `target` | exactly one of the two forms; a `trait_method` takes `target` |
| `trait_method` **or** `helper` | exactly one of the two |
| `bindings`, `cancellable`, `selector`, `require`, `ordinal`, `argument`, `slice`, `variable`, `result` | `helper` only: horizon-mixins' own fields. A point listener gets the point's record instead and cancels through its `cancel` |
| `priority` | horizon-mixins' order among helpers |

Point errors name the injection: its `target` isn't a point of the game the mod pins, or it names a method
that isn't the point's (`tick` at a `RETURN` is `tick_return`), or the mod's `impl Points` doesn't override
the method it names. Helper errors: `E_HOOK_UNKNOWN` (the `hook` name is not in the catalog: use a name the error lists, or use `target`);
`E_HOOK_VERSION` (the catalog lacks that `hook_version`: use a version the error lists, and rebuild against the
game the mod pins).

Naming: events and methods follow NeoForge 26.3 names with no `on_` prefix; the event is `&mut`, handles come from
the event, and writes take `host`.

## The decision ladder

1. **A hook or Part method exists: use it.** Read [Hooks](concepts/trait-mods.md) and the
   [rustdoc](reference/api.md). A per-block, per-item or per-entity moment is a Part method. Nothing in this
   chapter applies.
2. **No hook: a point injection (`trait_method`).** The default. Your `Points` method gets the side's host
   and the point's record (`mod_points::<Point>Args`): the function's arguments with API types, and a
   `cancel` or `result` where the function returns `()` or an API value. It uses only `mod_api` types.
   Every `Game` method is a point on both routes: the prebuilt game was built with them all, and a source
   build gets the same points, so a point injection always builds against the prebuilt game in seconds.
3. **You truly need private internals: a `helper` bound to `Game`.** The last resort. It reads game state no
   API exposes, so it breaks when the game changes, rebuilds the game, can never run as `mod.wasm`, and should
   always come with a hook request. `hook = "<name>"` names a catalog hook point's function for a helper, so
   the function can move without breaking it (see [the build log](#the-build-log)).

## The forms

| Form | Kinds | Where | Order | Game rebuild | `mod.wasm` |
| --- | --- | --- | --- | --- | --- |
| `trait_method` (point) | `HEAD`, `RETURN` | `Game` methods | dependency order | no | yes |
| `helper` | any | any crate | priority descending, then mod id, then injection id | yes | no |
| Hook or Part method (no injection) | — | — | dependency order | no | yes |

## Ordering

```text
a call site in the game
 │
 ├─ helper injections at the place
 │    priority descending → mod id → injection id
 │    an early return, or a claimed cancel, ends here
 │
 ├─ at a point: its head listeners (dependency order); a cancel skips the body
 │    and its return listeners after the body
 │
 ├─ trait-mod hooks (dependency order; ties in bundle order)
 │    the first Cancel stops the rest; replacement hooks: the first Some wins
 │    collection hooks call every mod
 │
 └─ the game's own body
```

Helpers run before trait-mod hooks, and an injection that returns early skips them. Dependency order
never changes injection order. `priority` defaults to 0.

## Make your injection convertible

An injection becomes a hook when its body is a single call a hook can take. Keep to four rules:

- **Take the host**, never `&mut Game`.
- **Never read `Game` fields.** Whatever you need comes from handles, attachments and services reached from the
  host.
- **One call.** Keep the body to a single call into your own code, so the hook's listener is that call.
- **One injection per moment.** Two injections at a moment are two hook requests.

The same behaviour, counting server ticks in an attachment, in the three shapes.

**1. A helper on `Game`** (works today, converts worst: it reads the game itself):

```toml
[[native.injection]]
id = "count"
target = "horizon_client::game::Game<'_>::tick_mobs"
kind = "HEAD"
helper = "src/count.rs"
bindings = [{ name = "game", ty = "&mut crate::game::Game<'_>", value = "&mut *self" }]
```

```rust,ignore
fn count(game: &mut crate::game::Game<'_>) {
    let host: &mut dyn mod_api::Host = game;
    if let Ok(old) = COUNTER.get(&mut *host, Holder::Level) {
        let _ = COUNTER.set(host, Holder::Level, old + 1);
    }
}
```

**2. A point injection** (the default). `Game::tick` is a point, so this builds on the prebuilt game. The
mod depends on `horizon-mod-points` as well as `horizon-mod-api`:

```toml
[[native.injection]]
id = "count"
target = "horizon_client::game::Game<'_>::tick"
kind = "HEAD"
trait_method = "tick"
```

```rust,ignore
{{#include ../examples/src/lib.rs:points}}
```

**3. The hook it becomes.** Say the catalog has a hook at that moment (the `server_tick` hook, which fires every server tick, stands in here), so the injection disappears and the
mod implements the hook. The listener body is the same call:

```rust,ignore
{{#include ../examples/src/lib.rs:tick_hook}}
```

```rust,ignore
{{#include ../examples/src/lib.rs:count}}
```

The `COUNTER` attachment is the same in all three:

```rust,ignore
{{#include ../examples/src/lib.rs:counter}}
```

Step 2 to step 3 is renaming: the `Points` method becomes the hook's, and its record the hook's event. That is what "convertible" means, and why shape 2 is the default.

## Ask for the hook

A gap is a hook request. File one as an issue on
[arismoko/horizon](https://github.com/arismoko/horizon/issues) in the form of a row of the API-gap table in #44.
Copy this, and fill every field:

```markdown
**Hook request: <short name>**

- **What the mod needs:** <one sentence: the moment, and what the mod does there>
- **NeoForge 26.3 counterpart:** <event or method name, or "none">
- **Vanilla class and method:** <`net.minecraft…Class.method`, the call the hook wraps>
- **The injection it replaces:** <the `[[native.injection]]` table, `target`, `kind` and binding>
- **Mod:** <name, repository or `.hmod`>
- **Side:** <server, client or both>
- **Contract wanted:** <listen, cancel, replace a value, collect; what a listener may change>
```

A hook with a NeoForge counterpart takes its name, side and contract. One without is welcome and names the
mod mixin or use it replaces. State the vanilla method: the hook fires where vanilla calls it.

## The build log

Before a build, the launcher reads each mod's injections and writes warnings to `native/build.log`. They never
stop the build:

- Two mods inject at one place: which runs first.
- Two mods wrap one function at its `HEAD` and its `RETURN`.
- A `cancellable` `HEAD` on a hook point: a `Break` skips the later injections and the function's body.
- A raw `target` that is a catalog hook point's function: ``use `hook = "frame"`, `hook_version = 1`, or implement
  `Hooks::frame` with no injection``.
  A scoped hook says "the `<name>` Part method" instead of `Hooks::<name>`.

A panic inside a point listener reports the mod and the method
(`in native mod <id>, hook <method>`).

## Support modules

A `[[native.module]]` table contributes a whole Rust module to a game crate, beside the injections:
self-contained items that a helper injection calls through ordinary paths. It is the shape
for support code a moment needs and one function cannot hold — Portizon's runtime is the case that
asked for it.

```toml
[[native.module]]
id = "runtime"
file = "crates/horizon-client/src/game.rs"
name = "portizon"
source = "src/runtime.rs"
visibility = "pub"          # optional: "pub" or "pub(crate)" (the default)
```

`source` holds the module's items without an outer wrapper; the tool emits them into `file` as
`mod name`. The destination file must carry no top-level glob import — horizon-mixins refuses one,
because it cannot prove which names the glob introduces — and the payload must be self-contained: no
`mod` items, `#[path]`, `include!` or file attributes; assemble multi-file code into one file first.
Ids are unique across a package's injections and modules together.

Modules are **native-only** and last-resort support code for helpers, outside the `mod_api`-only
line: a mod's behaviour still goes through hooks, handles and Parts. Any capability a module needs
that the API lacks is a hook request in #44's row form, not a reason to grow the module.

## Which functions are points

Every function of the game module is a point: `Game` methods, methods of its other types (`GameLevel`,
`Levels`…), associated functions and free functions. A crate below the game adds the functions
`crates/horizon-mod-api-gen/points.txt` lists, one path per line (`horizon_client_render::chunk::Chunk::get`);
each one there is measured first, since a slot on a hot function costs on every call. Left out:

- the hook functions themselves (`native_*`), which are hooks;
- a function compiled only under a `cfg`, and one in a `cfg`-gated module;
- `const`, `async` and `unsafe` functions, and methods that take `self` by value;
- `impl Trait` or `!` in the signature, `#[inline(always)]`, and a parameter named `cancel` or `result`.

A `Game` method that takes `self` has a point named after the method (`tick`). Any other is named by its full path, snake-cased
without `horizon_client::`: `game_levels_levels_parked_dims` for `Levels::parked_dims`, and for a lower
crate without `horizon_`: `client_render_chunk_chunk_get`, and `Game::new` is `game_game_new`. `Game` methods'
points get a host; the others have none.

A method whose return value borrows the game (`-> &T`, `-> Option<&mut T>`) has a head and no return; a
`'static` return (`-> Option<&'static str>`) borrows nothing and has both. A `usize` or `isize` crosses
the record as `u64` or `i64`, one width for native and wasm listeners.

**The raw form.** A point whose signature has no type or const generics and no `impl Trait` also has a raw
form: a native listener that gets the real receiver and a `&mut` to every argument in its own type,
internal types included, with `cancel` or `result` to stop the body. Its return listener gets the value
returned, a borrowed one included (`&mut &mut T`), where the API form has no return. The launcher's
generated `main` registers raw listeners by type; declaring them in `horizon-mod.toml` comes with the
per-mod internals list.
`points.json`, beside the prebuilt game, lists them all; the rustdoc of `horizon-mod-points` documents
each with the function it listens at.

The source tree carries no `#[point]`: the build of the prebuilt game, and the launcher's source builds,
put one on each method in the copy they compile. A plain `cargo build` of the game in a checkout has no
points; a mod listening there builds, and its listeners never fire.

## Raw listeners and declared internals

When an API-typed point cannot carry the state a mod needs, a native mod can declare game
internals and a raw listener. Each internal is an API request: propose the missing hook, handle
or Part on the [roadmap](https://github.com/arismoko/horizon/issues/771) so the mod can move back
to the typed API.

```toml
[native]
game = "26.3"
horizon = "<exact game commit>"
mixins = "<exact mixins commit>"
internals = [
    { path = "horizon_client::game::Game", hash = "<written by the tool>" },
    { path = "horizon_client::game::native_ticks::TickRound::listening", kind = "method", hash = "<written by the tool>" },
]

[[native.injection]]
id = "count"
target = "horizon_client::game::Game<'_>::tick"
kind = "HEAD"
raw = "count"
```

Run `cargo horizon-mod internals sync` in the mod's folder after editing its Rust code. It
checks the mod against a discovery-only copy of the pinned game's source, uses compiler
warnings to find actual internal uses (including glob imports and macros), and replaces
`[native].internals` with their exact paths, kinds and hashes. Unused declarations are removed;
retained declarations keep their formatting. The command prints added and removed entries.
It is explicit: a normal build never changes the declarations reviewers and players read.

Install `horizon-launcher` on `PATH`, or set `HORIZON_LAUNCHER` to its executable. The command
invokes `horizon-launcher native-build --sync-internals <mod folder> --trust-native-code`, whose
stdout is a JSON list of discovered catalog entries for the tool. This runs `cargo check`, not
the game, but Rust build scripts and procedural macros execute with your rights: run it only
on code you trust. No installed game or instance is needed. The launcher caches the isolated
source and metadata build per pin under `native/internals-sync/<commit>/`; the second check
reuses those compiler outputs. Neither that cache nor its temporary diagnostic attributes is
used by normal builds or published prebuilt games. The source must be cached or downloadable;
`E_INTERNAL_SOURCE_UNAVAILABLE` explains when it cannot be obtained, without changing the
manifest or falling back to a different pin. Any compiler or catalog error also leaves the
manifest alone. Discovery checks the mod's library on the current host with default features;
configuration-gated catalog entries are still refused. Mods with source-spliced helper files
or module injections are refused with `E_INTERNAL_SOURCE_INJECTION`: declare their internals
explicitly with `internals add` instead. This invocation stages one mod project; required mod
dependencies that are not in that project are refused by the dependency resolver, not silently
omitted.

`cargo horizon-mod internals add <path>` remains useful for those explicit declarations and
writes the path and hash from the pinned game's `internals.json`; don't invent hashes. When several member kinds share a path (such as a field
and a method named `listening`), pass `--kind <kind>` to select the catalog entry, for example
`cargo horizon-mod internals add horizon_client::game::native_ticks::TickRound::listening --kind method`.
The tool reports the available kinds if the choice is missing and writes the resolved `kind`
into each added declaration, even for unambiguous paths. A declaration may omit `kind` only
when its path has a single kind. Kind names match `internals.json`: inherent methods use `method`, free
functions use `fn`, and fields use `field`. A `--kind` applies to all paths in that invocation.
`cargo horizon-mod update` shows the signature changes and refreshes hashes while retaining
member kinds. Each injection has exactly one of `trait_method`, `helper` or `raw`.
`raw` names a function in the mod's crate root, not a helper source file, and accepts only
`HEAD` or `RETURN` with no mixin bindings or selectors.

Depend on `horizon-internals` (library `internals`) at the same pin as `horizon-mod-api`, never
on `horizon-client` directly. The build generates a facade for this mod alone. Its nested
modules re-export declared types and free functions: the declaration above makes
`internals::game::Game` available. Declaring a field or inherent method exports its owner type;
a type necessarily brings all its public fields and inherent methods. Undeclared types have no
name in the facade and do not compile.

The facade is generated when the game builds the mod, and it re-exports items the prebuilt
game makes public; the game's source keeps them private. So a plain `cargo check` of the
mod resolves `horizon-internals` to an empty crate and can't see `internals::…`. Compile
a mod that declares internals with `horizon-launcher native-build <mod folder>`, which stages it
against the pinned game the same way the launcher does on install, and report its errors.

A raw injection also exposes the point's listener and raw record in `internals::raw`, for
example `__TickListener` and `__TickRaw`. Discovery keeps those same typed exports and does
not change `[[native.injection]]`; raw listeners need no special sync step. Implement `pub fn count` with the exact listener type
from `points.json` (including the receiver and argument-record lifetime). The facade's safe,
typed `raw::tick` constructor registers it in the generated executable; a mismatched function
fails at that registration line, naming the injection. `RETURN` uses the point's return
listener and `raw::tick_return` constructor. Raw listeners work against the prebuilt game as
well as source builds; only source-spliced helpers and modules require a game rebuild.

Both routes check the actual game's metadata before compiling mods:

- `E_INTERNAL_MISSING`: the declared path is absent (type shapes not exposed as internals
  cannot be declared). Run `cargo horizon-mod internals sync` to discover this pin's used
  internals. Undeclared-item compiler errors also point to this command.
- `E_INTERNAL_CHANGED`: the hash changed; the error names the new signature and asks for
  `cargo horizon-mod update`.
- `E_INTERNAL_AMBIGUOUS`: several member kinds share the path; choose one of the listed kinds
  with `internals add <path> --kind <kind>`.
- `E_INTERNAL_GATED`: the entry exists only under a `cfg`; target-specific declarations are
  not supported.
- `E_NOT_RAW`: the injection's target has no raw form, for example because it is generic or
  takes `impl Trait`.
- `E_GAME_DEPENDENCY`: the mod depends directly on `horizon-client`; use the facade instead.
- `E_INTERNAL_PATH`: the entry cannot be re-exported as an item, for example an entire module;
  declare its types or free functions instead.

The build log lists each mod's internals and raw places. The same information is recorded in
`native-mods.json` beside the built game.

### Wasm raw listeners

A wasm mod uses the same `[native] internals` declarations and `raw` injections beside `[wasm]`,
but omits all three native pins and carries no helper or support-module source. Its generated view
imports `horizon:mod-internals/view`: roots, path nodes, plain values and declared function calls.
The compiled declaration includes each manifest internal and every named type whose layout it walks.
All paths and hashes are checked at load, before any listener registers. A field also requires its
owner's matching type hash. A missing opt-in entry names
`crates/horizon-mod-api-gen/wasm-internals/<mod_id>.txt`; a game built without a table refuses declaring mods.

The slot runs API listeners first, then raw listeners in the native/wasm registration order. A raw
listener has no Host; its only game access is through the view. Nodes last for one listener call:
keeping one into the next call refuses `gone`. Writes are journaled and reads see those writes.
Calling a declared game function applies the journal first and invalidates cached paths. A normal
listener return applies its remaining writes; a trap discards them and stops the group. Writes that
no longer resolve are dropped and logged. A trap after a game function call does not undo that call
or the writes already applied before it.

## Reference

- A point's methods are `<point>` (its head: the arguments it was called with) and `<point>_return` (after
  its body: the value it returns), on `mod_points::Points`. Its record is `mod_points::<Point>Args`.
- Depend on `horizon-mod-points` only to listen at points. It holds one record per point, so a crate that
  depends on it compiles a few seconds slower; a mod without point injections leaves it out.
- A listener's host is a `HostSide` on a function that takes `&mut self`, and `&dyn Host` on one that takes
  `&self`.
- The game sets a point's slots once, after the mods are installed; a point no mod listens at costs one
  atomic load and a branch.
- A point injection's `hook`, bindings and horizon-mixins fields are refused: a hook is its own `Hooks` method,
  and the record carries what the point passes.
- A helper on a `GameLevel` method that takes `&mut self` (`native_random_tick`, `native_tick_block`,
  `native_perform_bonemeal`, `native_is_bonemeal_success`), bind
  `{ name = "level", ty = "&mut dyn crate::mod_api::LevelRandomHost", value = "&mut *self" }` for attachments and
  `random()`. Scheduling and `set_block_property` stay on the `LevelTicksHost` and `LevelWriteHost` the hook points
  pass.
- A hook point's contract is versioned: `hook_version` names the contract a mod built against. A contract
  never changes; an incompatible change adds a version.
