# Paper and Bukkit porting ledger

This is a map for porting plugin logic to Horizon native mods, **not** a Bukkit or
Paper compatibility layer. Java plugins do not run through these APIs. Event-by-event
mappings belong in the native hook catalog; this page covers other plugin services.
The Paper/Bukkit references below are public Javadoc, not implementation sources.

| Plugin API | Horizon native-mod route and limits |
| --- | --- |
| [Permissions](https://hub.spigotmc.org/javadocs/bukkit/org/bukkit/permissions/Permissible.html) | **Partial:** `NativeCommand::op` uses vanilla operator levels 0–4, not named permission nodes or Bukkit attachments. `native_player_command` can reject a player command; implement any finer policy in the mod. The command tree and dispatch are in `crates/horizon-client/src/game/modding/native_server.rs`. |
| [Scheduler](https://hub.spigotmc.org/javadocs/bukkit/org/bukkit/scheduler/BukkitScheduler.html) | **Gap:** no general delayed, repeating or asynchronous plugin task service. `native_server_tick` provides start and end callbacks for non-paused server ticks: a mod can manage its own tick deadlines there, rather than assume Bukkit task ownership or cancellation. Block/fluid tick scheduling in `crates/horizon-world/src/level/` is for world ticks, not arbitrary tasks. |
| [PersistentDataContainer](https://hub.spigotmc.org/javadocs/bukkit/org/bukkit/persistence/PersistentDataContainer.html) | **Partial:** typed `AttachmentType<T>` with a codec persists mod-namespaced values on `Holder::Player` and `Holder::Entity`, among other holders. It is not a Bukkit data container or wire format; holders must be present, and player death copying is opt-in. See `crates/horizon-mod-runtime/src/attachments.rs` and [server attachments](modding/src/concepts/trait-mods.md#server-attachments). |
| [ServicesManager](https://hub.spigotmc.org/javadocs/bukkit/org/bukkit/plugin/ServicesManager.html) | **Gap:** no equivalent provider registry, priority selection or economy service contract. Native mods must arrange their own integration; the host and attachment APIs are not a service registry. |
| [CommandExecutor](https://hub.spigotmc.org/javadocs/bukkit/org/bukkit/command/CommandExecutor.html) and Brigadier | **Partial:** register `NativeCommand` usages through `native_register_commands`, then execute commands from any source in `native_run_mod_command` with typed source identity and arguments. Horizon builds a command tree from its supported usage syntax; this is not a general Bukkit executor or arbitrary Brigadier registration. See `crates/horizon-client/src/game/modding/native_server.rs` and [server commands](https://arismoko.github.io/horizon/reference/hook-notes.html#server-commands-and-players). |

For callback signatures, side and current limitations, use [native mods](modding/src/how-to/native-mods.md)
and the hook declarations under `crates/horizon-mod-api/src/hooks/` at the fork revision being targeted.

## Event counterparts: miscellaneous additions (#1015)

Paper API 26.3 (paper-api `26.3.build.244-stable`), public Javadoc only; Horizon hooks are cited by function name.
Scope: the twenty `misc-add` hooks below.

| Horizon hook | Paper event / status | Evidence and porting boundary |
|---|---|---|
| `add_pack_finders` | **Gap:** none (nearest supplied candidate [`PlayerResourcePackStatusEvent`][paper-PlayerResourcePackStatusEvent]) | The candidate reports a player's response to a resource-pack request, not the pack-discovery and registration moment used by `discover_mod_packs`. No Paper event exposes pack-finder registration. No key. |
| `add_server_reload_listeners` | **Gap:** none | `prepare_server_reload` registers data-load listeners before a reload; Paper has no event for registering server reload listeners at that point. No key. |
| `affect_neighbors_after_removal` | **Gap:** none | The hook runs inside level writes to choose ordered neighbour updates before a removed block's updates run; no Paper event exposes those update kinds or their order. No key. |
| `anvil_update` | **Partial:** [`PrepareAnvilEvent`][paper-PrepareAnvilEvent] | The event fires at anvil result preparation and exposes the inventory and result; its [`AnvilView`][paper-AnvilView] also exposes XP and material cost setters. It is not cancellable, unlike Horizon's cancel contract (although a null result can suppress output). No key. |
| `armor_hurt` | **Partial:** [`EntityDamageItemEvent`][paper-EntityDamageItemEvent] | The event covers durability damage to an item from a hit, with a damage setter and cancel, but it is generic to any damaged item and does not expose the hit's damage type/source or Horizon's armor slot grouping. Per-item cancel cannot express the hook's single cancel for all pieces. No key. |
| `arrow_nock` | **Partial:** [`PlayerReadyArrowEvent`][paper-PlayerReadyArrowEvent] | The event documents choosing an arrow while firing a bow and exposes the bow, arrow and per-arrow cancel; Horizon asks whether bow use may start, passes the hand and `has_ammo`, and still fires when no ammunition was found. Its single use-cancel is not the arrow-selection result. No key. |
| `baby_entity_spawn` | **Partial:** [`EntityBreedEvent`][paper-EntityBreedEvent] | The server breeding event exposes both parents, the breeder and the child and is cancellable. Unlike Horizon's pre-creation baby-kind hook, it exposes an existing child, the breeding item and experience, including an experience setter that Horizon cannot express. No key. |
| `block_drops` | **Partial:** [`BlockDropItemEvent`][paper-BlockDropItemEvent] | `BlockDropItemEvent` fires after a player breaks a block and exposes the block state, player and item entities with cancel. Horizon also serves non-player breakers and passes the tool and experience, which the event does not. No key. |
| `block_jump_factor` | **Gap:** none | Paper's supplied jump events describe an entity jumping, not reads of a block's jump factor (`Block.getJumpFactor`). No event has the hook's per-block-state moment. No key. |
| `block_speed_factor` | **Gap:** none | No Paper event covers reads of a block's speed factor; the supplied movement events concern entity outcomes, not per-block factor lookups. No key. |
| `block_grow_feature` | **Partial:** [`StructureGrowEvent`][paper-StructureGrowEvent] | `StructureGrowEvent` matches a sapling or mushroom attempting to grow and is cancellable, but it exposes a `TreeType` species and block list rather than Horizon's configurable feature key, and does not document replacing the feature. No key. |
| `bounce_on` | **Gap:** none | No Paper event covers a block's `bounceOn` response with the post-bounce movement vector; the hook's moment is inside entity movement. No key. |
| `fall_on` | **Gap:** none | No Paper event exposes the landed block state, the calculated fall damage and the requested landing writes together at the hook's moment. No key. |
| `block_tool_modification` | **Gap:** none | No Paper event covers a tool transforming a block with an old/new block state and a cancelable final-state replacement; the hook also runs for simulated tool uses. No key. |
| `use_item_on` | **Partial:** [`PlayerInteractEvent`][paper-PlayerInteractEvent] | Filtering `RIGHT_CLICK_BLOCK` identifies the server right-click-block path and exposes the block, item, hand, face and clicked position, but the event precedes the block behaviour rather than replacing `useItemOn` itself. Its interaction allow/deny controls cannot express Horizon's replacement answer (`TryWithEmptyHand`, `Pass`, and the other results). No key. |
| `use_without_item` | **Partial:** [`PlayerInteractEvent`][paper-PlayerInteractEvent] | The generic right-click-block event precedes block behaviour; Horizon's `useWithoutItem` stage can also follow an item-bearing call that returns `TryWithEmptyHand`, rather than only an empty-hand click. The event's interaction allow/deny controls cannot replace that block behaviour's answer. No key. |
| `bonemeal` | **Gap:** none (nearest [`BlockFertilizeEvent`][paper-BlockFertilizeEvent]) | The event reports resulting fertilization block changes after any `StructureGrowEvent`, rather than Horizon's pre-growth validity and success/cancel decision with the bone meal stack and player-or-dispenser-or-villager cause. No key. |
| `can_continue_sleeping` | **Gap:** none | No Paper event covers the per-tick check that a sleeping player, villager or wandering trader may continue sleeping; the supplied block-build event is unrelated. No key. |
| `can_player_sleep` | **Partial:** [`PlayerBedEnterEvent`][paper-PlayerBedEnterEvent] | `PlayerBedEnterEvent` fires as a player is about to enter a bed and exposes the bed and a vanilla result, but its `BedEnterResult` is read-only and only allow/deny can be set. Horizon lets mods replace the specific problem, including `other_problem`. No key. |
| `chunk_sent` | **Partial:** [`PlayerChunkLoadEvent`][paper-PlayerChunkLoadEvent] | The non-cancellable server packet notification identifies the player and chunk when the player receives that chunk, matching `run_chunk_watches` after the chunk packet was sent and its listen-only result; the Javadoc recommends this moment for sending client-related data such as fake entities. Key added. Not told for the far ring's columns (a Horizon server option, off by default), which a server may still send to players, so a port that sends data per received chunk misses those. No key. |

[paper-PlayerResourcePackStatusEvent]: https://jd.papermc.io/paper/26.3/org/bukkit/event/player/PlayerResourcePackStatusEvent.html
[paper-PrepareAnvilEvent]: https://jd.papermc.io/paper/26.3/org/bukkit/event/inventory/PrepareAnvilEvent.html
[paper-EntityDamageItemEvent]: https://jd.papermc.io/paper/26.3/io/papermc/paper/event/entity/EntityDamageItemEvent.html
[paper-PlayerReadyArrowEvent]: https://jd.papermc.io/paper/26.3/com/destroystokyo/paper/event/player/PlayerReadyArrowEvent.html
[paper-BlockDropItemEvent]: https://jd.papermc.io/paper/26.3/org/bukkit/event/block/BlockDropItemEvent.html
[paper-StructureGrowEvent]: https://jd.papermc.io/paper/26.3/org/bukkit/event/world/StructureGrowEvent.html
[paper-PlayerInteractEvent]: https://jd.papermc.io/paper/26.3/org/bukkit/event/player/PlayerInteractEvent.html
[paper-PlayerBedEnterEvent]: https://jd.papermc.io/paper/26.3/org/bukkit/event/player/PlayerBedEnterEvent.html
[paper-EntityBreedEvent]: https://jd.papermc.io/paper/26.3/org/bukkit/event/entity/EntityBreedEvent.html
[paper-PlayerChunkLoadEvent]: https://jd.papermc.io/paper/26.3/io/papermc/paper/event/packet/PlayerChunkLoadEvent.html
[paper-BlockFertilizeEvent]: https://jd.papermc.io/paper/26.3/org/bukkit/event/block/BlockFertilizeEvent.html
[paper-AnvilView]: https://jd.papermc.io/paper/26.3/org/bukkit/inventory/view/AnvilView.html

## Event counterparts: base-analog hooks (#1015)

Paper API 26.3 (paper-api `26.3.build.244-stable`), public Javadoc only; Horizon hooks are cited by function name.
Scope: the twenty `base-analog` hooks below.

| Horizon hook | Paper event / status | Evidence and porting boundary |
|---|---|---|
| `analog_output` | **Gap:** none | The hook answers vanilla's `BlockState.getAnalogOutputSignal` for a comparator read, a pure query whose `output` a mod may replace; no Paper event reports or replaces a block's comparator signal answer. No key. |
| `anvil_craft_post` | **Gap:** none (nearest [`PrepareAnvilEvent`][paper-PrepareAnvilEvent]) | The preparation event fires when an item enters a slot and can replace the shown result, not when the player takes the result after the levels, inputs and anvil wear are settled. No Paper event reports the completed take. No key. |
| `anvil_craft_pre` | **Gap:** none (nearest [`PrepareAnvilEvent`][paper-PrepareAnvilEvent]) | The event covers result preparation before the take, is not cancellable and does not report the take itself; Horizon's `cancel` refuses the click and leaves the result, inputs and cursor as before. No key. |
| `anvil_damaged` | **Partial:** [`AnvilDamagedEvent`][paper-AnvilDamagedEvent] | The event covers an anvil damaged from use, but fires before the change with a settable damage state and breaking flag and a cancel; Horizon fires after the block already changed from `from` to `to` and is listen-only, so it can express neither. No key. |
| `apply_effect_tick` | **Partial:** [`EntityEffectTickEvent`][paper-EntityEffectTickEvent] | Both are server-side and fire on a tick when an effect acts on an entity; the event's cancel skips that tick's application while Horizon's cleared `keep` ends the effect, and the event lacks Horizon's holder health and food. The hook is told only for a content mod's own effects, not vanilla ones such as poison, so a port that relies on Paper's event for vanilla effects loses them. No key. |
| `armor_stand_manipulate` | **Partial:** [`PlayerArmorStandManipulateEvent`][paper-PlayerArmorStandManipulateEvent] | Same pre-move swap, retrieve or place moment and cancel, with the player, stand and equipment slot; the event also exposes the interacting hand and both item stacks, which Horizon's hook does not pass. No key. |
| `arrow_loose` | **Partial:** [`PlayerReadyArrowEvent`][paper-PlayerReadyArrowEvent] | The event fires while a bow is fired and the server picks an arrow and can cancel that arrow; Horizon fires at the release with the charge, lets mods change the charge, and also fires when no ammunition was found, none of which the event expresses. No key. |
| `attack_entity` | **Partial:** [`PrePlayerAttackEntityEvent`][paper-PrePlayerAttackEntityEvent] | The event fires when a player tries to attack an entity before damage logic and cancelling prevents the blow, matching Horizon's moment and `canceled`; it additionally reports `willAttack` and starts cancelled for entities vanilla will not attack. No key. |
| `beacon_state_change` | **Partial:** [`BeaconActivatedEvent`][paper-BeaconActivatedEvent], [`BeaconDeactivatedEvent`][paper-BeaconDeactivatedEvent] | Both are server-side, non-cancellable and carry the beacon block, but activation is documented when the beam becomes visible and deactivation includes destruction of the beacon itself. Horizon's `tick_beacon_at` reports pyramid-level zero/nonzero transitions after checking the beam; it does not report all those lifecycle moments, and no single event covers both transitions. No key. |
| `bell_ring` | **Partial:** [`BellRingEvent`][paper-BellRingEvent] | The cancellable event fires as the bell is being rung, with the block and the rung entity, matching Horizon's moment, side and cancel; it also reports the rung `BlockFace` direction, which the hook does not pass. No key. |
| `block_break_progress_update` | **Partial:** [`BlockBreakProgressUpdateEvent`][paper-BlockBreakProgressUpdateEvent] | Both are non-cancellable server notifications of block-breaking progress, with the block and breaker; Paper exposes float progress from 0.0 to 1.0 and an entity, while `guest_dig_feedback` passes integer animation stages for a server guest. The public Javadoc does not establish an equivalent stage conversion or limit the breaker to players. No key. |
| `block_burn` | **Partial:** [`BlockBurnEvent`][paper-BlockBurnEvent] | The event fires as fire destroys a block and its cancel keeps it, matching Horizon's moment and `cancel`; it also names the igniting block, which the hook does not pass. No key. |
| `block_changed` | **Gap:** none | The hook observes every committed `Level.setBlock` change before side effects and neighbour updates; Paper has no general block-state-changed event, and NeoForge's `onBlockStateChange` also runs after the change. No key. |
| `block_cook` | **Partial:** [`BlockCookEvent`][paper-BlockCookEvent] | The event reports a block successfully cooking a source into a result, with a settable result, a recipe and a cancel; Horizon's `block_cook` is a listen-only notice after the cook finishes and passes no recipe. No key. |
| `block_damage_abort` | **Partial:** [`BlockDamageAbortEvent`][paper-BlockDamageAbortEvent] | The non-cancellable event reports a player stopping damage to a block, matching Horizon's notification after the abort; it also exposes the item in hand, which the hook does not pass. No key. |
| `block_dispense_armor` | **Partial:** [`BlockDispenseArmorEvent`][paper-BlockDispenseArmorEvent] | The event covers a dispenser equipping an item on a nearby entity and can cancel it before the equip; Horizon fires after the equip, consumption, sound and game event completed as a listen-only notice. No key. |
| `block_entity` | **Gap:** none | The hook supplies a declared block entity's initial saved data inside the write that creates it; Paper has no event for block-entity creation or its initial data. No key. |
| `block_entity_container` | **Gap:** none | The hook declares a mod block entity's item slots and menu when `native_containers` builds the ticking table, and is also queried by `open_native_screen`; Paper's inventory model has no event for this declaration. No key. |
| `block_entity_energy` | **Gap:** none | The hook declares a mod block entity's energy buffer once when the ticking table is built; Paper has no block-entity energy capability or event for it. No key. |
| `block_entity_fluids` | **Gap:** none (nearest [`FluidLevelChangeEvent`][paper-FluidLevelChangeEvent]) | The nearest event reports a world fluid block's level changing from adjacent blocks; the hook declares a mod block entity's fluid tanks when tables are built, a different moment and subject. No key. |

[paper-PrepareAnvilEvent]: https://jd.papermc.io/paper/26.3/org/bukkit/event/inventory/PrepareAnvilEvent.html
[paper-AnvilDamagedEvent]: https://jd.papermc.io/paper/26.3/com/destroystokyo/paper/event/block/AnvilDamagedEvent.html
[paper-EntityEffectTickEvent]: https://jd.papermc.io/paper/26.3/io/papermc/paper/event/entity/EntityEffectTickEvent.html
[paper-PlayerArmorStandManipulateEvent]: https://jd.papermc.io/paper/26.3/org/bukkit/event/player/PlayerArmorStandManipulateEvent.html
[paper-PlayerReadyArrowEvent]: https://jd.papermc.io/paper/26.3/com/destroystokyo/paper/event/player/PlayerReadyArrowEvent.html
[paper-PrePlayerAttackEntityEvent]: https://jd.papermc.io/paper/26.3/io/papermc/paper/event/player/PrePlayerAttackEntityEvent.html
[paper-BeaconActivatedEvent]: https://jd.papermc.io/paper/26.3/io/papermc/paper/event/block/BeaconActivatedEvent.html
[paper-BeaconDeactivatedEvent]: https://jd.papermc.io/paper/26.3/io/papermc/paper/event/block/BeaconDeactivatedEvent.html
[paper-BellRingEvent]: https://jd.papermc.io/paper/26.3/org/bukkit/event/block/BellRingEvent.html
[paper-BlockBreakProgressUpdateEvent]: https://jd.papermc.io/paper/26.3/io/papermc/paper/event/block/BlockBreakProgressUpdateEvent.html
[paper-BlockBurnEvent]: https://jd.papermc.io/paper/26.3/org/bukkit/event/block/BlockBurnEvent.html
[paper-BlockCookEvent]: https://jd.papermc.io/paper/26.3/org/bukkit/event/block/BlockCookEvent.html
[paper-BlockDamageAbortEvent]: https://jd.papermc.io/paper/26.3/org/bukkit/event/block/BlockDamageAbortEvent.html
[paper-BlockDispenseArmorEvent]: https://jd.papermc.io/paper/26.3/org/bukkit/event/block/BlockDispenseArmorEvent.html
[paper-FluidLevelChangeEvent]: https://jd.papermc.io/paper/26.3/org/bukkit/event/block/FluidLevelChangeEvent.html

## Event counterparts: base block hooks (#1015)

Paper API 26.3 (paper-api `26.3.build.244-stable`), public Javadoc; Horizon cited by function name; scope = the hooks below.

| Horizon hook | Paper event / status | Evidence and porting boundary |
|---|---|---|
| `block_entity_tick` | **Gap:** none | No Paper/Bukkit event covers a block entity's server tick (`Level.tickBlockEntities`); NeoForge has no counterpart either. No key. |
| `block_entity_ticks` | **Gap:** none | A world-open query (`EntityBlock.getTicker`), asked for each block state, for whether a block entity ticks; no Paper event or API hook exists for it. No key. |
| `block_entity_update_tag` | **Gap:** none | Server-side serialisation of a block entity's client update tag (`BlockEntity.getUpdateTag`) has no Paper event. No key. |
| `block_fade` | **Partial:** [`BlockFadeEvent`][paper-BlockFadeEvent] | Both fire before a block fades, for melting and coral death, and both can be cancelled so the old state stays; the event gives the block (position and current state) and `getNewState()`, which is `to`. Paper also fires it for fire burn-out and turtle eggs, which this hook does not reach, so a plugin relying on those cases loses them. No key. |
| `block_failed_dispense` | **Partial:** [`BlockFailedDispenseEvent`][paper-BlockFailedDispenseEvent] | Paper fires only when the dispenser's inventory is empty, while the hook fires for any failed dispense and passes the `item` that was not dispensed; the event exposes no item. Paper can also change whether the failure effect plays, which the listen-only hook cannot express; the narrower moment and missing item also prevent coverage. No key. |
| `block_form` | **Partial:** [`BlockFormEvent`][paper-BlockFormEvent] | Both fire before a block forms from world conditions and can cancel it, with the position and the new state (`getNewState()`); snow and ice forming are the event's documented examples. Paper also covers water/lava and concrete forms, which this hook does not reach, so a plugin relying on those cases loses them. No key. |
| `block_ignite` | **Partial:** [`BlockIgniteEvent`][paper-BlockIgniteEvent] | Both fire server-side before ignition and can cancel it, with the target block, nullable player and cause. Paper also exposes the igniting entity and source block, which the hook does not pass; its Javadoc directs player fire placement to BlockPlaceEvent. No key. |
| `block_lock_check` | **Partial:** [`BlockLockCheckEvent`][paper-BlockLockCheckEvent] | Both check a player's container lock and can change the answer (`setResult` / `locked`). Paper additionally exposes and can replace the key item and customize denial feedback; the hook passes only the player, block and boolean result, for server guests. No key. |
| `block_receive_game` | **Partial:** [`BlockReceiveGameEvent`][paper-BlockReceiveGameEvent] | Both can suppress a sculk sensor's response to a received game event, with the receiving block and event. Paper additionally exposes the source entity, which the hook lacks, and its Javadoc documents sensors only while the hook also reaches shriekers. No key. |
| `block_shear_entity` | **Partial:** [`BlockShearEntityEvent`][paper-BlockShearEntityEvent] | Paper fires when a dispenser shears a nearby entity and is cancellable, exposing the tool and a mutable drop list; the hook fires only after the shearing effects, is listen-only, and passes neither tool nor drops. No key. |
| `block_signal` | **Gap:** none | Redstone signal computation (`getSignal`, `getDirectSignal`) has no Paper/Bukkit event; this is a query, not a moment. No key. |
| `block_spread` | **Partial:** [`BlockSpreadEvent`][paper-BlockSpreadEvent] | Paper's cancellable event covers fire spreading from a source block, matching the hook's `fire` half, with `getSource()` as `from`. It has no fluid-flow case and no `kind`, so the hook's `fluid` half is not covered by this event; Paper's cancellable [`BlockFromToEvent`][paper-BlockFromToEvent] covers water and lava flow with source and destination blocks, but has no `kind` either. No key. |
| `block_step_on` | **Gap:** none | `Block.stepOn` has no Paper/Bukkit event; general movement events do not describe a block's own step callback. No key. |
| `blocks` | **Gap:** none | Paper has no event for declaring or registering new block types at startup; this is a registration collect hook, not a runtime moment. No key. |
| `break_block` | **Partial:** [`BlockBreakEvent`][paper-BlockBreakEvent] | Both fire server-side before a player breaks a block and can cancel it or override dropped experience. Paper exposes the sampled experience and an independent `setDropItems` control; the hook starts with an optional experience override and has no independent item-drop control. No key. |
| `brewing_complete` | **Partial:** [`BrewEvent`][paper-BrewEvent] | Paper documents brewing completion, but `getContents()` still holds the pre-finalization items and the event can cancel or change the result list; the hook fires after brewing, is listen-only and passes only the stand position. Paper also exposes the remaining fuel level. No key. |
| `brewing_fuel` | **Partial:** [`BrewingStandFuelEvent`][paper-BrewingStandFuelEvent] | Paper fires before a fuel item raises the fuel level and can cancel or change the fuel power; the hook fires after the fuel was consumed and gained, is listen-only, and cannot change the amount. Same fuel item, different moment and result. No key. |
| `brewing_start` | **Partial:** [`BrewingStartEvent`][paper-BrewingStartEvent] | Both concern a stand starting to brew, but the hook fires after fuel and the timer are set and is listen-only, while the event exposes setters for the brew time. No key. |
| `broadcast_message` | **Partial:** [`BroadcastMessageEvent`][paper-BroadcastMessageEvent] | Both fire before a server broadcast, let listeners change the message and cancel it. Paper additionally exposes the recipient set, which the hook lacks; Paper notes that this event is async when fired from an async thread. No key. |
| `bucket_empty` | **Partial:** [`PlayerBucketEmptyEvent`][paper-PlayerBucketEmptyEvent] | Both concern a player emptying a bucket server-side and can cancel placement, with the player and target block. Paper additionally exposes the clicked block, face, hand and exact bucket material and can replace the resulting held item; the hook passes only the fluid, collapsing aquatic-entity buckets to water, and cannot replace the result item. The hook is told only for guests of a server, not for the local player in single player or on a LAN host. No key. |

[paper-BlockFadeEvent]: https://jd.papermc.io/paper/26.3/org/bukkit/event/block/BlockFadeEvent.html
[paper-BlockFailedDispenseEvent]: https://jd.papermc.io/paper/26.3/io/papermc/paper/event/block/BlockFailedDispenseEvent.html
[paper-BlockFormEvent]: https://jd.papermc.io/paper/26.3/org/bukkit/event/block/BlockFormEvent.html
[paper-BlockIgniteEvent]: https://jd.papermc.io/paper/26.3/org/bukkit/event/block/BlockIgniteEvent.html
[paper-BlockLockCheckEvent]: https://jd.papermc.io/paper/26.3/io/papermc/paper/event/block/BlockLockCheckEvent.html
[paper-BlockReceiveGameEvent]: https://jd.papermc.io/paper/26.3/org/bukkit/event/block/BlockReceiveGameEvent.html
[paper-BlockShearEntityEvent]: https://jd.papermc.io/paper/26.3/org/bukkit/event/block/BlockShearEntityEvent.html
[paper-BlockSpreadEvent]: https://jd.papermc.io/paper/26.3/org/bukkit/event/block/BlockSpreadEvent.html
[paper-BlockBreakEvent]: https://jd.papermc.io/paper/26.3/org/bukkit/event/block/BlockBreakEvent.html
[paper-BrewingStandFuelEvent]: https://jd.papermc.io/paper/26.3/org/bukkit/event/inventory/BrewingStandFuelEvent.html
[paper-BrewingStartEvent]: https://jd.papermc.io/paper/26.3/org/bukkit/event/block/BrewingStartEvent.html
[paper-BroadcastMessageEvent]: https://jd.papermc.io/paper/26.3/org/bukkit/event/server/BroadcastMessageEvent.html
[paper-PlayerBucketEmptyEvent]: https://jd.papermc.io/paper/26.3/org/bukkit/event/player/PlayerBucketEmptyEvent.html

[paper-BrewEvent]: https://jd.papermc.io/paper/26.3/org/bukkit/event/inventory/BrewEvent.html
[paper-BlockFromToEvent]: https://jd.papermc.io/paper/26.3/org/bukkit/event/block/BlockFromToEvent.html
