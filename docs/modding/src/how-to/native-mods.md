# Native mods

## Native mods

Behaviour that needs code (a new key, a HUD, a tool that changes how mining works) is a native mod.
It is one flat Rust project with `horizon-mod.toml`, `Cargo.toml` and `src/`, beside its `assets/` and `data/`.
Start with `cargo horizon-mod new my_mod --template native` and implement the typed hooks;
raw injections are a rare escape hatch ([Injections](../injections.md)). How the launcher builds one, what it pins and why it is not sandboxed:
[native mods](native-mods.md).
The typed hook API is generated automatically at build time, in debug and release builds alike; its generated
Rust files are build output, not files to regenerate or commit. Engine contributors adding a hook only run
`cargo run -p xtask -- mod-api` to fill its hand-written placeholder body
([Adding a hook](../concepts/trait-mods.md#adding-a-hook)).

A native mod's config files go in the game's shared `config/` folder, beside `options.txt` (on a dedicated server,
beside `server.properties`), as Fabric and NeoForge mods' do: the mod picks its own file names, such as
`config/<mod id>.json` or a folder `config/<mod id>/`. It reads and writes them with `Host::config_files()`, which is
bound to the calling mod and refuses any other mod's files. A mod that lists its files with
`native_register_configs` gets them read for it, written from their defaults when missing, and handed over again
when edited (`native_config_loading`, `native_config_reloading`, as NeoForge's `ModConfigEvent`), and a world can
keep its own server config in `serverconfig/` ([native mods](native-mods.md)). Settings kept with `native_set_setting` stay where they were, in `horizon-mods/.horizon-mod-data/<id>/`.
8c838a6b381b93150deaeed2057acd1

For a client HUD's session information, use the read-only
`ClientHost::client_session()` service and the local `PlayerRef`'s
`block_breaking` and `ambient_mood` reads rather than raw injections. Missing
sessions, external display names and memory addresses remain explicit optional
values; all counters stay local. See [client session and local-player status](../reference/hook-notes.md#client-session-and-local-player-status)
for their exact availability and frame-time semantics.

`native_compute_fov_modifier(player, base, modifier)` runs on the client each tick for the local player's FOV scale, after vanilla effects and before zoom and camera easing. `player` contains the calculation inputs and `base` is the original scale. Non-finite modifiers or values outside 0.01..=10 fall back to `base`. It does not affect a spectator camera on a mob.

The client hook `native_render_name_tag(entity_id, kind, vanilla_visible, visible, text)` runs at player and mob, armor stand, and item frame name decisions before shared geometry. Set `visible` to override vanilla and `text` to `Some` literal replacement text (NeoForge `RenderNameTagEvent`). Distance and entity availability still apply; the local player's tag is considered only when its own camera and settings gates allow it.

Native mods are Rust projects that implement the game's typed hook API (`impl Hooks`). Start with
`cargo horizon-mod new my_mod --template native` ([Trait mods](../concepts/trait-mods.md#trait-mods-draft)).
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

## Server services and player handles

Server hooks reach commands through `host.commands()`: `run_command(line)` uses
console permission level 4 and the existing console command path, while
`feedback(text)` answers the current command source. `host.messages()` provides
`broadcast(text)` (including the broadcast hook and server log) and
`text_allowed(text)` (the server chat filter).

`host.players().server_players()` returns `Vec<PlayerRef>` in connection order;
`host.players().acting_player()` returns the optional `PlayerRef` whose state is
swapped in for this hook. The online list includes connected players even during
nested player turns; a displaced body's reads still refuse where they did before.
`host.players().player_by_name(name)` finds a connected real player by account
name, case-insensitively, or returns `None`; it does not match nicknames or fake
players. `host.levels().list_levels()` returns loaded server level handles in
vanilla dimension order, or an empty list before a server world opens. Both
services are available through `&dyn ServerHost` and do not mutate the world.
Read identity through `player.uuid()`, `player.name(host)`,
`player.permission_level(host)` and `player.address(host)`. The address is the
connection IP without a port (the forwarded address behind a Velocity proxy),
empty for an acting player with no known connection. Name, permission and address
reads retain connected identities during displacement and refuse stale handles.
Use the entity and living handles for world-state reads.
No snapshot of event data is returned by these services.

Send to one player with `player.tell(host, text)`, `action_bar(host, text)` or
`tab_list(host, header, footer)`. `set_nickname(host, nickname)`,
`set_sleep_ignored(host, ignored)` and `hide(host, permission)` keep their existing
server behavior. Messaging reaches the acting player and other connected players,
including displaced online records; it refuses stale or disconnected identities.
Vanish state is stored by UUID and can be changed with a retained handle after
disconnection, until its world epoch closes. It cannot target arbitrary offline
UUIDs; retain the handle value, not the borrowed service. Nickname and sleep
writes still require an online player. The settings survive reconnects.

These small services exist only on `ServerHost`, not `Host`, `ClientHost` or a
restricted level host. Native and wasm mods use the same calls: service methods
are generated imports, and acquiring a service just reborrows the current call's
frame resource. Block and fluid scheduling stays on `LevelTicksHost`, and
`tick_stats()` stays on `ServerHost`.

## Typed server commands

Register each server usage with `register_commands` and `NativeCommand`. The
`perform_command` and `perform_console_command` events carry a `CommandInvocation`,
not a raw line to split. Match its `literals` to your command path, then read named
values through `command.argument(name)` and `CommandArgumentValue`. An integer is
already an `i32`; a player argument is a live `PlayerRef`, not selector text.
Return `Flow::Cancel` when your listener handles the command.

The server parses and checks permissions before resolving arguments or calling a
listener. Omitted optional arguments are absent, including an omitted optional
player: they do not silently select the source player. A present player argument
must select exactly one online player; missing players or invalid input produce
command feedback without calling the listener. Word and greedy text arguments
are strings, numbers are finite doubles, booleans are typed, and item arguments
are component-bearing stacks of one item. Block positions are resolved and floored
from the source's world, relative or local coordinates without loading chunks.
A player's source is their position and rotation. The console has no source
entity and uses the Overworld spawn's block corner with zero rotation, so `@s`
cannot resolve there. Both hooks receive the same owned data, on native and wasm.

## Observing advancement progress

Subscribe to `player_advancement_progress` to inspect the whole advancement immediately after
one criterion is granted or revoked, before its completion rewards. `event.progress.criteria`
includes obtained and unobtained criteria; `obtained_at` is the original saved grant date or
`None`. `requirements` lists the OR groups, `completed_requirements` counts satisfied groups,
and `done` is the game's completion result. A player-issued `/reload` mutates and snapshots
the acting player's live progress, including during a nested turn. For a suspended outer
player, reload grants still use the displaced hub record; full owner-correct reload awards
for suspended and inactive players remain tracked in #1240.

Keep `event.progress.clone()` if it is needed later: the owned `AdvancementProgress` snapshot
cannot change the player's progress and is not updated by subsequent events. No-op grants and
revokes do not fire. The snapshot uses the same plain-data bindings in native and wasm mods.

## Filtering toast notifications

Implement `Hooks::toast_add` to filter a client notification before it enters its
queue (NeoForge `ToastAddEvent`). The hook receives a read-only `ClientHost` and
`ToastAddEvent`: `kind` is a `ToastKind`, and `title` and `description` are mutable
text components. Return `Flow::Cancel` to discard only that candidate, or
`Flow::Continue` after optionally replacing either component. No slot, animation
or toast sound is started for a cancelled candidate.

The families are `Advancement`, `Recipe`, `Tutorial`, `System` and `Other` (including
now-playing music). Match non-exhaustively. For advancement toasts, the title is
the task/goal/challenge label and the description is the advancement's name, not
its advancement-screen description. Recipe notifications keep their rotating
station/result icons; tutorial hints keep their progress bar. Edits change only
the displayed text, not these identities or lifetimes. Setting the optional
description to `None` removes that line.

The hook runs for new additions, not in-place `addOrUpdate` changes to an existing
recipe or system toast. A cancelled tutorial hint is not retried every frame;
a later, different hint is a new candidate. Title-screen backup/access failures,
narrator notices, pack download/failure and chunk IO notices all use the same
admission point. Client-lib prepares title candidates and Game admits them with
a host before applying them; candidates produced during the initial resource
loading screen wait until that handoff. The pause screen's persistent song label
is HUD content, not a queued toast, and is not filtered by this hook.

## Native mod source revisions

A native mod targets one exact Horizon source revision: `horizon` in its `horizon-mod.toml` `[native]` section.
The same section pins `game` and the exact `mixins` commit. Match the mod to the game revision it will run with,
and rebuild it after game updates. A source pin is not a promise that a later game or launcher will keep
building the mod unchanged.

An installed native build with a missing or invalid build record must be rebuilt from an installed
Horizon version rather than used as a source revision.

Hook points are functions the game calls at fixed places. Their names, parameters and call sites belong to the
game source and may change. Per-hook catalog versions describe individual contracts, not minimum game versions
or a cross-release compatibility range.

Hooks are declared in Rust now, not in JSON: a hook or helper author adds a `#[hook(...)]` (or `#[helper(...)]`)
declaration, with its `///` doc, to a file under `crates/horizon-mod-api/src/hooks/` (a new file for a new area, named
for it). The generator discovers these files without a shared module list. `native-hooks.json` and
`tools/native-hook-fragments/` are retired, and a file there fails the build. Collection updates
the catalog, not a global API number, source comments or documentation histories.

Hand-written API types have a selected public surface in `crates/horizon-mod-api/src/exports/<area>.rs`:
module declarations and re-exports stay in their source area's fragment. Engine-only re-exports sit in a
separate section of the same fragment and keep their existing paths. The generator discovers all fragments
and includes each section at its original scope. A new hand-written area adds one fragment, while
catalog-generated hook types need no export-list edit. Do not add declarations or re-export lists to
`lib.rs`.

## Runtime packages and identity

The launcher generates the game's embedded native inventory from the selected `horizon-mod.toml`
manifests: mod id, version and side. Cargo package metadata and hook factories do not supply identity.
The game validates each compiled mod's package at
`<executable directory>/horizon-native-mods/<id>/` before installing hooks or starting a world.
Missing, unreadable, content-only or identity-mismatched packages refuse startup with the affected
mods and paths; restore the packages exported with that executable or rebuild it. There is no
build-record fallback and no discovery of native runtime packages from the working directory.
Content-only mods still follow the working directory, player toggles and explicit `--dev-mod` paths;
a content package cannot shadow a compiled mod's id.

Move the executable, prepared resources and canonical package directory together. The copied
projects retain their flat manifest, crate, assets and data. The game's mod list and crash details
use its embedded inventory, including code-only mods. Display names and descriptions come from the
validated manifests; Controls bindings are declared through `Registrar::register_key_mapping`, not a separate `NativeMod` metadata hook.

`native-mods.json` is provenance and rebuild status, not game identity, authentication, announcements
or startup input. The launcher retains it for build/status displays. The separate `horizon-server`
supervisor may report the adjacent record as a provenance snapshot, not a query of the running engine.
Its SHA-256 values and revision describe the built binary's inputs, not the running mod set.
The launcher prepares jar data from canonical manifests as non-authoritative candidates; the engine
validates identity against its embedded inventory. Original-jar pack order and
whole-package refusal rules are in [Original Java jar layers](../reference/manifest.md#original-java-jar-layers).

## A bundle

The project and its `.hmod` have the same flat layout, with no mod-id directory level inside the archive:

```text
my_mod/
  horizon-mod.toml        # the only manifest; mod identity and build pins
  icon.png               # optional launcher icon
  assets/my_mod/
  data/my_mod/
  Cargo.toml             # one crate; Cargo metadata is not mod identity
  src/lib.rs             # impl Hooks
  src/frame.rs           # optional raw injection helper
```

Without `Cargo.toml`, it is a content mod. Code requires both `Cargo.toml` and `[native]`;
inconsistent declarations are refused. The mod's id and version come only from `horizon-mod.toml`,
not Cargo's package name or version. An id is a plain `[a-z0-9_]{1,64}` name, also used as its resource namespace.
A code mod has no `rust-toolchain.toml`: the pinned Horizon commit decides the toolchain that builds it.
There is no input `native/` or `rust/` directory, `horizon-native.json` or hand-written `horizon-mixin.toml`.
The launcher writes the tool's manifest and stages the crate itself; those internal paths are not package layouts.

The manifest keeps the [content fields](content-mods.md#the-manifest), including the exact `api = "0.2"`
contract and `capabilities = ["registries"]`. A code mod adds:

```toml
[native]
game = "26.3"
horizon = "<exact 40-character lowercase Horizon commit>"
mixins = "<exact 40-character lowercase horizon-mixins commit>"
```

The placeholders above must be replaced by exact commits; the native template fills them in.
Unknown fields are refused. `icon.png` has no manifest key: when present it must be a valid,
nonzero square PNG, at most 512 × 512 pixels and 256 KiB encoded. It is optional for both content
and code mods; the scaffold does not create one.

Raw injections belong in the same manifest as `[[native.injection]]` tables. The decision ladder, the fields and the
forms are in the modding book: [Injections](../injections.md).

### Dimension shape

`LevelRef::dimension_shape` reads the loaded server level's build range, logical height,
skylight and ceiling from its dimension shape. It accepts `&dyn Host`, including when the
level is parked, and returns `None` for a stale or unavailable level or on a client. It does
not load a level or allocate.

### Raid handles

Raid handles returned by `LevelRef::raid`, `raid_at` and `raids` resolve their
own loaded server level even while another dimension is in place. Raid reads,
omen-level writes and `stop` do not swap levels. Raider lists and living health
come from that level's mobs; raid ids are local to a level. A closed level, an
earlier world epoch or a client refuses the handle. Entity handles in a parked
raid's member list keep their identity; their ordinary entity operations still
require the entity's level to be in place.

### Tool transformation previews

`LevelRef::block_transform` previews a tool against a loaded block in the server level in place.
Pass the live player, hand, stack, position and hit; the owned result describes the new state,
relative durability cost, sound, loot and particle. The call does not apply the result. It fires
`block_tool_modification` with `ToolUse::simulated` set to `true`: listeners must not change the
level during a simulation. A stale or unavailable player or level, unloaded position, invalid face,
or tool with no transformation returns `None`. The real action fires the same hook with
`simulated` set to `false`.

### Named hook points

The declarations under `crates/horizon-mod-api/src/hooks/` name each hook point and the version of its contract (signature,
when it is called, what it does): `#[hook(side = "client", contract = "listen", ...)] fn render_gui_post();`, fired by `horizon_client::game::native_hooks::Game<'_>::native_render_gui_post`.
An injection names `hook` and `hook_version` instead of `target`, and horizon-mixins resolves it from the catalog
of the source it applies to, so a hook point can move in the source without breaking the mods that use it. A name
the catalog lacks is `E_HOOK_UNKNOWN` (the error lists the names it has), a version it lacks `E_HOOK_VERSION`. A
contract is never changed, its `mod_api` entry (the trait method's contract) included: an incompatible change adds
a version. New parameters before the out-parameter, with the old ones kept in name and order, keep the version: a
binding names a parameter, and a trait mod's event gains fields; a test that calls the `dispatch_*` function passes them. Helpers (`native_text`, `native_player`…) have no hook entry; the catalog's `helpers` list names the ones
the generated `Host` offers, and they are kept stable as the hook points are. A test (`catalog_matches_hook_points`) keeps the
catalog and the hook points' sources (`native_hooks/`, `horizon-mod-api`'s `render.rs`, `horizon-world`'s `loot.rs`) in step.

### State flags on the living handles

`host.living(entity)` and `host.mob(entity)` make the handles of [#7387](../reference/hook-notes.md)'s handle design, and
the state flags sit where vanilla keeps them: the ones `LivingEntity` owns read and write through `LivingRef`, the ones
`Mob` owns through `MobRef`. Reads take the shared host, writes the server one; a stale handle or a kind the game keeps
no such state for reads `None` and writes `false`, as every handle does.

Through `LivingRef`: the glide (`is_gliding`, `gliding_ticks`, `stop_gliding`), the sleep of a server player or a
villager (`is_sleeping`, `sleeping_pos`, `start_sleeping`, `stop_sleeping`, `set_sleeping_pos`, `clear_sleeping_pos`),
the jump of the player in place (`is_jumping`, `set_is_jumping`), climbing (`is_climbing`, the same `onClimbable`
movement uses, including the gliding exclusion for vines) and the baby read (`baby`). Server player sleep writes
work for the integrated owner, LAN guests and dedicated-server guests in the level in place, both inside and
outside their own turn; guests in another dimension and fake players refuse all four sleep writes without
mutation. A suspended outer actor is unavailable during a nested turn. Glide counters and stops follow the
same authoritative body across turns. These reads do not add client tracked-entity coverage.

`start_sleeping` requires a bed with an available sleep shape before changing any state (a villager also refuses an
occupied bed). It moves the sleeper onto that shape and occupies the bed. `stop_sleeping` stands the sleeper up
beside the bed with the normal wake animation, teleport and pose sync; the usual level pass frees a player's bed.
`set_sleeping_pos` and `clear_sleeping_pos` only change a player's sleeping position/pose, without moving them,
occupying a bed or playing a wake animation. The next entity sync publishes the changed pose and bed to clients.
Villagers use `start_sleeping`/`stop_sleeping`, not the raw player position setters.

`EntityRef::portal_cooldown` takes `&dyn Host` and reads the remaining portal cooldown in ticks from the
server's portal state: a player's own (the acting player inside its turn and other players outside it), and the
level's for mobs, items, experience orbs, boats, minecarts, primed TNT, falling blocks, arrows and the thrown items
and fireballs a chunk saves, fireworks and tridents; an ender pearl's comes from End gateways only. Zero means no
cooldown; `None` means a kind with no portal cooldown (armor stands, wind charges, the separately modelled dragon and
others), a client, or a stale, parked or otherwise unavailable handle. The read does not load levels.

`MobRef::can_use_portal` reads the existing server portal decision for a mob in the current level, including mounts and player riders. It takes `&dyn Host`: `Some(false)` means the mob is ineligible, while `None` means a client, stale or unavailable handle, parked mob, or the separately modelled dragon. It does not load a level.

`MobRef::leash_holder` takes `&dyn ServerHost` and reads the live mob leash. `MobLeashHolder::Unleashed`
is a definite absence; `Entity` holds an existing entity handle for a player, mob or boat, and `Knot` holds
fence coordinates. `None` means the mob or holder is unavailable, the level is parked, or a saved holder has
not resolved; this read neither loads a level nor changes the leash.

Through `MobRef`: `no_ai`/`set_no_ai` (`Mob.isNoAi` and `setNoAi`: the mob stands, saved as `NoAI`) and
`set_baby` (`Mob.setBaby` through the canonical species-specific setup). Ageable mobs start at their species'
baby age (`-48000` for a sniffer, otherwise `-24000`) or become adults at `0`, including the age-boundary
attribute, navigation and AI changes. Their age is saved as `Age` where the kind saves one. Non-ageable babies,
such as zombies and piglins, keep their non-growing baby state instead. A species with no baby form refuses
`set_baby` without changing anything. Neither `set_baby` nor `set_no_ai` changes `PersistenceRequired`.

The persistence pair `persistence_required`/`set_persistence_required` reads and sets `Mob.isPersistenceRequired`;
vanilla gives that setter no way back. `requires_custom_persistence` is a separate despawn predicate
(`Mob.requiresCustomPersistence` and its species overrides): passengers, leashes and species-specific reasons
such as an enderman carrying a block. It is independent of `PersistenceRequired` and `NoAI`, not an alias for
either flag. The separately modeled dragon has no such predicate and answers `None` to this query, even though
its persistence flag can be read and set. A `/summon` and a spawn egg set their flags before the entity-join hook
runs, so a mod's own write in that hook is what stays.

### Client effect interpolation

`LivingRef::effect_blend_factor(host, effect, partial)` reads the local player's client effect blend.
A newly blended ordinary effect reads 0 before its first effect tick, then interpolates to its
post-countdown target each client tick: 1 while active, 0 once expired. A finite effect that expires
on its first tick never rises; an infinite effect still has the initial interpolation.
An unblended addition starts at its target immediately; replacing an existing effect keeps its blend.
Darkness and nausea use their existing longer fades. Pass the render partial tick unchanged: the read
does not clamp it. A missing effect reads `Some(0.0)`; an unknown effect, stale handle or entity other
than the local player reads `None`. Other visible entities' effect blends are not yet supported.

### The build's warnings

Before it builds, the launcher reads the mods' injections and writes warnings to `native/build.log`; none stops the
build. They are listed in [Injections](../injections.md).

### A mod's own ingredient kinds, matching at craft time

The load-time list only names items. Matching a concrete stack — its components, usually —
is `native_ingredient_test(context, kind, data, stack, accept)`: the ingredient's `test(host, stack)` reaches it with the loop `context` of the side `host` runs on, the
kind and JSON the load-time hook saw, the stack being matched and the verdict, already `true`
(the list named the item). Set `accept` to `false` to refuse the stack (`ICustomIngredient.test`).
The hook may be called many times a craft; the answer must be the same for the same stack.

### The mod's crate

The root `Cargo.toml` and `src/` form the mod's one crate. It depends on `horizon-mod-api` (lib
`mod_api`) and never on the game, so the same crate builds native or to `mod.wasm`; the launcher
links it into the generated game executable, rather than making the game library depend on the
mod. The launcher stages the crate as `native-api/<mod>/`, rewrites its mod API dependency to the
staged one and resolves [mod dependencies](../concepts/trait-mods.md#dependencies-between-trait-mods).
A project has no separate workspace or lock file. Arbitrary path and git dependencies are refused;
the launcher supplies the checked paths needed for the game and declared mod dependencies.

**Dependencies come from Horizon's `Cargo.lock`.** In `Cargo.toml`, a mod names registry crates by
version requirement (`crc32fast = "1"`, `{ version, features, default-features, optional }`); each
must already be in Horizon's lock file in a matching version. The launcher checks every downloaded
package in the staged lock against Horizon's: name, version, source and checksum. Only the staged
workspace packages are added. A missing crate or a feature that introduces an unlocked dependency
stops the build; the game still builds with `--locked`.

Crates included only for mods are listed as optional dependencies in
`crates/horizon-mod-deps/Cargo.toml`: `mysql` 27 (blocking MariaDB/MySQL, with
`minimal-rust` and `rustls-tls-ring`, no default features), `redis` 1 (blocking,
no default features), `tungstenite` 0.30 (websockets over a stream the mod opens,
no default features), and `rusty-opus` 1 (Opus encoder and decoder). These entries
add their dependencies to the lock without building them into the game. A feature
that introduces a dependency not listed there is refused.

### Conditional data

A data file that carries NeoForge's `neoforge:conditions` loads only when every condition holds, and is
silently absent otherwise — as on NeoForge, where packs gate cross-mod recipes on the other mod being
there. The recipe loaders (crafting, the stations, brewing, the recipe book, and the packs' overlays
over them) all answer the gate; `mod_loaded` asks which mods' data is in the root: `minecraft`, always,
and the content mods that are on. `neoforge:true`, `not`, `and` and `or` combine; a condition the game
does not know leaves the file out, so a future shape fails closed. Tags, loot tables and advancements
do not answer the gate yet.

### Reading recipe rules

`recipes_of_type` returns the loaded recipes after pack overrides. Shaped,
shapeless and cooking recipes expose their resolved ingredients and fixed
results. Stonecutting, smithing and special crafting recipes expose a typed
`RecipeRule` instead, including resolved ingredient roles. A smithing or
special rule's result depends on its inputs: its template is not a fixed
stack. Use `LevelRef::craft` for the actual output and remainders of a
crafting grid. Smithing transform's template and addition ingredients can be
absent, requiring empty slots. Trim patterns are either registry references or
inline definitions. Result templates retain their count and component patch;
stonecutting exposes the complete fixed output. A mod-defined recipe type
retains its original `json`. Look up recipes again after a data reload.

### Recipe filters

A content mod or a data pack can remove recipes, or swap one of their inputs or outputs, without knowing
their ids: a file `data/<ns>/recipe_filter/<name>.json` holds `{"filters": [...]}`. Each filter has match
keys, all of which must hold (a filter with none matches every recipe), and exactly one action:

| Key | Meaning |
|---|---|
| `id` | the recipe id; `*` matches any run of characters (`minecraft:*_planks`) |
| `type` | the recipe's `type`, with the same `*` (`mymod:*`) |
| `input` | an item id or `#tag` among the recipe's inputs |
| `output` | an item id among its outputs (anything under `result`, `results`, `output` or `outputs`) |
| `remove` | `true`: the recipe is not loaded |
| `replace_input` | `{"from": ..., "to": ...}`: every input that is `from` becomes `to` |
| `replace_output` | the same, for the outputs |

```json
{"filters": [
  {"id": "minecraft:*_planks", "remove": true},
  {"type": "mymod:*", "replace_input": {"from": "minecraft:iron_ingot", "to": "#c:ingots/iron"}}
]}
```

Filters run on each recipe after the packs are merged and its load conditions held, so they see the final
set and reach every recipe type, mods' own included: they work on the recipe's JSON, every string outside
`type`, `group`, `category` and the conditions (a `{"tag": ...}` object's tag reads as `#tag`). Every recipe
loader applies them: crafting, the stations, brewing, the recipe book and what players are sent. Files apply
in id order and each file's filters in order; a pack's file replaces a file of the same id, as any data file
does. A filter with an unknown key or not exactly one action is skipped with a warning, so a typo never
matches every recipe.

### A mod's own enchantment effect kinds

A `minecraft:post_attack` entry with `enchanted` attacker and `affected` victim whose `effect.type` is in a
namespace outside `minecraft:` reaches
`native_enchant_effect(enchantment, kind, data, level, victim)` when a player's weapon hits a mob and the entry's
`requirements` pass. `victim` is the mob's id, and the acting player is the one holding the weapon. A bare type
name is minecraft's own. Other components and pairs, a hit on a player, and an entry inside `minecraft:all_of`
aren't told yet. An unclaimed kind does nothing.

### A mod's own consume effects

A consumable's `on_consume_effects` entry of a type in a namespace of its own reaches `native_consume_effect(kind, data)` when the item is consumed, on the server
thread, for the acting player. The kind and the JSON the file wrote are there; the game's own effects,
sounds, teleports and drops are open to the hook. An unclaimed kind does nothing.

### A mod's own loot functions and conditions

A loot table's function or condition that names a `type` in a namespace outside `minecraft:` reaches `native_loot_function(kind, data, ctx, stack)` or
`native_loot_condition(kind, data, ctx, holds)` when the table rolls. The function changes the stack; the
condition says whether it holds. The context brings the entity, the damage, the player who killed it, the
tool, the block, the luck, the biome and a seed from the roll's random. An unclaimed function leaves the
stack, an unclaimed condition fails, as unknown kinds do; a bare type name is minecraft's own.

The tables the items engine rolls (block drops, entity drops, `loot_table`, `interact_loot` and the
built-in tables) answer both hooks, with the tool and the block in the context. The function hook there
gets the stack as the main engine's (`LootStack`): its item, count, damage, enchantments (a book's stored
ones), potion, ominous bottle amplifier, stew effects and the components the engine keeps as saved, as
their JSON; what it changes comes back to the engine's stack, and a component it adds arrives with the
drop. A function that asks for a map's structure search (`map`/`exploration`) changes nothing there: the
items engine has no level to search. In the items engine's context the `entity`, `damage`, `player` and
`biome` fields are `None` and `luck` is 0; in the main loot engine's context the `tool` and `block` fields
are `None`.

Both engines' contexts carry `place`, where the roll happens: `origin` (a broken block's centre, a dead mob's
position), `block_entity` (the broken block's entity as it was before the break, saved with its id, position and
`Items`) and `level` (the dimension, the game time, the time of day, the rain and the thunder, for conditions like
vanilla's `time_check` and `weather_check`). The game sets it for a block a player or an explosion breaks and for a
mob's death loot; on other rolls `place` is `None`.

### Recipe book category declarations (first slice of #5501)

The `recipe_book_categories` hook declares recipe book categories. Trait mods import
`RecipeBookCategory` from `mod_api` and append declarations in
`Hooks::recipe_book_categories`:

```rust,ignore
event.categories.push(RecipeBookCategory {
    id: "mymod:crushing".into(),
    recipe_type: "mymod:crusher".into(),
    icon: "mymod:crusher".into(),
});
```

Mixin bundles use `native_recipe_book_categories(categories)` at the same hook
point. `host.recipe_book_category("mymod:crusher")` (the mixin helper is
`native_recipe_book_category`) returns the validated declaration, including its
icon, or `None`. It does not grant recipes. Declarations are requested lazily at
first lookup, cached for that recipe table, and invalidated on a new session,
server recipe-content replacement or server `/reload`. No lookup means no hook
call or per-frame work. A client validates against its own custom recipes unless
`native_datapack_sync` has supplied the server's type.

Every field must be a fully namespaced resource id. Category ids and recipe types
must be outside `minecraft:`; the type must occur in the current custom recipe
table. The icon must name a registered, non-air item. Invalid ids, unknown types,
missing/unknown icons, and duplicate category ids or recipe types are logged and
skipped, never panicked on. The first valid declaration wins each id and type;
an invalid declaration does not reserve either. This slice supports one category
per type, not per-recipe category selectors.

**Not yet book UI support:** custom recipes still have no book displays or
unlock/display-id synchronization. This slice deliberately leaves vanilla tabs,
search, layouts and unlock behavior unchanged. Adding custom tabs, displaying and
searching recipes there, and unlocking them like vanilla remain follow-up work
for #5501; a declaration alone will not make a recipe visible.

### A mod's own ingredient kinds

A recipe's ingredient that names a type in a namespace outside `minecraft:` reaches
`native_ingredient(context, kind, data, items)` at load time. Fill `items` with the item ids the
ingredient matches (item ids only: components are not checked; a match-time hook follows
separately). Duplicates are removed keeping first-seen order. The hook gets the `DataLoadContext` of the load that parses the recipes and hears each occurrence of the kind once per load, nested ones and ones under any slot a recipe reads an ingredient from included, before the load's tables parse the recipes: the client's own recipe load has `origin()` `ClientPreload`, the server's `ServerDataLoad`. A server loads its recipes once however many tables and copies read them (its recipe tables, the stations and the copy synchronised to guests, on a dedicated server too), and a `/reload` asks again once; a client that only joins a remote server is never told `ServerDataLoad`. The answer must be the same every time. An unclaimed kind leaves the list empty and the recipe fails, as an unknown kind does.

### Structure types

A structure type is a mod's code where a data pack names one: a structure JSON whose `"type"` is
`"mymod:tower"` instead of a vanilla type generates by the mod's `StructureType` (vanilla's
`StructureType`, whose `find_generation_point` returns the start and whose pieces are
`StructurePiece`s, loaded by the `StructurePieceType` that `load_piece` is — NeoForge registers
these in the registries `STRUCTURE_TYPE` and `STRUCTURE_PIECE`). Register it once, at startup, in
`Hooks::register`; the id is in the mod's namespace, and a duplicate id, an id in another mod's
namespace or an empty name stops the game at startup with a message naming the mod:

```rust,ignore
#[mod_api::horizon_mod]
impl Hooks for MyMod {
    fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
        r.register_structure_type("tower", Tower);
    }
}
```

There is no `Host` in world generation: starts build on worker threads and pieces place per chunk,
so the type and its pieces see only two small contexts, `Send + Sync` and deterministic. The same
seed gives the same pieces, and a type keeps no mutable state, because two workers can build
neighbouring chunks at once.

`StructureType::find_generation_point(&self, context: &mut dyn GenerationContext)` answers
`FoundStructure { position, pieces }` (each piece an `Arc<dyn StructurePiece>`) or `None` for "no start in this chunk". The
`GenerationContext` is vanilla's `GenerationContext` over one chunk: the seed, the chunk, the
random seeded as vanilla seeds it for the chunk (`setLargeFeatureSeed`), the height and
base-column queries, the noise biomes and the valid-biome test, and the structure's own JSON
config (`context.config()`, shared). Both contexts are traits and the random is a
`StructureRandom` (`next_int_bound`, `next_long`, `next_float`, `next_double`, `next_boolean`),
so a type built to `mod.wasm` runs the same code. Everything around the type stays the game's: the structure set's
spacing, the structure's `biomes` and `terrain_adaptation` are placed by the engine, exactly as
for a fortress.

Each piece is a `StructurePiece` (`Debug + Send + Sync`): `bounding_box`, `post_process` and
`add_additional_save_data` are required. After all pieces of the start were placed, the type's own
`after_place` (vanilla's `Structure.afterPlace`) runs once per chunk with every piece's current
box; it defaults to doing nothing. The
`PieceContext` of `post_process` and `after_place` is the chunk being placed: `set_block` writes a
block and drops a position outside the writable box silently, as vanilla's
`StructurePiece.placeBlock` skips it; `block` and `height` read, `state`/`state_with` resolve a
block by name and properties, `random` is the piece's `RandomSource` as a `StructureRandom`, `name` gives the block's name, `solid_render` asks whether
a state draws as a solid block, and `entity` queues an entity from its saved NBT — its `id`, `Pos`
and fields — which the generating chunk holds and the game finishes as a structure mob's
(`Mob.finalizeSpawn` with `STRUCTURE` when the NBT keeps the `"horizon:finalize"` byte a structure
template's entities carry). `block_entity` places a block entity from its NBT.

Saving goes through the engine's `StructurePiece.createTag` — the type id, `BB`, `O`, `GD` —
around the piece's `add_additional_save_data`. When the chunk loads back, the type's `load_piece`
(the `StructurePieceType` loader) rebuilds the piece exactly as it was saved; there is no
regeneration from the seed. `load_piece` is required: a type whose pieces cannot load cannot save
a start that comes back.

**Open question — the saved piece's id is the structure type's, not a piece type's.** Every piece
of a type saves with that type's id, the `STRUCTURE_TYPE` id; Horizon has no separate
`STRUCTURE_PIECE` registry, so there is no per-piece-kind id in the tag. A mod whose type has
several kinds of pieces tells them apart in `load_piece` by its own saved data (a tag field its
`add_additional_save_data` writes). The `O` (orientation) and `GD` (genDepth) fields are written
as `-1` and `0` — the adapter has no orientation or depth to record — so a piece must not read
them back as meaningful.

The pieces ride the one piece mechanism the game has: they are legacy pieces behind an adapter, so
the start cache, the cross-chunk placement, the beardifier and `/locate structure` treat a tower
piece like a fortress piece's. `StructureRegistry` reads the registered types before it falls to
`Unsupported`, so a data pack structure whose `"type"` no installed mod registers places nothing,
exactly as before.
### Fluid ingredients

A mod's own recipe type that drinks from a tank reads its fluid inputs as a `FluidIngredient`, the
fluid analogue of an item ingredient: `FluidIngredient::from_json(json, context)` parses a fluid
(`"minecraft:water"`), a `#tag` (the fluid tags the load's `DataLoadContext` holds), a list of
them, or NeoForge's `compound`, `intersection` and `difference` shapes, with the same walk the item
ingredient parses its own JSON with — call it where a data load hook hands you the context, as the
`ingredient` hook does while the game resolves recipes. `test(&FluidStack)` asks whether a tank's
fluid answers, and `fluids()` lists the fluids it accepts, in tag order, for a recipe book. A kind
outside the standard shapes fails the parse: fluids have no custom ingredient hook, and components
are not checked — any stack of a named fluid answers.

`SizedFluidIngredient` adds the amount, as NeoForge's `SizedFluidIngredient`:
`{"ingredient": <fluid ingredient>, "amount": 500}` in millibuckets, `"amount"` defaulting to a
bucket (1000) and always positive. Its `test(&FluidStack)` asks that the stack matches the
ingredient *and* carries at least that much.

### Data files

A mod has one top-level `data/` tree and one `assets/` tree, with the same namespace and
content checks as a content mod. They are normal pack resources, not a second native-only data folder.
[Server data reload listeners](../reference/hook-notes.md#server-data-reload-listeners) register through
`Hooks::add_server_reload_listeners`. Their `ServerReloadContext` lists namespaced IDs and reads the
winning bytes from selected datapacks and the mod's own `data/`. Parse once in prepare and publish in
apply, not on the tick. The original pack trees remain separate from the launcher's code-only
horizon-mixins input.

Client-side asset reads use [client asset reload listeners](../reference/hook-notes.md#client-asset-reload-listeners); server
listeners cannot read `assets/`. Packaged assets also participate in the game's normal resource
packs.

Code-only bytes belong inside the crate: use Cargo's ordinary `build.rs` to generate a table in
`OUT_DIR`, then compile it with `include_bytes!(concat!(env!("OUT_DIR"), "/table.bin"))`.
They are not runtime pack resources and do not participate in resource reloads.

**What a native bundle holds.** The one manifest, optional icon, root `Cargo.toml` and `build.rs`,
`.rs` and `.toml` files under `src/`, `assets/` and `data/`, and root text files (`README*`, `LICENSE*`,
`LICENCE*`, `COPYING*`, `NOTICE*`). Source and resource size limits are checked separately;
anything else refuses the archive ([the manifest](../reference/manifest.md)).

**One `.hmod` for content and code.** Without root `Cargo.toml`, nothing compiles. With it and `[native]`,
the launcher builds the code and the game loads the same package's content through `horizon_mods::Content`.
Both kinds use `horizon-mod.toml` for their identity. Executable `mod.wasm` files are unsupported.

## What the launcher does

**Developing a mod?** Run `horizon-launcher settings native-profile developer` once, or pick Developer in the Mods
page's "Build mode" card. Developer builds the game crate unoptimized: a mod edit rebuilds about 6× faster than with
Balanced, while game logic runs up to about twice as slow. Use it while developing, not for playing; Optimized stays
the default for players (step 5 below).

In the window: install a native mod from the Mods page, into the instance picked on the left ("Installs into
{instance}", shown even with one instance). The first one shows a trust notice once (native mods aren't sandboxed; the
first build downloads Rust and can take 20 to 30 minutes; later builds are faster); `mods install` prints it too. Each
installed native mod's row says "Installed in {instance}" and has **Open folder** (`<instance>/horizon-native-mods/`).
The installed list has a **Native build of {instance}** row for the instance picked on the left, never another's:

- A badge: **Not built** (the version it plays isn't a `native-*` build), **Needs rebuild** (the `mods` in its
  `native-mods.json` differ from those turned on, by id and version) or **Up to date**, with **Build** for the first
  two;
- For a native build, "Plays native-…, built on Horizon {version}" (the `base` of its `native-mods.json`);

The instance list on the left shows such an instance as "Native build · Nightly 10-28" (its base). The launcher names
the mods, warns in red, and builds only from "I trust these mods: build"; the row shows each step and the error if
one fails, on the instance being built only; one build runs at a time. From the command line:
`horizon-launcher native-build [FOLDER] [--instance NAME] [--update] --trust-native-code`. FOLDER is a bundle, an
unpacked `.hmod`, or a folder of them; without it, the native mods turned on in the instance. It is not an MCP tool:
trusting native code is the player's decision, never an agent's.

1. **Rust.** The launcher's own toolchain ([`rust_toolchain`](../../../../launcher/src/rust_toolchain.rs)): Rust 1.99.1's
   official `rustc`, `cargo` and `rust-std` (and `rust-mingw` on Windows), each pinned by SHA-256, downloaded once
   from static.rust-lang.org into `<launcher data>/rust/`, with its own Cargo home `<launcher data>/cargo/`. No
   rustup, nothing on the system; the player's own Rust is never used. It ships for macOS on Apple Silicon, Linux
   x86-64 and Windows x64; an Intel Mac has none yet, so native builds there stop with "the launcher has none for
   macos x86_64". Windows uses the `windows-gnu` host, which links without Visual Studio. `rust-mingw` only links, so on Windows the toolchain also brings MinGW-w64
   (`rust_toolchain::MINGW`: WinLibs' GCC 16.2.0 on MinGW-w64 14.0.0, MSVCRT like `rust-mingw`, one zip pinned by
   SHA-256, its `mingw64/` without the documentation) on the build's `PATH`: `ring` and `hidapi` compile C,
   and `windows-sys`' `raw-dylib` imports need `dlltool` and the assembler it runs. The linker is the system's on
   macOS (the Command Line Tools: the launcher asks for `xcode-select --install`) and Linux (`cc`). On Linux the
   game also builds against the system's ALSA (`alsa-sys`) and libudev (`libudev-sys`, `hidapi`), found with
   `pkg-config`, whose development files desktop distributions don't install. Before downloading anything the
   launcher checks for `cc`, the C library's headers (`cc` compiles an include of `stdint.h`), `pkg-config`,
   `alsa` and `libudev` and names what is missing, with the install command for the distribution in
   `/etc/os-release` (Debian, Ubuntu and Mint:
   `sudo apt install build-essential pkg-config libasound2-dev libudev-dev`). SteamOS has GCC without the C
   library's headers, and a read-only system: native builds don't work there yet.
   On Windows, Smart App Control can block newly compiled build scripts or toolchain programs with
   `os error 4551`. The launcher adds advice when it sees that error; it does not check the setting
   before a build. To build after a block, Windows Security > App & browser control > Smart App Control
   settings offers an off switch. On some Windows versions, turning it back on requires reinstalling
   Windows. Content mods need no build and work with Smart App Control on.
   `horizon-launcher rust status|install|remove`.
2. **horizon-mixins**, at the exact commit the mods name (`[native] mixins` in `horizon-mod.toml`),
   built once with that cargo (`cargo install --locked --git … --rev …`) into `native/tools/<commit>/`.
3. **The source** of the pinned commit: GitHub's archive of it, once per commit (`native/source/<commit>/`, about
   63 MB unpacked), with the commit in `.horizon-revision`. Never the player's own checkout. Links in the
   archive are left out: the tool refuses them and nothing needs them to build.
4. **Stage.** The projects are copied first, so the build and its key use that copy whatever happens to the
   folder meanwhile. For raw injections the launcher writes `horizon-mixin.toml` and runs `horizon-mixins apply`.
   It stages the mod crates and generated executable, copying only changed files into `native/stage/`,
   so Cargo, which goes by file times, recompiles only the crates the mods touch.
   Each file written gets the current time, never the archive's (the commit's) or the copy's original: on
   Windows and macOS a copy keeps it, and a file older than the last build reads as already compiled, so
   switching Nightlies linked the previous one's crates (horizon-rust/horizon#10421). A stage from before
   that fix is written again once (`native/stage.stamped` marks it). Registry dependencies
   are checked against Horizon's `Cargo.lock` ([The mod's crate](#the-mods-crate)).
5. **Compile**: `cargo build --locked --profile <fast|release> [--config ...] [--features horizon-client/mods] -p
   horizon-native-game`, into `native/target/`, with one of three build modes:

   | Mode | Cargo profile | Use |
   |---|---|---|
   | `optimized` (the default) | `release`, the release's own (game-build.yml: thin LTO, one codegen unit) | The mode for playing: mods that need nothing but the mod API compile against the prebuilt game, built with this profile, in seconds; any other set builds the whole game from source with it |
   | `balanced` | `fast`, the workspace's own: `codegen-units = 16`, incremental, and `lto = false`, Cargo's default, which is thin-local LTO across one crate's codegen units, none across crates | Always builds the game on this computer: a rebuild recompiles and links only what changed, and the game plays a little slower than Optimized |
   | `developer` | `fast` with `horizon-client` at opt-level 0 (`--config profile.fast.package.horizon-client.opt-level=0`) | Developing a mod: the quickest rebuild after an edit, but the game runs about twice as slow, so iterate with it rather than play |

   Only Optimized uses the prebuilt game. The Mods page shows how each native mod reaches the game: a bolt for
   the prebuilt game, a hammer for a full game build (its menu names the reason), a check when the game already has
   the mod. A build under way shows its stage and can be cancelled.

   With Developer, an edit to a function the machine sample's mixin injects into `game/native_hooks/` compiled
   about 6× faster than with Balanced, and a tick took up to about twice as long. `dev` lowers only `horizon-client`, so an edit in another workspace
   crate (`horizon-client-lib`, `-render`, `-world`) still compiles at opt-level 3, as with Balanced. The
   launcher sets it with `--config` (`native_mods::Profile`), so the workspace's own `[profile.fast]` builds stay
   as they are. The Mods page's "Build mode" card, `horizon-launcher settings native-profile optimized|balanced|developer`
   or `native-build --profile ...` pick one. `native-mods.json` records the profile.

   Each profile is in the build's key, so their builds never mix and may all stay. `balanced` and `developer` share
   `native/target/fast/`: every crate but `horizon-client` is built with the same settings, so switching between
   them recompiles only `horizon-client`, and Cargo keeps both builds of it, so switching back finds the other
   still there. `optimized` has its own folder (`native/target/release/`). The first build in a folder builds every
   crate once (1½ to 3½ minutes measured through the launcher, depending on the machine); later builds reuse them.
   Cargo downloads the locked crates from crates.io (Horizon has no git
   dependencies). The compiler and the horizon-mixins build run at the lowest priority (`rust_toolchain::low_priority`:
   nice 19 and the idle I/O class on Linux, nice 19 on macOS, below-normal priority on Windows), so the desktop and a
   game in play stay smooth; on an otherwise idle computer they still use every core. The log is `native/build.log`;
   a failure shows its last lines (a horizon-mixins diagnostic in one line).
6. **Install**: a new version, `native-<commit>-<key>`, a copy of the starting version's folder (resources
   included, cloned on APFS, Btrfs and ReFS) with the built game in place of its own, and a `native-mods.json`
   saying which release or Nightly it started from (`base`) and which commit, mods, horizon-mixins and Rust made
   it. Canonical mod projects are copied into `horizon-native-mods/<id>/` beside the executable.
   The game lists its embedded inventory in a crash report's details (`Native Mods: id version, …`;
   they stay in the local report). The instance the mods are installed in plays it, the default **Horizon** instance included: the build
   pins its version, as picking a version does. Updates of the launcher's version leave a pinned instance where
   it is, and their clean-up never removes a native build. The **Native mods**
   instance keeps its worlds and its build.

### Build log

The full log of a build is `native/build.log`, beside the build's own folders under the launcher's data
folder (`native_mods::log_path`). Each new attempt replaces it. Compiler output and the final result
stay in the log, including failures before compilation or while selecting or exporting the built game.

The launcher has a **Build log** action at the top right. It is there whenever a log exists, also right
after the launcher starts, and it always opens the latest build's log: the next build replaces it. The
panel shows a bounded preview of the end of the log and says when it truncated; **Copy log** copies the
whole file, **Open full log** opens it outside the launcher, **Show in folder** reveals it, and
**Refresh** reads it again (also after a build finishes). The panel is a snapshot read on open and
refresh, not a live stream: a running build's later output appears only on the next refresh. Nothing is
uploaded; the log stays on the player's computer.

## Rebuilds stay on the instance's Horizon

A build starts from the version the instance plays (`native_mods::base_for`). When that is a native build, the
next one compiles the same commit (its `native-mods.json`'s `horizon`) from the same game files, unless a mod pins
another, so a rebuild after a mod changes or is switched off compiles only what changed. A newer Horizon is taken
only when the player asks: `native-build --update` starts from the newest installed release or Nightly (a full
rebuild); `native_mods::update_for` says which one, if any, the window can offer.

The compiler runs as the release's build does, whatever the player's own Rust setup: the launcher's toolchain
drops the variables that change compiler flags or the target (`RUSTFLAGS`, `CARGO_ENCODED_RUSTFLAGS`,
`RUSTDOCFLAGS`, `CARGO_INCREMENTAL`, `CARGO_PROFILE_*`, `CARGO_BUILD_*` but `CARGO_BUILD_JOBS`, `CARGO_TARGET_*`,
rustup's and the wrappers'), and starts Cargo from the disk's root with `--manifest-path`. Cargo reads `.cargo/config.toml` from
the folder it starts in and every one above it, whatever `CARGO_HOME` says, so from the launcher's data folder it
would read the player's `~/.cargo/config.toml`, whose `[profile]`, `[build]` and `[target]` settings beat the
workspace and the environment; no switch turns that search off. The source's own `.cargo/config.toml`, if it
has one, is passed with `--config`.

Every path the build's tools see — the toolchain, the Cargo home, `native/` and the tools' temp dir (TMPDIR,
TMP and TEMP, where gcc, the assembler, `ld` and `cargo install` put their scratch files) — comes from
`backend::build_dir`: the launcher's data folder, resolved (never through a link, which horizon-mixins
refuses). On Windows, when that path is not spelled in ASCII — a Cyrillic user name in `%APPDATA%`, which
MinGW-w64's tools can't open — it is a real folder under `ProgramData` named for it (`horizon-launcher-<id>`),
and only when that can't be created the data folder's short (8.3) name. A short name alone is not enough
(#6009): rustc canonicalizes its own path to find its sysroot, which spells the long name again for the
linker. A data folder that is itself a junction to a drive with an ASCII path resolves to that and needs
neither.

## Install from GitHub

An author can ship a native mod without a registry pull request per version: the player installs it from a
GitHub link, on the Mods page ("Add from GitHub") or with `horizon-launcher mods add-github <url>
[--mod ID | --all] [--instance NAME] --trust-native-code`. The link is `https://github.com/<owner>/<repo>`
(its latest release), `.../releases/tag/<tag>`, `.../tree/<ref>` or `.../commit/<sha>`.

- **Resolved to something exact.** GitHub's public API (no sign-in; a rate limit is said plainly, and
  `HORIZON_GITHUB_TOKEN` raises it) turns the link into a release's `.hmod` file or a **commit SHA**, never a
  branch that moves. A release with a `.hmod` asset installs that file (its SHA-256 is compared with the one GitHub
  lists). With no release or no such asset, the launcher fetches
  `codeload.github.com/<owner>/<repo>/tar.gz/<sha>` and looks in it for mods by the rules of
  `native-build <folder>`: a flat project with `horizon-mod.toml`, `[native]`, `Cargo.toml` and `src/`.
  Several mods: they are listed, and the player picks one or installs all. Each source folder pins the Horizon commit
  in `[native] horizon`; the launcher packs the checked project into a `.hmod`.
  Release packages must contain native code; content-only packages are rejected by this native installer.
- **Checked like the registry's files.** The `.hmod` is read as `horizon_mods::index::native::Bundle::read` reads a
  registry file (flat source and content, size limits, nothing run) before it is installed.
- **Said first.** Before anything is downloaded: "not reviewed by the Horizon registry; native mods run with full
  access to this computer; read the source", with a link to the exact commit. The command needs
  `--trust-native-code`, as `native-build` does. It is not an MCP tool: trusting native code is the player's
  decision, never an agent's.
- **Origin kept.** `horizon-native-mods/<id>.origin.json` beside the `.hmod`: owner/repo, release tag (for a
  release), commit, asset name and the SHA-256 of the installed file. `mods list` shows "from GitHub: owner/repo @
  tag" (or the commit's first 7 digits).
- **Updates.** `horizon-launcher mods update-github [ID] [--check]` compares a mod from a release with the
  repository's latest release, and a mod from a commit with the head of the default branch; a newer one is
  installed with the same warning and `--trust-native-code`. `mods remove <id>` removes it. A mod already
  installed from the registry (or by hand) is never replaced.

Installing turns Native mods on, as any native mod does; then build as usual.

## Cache and disk

The key hashes the commit, the horizon-mixins commit, Rust's version, the profile and features, and every file of
every bundle. The same set on the same commit is never built twice. A change gives a new build, and
`native/target/` stays, so a rebuild compiles only `horizon-client` and what the mods touched.

**Incremental rebuilds.** With Balanced and Developer, Cargo recompiles only the changed parts of `horizon-client`, not the whole
crate with LTO. Patched source is cached separately by its mixin inputs and pins (the three most
recent snapshots are kept), so editing only a trait crate skips the mixins pass. This holds because
the stage (`native/stage/`) is one folder for every build and the launcher writes
only the files whose content changed (Cargo goes by file times), switching mods on the same commit reuses the same
`native/target/`, and the launcher removes `CARGO_INCREMENTAL` and `CARGO_BUILD_INCREMENTAL` from the build's
environment. Optimized has no incremental compilation (the release profile's own).

**Build status.** Before it starts, the build says which case it is, with the profile: "Up to date (no build needed)",
"Incremental rebuild: mods X, Y" (or "Horizon commit a to b", "horizon-mixins"; compared with the last build in that
profile's folder, kept in `native/built.json`) or "First build: the whole game (several minutes)". While compiling it shows the
crate, crates done of the total (Cargo's own progress), and the time taken; afterwards "Built in 42 s (performance,
incremental)". The window shows it in the Native mods row, `native-build` prints each step with its time on standard
error; `native/build.log` keeps Cargo's output without the progress bar.

Measured 2026-10-30 on Linux (x86_64 desktop), Zoom 2.0.0 and FPS Counter 2.0.0 on Nightly `6f8c7a0`, the `fast`
profile, the launcher at nice 19 with `CARGO_BUILD_JOBS=10` (an empty launcher home: Rust, the crates and the
source downloaded first):

| What | Time |
|---|---|
| First build: Rust, horizon-mixins, the whole game (384 crates) | 4 min 29 s in all; 3 min 6 s of compiling the game; `native/target/` about 2 GB |
| One mod switched off, rebuild | 56 s in all, 51 s of compiling (only `horizon-client`) |
| That mod switched on again | under 1 s: the kept build plays again |
| Nothing changed, rebuild | 1 s, nothing compiled |

Measured before builds used `fast`, with the `release` profile, on 2026-10-28 on the Mac mini (M4 Pro, 14 cores) with Veinminer 0.3.0 on Nightly `83f15fa`,
`CARGO_BUILD_JOBS=8` while other agents' builds ran (upper bounds):

| What | Size, time |
|---|---|
| Rust 1.99.1 (`<launcher data>/rust/`) | 453 MB; 58 s to download and unpack |
| horizon-mixins (`native/tools/`) | 7 MB; built once in 19 s |
| Crates in the launcher's Cargo home (`<launcher data>/cargo/`) | 289 MB |
| Source of one commit, and the stage | 64 MB each |
| First build: the whole game | 9 min 10 s of compiling, 9 min 41 s in all; `native/target/` 1.5 GB |
| Rebuild after a helper's code changed | 2 min 46 s of compiling (only `horizon-client`), 2 min 53 s in all |
| A change horizon-mixins ignores (a comment) | nothing compiled; 31 s, mostly the version folder's copy |

Measured 2026-10-29 on Windows 11 (Ryzen 7 9800X3D, 16 threads), the same mod and Nightly, an empty `native/target/`
(horizon-mixins, the source and the crates were already downloaded):

| What | Size, time |
|---|---|
| Rust 1.99.1 and MinGW-w64 (`<launcher data>/rust/`) | 1.7 GB, MinGW-w64 875 MB of it |
| Crates in the launcher's Cargo home | 657 MB |
| First build: the whole game | 6 min 57 s of compiling, 8 min 3 s in all; `native/target/` 2.0 GB |

Measured 2026-10-28 on Linux (an x86_64 server with no Rust of its own), Zoom on Horizon `1149c92`, 3 jobs while CI
ran: `rust install` in 11 s (592 MB), the first build in 12 min 9 s. The built game ran Zoom's proof (under `xvfb-run`:
the game needs a display even with `--offscreen`).

The native builds an instance plays are kept, and the three newest no instance plays (`native_mods::prune`, by when
each was built or last used): switching a mod off and on again plays the kept build again without compiling.
`native/target/` stays for the next one.

## Trust

- **A notice, then a warning.** The first native install shows a trust notice once. The command needs
  `--trust-native-code`; the window
  shows a warning in red before every build: the mods run with full access to the computer, and their ids and
  versions. The Mods page says who made each one, and "Not made or endorsed by the Horizon project" for any mod not
  Horizon's own.
- **The registry** (horizon-rust/horizon-mods) lists them as their own kind, `native-mod`, only after a maintainer
  has read the source and the repository they come from. Its entry names the GitHub repository (`repo`); each
  release of it with a `.hmod` asset is a version once the bundle passes the registry's checks, with no further
  review (nothing is built or run by the registry; the player's launcher builds it behind its warning).
  The `.hmod` contains the flat Rust project and may contain `assets/`, `data/` and an optional `icon.png`.
  The launcher checks the bundle without running anything. Native code is trusted.
  The file is pinned by SHA-256 like every registry file; its identity and exact build pins come from
  `horizon-mod.toml`. `revoked.toml` takes a bad release down.
  A project's kind never changes.
- **Isolation of data.** Native mods run only in the instance they are installed in, with its worlds, options and
  logs (the default Horizon instance's, when they are installed there); other instances play their own versions.
- What this does not do: sandbox the compiled game. horizon-mixins' checks stop mistakes, not a malicious helper
  ([security notes](https://github.com/horizon-rust/horizon-mixins/blob/main/docs/security.md)).

## Multiplayer

- **Single player and LAN host:** the mods run in the host's game and its integrated server, so a server-side
  hook applies to every player of the world.
- **Joining a server:** client-side hooks run locally. Configuration requires matching both-side mod ids
  and versions; a refusal names the missing, extra or different-version mods ([configuration policy](../start/installing.md)).
- **Dedicated server:** native mods run in a standalone engine export built with `--server-output`, not in an
  instance build (`--server-output` conflicts with `--instance` and needs an explicit mod `FOLDER`). See
  [Standalone server export](#standalone-server-export).

### Standalone server export

`horizon-launcher native-build FOLDER --server-output DIR --version TAG --trust-native-code [--profile PROFILE] [--allow-jar-downloads]`

Builds a standalone dedicated-server engine into `DIR`, which must not exist yet. `FOLDER` (a mod folder or a
folder of them) is required; no instance is read, selected, edited or played, and no launcher config is written.
It reuses the pinned toolchain, source and horizon-mixins and compiles `--no-default-features` (no audio or
controllers). `--version` pins the Horizon release/Nightly the
export reuses (explicit for reproducibility). The export holds the built `horizon-engine` executable, prepared resources
the canonical `horizon-native-mods/<id>/` projects and the provenance `native-mods.json`, and prints its absolute path. It runs on the same OS/architecture as the
host only (no cross compilation), needs a prepared installed launcher base release, and survives cache pruning
because it is copied outside the managed folders. On Windows use the printed `horizon-engine.exe` path.

A mod's crate depends on `horizon-mod-api` and never on the game: a `horizon-client` dependency,
under any name or target, is refused naming the mod and the key, before toolchain work. The mod API
has no sound or controller features, so server and client builds stage the same crate.

Declared jars use the same exact-hash preparation as ordinary launches. A server export refuses
missing jars by default and never asks interactively; `--allow-jar-downloads` permits the shared
preparer to download the exact files into the local cache. Cached jar bytes are **never exported**.
When moving the runtime to another machine or game home, prepare its jar cache separately; errors
name the required files and cache paths. No Java code from a jar runs.

Point a server at it:

    horizon-server --dir SERVER config set engine.path=/absolute/export/horizon-engine
    horizon-server --dir SERVER restart

Updates do not rebuild a pinned `engine.path`: build a new export folder, set `engine.path` to it, restart, and keep
the old folder for manual rollback. The supervisor does not auto-update or rebuild a pinned engine path
(remaining work in horizon-rust/horizon#6304). To inspect the running server, its authenticated
[`GET /api/v1/mods`](../../../api/server.md) queries the engine's canonical `LoadedMods` inventory:
`mods` lists loaded native, wasm and content package ids and versions in inventory order.
`provenance` separately reports the configured program's build record, which may differ and is never
used as an inventory fallback. A stopped or unreachable engine returns HTTP 409, not an empty list.
Configuration mod matching is implemented as described above.
Native mods run unsandboxed: only build code you have read and trust.
`--trust-native-code` records the operator's decision; an agent must not supply that consent.

### Developer acceptance workflow

Use a disposable server folder and fresh homes when developing:

```sh
SCRATCH=$(mktemp -d)
export HORIZON_HOME="$SCRATCH/game" HORIZON_LAUNCHER_HOME="$SCRATCH/launcher"
export XDG_DATA_HOME="$SCRATCH/data" XDG_CONFIG_HOME="$SCRATCH/config"
export HORIZON_NO_AUDIO=1 HORIZON_LAUNCHER_NO_SELF_UPDATE=1
export HORIZON_LAUNCHER_TRASH="$SCRATCH/trash"
export HORIZON_LAUNCHER_CLIPBOARD="$SCRATCH/clipboard"
export HORIZON_LAUNCHER_DESKTOP="$SCRATCH/desktop"
```

Install and prepare the base release in that scratch launcher home first, then test your own licensed bundle:

1. `horizon-launcher native-build bundle/ --server-output $ENGINE --version TAG --trust-native-code` (scratch homes,
   `HORIZON_NO_AUDIO=1`, `HORIZON_LAUNCHER_NO_SELF_UPDATE=1`).
2. `horizon-server --dir SERVER config set engine.path=$ENGINE/horizon-engine`; `horizon-server --dir SERVER run`.
3. Join with the same mod's client half where the hook needs one; test lifecycle, tick, command and net hooks.
4. Headless compatibility: the mod's server code must not require an audio device; run the server audio-free (`HORIZON_NO_AUDIO=1`).
