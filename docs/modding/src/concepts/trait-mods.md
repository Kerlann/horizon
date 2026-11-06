# Trait mods, Parts and attachments

## Trait mods (draft)

`cargo horizon-mod new <path> --template native` pins an exact Horizon commit (the newest prebuilt game's,
or `--horizon <commit>`'s; `cargo horizon-mod update` repins) and writes a flat project: `horizon-mod.toml` with `[native]`, `Cargo.toml`, `src/lib.rs`, and empty `assets/`
and `data/` directories. The template greets each player who joins. Build it with
`horizon-launcher native-build <path> --trust-native-code`; `cargo horizon-mod build` packages the same
project into a `.hmod` without compiling it ([the manifest](../reference/manifest.md)).

The root Rust package depends on `horizon-mod-api` (lib `mod_api`), never on the game. Its root `src/lib.rs` marks one
`impl Hooks` with the attribute:

```rust,ignore
use mod_api::{self, Hooks};

#[derive(Default)]
struct Mod;

#[mod_api::horizon_mod]
impl Hooks for Mod {
    // Implement the hooks this mod needs.
}
```

The attribute generates the `hooks()` factory and derives optional hook subscriptions from this impl.
The mod type implements `Default`; do not write a second factory. Keep the marked impl at the crate root.
Rustc's "cannot find function `hooks`" at the launcher's generated call means the marked implementation
is not at the crate root. The launcher links this package into a generated game executable and calls
the generated factory in dependency order (a mod after the mods it depends on, else in bundle order).
A mod with no raw injections skips horizon-mixins; typed hooks need no injection declarations.
Dependencies follow the pinned Horizon `Cargo.lock` rules in [The mod's crate](../how-to/native-mods.md#the-mods-crate).

### Dependencies between trait mods

A mod can publish functions and statics from its root crate for another mod to call.
Declare dependencies in `horizon-mod.toml`, not `Cargo.toml`:

```toml
[dependencies]
base_mod = "^1"
extra_mod = { version = "^2", optional = true }
```

Requirements use Cargo semver syntax and are checked against each installed mod's `horizon-mod.toml`
`version`, not its Cargo package version. Content-only mods use these declarations to validate their
requirements and load their layers after dependencies. Code dependencies additionally require API packages
for the dependent and every installed dependency.
Missing required dependencies, incompatible versions and cycles stop the build with the mods named.
An absent optional dependency is permitted; an installed optional dependency must match its requirement.

The launcher adds a path dependency using the mod ID as the Cargo alias and the target's actual Cargo
package name. Optional dependencies also get a Cargo feature named exactly as their mod ID, enabled
only when that mod is installed. Guard calls with `if_installed!`, which builds the block only with
that feature and runs it only when the mod is installed, so the same source works when built to
`mod.wasm`, where the library is linked at load and an absent one's functions trap:

```rust,ignore
mod_api::if_installed!(host, extra_mod, {
    extra_mod::register_extra(host);
});
```

Do not define these dependencies or features yourself in Cargo.toml.
Public statics must use appropriate synchronization for shared and reentrant hooks; private state in
another mod's hook object is not exposed.

Trait dispatch follows dependency order, with ties kept in bundle order. Mixin injection order remains
horizon-mixins' priority, mod ID, then injection ID; dependency order does not change injection order.

The generated executable uses the installed game's `crates/horizon-client/src/native_main.rs.template`.
The launcher supplies only ordered literal mod IDs and hook factory paths. The template owns entry
construction, the allocator and the canonical game identity. Source without this template is refused
before staging trait mods: update the game first.

A panic in a trait mod's hook adds `in native mod <id>, hook <hook>` to the panic's message, in the crash report
and in the server thread's log line, with the `horizon-mod.toml` id. Each entry passed to
`entry::main_with` must bind its id and hooks together: `ModEntry::new("example", example::hooks())`.
The ordered list is `Mods = Arc<[ModEntry]>`; there is no separate naming step or unnamed entry.
Before installation, `mod_api::mods()` still returns an empty list. Parts retain their registering mod's
id for startup errors and hook panics. The game stops. A point listener's panic names its mod and
method; render hooks are not named yet.

A code mod may listen at the game's injection points as a rare escape hatch, through `impl mod_points::Points`
(the `horizon-mod-points` crate) on the same type as its `Hooks`; the form
and its limits are in [Injections](../injections.md).

A linked trait mod implements `mod_api::Hooks`. Helper injections run first.
An injection that returns early skips trait mods. A claimed cancellation also skips trait mods.
Otherwise, trait mods run in their supplied order. The first `Cancel` stops dispatch.
For replacement hooks, the first `Some` wins. Collection hooks call every mod. Receiverless hooks have no `Host`.

Each hook method takes its host, where it has one, and one event:

```rust
fn living_incoming_damage(
    &self,
    _level: &mut dyn LevelRandomHost,
    event: &mut LivingIncomingDamageEvent<'_>,
) -> Flow {
    if event.kind == "minecraft:pig" {
        let halved = event.container.new_damage() * 0.5;
        event.container.set_new_damage(halved);
    }
    Flow::Continue
}
```

The event is a generated struct named after the catalog hook, in CamelCase plus `Event`, with
the hook point's arguments as public fields: values as values and borrowed ones as references (`event.kind`). A
number or `bool` a mod may change is a plain field, written back once every mod has run, even after a `Cancel`;
anything else a mod may change, a game struct or a list, is a `&mut` field. A cancellation or replacement is still
the return value; a collection hook's list is a field (`event.lines`). One event goes through every mod, so each
sees the changes before it. Because no mod can build or match the whole
struct, a later version can add a field without breaking any mod, as NeoForge's event objects do; the cost is that
a mod's own tests drive a hook through its dispatch function ([Testing a trait mod](#testing-a-trait-mod)).

Hook methods take `&self` and require `Send + Sync`. The process-wide mod list is installed once, like mixin module statics. One mod instance serves both
the window's game and the server-thread game. A hook can also fire again inside
the same mod: for example, `LevelRef::set_block` fires `block_changed`. Mods keep mutable
state with interior mutability so these calls can share the instance safely. Data-load
hooks can run on any loading thread; item default changes must be identical on every side.

The trait has every hook declared under `crates/horizon-mod-api/src/hooks/` ([Adding a hook](#adding-a-hook)).
Level hooks use access-specific hosts rather than `Host`. `LevelHost` provides
`block_state` through `LevelAccess::block_state`, client-side detection and
`state_with_property`. `state_string(state)` serializes the full name and properties, or returns `None`
for an unknown state; `min_y()` and `height()` read the level's build range. It also reads light and loaded state, through the game's own queries:
`brightness(LightLayer::Block | Sky, pos)` (`getBrightness`, the stored 0 to 15),
`raw_brightness(pos, sky_darken)` (`getRawBrightness`, which `CropBlock.randomTick` asks with 0)
and `is_loaded(pos)` (`Level.isLoaded`: inside the build height and in a loaded chunk),
`is_raining_at(pos)` (`Level.isRainingAt`, false where `pos` is not loaded) and `enabled_features()`
(`LevelReader.enabledFeatures`, a borrowed `FeatureFlags<'_>`). Neither allocates.
None of them loads a chunk. Light above the build height reads as full sky and below it as
none, and a chunk that isn't loaded reads as the game's query does (full sky, no block light),
so check `is_loaded` first. It also answers tag and fluid questions without a `Host`: `block_in_tag(block_name, tag)`,
`item_stack_in_tag(&stack, tag)`, `fluid_state(pos)` (`getFluidState`, `None` where the block read is),
`state_fluid_state(state)` and `fluid_in_tag(fluid_name, tag)`. `block_state_id(name)` parses a block and properties, and `block_state_default(block)` gives a block's default state, as on `Host`; both return `None` for invalid input or unavailable context. Tag reads are `None` when authoritative tags are unavailable, including on a client level. Shared `Host::tag_members` and `Host::item_stack_in_tag` likewise return `None` on clients until server-sent tags are synchronized; they never substitute local resource-pack tags.
`LevelHost` has no random, by design: a draw from a read-only hook inside a level write would shift the level's random sequence away from vanilla's order. A hook that may draw is given a `LevelRandomHost`.
`LevelRandomHost` provides vanilla's `Level.random` so
mod draws come in vanilla's order. `LevelTicksHost` adds scheduling through
`native_schedule_block_tick` and `native_schedule_fluid_tick`, while
`LevelWriteHost` adds random access, `set_block`, `native_set_block_property`, `native_play_sound` and
`native_send_particles` and `native_level_event` (`levelEvent`, queued in order with the write's own events).
`set_block(pos, state, flags)` is `Level.setBlock` on a whole state, as `LevelRef::set_block` takes it: it
removes the old block's block entity and creates the new block's in the write (`LevelChunk.setBlockState`), runs
the neighbour and shape updates `flags` ask for, and queues the write's level events in order. A mod block entity
it creates starts with the data the `block_entity` hook gives, which is fired in the write and receives the level
(`LevelSide`, read-only: vanilla's `newBlockEntity` gets no level), as the `Host`'s `native_set_block` fires it. It returns true when the block
changed, and false off an authoritative server, for a position that is not loaded or outside the build height,
an invalid state, or the state the block already has. It never loads a chunk. The game owns
the tick queue; mods do not pass it back to these helpers.
For frame hooks, implement `RenderHooks` and return it from `Hooks::render`. The ordered
renderer list is installed once alongside the mod list. `render_needs` collects requests;
`world_drawn` observes the world pass and can encode drawing before the hand and HUD. Both
run on the render thread while the frame is built, before any deferred GPU submission.
The renderer module re-exports the game's wgpu types.

### The typed API from mixin code

A helper injection can use the typed API: bind the game or the `Host` and call attachments and handles. The
worked examples, and which shape to choose, are in [Injections](../injections.md#make-your-injection-convertible).

A catalog hook's trait method, event and return make up its `hook_version` contract
([Named hook points](../how-to/native-mods.md#named-hook-points)). These per-hook contract versions are catalog metadata, not minimum game
versions. Catalog hooks, `Host` calls and raw `target` injections all build against the
[game's exact source revision](../how-to/native-mods.md#native-mod-source-revisions). Rebuild the mod when that contract changes.

Every `impl Hooks` and `impl RenderHooks` must have `#[mod_api::horizon_mod]`. The attribute derives
subscriptions from the methods implemented in that block, so dispatch knows them at installation.

### Parts

A part is code a trait mod attaches to one block, block entity, item or mod menu type, its own or vanilla's
([the spec](https://github.com/arismoko/horizon/issues/41)). A mod attaches its parts once, at startup, in
`Hooks::register`. Each part trait implementation must have `#[mod_api::horizon_part]`; the attribute
subscribes it only to the methods it implements. A `BlockEntityPart` also needs a separately annotated
`BlockPart` implementation:

```rust
struct Gauge;
#[mod_api::horizon_part]
impl BlockPart for Gauge {
    fn has_analog_output(&self, _: &dyn LevelHost, event: &mut HasAnalogOutputEvent) {
        event.output = true;
    }
    fn analog_output(&self, _: &dyn LevelHost, event: &mut AnalogOutputEvent) {
        event.output = 15;
    }
}

#[mod_api::horizon_mod]
impl Hooks for MyMod {
    fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
        r.block_part("minecraft:stone", Gauge);
    }
}
```

A hook whose catalog entry has a `scope` (`block`, `block_entity`, `item`, `menu`) and a `key`, the argument
naming the thing it is about, is a method on that scope's trait only: `BlockPart`, `BlockEntityPart` (a
`BlockPart` too), `ItemPart` and `MenuPart`, as vanilla's and NeoForge's `Block` and `Item` methods are. It has
no `Hooks` method: a thing without parts is never asked, and the game asks the parts attached to it in dispatch
order, through the hook's contract, so a `Cancel` or a replacement skips the parts after it. Where NeoForge fires
a bus event at that moment, the event is a hook of its own on `Hooks`, asked first for every thing, and its cancel
skips the parts: a mod reacting to its own blocks uses a part, one reacting to every block the event. A part's kind is its type: the same kind twice on one
thing, or an id with no namespace, stops the game at startup with a message naming the mods. A panic in a part
adds `, part <type>` to the mod and hook it names. Parts take `&self` and are `Send + Sync`, like `Hooks`.

`BlockPart::clone_item_stack` answers on the server when a player picks the block. It receives the
level, position, block state and `include_data`, true when a creative player picks with Ctrl. Return
`None` to keep the game's item.

`BlockPart::contents_changed(host, event)` observes a world block's storage at `setChanged`,
with a `ServerHost`, typed `event.level`, `event.pos: BlockPos` and `event.state`. Attach it to the specific
blocks you observe, including vanilla containers; it has no global `Hooks` counterpart. The backing
contents are already written, before dirty/comparator updates. Native transactions notify once per
backing block at root commit, never on abort or simulation. An explicit `BlockEntityRef::set_changed`
notifies even if contents are unchanged. A double container notifies each component, not once again
for the other half's comparator refresh. A listener's writes still apply, but a nested notice for that
same backing block does not re-enter the listener; writes to a different backing block notify normally.
Entity/menu placeholder coordinates are not block notices.
This replaces Create's positional `contents_changed` mixin for depot/package/schematic storage; there is no
NeoForge bus event. A compiled listener that reads the committed items through the typed level (the game's
own test, so it cannot drift):

```rust,ignore
{{#include ../../../../crates/horizon-client/src/game/machines/contents_changed_tests.rs:contents_changed_listener}}
```

Declare a block entity type and its valid blocks during `Hooks::register`, then attach its part:

```rust,ignore
let press = r.block_entity_type("press")
    .blocks(&["mymod:press", "mymod:big_press"])
    .build();
r.block_entity_part(&press, Press).expect("native block entity type");
```

The builder qualifies the type ID with the registering mod's namespace. It owns the save ID, wire type
and valid blocks; `block_entity` returns only initial `Compound` data (or none for an empty compound). It is fired
inside the write that creates the entity (`LevelChunk.setBlockState`) and receives the read-only level, a `LevelSide`,
on a server and on a client of one.
Unknown blocks, duplicate types, overlapping membership and attempts to own a `minecraft:` block are
refused at load. Refused native block, item or block entity declarations stop startup, as does unreadable
known-components input; the game does not fall back to vanilla content. An explicitly declared mod
template block uses its declared type; undeclared template blocks retain their vanilla behavior. Adding mod blocks to vanilla types is not supported by this builder.
The hooks about the entity (`block_entity_tick`, `block_entity_update_tag`, `pre_remove_side_effects`,
`menu_button`, `menu_quick_move`) find it by that declared type; the others (`block_entity`, `screen`, the
block hooks) by block. The factory is a `BlockEntityPart` method, not a global `Hooks` method.

`r.block_entity_type_key("other:press")` looks up another mod's type, and
`r.block_entity_type_key("minecraft:chest")` looks up a vanilla type. Both return the same opaque
`BlockEntityTypeKey` as the declaration builder: equality and hashing use the type ID, not its blocks.
The final registry checks these requests at load and refuses unknown IDs, naming the requesting mod
and type. Use a canonical type ID, not a block ID (`minecraft:shulker_box`, not
`minecraft:red_shulker_box`). `BlockEntityRef::kind(host)` returns `Option<BlockEntityTypeKey>`.
`living.hurt_and_break(host, slot, amount)` changes equipped durability on the server; negative amounts repair. It returns the pre-shrink stack only on break, and `None` when nothing breaks or the entity or slot is unavailable. Creative players' equipment is exempt. The operation does not fire equip effects.

`block_entity.replace_data(host, data)` replaces its saved payload, clearing omitted fields while retaining
identity and position. Like `merge_data`, it loads transactionally and synchronizes the change; it returns
false for a missing or stale block entity, data the entity cannot load, or an unchanged result.
Lookup keys may answer capabilities for vanilla and native types. `block_entity_part` accepts only
native types and returns `BlockEntityPartError::VanillaType` immediately for a vanilla key. Native
Parts are expanded after every mod has registered, so a Part can target a type declared later.

`r.block_part_tag("minecraft:logs", Rotates)` and `r.item_part_tag("minecraft:axes", Sharp)` attach a part to
every member of a tag (its name without `#`). Each game resolves them against its own tags when its world
opens, again on `/reload`, and when a client joins a server, so the window's game and its server thread each
answer from the tags they play with; a block that leaves the tag is no longer asked. A mod's parts on one
thing run in the order it registered them, by key or by tag. A part a tag gives a thing
that already has that kind of part warns, naming both mods, and the earlier mod's part stays. A tag that
doesn't exist warns. Opening a world fails, naming the mod, when a part is attached to a block or item the game
doesn't have. Scoped hooks run on a game's thread; one dispatched from any other thread (which no game hook
does) can't see tag parts, and warns once.

A capability is answered by providers, registered by block entity type as NeoForge's
`RegisterCapabilitiesEvent` registers them: obtain `pipe` with the declaration builder or
`r.block_entity_type_key("mymod:pipe")`, then `r.block_entity_capability(&ITEMS, &pipe, |host, at|
Some(Box::new(PipeItems)))` gives a handler for the block entity and side `at` (a `BlockSide`), or none to
leave it to the next provider. A provider may answer for a vanilla block entity type too, using its lookup key.
Providers add up: `level.capability(host, &ITEMS, pos, side)` (`Level.getCapability`, on a `LevelRef`) asks those registered
with `r.block_entity_capability_before_game` first, then the game's own (a block entity's slots by side, by the
rules hoppers follow, and a mod block entity's `block_entity_container`), then the others, each in registration
order, and the first to answer wins. That order is the fork's choice, the game first and `_before_game` to go ahead
of it: NeoForge 26.3's javadoc doesn't state one between mods' providers and the game's. It returns a `Capability`, the handler and who answered. A handler holds
no borrow: each call is given the host, so it may call a
neighbour's capability, and it keeps its state in the block entity's slots, compound or attachments. A provider
is given the block entity and side, so the handler it makes knows where its contents live. Item handlers are
NeoForge 26.3's `ResourceHandler<ItemResource>`: indices from 0 (`size`), what each holds (`resource`, an
`ItemResource`: an item and its components, without a count) and how much (`amount`), how much of a resource an
index holds when it holds that (`capacity`, none where it may not go), and `insert` and `extract`, which take a
`&Transaction` and answer how much moved. There is no `simulate`: a write happens in a transaction
(`host.transaction(|host, tx| …)`, below), and to try one, abort it. `move_resources(host, &from, &to, filter, max,
tx)` (NeoForge's `ResourceHandlerUtil.move`) moves what `filter` accepts, up to `max`, index by index out of `from`
into wherever `to` takes it, each move all or nothing: what `to` won't take stays put. It answers how much moved, or
`Overanswered` (below). Hoppers use a mod's handler
where one is registered, each move in a transaction of its own. A mod's own capability is a
`CapabilityKey::new("mymod:chemicals")`, answered only by parts.

A `Capability` may be held for the tick: look a neighbour's up once per tick and reuse it for
every move that tick, as pipes and machines do. Ask again on the next tick, since the block
entity behind it may be gone by then. For a wasm mod each lookup crosses the sandbox (about
2.5 µs, see [Building a mod to `mod.wasm`](../how-to/wasm-mods.md)), so holding it matters more there.

An item's own capability is reached where its stack lives, an `ItemPlace`: a player's slot, hand or cursor, a
block entity's slot, an item entity, a stack inside another item (`ItemPlace::Nested`) or a mod's own place
(`ItemPlace::Mod`, read and written by the `PlaceProvider` it registers with `r.place_provider`).
`r.item_capability(&ITEM_ITEMS, "mymod:backpack", |host, place| Some(Box::new(Backpack)))` answers
`ITEM_ITEMS` for an item with a `ResourceHandler<ItemResource>` made for that place, and
`r.item_capability_before_game` ahead of the game's own over `item_container`;
`place.capability(host, &ITEM_ITEMS)` (`ItemAccess.getCapability`) asks them in the same order. A handler keeps its contents in the
stack, and reads and writes it through the place, as NeoForge 26.3's `ItemAccess`: `place.stack(host)`,
`place.resource(host)` and `place.amount(host)`, and `place.insert`, `place.extract` and `place.exchange(host, tx,
&resource, amount)`, which answer how much moved. Each runs in a transaction of its own, nested in `tx`, and changes
the place where it is, journaled, so an abort puts it back at every level of an item inside an item. A handler
answering more than it was asked or offered is a mod bug: the move it was part of is undone, and the game warns
once, naming the mod. What doesn't stack with the place, or doesn't fit, goes where vanilla puts an item split off
it, so `exchange(host, tx, &filled, 1)` on a stack of empty buckets is vanilla's filled bucket: from a player's
place into their inventory, else dropped with no thrower (a creative player's hand takes anything and gives them
one if they have none, as `forInfiniteMaterials`); from an item entity dropped beside it; from a block entity's,
an entity's or an item's slot into that container's other slots (`forHandlerIndex`). A handler rewrites its item
with an `exchange` of one. An exchange inside one of two backpacks is done in one of them: that one is rewritten
holding the change and splits off, and the other stays as it was.

A stack's components change the same way, where it lives (NeoForge's `ItemResource.with` and `without`). A component
is named by a `ComponentId`, checked once as a namespaced id (`ComponentId::new("minecraft:damage")`, None for
`Damage` or `!minecraft:food`). `resource.component(&id)` reads the stack's own value in its patch as saved;
`resource.with_component(host, &id, value)` sets it as vanilla's `PatchedDataComponentMap.set` does, keeping no value
of the stack's own where `value` is the item's default (`host.item_default_component(item, id)`, compared as the component's codec reads it: a vanilla
component's numbers by exact value, `custom_data` and the other raw NBT components and a mod's own type for type), and `resource.without_component(host, &id)` removes the component as `remove` does: the stack's own value
goes, and where the item has the component by default a removal (`"!minecraft:max_damage": {}`) is recorded in the
patch.
An exchange of one puts the changed item in place: `place.exchange(host, tx, &place.resource(host)?.with_component(host, &damage,
Tag::Int(5)), 1)` damages one sword of a stack, splitting it off.

#### An entity's items

`entity.capability(host, &ENTITY_ITEMS, ())` (NeoForge 26.3's `Capabilities.Item.ENTITY`) is an entity's items, None for a
stale handle and an entity outside the level in place. The game answers it for a player with their inventory
(`PlayerInventoryWrapper`), indices as vanilla's `Inventory` numbers them: 0..36 the main inventory (0..9 the hotbar),
36..40 the armour from feet to head, 40 the off hand. Each index is the player's `ItemPlace::PlayerSlot` and takes only
what fits there: an armour slot one of what its `equippable` names it for (`ArmorSlot`: its allowed entities hold the
player by id or by tag), and bound armour comes off only in creative. `insert_any`, without an index, goes where
`Inventory.add` puts a stack: onto the same item in the selected slot, the off hand and the main inventory in order,
then into its empty slots, never the armour. For a chest or hopper minecart, a chest boat and a chested donkey, mule or
llama the game answers with their container's slots (`ItemPlace::EntitySlot`). A handler kept past its entity's removal
or its world's close reaches nothing. `r.entity_capability(&ENTITY_ITEMS,
"mymod:robot", |host, robot, ()| …)` answers it for a mod's entity type, and `r.entity_capability_before_game` ahead of the
game's own, asked as block entities' providers are. Giving, taking and counting a player's items go through it:

```rust
// A trade: each offer is (index, the exact stack it must still hold). A stale offer or no
// room for one aborts the transaction, and every move in it is put back.
let (a, b) = (seller.capability(host, &ENTITY_ITEMS, ())?, buyer.capability(host, &ENTITY_ITEMS, ())?);
host.transaction(|host, tx| {
    for (from, to, offer) in [(&a, &b, &seller_offer), (&b, &a, &buyer_offer)] {
        for (index, stack) in offer {
            let offered = ItemResource::of(stack);
            let still = from.resource(host, *index).as_ref() == Some(&offered)
                && from.amount(host, *index) == stack.count;
            if !still
                || from.extract(host, tx, *index, &offered, stack.count) != stack.count
                || to.insert_any(host, tx, &offered, stack.count) != stack.count
            {
                return Err(());
            }
        }
    }
    Ok(())
})
```

#### Entity energy and fluids

`entity.capability(host, &ENTITY_ENERGY, side)` and `entity.capability(host, &ENTITY_FLUIDS, side)` reach a mod's
entity energy and fluid handlers (NeoForge 26.3's `Capabilities.Energy.ENTITY` and `Capabilities.Fluid.ENTITY`), where
`side` is the `Option<Direction>` of the entity the lookup reaches it from, `None` for no particular side. Vanilla
answers neither; register a provider with `r.entity_capability(&ENTITY_ENERGY, "mymod:robot", |host, robot, side| …)`,
which is given that side and may answer differently for each. An entity capability's context is its key's second type:
`()` for `ENTITY_ITEMS`, `Option<Direction>` for these two and for `ENTITY_AUTOMATION`. The existing `move_energy` and `move_resources` transfer
between these and block or item handlers inside a transaction.

#### Entity automation

`entity.capability(host, &ENTITY_AUTOMATION, side)` (NeoForge 26.3's `Capabilities.Item.ENTITY_AUTOMATION`, "automation-accessible
inventory of an entity (boats, minecarts, etc)") is the items of an entity as a hopper reaches them, with the same
`Option<Direction>` context as the energy and fluid keys. The game answers it for the container entities
`EntitySelector.CONTAINER_ENTITY_SELECTOR` picks (chest and hopper minecarts, chest boats) with the slots their
[`ENTITY_ITEMS`](#an-entitys-items) handler has; a player and a chested donkey, mule or llama answer none. A hopper
pushing into the entity in front of it asks for the side it faces the entity from (`Up` for a hopper over it), a hopper
pulling from the entity above it for `Down`, and a dropper for the side it drops onto. Beside the game's container
entities, those entities at the target whose mod provider answers it join the candidates, and the hopper picks one at
random with the draw `getEntityContainer` makes; a mod handler takes the container's place and moves one item at a time
in a transaction, as a mod block's `ITEMS` does. `r.entity_capability(&ENTITY_AUTOMATION, "mymod:robot", |host, robot, side| …)`
registers one, and `r.entity_capability_before_game` answers ahead of the game's own for a minecart or a boat.

#### A menu's slots

Client hooks can read the local player's menu with `host.local_player()?.container_menu(host)`,
including the inventory when no container is open. `MenuRef::slot_count`, `slot` and `carried`
read on either side; `slot` and `carried` return an outer `None` when the handle or slot is
unavailable and `Some(None)` for an empty slot. Each read checks the world epoch and menu generation,
so a handle cannot read a replacement, reopened or respawned menu. These methods do not click or
change slots.

Mutable client hooks can submit a vanilla click with
`menu.click(host, &[mod_api::MenuClick::QuickMove { slot }])`. The slot is the same menu-order
index used by `slot`, not a hotbar index or a GUI slot id. For example:

```rust,ignore
{{#include ../../examples/src/lib.rs:menu_quick_move}}
```

The typed actions are `Pickup` (with `MouseButton::Left` or `Right`), `QuickMove`,
`Swap` (with `SwapWith::Hotbar(0..=8)` or `Offhand`), `Clone`, `Throw`,
`Drag` and `PickupAll`. `Pickup` accepts `None` for outside, dropping the carried stack.
A `Drag { kind, slots }` owns its menu-order slot list and always submits a complete gesture;
`DragKind::Split` divides evenly, `One` places one per slot and `Clone` fills slots in creative mode.
Every action in a batch is validated before any prediction or sending; out-of-range slots/hotbar
indices, creative-only actions in survival and empty batches refuse without effects.
Stale/replaced/respawned menus, spectators, non-client hosts and the creative inventory screen refuse with `false`.
`true` means submitted, not server acceptance: prediction uses the normal menu engine and sends
`ContainerClick` with its state id, changed slots and carried item. Server corrections still win.
Inventory reads and direct server-side item transactions keep their existing contracts; a click does
not expose direct client inventory writes or enforce a mod's locked-slot policy. Call from an explicit
input action rather than submitting every render/tick. Diverges from upstream: new helper.

`ItemPlace::MenuSlot { menu, slot }` is slot `slot` of a player's open menu, a `MenuRef` (`player.container_menu(host)`). It is
a fork addition: NeoForge 26.3's `ItemAccess` reaches a player's inventory slot (`forPlayerSlot`) and cursor
(`forPlayerCursor`), not a menu's slot. It is the storage the slot shows, named by `menu.slot_backing(host, slot)`
(`MenuSlotBacking`), and a write goes to that storage through its own handler, so it is kept once by the transaction
and the storage's rules hold: the player's inventory, armour and off hand by their
[`ENTITY_ITEMS`](#an-entitys-items) index, a chest's or a furnace's slot through its `ITEMS` handler, a chest minecart's
or a chested mount's through its `ENTITY_ITEMS`, and the player's ender chest, inventory crafting grid and a mod's chest
menu (`native_open_menu`), kept by the game. The slot's own rules come first, as a click has them:
`menu.slot_holds(host, slot, &stack)` is how many of a stack it holds (`Slot.mayPlace`, then `Slot.getMaxStackSize`: a
furnace's fuel slot one bucket, an armour slot one piece that fits it, a shulker box's slot no shulker box, a result
slot none), and `menu.slot_gives(host, slot)` whether it gives up what it holds (`Slot.mayPickup`: bound armour stays
on outside creative). An insert fills that slot alone, and what doesn't fit stays out. A crafting grid a transaction
changed has its result made again at the outermost commit, once for each player however many writes it took
(`InventoryMenu.slotsChanged`, with `native_prepare_item_craft`); the result slot takes nothing and gives nothing,
since taking it is a craft. A menu closed, replaced or reopened since the `MenuRef` was made, or one whose player left,
reads and writes nothing. `menu.cursor()` is that menu's cursor (`ItemPlace::PlayerCursor { menu }`,
`forPlayerCursor`), which reaches nothing once the menu is gone, so a kept cursor never reaches the next menu's. The player's screen follows with the tick's container sync.

```rust
// Put four of the menu's offered emeralds into slot 0 of the open chest, or nothing.
let (slot, cursor) = (ItemPlace::MenuSlot { menu, slot: 0 }, menu.cursor());
host.transaction(|host, tx| {
    let took = cursor.extract(host, tx, &emerald, 4);
    if took == 4 && slot.insert(host, tx, &emerald, 4) == 4 { Ok(()) } else { Err(()) }
})
```

#### A block's fluids

Fluids are the capability `FLUIDS` (NeoForge's `Capabilities.Fluid.BLOCK`), answered as items are, by a
`ResourceHandler<FluidResource>` in millibuckets. A `FluidResource` is a fluid id and its data components, without
an amount, kept as an item stack's are (a potion fluid's contents; water and lava have none), and a tank fills only
from empty or with the same fluid and the same components. A `FluidStack` is one with an amount, as tanks keep it
(`resource.to_stack(amount)`, `FluidResource::of(&stack)`). The game's own answer is a mod block's tanks by side
(`block_entity_fluids`) and a cauldron, one tank of 1000 mB that holds water in thirds and lava only whole; a
cauldron has no block entity and is answered by position, and a powder snow cauldron holds no fluid. A cauldron
takes water and lava whatever their components and discards them, as NeoForge's `CauldronFluidContent` does, so
what comes out of it is plain: the one place a fluid's components are dropped. A mod block's saved tanks take no
fluid with components. `level.capability(host, &FLUIDS, pos, side)` finds a handler in a `LevelRef`'s level, as
`ITEMS`' are found, and `move_resources` moves fluids as it moves items. A move the target takes only part of is
undone and tried again with that part, so a cauldron, which gives whole thirds and never more than asked, gives
what fits: asked for a bucket with room for 500 mB, it gives one level, 334 mB.

Fluid held by an item is `ITEM_FLUIDS` (NeoForge's `Capabilities.Fluid.ITEM`), found where its stack lives, as
`ITEM_ITEMS` is: `place.capability(host, &ITEM_FLUIDS)` gives a `ResourceHandler<FluidResource>` for the item at
that `ItemPlace`, and a write rewrites the item there by an exchange of one, so from a stack of more than one the
changed item splits off (`r.item_capability(&ITEM_FLUIDS, "mymod:canister", …)` answers it for a mod's item). The
game answers it for vanilla's buckets, as NeoForge's `BucketResourceHandler` does: one tank of 1000 mB, filled and
drained whole, an empty bucket taking plain water or lava and becoming its full bucket, a full one becoming an empty
bucket. `move_resources` moves between an item and a block as between two blocks: a full cauldron fills a bucket in a
player's slot, and two thirds of one fill none.

#### A block's energy

Energy is the capability `ENERGY` (NeoForge's `Capabilities.Energy.BLOCK`), answered by an `EnergyHandler`: an
amount and a capacity, with no resource, and `insert` and `extract` written in a transaction as a
`ResourceHandler`'s are. `r.block_entity_capability(&ENERGY, &battery, …)` (with a declared or looked-up
type key) registers a provider, asked as `ITEMS`' are, and `level.capability(host, &ENERGY, pos, side)` finds one. The game's own answer is a mod block
entity's `block_entity_energy` buffer, as NeoForge's `SimpleEnergyHandler`: a fill through the sides it comes
through, up to the buffer's room and its limit per insert, and a drain by the sides it leaves by, up to the limit per
extract; a side that reaches neither finds no handler. `move_energy(host, &from, &to, max, tx)` moves between two
(`EnergyHandlerUtil.move`) as one step of `move_resources`: all or nothing, a move the target takes only part of
tried again with that part.

Energy held by an item is `ITEM_ENERGY` (NeoForge's `Capabilities.Energy.ITEM`), found where its stack lives with
`place.capability(host, &ITEM_ENERGY)`, as `ITEM_FLUIDS` is, and moved by `move_energy` as a block's is. A mod
answers it for its item with `r.item_capability(&ITEM_ENERGY, "mymod:cell", …)`, keeping the energy in the item's
own stack and rewriting it by an exchange of one, so from a stack of more than one the charged item splits off.
Vanilla has no energy items, so the game answers it for none.

The game never trusts an answer past what it asked or offered. A handler answering more than it was asked has its
move undone, in its transaction, and is warned once naming the mod, and `move_resources` or `move_energy` returns
`Overanswered` naming it. Returned from the closure of `host.transaction` with `?`, it aborts the whole move, so
neither end keeps any of it: a broken handler can't make a move overflow, duplicate or lose items, fluid or energy, or
panic the game.

A `LivingRef` on the server can apply one effect from an item's loaded consumable with
`living.apply_consume_effect(host, &stack, index)`. The index is zero-based in component
order; this does not eat food, shrink the stack or run consume triggers. `Some(true)` or
`Some(false)` is the effect's result; `None` means the item, index or target is unavailable
or the effect is unsupported. Mod-defined effects, mob random teleport, armor stands and
dragons are not supported. Player random teleport uses the level random as normal.

On a `ServerHost`, import `HostTransaction` to call `host.transaction(|host, tx| …)` (NeoForge 26.3's `Transaction`). It runs a closure whose `Ok` commits and whose
`Err` aborts, nested in the open transaction if any. `tx`, a `&Transaction` (NeoForge's `TransactionContext`), is
what every handler and place write takes, so none happens outside one. Inside, a capability write applies at once, so later steps see it,
and the game keeps each storage as it was before the transaction first wrote it: a block entity's item slots, saved
compound and attachments, a hopper's cooldown, a cauldron's block state, a player's inventory (slots, armour, off
hand and cursor, whether the player is the host or a guest), an entity's inventory (a chest or hopper minecart,
a chest boat, a chested donkey, mule or llama) and an item entity's stack (one emptied stays, empty, until the
outermost commit removes it). A nested commit hands its changes to
the transaction around it, and only the outermost commit makes them final. Then the world is told of each change
once, as `SnapshotJournal.onRootCommit`: `setChanged` and comparators, a jukebox starting or stopping, a cauldron's
block update, and an item dropped at a block, thrown by a player (what a filled bucket's split no longer fits) or
by an entity or split off an item entity. An abort puts every storage back quietly, so nothing outside saw the
change. A panic unwinding out of the closure aborts that transaction as it passes, and the one around it stays open. `host.transaction` is the only way to open one: the raw `transaction_open` and `transaction_close` helpers take a `TransactionSeal` that only it makes. A handler keeps no journal of its own:
its state is the game's, so the game's journal covers it. Direct writes are not covered: an item entity's stack,
`LivingRef::set_equipment`, and the player and armor-stand `slot_set` paths take effect and are final at once, even
inside a transaction. Any equip/unequip event and equip sound from `set_equipment` also happen immediately; an
abort does not undo them. This differs from writes through journaled capability handlers.

`ItemPlace::EntitySlot { entity, slot }` is slot `slot` of such an entity's inventory (NeoForge's
`Capabilities.Item.ENTITY`), read and written like any other place.
Reading or writing a minecart's slots unpacks its loot table with no player, as `getChestVehicleItem` does.

#### A mod's own capability

A mod can define a capability of its own, as Mekanism defines chemicals: a resource type and a key, published from
its root crate ([Dependencies between trait mods](#dependencies-between-trait-mods)) for other mods to use.

```rust
// The chemistry mod's root crate
#[derive(Clone, PartialEq)]
pub struct Chemical(pub String);
pub const CHEMICALS: CapabilityKey<dyn ResourceHandler<Chemical>> = CapabilityKey::new("chem_api:chemical");
```

A mod that names it under `[dependencies]` builds against the same `Chemical` and `CHEMICALS`, so its providers,
`r.block_entity_capability(&chem_api::CHEMICALS, &vat, …)` (where `vat` is a declared type key or
`r.block_entity_type_key("chem_pipe:vat")`), answer the same capability, and
`level.capability(host, &chem_api::CHEMICALS, pos, side)` finds either mod's handler. `move_resources` moves a
chemical as it moves items, in the caller's transaction. The game has no answer of its own for a mod's key, so only
the mods' providers do, and a lookup with none registered costs one table check. A handler that keeps its amount in
its block entity's saved data (`BlockEntityRef::merge_data`) is covered by the game's journal, so an aborted
transaction puts it back. `ItemCapabilityKey::new` and `EntityCapabilityKey::new` define item and entity
capabilities the same way.

A key is known by its name, which must be namespaced (`chem_api:chemical`), and a name has one handler type. Two
mods that each declare their own `Chemical` under one name, rather than sharing the defining mod's, would never see
each other's handlers. So the game refuses that at startup, naming both mods and both types, as NeoForge's
`CapabilityRegistry.create` refuses one name created with a different type, and it refuses a key whose name has no
namespace. Both checks run once, when the mods register.

### Entity parts and mob goals

`r.entity_part("minecraft:zombie", ZombieGoals)` attaches an `EntityPart` to one entity type, a mod's
own or vanilla's. Its single method `register_goals` runs at every `Mob` construction of that type —
spawned or loaded on the authoritative server, never for a client packet copy, at the moment vanilla's
`registerGoals` fan-out ends — and the goals it adds join the
mob's two selectors beside the species' own, with the same priorities, flag locks and interruption:

```rust
struct ZombieGoals;
#[mod_api::horizon_part]
impl EntityPart for ZombieGoals {
    fn register_goals(&self, goals: &mut GoalRegistrar<'_>) {
        // goals.mob().id() is "minecraft:zombie": branch as per-species code does.
        goals.add_goal(7, Stare::default());
        goals.add_target_goal(3, Remember::default());
    }
}
```

`goals.add_goal(priority, goal)` and `goals.add_target_goal(priority, goal)` are `GoalSelector.addGoal`
on the mob's two selectors; `GoalFlags` claims MOVE, LOOK, JUMP and TARGET as vanilla's `Goal.Flag`
does, and a higher priority (a lower number) interrupts a running goal holding the same flag. A goal
object is per mob, built fresh at every construction, and its state is never saved: a goal that needs
memory across loads keeps it in attachments. A mob whose type has no part, or a game with no entity
parts at all, pays nothing — the wrapper and the tick's read view exist only where a mod goal runs.

A `Goal` reads and writes through its `GoalContext`: `mob()` is a fresh handle of the mob it runs on,
`host()` the tick's read view, `random()` the mob's own random (`mob.getRandom()`, so a mod's draws
keep the mob's stream in shape). The read view answers the ticking mob itself directly, the player the
tick sees, and every other entity from the tick's snapshot — a `getEntities` of the moment the mob
list was taken out — so a read the snapshot does not carry (a mob's gear or effects, its species id)
answers its typed refusal (`None`, or `false` for a predicate), and the handle docs mark the reads that
carry the snapshot rule. The selected connected player's UUID and entity ID travel with its view; fake
players are not targetable, and a player cannot narrow to `MobRef`. Intact level and block-entity reads
forward to the real game backend. Writes apply to the
mob itself through the engine's own paths: `move_to(pos, speed)` and `stop_navigation()`
(`PathNavigation`), `look_at(pos)` (`LookControl`), `jump()`, and `set_target(Option<LivingRef>)`
(`Mob.setTarget`, with the write handle's rules: a brain-backed species refuses, as do foxes,
endermen and zombified piglins, whose setter side effects are not modeled). Writes to other entities
are not part of a goal; the write handles a `ServerHost` takes do not exist in a goal tick. Navigation
and look requests reject non-finite or unsafe coordinates without changing the current controls;
navigation also rejects non-finite speed. `jump()` requests JumpControl input, so the normal control
and travel steps consume it in the same AI tick.

Entity parts are a native mod's for now: the goal surface does not cross to wasm (see the
`attach_entity_part` note), so `r.entity_part` is absent from a wasm build.

### Block-state facts

A `BlockStateId` is a registry value, not a live object, so the facts about one that need no world are a small
read-only service on any host, both sides: `host.block_states()` (import `HostBlockStates`). Each reads the game's own
tables and allocates nothing:

| Method | Vanilla |
|---|---|
| `replaceable(state) -> Option<bool>` | `BlockState.canBeReplaced()` |
| `destroy_speed(state) -> Option<f32>` | `BlockBehaviour.Properties.destroyTime` (`getDestroySpeed`): stone 1.5, -1 unbreakable |
| `sound(state) -> Option<BlockSoundType>` | `BlockState.getSoundType()`: volume, pitch and the break, step, place, hit and fall events |
| `has_block_entity(state) -> Option<bool>` | `BlockState.hasBlockEntity()` |
| `states_of(block) -> Option<BlockStateRange>` | `StateDefinition.getPossibleStates`, as the block's consecutive ids |

None for an unknown state or block, and on a host with no game behind it (a mod's test double).

### Fake players

`host.fake_player(level, uuid, name)` makes, or finds again, a player with no connection in the level in place, as
NeoForge's `FakePlayerFactory` does: a machine that acts as a player (a deployer, a block breaker). It is no
online player: the tab list, player counts, sleeping, natural spawning, chunk tickets, statistics and saves leave
it out. `player.is_fake(host)` tells one apart; `player.remove_fake(host)` removes it.

`player.perform_action(host, action)` makes it act as a player does, through the game's own path for the action, as
that player's packet would run it:
- `PlayerAction::UseOn { hand, pos, hit }` is a click on a block (`ServerPlayerGameMode.useItemOn`): reach, the
  hit's place on the block, the build height, spawn protection and the border, then the block's use and the item's
  `useOn`, which is how a block item is placed;
- `PlayerAction::UseItem { hand }` is a use in the air (`ServerPlayerGameMode.useItem`), by the item's own ray from
  the player's eyes and look: a bucket clicked on a block passes, as vanilla's does, and empties when used.
- `PlayerAction::InteractOn { hand, target, location }` interacts with that entity (`Player.interactOn`),
  after the border and interaction-range checks, with the hit relative to its origin. A passed click does not
  fall back to using the item in the air. The fake keeps its own supplied look, rather than aiming a new ray;
- `PlayerAction::Attack { target }` is a hit with the main hand (`handleInteract`'s ATTACK, then `Player.attack`):
  the border and the attack range, the native mods' `attack_entity`, then the blow on a player, a mob or another
  entity, or a fireball, wind charge or shulker bullet turned back. The blow is scaled by the attack strength, as a
  player's is, so a fake that hits every tick does a fraction of a full blow. A mob doesn't take the fake as the
  player to answer;
- `PlayerAction::BreakBlock { pos }` breaks the block at once (`ServerPlayerGameMode.destroyBlock`): reach, the build
  height, spawn protection and the border, then the game mode's and the held tool's rules (adventure mode can't, a
  sword in creative can't, and outside creative a block no digging breaks, destroy time -1 such as bedrock, stays), the native mods' `break_block`, the tool's wear and the drops. There is no digging time:
  a machine that digs over time counts its own progress and breaks the block when it is done.

Its items are its own inventory: set the stack in its hand first (through `ENTITY_ITEMS` or an `ItemPlace`), and
read what is left after. The answer says what happened:

| Answer | Meaning |
|---|---|
| `Ok(ActionOutcome::Consumed)` | Vanilla's `InteractionResult` consumed (a success or a consume), the blow landed, or the block broke |
| `Ok(ActionOutcome::Passed)` | Nothing used the click, a mod or the game stopped the blow, or the block stayed |
| `Ok(ActionOutcome::EffectBlocked(effect))` | The click ran, but what it would start was stopped: a menu or screen, sleeping, riding, or going to another dimension (`FakeEffect`). The click's own side effects have still happened then: the swing seen by others, the hooks that run before the use, vanilla's `Success` |
| `Err(ActionRefused::…)` | Nothing ran: `NotFake` (a real player's handle), `Stale`, `Busy` (another player's turn, an open transaction, or a hook inside another `perform_action`), `OutOfReach`, `Protected`, `InvalidSetup` (non-finite scoped viewpoint or entity hit), or `InvalidTarget` (an interaction or attack on the fake itself or an entity not in its level) |

For a machine using an isolated stack, `player.perform_action_scoped(host, action, setup)` composes that same
persistent player's action with temporary state. Build `PlayerActionSetup::new(stack, eye, yaw, pitch,
secondary_use)`: the fake acts in survival with a fresh standing body (eye height 1.62), that stack in its main
hand, and no other inventory or equipment. Its supplied rotation is used for item rays. Secondary use holds
semantic sneak without changing that standing eye height. No second player is created.

The `ScopedActionResult` contains `outcome` (the same `ActionOutcome`), `held` (the nonempty main-hand remainder)
and `other` (other nonempty inventory stacks, then offhand, carried stack and armor). These stacks are returned
to the caller, not kept by the fake. The fake's persistent body, inventory, game mode, look, secondary-use state
and action timers are restored even on refusal; world changes and the usual interaction/Parts hooks remain.
The ordinary fake safety rules still apply: no screens, sleep, rides, dimension travel, online participation,
statistics, advancement credit or network connection. Neither action method can run inside another player turn
or an open transaction; the action's own ordinary write paths still run transactionally.

On either side, `player.entity().is_shift_key_down(host)` reads semantic secondary use rather than a physical
key mapping or a crouching pose. On the client obtain the reachable local handle with `host.local_player()`
(or `host.entity(id)`); it reads live secondary use even before the next movement tick updates the body's
shift flag. On the server the existing entity flag is read, including a scoped fake's setup. Stale handles and
clients without a joined local player return `None`.

What a machine does beyond a player's action stays in the mod: its own mining progress and catching world drops.
Scoped actions retain player reach, protection and border checks; eye placement is supplied by the mod.

### Testing a trait mod

Every `Host` method defaults to a panic naming it, so a test double implements only what its hook calls, and an
unexpected call fails loudly. Enable the `testing` feature on the mod's `horizon-mod-api` test dependency. Then
`testing::with_mods` takes an owned `Mods` list and runs a closure with it installed on the current thread.
It restores the previous list even if the closure panics. The test allocation is retained because dispatch exposes
a static slice. Call `dispatch_<hook>(&mut host, args…)` inside the closure to build the event as the game does:

```rust
use mod_api::{self, Hooks, Host, HostSide, ServerHost, ModEntry, TickEvent, dispatch_tick, testing::with_mods};
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

struct HostDouble;
impl Host for HostDouble {}
impl ServerHost for HostDouble {}
#[derive(Default)]
struct Counter(Arc<AtomicUsize>);
#[mod_api::horizon_mod]
impl Hooks for Counter {
    fn tick(&self, _: HostSide<'_>, _: &mut TickEvent) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}
let count = Arc::new(AtomicUsize::new(0));
let mods = [ModEntry::new("counter", Box::new(Counter(count.clone())))];
with_mods(mods.into(), || dispatch_tick(HostSide::Server(&mut HostDouble)));
assert_eq!(count.load(Ordering::Relaxed), 1);
```

If a `Host` method returns a game value a test can't build, a double can return `None` instead, as with
`Players::acting_player() -> Option<PlayerRef>`. If the dispatch function itself takes an unconstructible value,
such as `dispatch_perform_command`'s `&NativeServerPlayer`, test the logic beneath the hook instead. The level
hosts are small enough to implement whole. NeoForge's GameTest is the in-game counterpart.


### Adding a hook

A new hook needs its declaration in the area file of `crates/horizon-mod-api/src/hooks/` (`///` lines and
`#[hook(side = "server", contract = "cancel", out = "cancel")]` on `fn name();`; the argument list is in that folder's `mod.rs`), its `native_*` function with the placeholder body
`let _ = (every, parameter);` (all its parameters, in order), and a run of `cargo run -p xtask -- mod-api`.
That command only replaces the placeholder in `native_hooks.rs` with the `dispatch_<hook>` call; commit
that source change and the declaration. The `horizon-mod-api-gen` build dependency generates the hook traits,
`Host` methods and dispatch code into Cargo's `OUT_DIR` when the game and renderer build, including release
builds. These generated Rust files are not kept in the source tree or committed, and no manual regeneration
is needed after a merge. Every hook needs a `mod_api` entry: the build fails naming any hook without one. A hook that stands in for a
method a vanilla block, block entity, item or menu overrides also gets a `scope` and a `key`
(`{"contract": "collect", "out": "output", "scope": "block", "key": "state"}`): the key is a
`BlockStateId` (looked up directly by state for block parts), an `&ItemStack`, a `ModBlockEntity` or an id `&str`, and
the generator also puts the method on that scope's part trait ([Parts](#parts)). A body that is neither the
placeholder nor already calls the dispatch is an error, and the generator never changes it. Hooks that are
free functions outside `Game` are wired by hand: those crates can't see the mod list, so each calls a callback
the executable installs once, such as `NativeLoadHooks` in `horizon-world` and `NativeClientHooks` in the
client library (`menu_slot_rules`, `living_breathe`, `ingredient`). A build reports a placeholder still to
fill instead of changing hand-written source; run `cargo run -p xtask -- mod-api` to fill it. Mixin injections
go at the head of the body, before the dispatch, so they still run first.

### Server attachments

A mod can declare an `AttachmentType<T>` once and call its `get`, `set`, `remove`,
`has` and `get_existing` methods with the hook's host and a `Holder`. `T` is
`Clone + Send + Sync + 'static`; `get` returns a clone and inserts the default on
first access. `has` and `get_existing` read without inserting (NeoForge's `hasData`
and `getExistingData`): `has` says whether a value is stored, saved or set, even
before it decodes, and `get_existing` returns `None` instead of the default. Neither
adds anything to the holder's saved data. `set` runs the optional codec before
changing the value, returning an `AttachmentError` if it cannot encode.
Keys are the pair `(namespace, name)`; a key already decoded as another Rust type is
an error. On a server, a missing holder is an error, not a new phantom holder; a client
keeps a copy for any holder it is asked about (see the sync paragraph below).
`AttachmentError` distinguishes `MissingHolder`, `TypeMismatch`, `Decode`, `Encode`,
`InvalidNamespace` (a malformed saved compound), `InvalidCopyPolicy` (death-copy
without a codec) and `InvalidSyncPolicy` (sync without a codec).
Every host is an `AttachmentStore` (and `host.attachments()` gives it): the typed methods
stand on its few erased ones, so they work the same in a native mod and a wasm mod. A
value keeps its own Rust type, shared: reading one clones it out, and copying a holder's
data copies no value.

```rust
use mod_api::{AttachmentCodec, AttachmentError, AttachmentType, Compound, Holder, Tag};

static COUNTER: AttachmentType<i32> = AttachmentType {
    namespace: "example", name: "counter", default: || 0,
    codec: Some(AttachmentCodec {
        encode: |n| Ok([("count".into(), Tag::Int(*n))].into()),
        decode: |data: &Compound| match data.get(&"count".into()) {
            Some(Tag::Int(n)) => Ok(*n), _ => Err(AttachmentError::Decode),
        },
    }),
    copy_on_death: false,
    sync: false,
};
// Inside a server hook, or mixin code, with `host: &mut dyn ServerHost`:
if !COUNTER.has(host, Holder::Chunk([0, 0]))? { /* chunk carries no counter yet */ }
let old = COUNTER.get(host, Holder::Chunk([0, 0]))?;
COUNTER.set(host, Holder::Chunk([0, 0]), old + 1)?;
Ok::<(), AttachmentError>(())
```

For mixin code, see [The typed API from mixin code](#the-typed-api-from-mixin-code).

Use `Holder::Level`, `Holder::Chunk([x, z])`, `Holder::BlockEntity([x, y, z])`,
`Holder::Entity(id)`, or `Holder::Player(uuid)`. Entity IDs cover every entity in
the level apart from players: mobs, items, experience orbs, projectiles, tridents,
falling blocks, primed TNT, fireworks, boats, minecarts, armor stands, cushions, item
frames, paintings, End crystals, the ender dragon, ominous item spawners and the
server's `OtherEntity` records. An entity keeps its UUID through saves, so its saved
attachments load back with it; an orb saved with a `count` above one loads as several
orbs, and only the first keeps the attachments. Player UUIDs cover online guests and
the acting player. Mutable level
hosts can access their level, loaded chunks and block entities; read-only level hosts
cannot access attachments. Unserialized values disappear on unload or restart. A saved
value is stored under `horizon:attachments`, then the mod namespace and attachment name,
as `{value: <compound>, copy_on_death: 1b, sync: 1b}` (a flag is omitted when false).
Unknown mods' saved entries survive load and save without being decoded. Level
attachments use the dimension's `data/minecraft/horizon_attachments.dat` SavedData
file, not `level.dat`.

A type with `sync: true` sends its value to Horizon clients when `get` creates the
default, on `set` and `remove`, and on demand with `AttachmentType::sync_data(host,
holder)`; changes made in one tick are sent once, at its end. The recipients are the
players who track the holder: guests who were sent a chunk for a chunk or block entity,
who track an entity for an entity, the player and those tracking them for a player, and
everyone in the level for the level. A client also gets all of a holder's synced values
when it starts seeing it: a chunk sent (with its block entities), a level joined or
re-entered, an entity tracked, and its own player. With no native
mod loaded nothing is sent. The wire form is the codec's compound, so `sync` requires
a codec.

Only Horizon clients are sent values. A message with a namespace or name over 256
characters, a value that cannot be written or a payload past 1 MiB is not sent.

On a client, `Host::attachments` returns a copy of what the server synced, which `get`
decodes; `set` and `remove` change only that copy, which is dropped when the chunk,
entity or player goes. A client keeps no copy for the level hosts (`LevelRandomHost`):
they stay server-only there, though their changes on a server are synced.

`copy_on_death` requires a codec. Player respawn copies opted-in entries via their
encoded representation, including raw entries of an absent mod; other entries are
removed. Living-entity conversion is not supported. This follows the default supplier,
optional server-side persistence, player-respawn option and `sync` with a `StreamCodec`
(with `syncData`) of NeoForge 26.3's public `AttachmentType.Builder` contract, and
`IAttachmentHolder`'s `hasData`/`getExistingData`, but not its holder-aware supplier,
custom copy handler, conditional serialization, the `sendToPlayer` predicate,
`AttachmentSyncHandler` partial updates or a wire codec apart from the disk codec.

`Host` exposes the documented `Game` helpers declared in the `Helpers` traits of
`crates/horizon-mod-api/src/hooks/`, without their `native_` prefix. Each keeps the helper's
receiver and documentation; a helper without `self` (`config_files`) takes
`&self` in `Host`. Level services stay on the level hosts above.
For item registry facts, `host.item_crafting_remainder(&stack)` returns a count-one
plain remainder stack, or `None` when there is none or the item is unknown. Horizon
currently stores only the remainder item id, not a template's count or components.
`host.block_item("minecraft:stone")` returns its item registry name; an unknown block
returns `None`, while a known block with no item returns `Some("minecraft:air")`.
Both reads are available on either side.
A helper's `receiver` also exposes it on that handle. Receivers come from public
`*Ref` structs in `mod_api.rs`: `BlockEntityRef` gives `block_entity`, for example.
Adding a handle requires no separate generator registration.
The following documented `Game` helpers are not in the trait yet:

- `spawn_mob`, `mob`, `mobs`, `block_in_tag`, `item_in_tag`,
  `server_block_in_tag`, `server_item_in_tag`, `server_block`,
  `set_display`, `sidebar`, `experience_orbs`,
  `item_entities`, `player`, `panel_recipes`, `recipes`, `component`,
  `set_component`, `remove_component`, `spawn_falling_block`, `explode`,
  `container_slots` uses gameplay-private types.
- `with_mod_block_entity` is generic, so its signature cannot be called through `dyn Host`.

Advancement triggers instead live on `PlayerRef::trigger_criterion`, with a mod-owned
`CriterionTest` and the game's scoped `CriterionContext` resource. The context keeps real
world-aware item predicates; it is not a conditions-only snapshot. See
[advancement triggers](../reference/hook-notes.md#advancement-triggers) for the contract and
the wasm runtime's current same-group callback limitation.

The following catalog hooks are not in the trait:

- `player_command`, `perform_console_command`, `item_decorations`,
  `client_resources_reloaded`, `camera_shake`, `screen_key_pressed_pre`, `screen_char_typed_pre`,
  `screen_mouse_scrolled_pre`, `use_duration`, `use_animation`, `can_eat`,
  `living_entity_use_item_start`, `living_entity_use_item_finish`, `render_hand`, `shader_params`
  and `block_highlight`.
- `biome_colors`, `camera_distance`, `compute_fog_color`, `compute_fov`,
  `data_pack_files`, `mob_submit`, `mouse_button_pre`, `mouse_button_post`, `player_respawned`
  and `server_data_loaded`.

The client `tooltip_color` hook changes the background and top and bottom border gradient colours as mutable ARGB `u32` values; it does not change text colours. It covers solid-fill item tooltips (including creative, recipe book, chat, trading and bundle-selected item names), screen text boxes, and native panel tooltips. The stack is `None` for text-only boxes, including recipe-book item lines where the stack is discarded before drawing. Sprite-based screen tooltips (including options and title screens) use a separate renderer and are not covered; no texture hook is offered.

## Camera and collision reads

`ClientHost::is_paused()` reads the applied client pause state, not whether a pause
screen is open. A multiplayer game continues running while its pause screen is open.
The pause-change pre hook sees the old state; the post hook sees the new state.

`ClientHost::playing()` is true while the player is in the world with nothing over it: no screen, title, sign
editor or book, so keys and zoom apply. `ClientHost::mapping(name)` reads a vanilla key mapping (`key.forward`,
`key.left`, `key.back`, `key.right`, `key.jump`, `key.sneak`, `key.sprint`, `key.attack`, `key.use`) as a
`NativeKeyMapping`: whether it is held (`KeyMapping.isDown`) and the `label` of the key it is bound to, as Controls
shows it. It is `None` for any other name. Both read without `&mut`, so the HUD and screen hooks can ask them.

`ClientHost::clipboard_text()` reads the system clipboard (`KeyboardHandler.getClipboard`, empty text when it
holds none) and `ClientHost::set_clipboard_text(text)` writes it (`setClipboard`; empty text copies nothing). Both
work only while the game handles a key, mouse or typed-text event, from the window or from the control socket
(`horizon-ctl key`, `mouse`, `type` and its clicks): a key, mouse or screen hook that fires inside one, such as
Ctrl+V into a mod's own field. At any other moment (a tick, a frame, a packet) the read is `None` and the write
is `false` with nothing copied, so no mod reaches the clipboard without the player doing something. Both are
`None` and `false` on a server, and take `&self`, so a screen hook can call them. Vanilla's `KeyboardHandler`
has no such rule: Horizon adds it.

The `native_camera()` helper returns the most recently extracted world frame's camera, or none before one exists. Its position, interpolated eye height, partial tick, angles, final FOV, near/far planes and dimension accompany column-major view and projection matrices. The view is camera-relative and includes bob and spin; projection uses reversed Z. Earlier callbacks in a frame can see the previous frame. `native_block_collision(pos)` returns block-local `[min_x, min_y, min_z, max_x, max_y, max_z]` boxes with an empty collision context; unloaded blocks and collision errors give no boxes.

`ClientHost::crosshair_target()` returns the ordinary interaction-range target as
`NativeCrosshairTarget::Block { pos, state, hit }` or
`NativeCrosshairTarget::Entity { entity, point }`. The block hit contains its world-space
location, face and inside flag; the entity point is also in world coordinates.
It uses the game's current targeting ray and block/entity reach, respects blocks
occluding entities, and returns `None` on a miss or without a client world.
It can be read with a screen open. `entity` is an `EntityRef` carrying the current
client level's epoch and network entity ID, including dragon parts. It becomes stale
when the client level is replaced or the client disconnects; a same-level respawn
keeps its epoch. This supplies identity, not client entity reads: the server-only
entity data methods still reject client handles. Unlike `view_hit` and `raycast`, this does not find blocks
in distant terrain or return a block behind a targeted entity.
