<!-- chapter: Entity parts and mob goals -->
<!-- separator: 1 -->

### Entity parts and mob goals

`r.entity_part("minecraft:zombie", ZombieGoals)` attaches an `EntityPart` to one entity type, a mod's
own or vanilla's. Its method `register_goals` runs at every `Mob` construction of that type —
spawned or loaded, the moment vanilla's `registerGoals` fan-out ends — and the goals it adds join the
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
answers `None`, and the handle docs mark the reads that carry the snapshot rule. Writes apply to the
mob itself through the engine's own paths: `move_to(pos, speed)` and `stop_navigation()`
(`PathNavigation`), `look_at(pos)` (`LookControl`), `jump()`, and `set_target(Option<LivingRef>)`
(`Mob.setTarget`, with the write handle's rules: a fox, an enderman or a zombified piglin stores it
with its setter's side effects, and a brain-backed species refuses, since its target is a brain
memory that `MobRef::set_brain_target` writes from a server host). Writes to other entities
are not part of a goal; the write handles a `ServerHost` takes do not exist in a goal tick.

#### Read publication schema

`EntityReadCapture` records a logical engine tick, owner pass and publication revision.
Its phase (`PassStart`, `BeforeCallback`, `AfterEntityTick` or `AfterAppliedWrite`)
describes applied engine state, not enqueue acceptance. A speculative light overlay
is marked separately by `pending_overlay: Option<CommandId>`; the command id is
identity, not authority or proof of completion. These are cached, potentially stale
publications. Removal, physical lifting and retained output are separate states.

`EntityReadError<E = core::convert::Infallible>` keeps one live-read refusal family:
`Lifted` for a physically lifted foreign owner, `Domain(E)` preserving the operation's
typed error, or `Pending(CommandId)` for a resident foreign owner with a pending heavy
dependency relevant to this read. FIFO ordering may queue heavy writes even while the
owner remains resident; this is Pending, never Lifted or a stale successful read. An
unrelated pending light overlay does not make every heavy read Pending. Only the explicit
nested callback actor is OWN; its live reads remain own reads. Cached scalar Option
accessors do not gain this error. Native and wasm share the Wire form, and the default
uninhabited Infallible domain always refuses decoding. This schema foundation does not
implement the deferred heavy-read or queued-write routing.

The retained families expose `cached_row(&dyn Host) -> Option<...ReadRow>`; raid data
uses its separate level-owned `RaidReadRow`. Row availability is the outer Option.
Nullable fields retain domain absence, unsupported subkind or genuinely unknown client
data, never a foreign live-read refusal. Base identity/position/geometry fields require
known values, without invented defaults. Scalar Option contracts remain; `added_to_level`,
`is_started` and `is_active` now return Option<bool>. Projectile pierce level keeps its
existing domain-only `PierceLevelError`.

Owned payloads materialize on request, not by cloning every capture pass. Distance and
restriction checks project cached position and restriction center/radius. Pathfinding
malus is a finite `Vec<(PathType, f32)>` table; can-eat stores `[bool; 2]` indexed by
`always`. Ray tracing requires the engine builder's row-bounds/immutable-world proof and
adds no LevelReadRow. Minecart payloads, player/session strings, passengers, block transforms,
melee range, heavy payload reads and queued-write surfaces remain deferred.

#### Write receipts and completion

`WriteReceipt<R, E>` distinguishes `Applied(actual_result)`, `Pending(CommandId)`
and `Refused(typed_error)`. Pending is acceptance only: it carries no invented
boolean, changed count or delivery guarantee. Available validation runs before
acceptance. `EntityWriteError<E>` keeps authority, epoch, generation, removal and
work-limit refusals separate from the operation's original `Domain(E)` error.

`CommandCompleted<R, E> { id, outcome: Result<R, E> }` is terminal: Pending cannot
be represented in it. Each implemented queued domain supplies its concrete typed
operation variant and the same result to the issuing mod's
`entity_command_completed` event and `CommandId::query(&dyn Host)`. No placeholder
operation or unsupported event/query route is introduced by these carrier types.
The real route lands with its first converted operation, then grows by domain.

Completion is published after authoritative apply/refusal and row refresh. Events
are the primary path; terminal query results remain through the end of the following
logical engine tick. Queries do not consume results. Pending records do not silently
expire. `CommandQueryError::StaleEpoch` is distinct from `UnknownOrExpired`, which
also covers commands inaccessible to the calling mod without disclosing another
mod's work. Transactions discard their accepted commands and overlays on rollback.

These declarations do not convert the deferred write methods by themselves. The
18 RNG/veto/callback/lifecycle methods remain synchronous; 109 deterministic edits
and 19 heavy-dependent edits require their actual ordered implementations. The
heavy-dependent group has no speculative heavy payload overlay.

#### Brain behaviors

Native mods only: entity parts do not cross to wasm yet.

`EntityPart::register_behaviors` receives a `BehaviorRegistrar` whenever a brain-driven species builds or rebuilds its brain. Use `add(activity, priority, min_duration, max_duration, requirements, behavior)` to join a fresh `Behavior` to the species' supported activity. Registration returns a `BehaviorRefusal` for an unsupported activity or invalid duration. Behaviors start and stop in vanilla `Brain.tick` order: priority, activity bucket, then insertion order, with mod behaviors after vanilla behaviors in the same bucket. Their state is not saved. `BehaviorCx` provides the mob, host, random source and typed brain memory access; each species refuses memories it does not support.

The part's other method, `spawn_cluster_size`, changes how large a natural spawn group of the type grows
(`Mob.getMaxSpawnClusterSize`); [controlling spawns](../../how-to/spawning.md#changing-how-large-a-spawn-group-grows)
shows it.

Entity parts are a native mod's for now: the goal surface does not cross to wasm (see the
`attach_entity_part` note), so `r.entity_part` is absent from a wasm build.
