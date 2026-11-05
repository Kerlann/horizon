# Where the mod API is going

The mod API is built toward one shape, and every change to it is judged against that shape. This page
explains the shape and why it is the one. The [trait mods page](trait-mods.md) says how to write against it
today; the [project board](https://github.com/users/arismoko/projects/1) and the pinned roadmap issue
[#771](https://github.com/arismoko/horizon/issues/771) hold the work that is still open.

## One clean break

A mod is written against one API, `mod_api` (the crate `horizon-mod-api`), and the API keeps one hook per
moment. A hook whose NeoForge 26.3 counterpart exists takes its name, side and full contract. Two hooks at the
same vanilla call site, `_v1` and `_v2` pairs included, are one hook, and the old one is deleted, not kept
beside it. A veto before an action and a notice after it stay separate when vanilla runs code between them. A
hook whose vanilla call site is gone leaves the catalog.

There are no shims: no aliases, deprecated names, fallback paths, version gates or "kept for old mods"
branches. An old mod rebuilds, and the compiler says what moved. Removing a path updates every caller, the
catalog, these docs, the tests and the sample mod in the same change. A hook with no NeoForge counterpart is
judged on its merits like any other.

Every hook is declared once, in the catalog (`crates/horizon-mod-api/src/hooks/`): its name, side, contract,
scope and `doc`. The generator writes everything repeated from it: the `Hooks` trait, the part traits, the
`Host` traits, the dispatch and the [reference](../reference/api.md). A hook is added by declaring it, not by
editing match arms.

## The `Host` is split by side

A hook receives the game through a host, and the host is a small set of traits, so that a call on the wrong
side does not compile:

- `Host` is the shared base: handle constructors, registries and settings.
- `ServerHost: Host` and `ClientHost: Host` are generated from each catalog entry's `side`. Only the server
  writes the world, so `transaction` is on `ServerHost` alone.
- A hook that fires on both sides receives a typed `HostSide` to match on. A hook fired inside a level write,
  where there is no game to borrow, receives the read-only `LevelHost` (a `LevelSide`) instead.
- Drawing goes through a `DrawContext`, a trait object given only to render and screen hooks, beside a
  `&dyn ClientHost`.

A method with a natural receiver belongs to a handle, not to the host: `EntityRef`, `LivingRef`, `PlayerRef`,
`LevelRef`, `BlockEntityRef` and the other handles carry the per-object methods. Reads take `&dyn Host`; writes
take `&mut dyn ServerHost`. Once a handle method exists, the id-based helper it covers is removed, so there is
one way to do each thing. Attachments live on handles too. Commands, players, messages and scheduling are small
services reached from the host, not methods on it.

A hook fires with the right host for its moment. A client-side decision that vanilla makes inside a client
library is split into decide, fire and apply: the library decides, the game fires the hook with a `ClientHost`,
and the apply re-checks the decision's preconditions and refuses, never recomputes, if they changed.

## Parts

A per-block, per-item or per-entity moment is a method on a part, as it is a method on `Block` or `Item` in
vanilla and NeoForge. A mod attaches parts once, in `Hooks::register`, and the game asks only the parts
attached to the thing in question. [Trait mods, Parts and attachments](trait-mods.md#parts) covers writing
them; the design is this:

- **One trait per scope.** `BlockPart`, `BlockEntityPart`, `ItemPart` and `MenuPart` are generated from the
  catalog entries whose `scope` names them, so a part on a plain block has no tick method to write.
- **A global hook only where NeoForge has a bus event.** Where NeoForge fires an event at the same moment as
  the override, that event is a hook on `Hooks`, asked first for everything, and its cancel skips the parts.
  Where it has none, the moment is a part method alone and has no global form. A mod reacting to its own blocks
  uses a part; one reacting to every block uses the event.
- **A part is configuration.** It takes `&self` and is `Send + Sync`, like a vanilla `Block`'s properties. State
  belongs to the game, in the block entity's slots, saved compound or attachments, so it saves, syncs and
  copies with the holder and is never kept in a mod's statics.
- **The same kind twice on one thing is refused at load**, naming the mods, and a part's kind is its type.
  Parts attach by key or by tag, and the tags resolve against each game's own tags.

Items, fluids and energy are Parts as well, as capabilities in NeoForge's transfer shape. A capability is a
`CapabilityKey` (`ITEMS`, `FLUIDS`, `ENERGY`) for a block entity, and an `ItemCapabilityKey` (`ITEM_ITEMS`,
`ITEM_FLUIDS`, `ITEM_ENERGY`) or `EntityCapabilityKey` (`ENTITY_ITEMS`) for a stack or an entity. The game
answers a key for what it owns, a mod's provider answers it for what the mod owns, and the first to answer
wins. Handlers are NeoForge's `ResourceHandler<R>`, and every write runs in a transaction opened with
`host.transaction`:

- **One journal, in the game.** A handler holds no state and no mod writes a journal. A write applies in place;
  the first write to a storage at each depth snapshots it; a nested commit hands its snapshots to its parent;
  an abort restores them. Notifications to the world run once per changed storage at the outermost commit.
- **One move.** `move_resources` moves items and fluids between two handlers, each step all or nothing, and
  energy shares its step. A handler that answers more than it was asked or offered is a mod bug: the move is
  undone and the game warns once, naming the mod.
- **Exact units.** Fluid amounts are millibuckets. A Fabric amount that is not a whole number of millibuckets
  (81 droplets) is refused, never rounded.
- **Holders have stable identities.** An `ItemPlace` names where a stack lives, so a handler inside an item
  inside a player's inventory changes the stack where it is, journaled.

Systems that run over many things at once can be added beside the part methods later without changing them,
because the game owns the data, the data saves and syncs through the attachment codecs, holders have stable
identities, and a per-hook part method stays when a system joins it.

## One dependency model, native or wasm

A mod can be built two ways, from the same source: native, compiled into the game, or to `mod.wasm`, which the
game's one sandboxed runtime runs with no rebuild. The only difference between them is the build target.

- `[dependencies]` in `horizon-mod.toml` means the same for both: required and optional dependencies, semver
  against the manifest's versions, dispatch in dependency order, and a mod publishing functions and statics its
  dependents call. Native links through Cargo; wasm links through the runtime's linker, with one copy of each
  library shared by its dependents.
- There is no wasm-only packaging unit and no native-only dependency rule. A change to the model lands for both
  targets together.
- The wasm interface is generated from the same catalog as the `Host`, at build time, and never committed: a
  committed generated file would make every two catalog changes conflict. CI reports the added, changed and
  removed imports of a change in its job summary, and a coverage test fails on an entry with no binding.
- There is no second API. Every `mod_api` type is plain data or a resource, both generated, and nothing in the
  API is written twice for wasm. An API shape that cannot cross is fixed in the API.

[Building a mod to `mod.wasm`](../how-to/wasm-mods.md) is the how-to. The two targets differ in how a mod is
checked against the game, because they differ in what they link to:

| | Native | Wasm |
| --- | --- | --- |
| Pinned to | the exact game build, in `[native]` | the interface: the hooks it imports |
| Refused when | the build differs | an import it needs is missing or changed |

A wasm mod keeps running on a newer game while the hooks it imports are unchanged. `horizon-mod-api` therefore
always builds for `wasm32-unknown-unknown`: a hook that needs an engine type takes a plain-data type of the
API's own, converted on the game side.

## Injections stay, and the API comes first

`[[native.injection]]` stays as the native-only escape hatch for what the API does not yet reach. The rule is
API first: a mod needing something the API lacks asks for the hook that replaces the injection, and the
injection is the stopgap until it exists. The [injections chapter](../injections.md) holds the current forms,
the decision ladder and the hook-request format.

## The order of the build route

Compiling a native mod against a game already built is the route that makes native mods cheap, and each step
rests on the one before:

1. **The prebuilt game.** The game as a dylib with its metadata, built daily and published as prerelease
   builds, so a native mod compiles against it in seconds instead of rebuilding the game.
2. **Slots.** Injection targets compiled into the prebuilt game, so a mixin does not force a rebuild. The
   generated slot points exist; filling them from a mod follows.
3. **The internals tier.** A per-mod declared list of game internals the prebuilt game exposes. It is native
   only and logged, and each internal a mod needs is a request for an API.
4. **The blend.** A wasm mod that also carries slot mixins. An API-typed mixin is filled by the `mod.wasm`
   itself, with no native code. A mixin that needs internals ships as source, compiles on install, and marks
   the package as including native code. The native and wasm parts talk only through interfaces the mod
   declares, with plain data or resources crossing, so raw internals never reach the wasm side.

A mod that needs a change to a non-slot part of the game still rebuilds the game.

## No telemetry

The launcher and the game send nothing about the player or their machine to any server: no performance, crash,
bug or benchmark uploads, and no "I'm playing" ping. Reports stay local files the player can read and share by
hand. A mod API that reported on mods would break this, so none does; the repository's `docs/telemetry.md`
says what stays local.

## What is next

The open work is not listed here, because a list in a page goes stale. It is on the
[project board](https://github.com/users/arismoko/projects/1), grouped by phase, and the pinned roadmap issue
[#771](https://github.com/arismoko/horizon/issues/771) gives the direction. A change to the mod API updates its
catalog `doc` and the pages it affects in the same pull request, so this page and the reference follow the code.
