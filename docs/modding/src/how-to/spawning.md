# Controlling spawns

Four hooks stand in for the checks and setup around a mob. All run on the server, get the read-only host, and fire on replayed ticks, so a listener must be a pure function of its inputs.

| Moment | Hook | Answer |
|---|---|---|
| The spawn predicate for a type and position | `mob_spawn_placement_check` | `NativeSpawnCheck::{Default, Succeed, Fail}` |
| The mob's own position and obstruction checks | `mob_spawn_position_check` | `NativeSpawnCheck::{Default, Succeed, Fail}` |
| Just before `finalizeSpawn` | `finalize_spawn` | `Flow::Cancel`, `spawn_cancelled`, a new `difficulty` |
| Each tick, before a mob's `checkDespawn` | `mob_despawn` | `NativeDespawn::{Default, Allow, Deny}` |

## Stopping a spawn or leaving a mob plain

`finalize_spawn` has two cancels, as in NeoForge. Returning `Flow::Cancel` skips finalization: the mob still spawns, but with no equipment, attributes or class setup. Setting `event.spawn_cancelled = true` adds no mob at all.

```rust,ignore
fn finalize_spawn(&self, _: &dyn mod_api::Host, event: &mut mod_api::FinalizeSpawnEvent<'_>) -> mod_api::Flow {
    if event.kind == "minecraft:zombie" && event.reason == mod_api::NativeSpawnReason::Natural {
        event.spawn_cancelled = true;
    }
    mod_api::Flow::Continue
}
```

`finalize_spawn` fires only where vanilla calls `finalizeSpawn`, so a breeding, an infested block's silverfish or a thrown egg's chick does not tell it.

For `/summon minecraft:ender_dragon` without NBT, command placement starts at zero
rotation and consumes no random yaw draw. Skipping finalization also skips its
follow-range and handedness draws. Spawn eggs retain their random placement yaw;
an explicit NBT compound follows the load route without finalization.

See [the hook notes](../reference/hook-notes.md#finalize-spawn) for what each spawn site covers and what is left out (group data, breeding, loads, worldgen structure mobs).

## Keeping or removing a mob

`mob_despawn` fires once per tick for every mob in a loaded chunk, just before vanilla's `Mob.checkDespawn`, persistent mobs and mobs in peaceful mode included. `Allow` discards the mob, `Deny` keeps it, `Default` leaves vanilla's rules to decide. An answer other than `Default` skips all of vanilla's checks, so a `Deny` keeps a hostile mob in peaceful mode and neither answer takes the idle-despawn `nextInt(800)` draw.

```rust,ignore
fn mob_despawn(&self, _: &dyn mod_api::Host, event: &mut mod_api::MobDespawnEvent<'_>) {
    if event.kind == "minecraft:zombie" {
        *event.result = mod_api::NativeDespawn::Deny;
    }
}
```

The wither and the ender dragon override `checkDespawn` in vanilla, so the event is not fired for them.

## Adding or removing a custom spawner

The level's custom spawners (phantoms, patrols, cats, village sieges, the wandering trader) are a list a mod edits once, in `modify_custom_spawners`. A mod's own spawner is a key plus `custom_spawner_tick`, which runs every tick while mob spawning is on:

```rust,ignore
fn modify_custom_spawners(&self, _: &dyn mod_api::Host, event: &mut mod_api::ModifyCustomSpawnersEvent<'_>) {
    // no phantoms in this world
    event.spawners.remove(&mod_api::SpawnerEntry::Builtin(mod_api::SpawnerKind::Phantom));
    event.spawners.add(mod_api::SpawnerKey::new("mymod:rat_swarm").unwrap());
}

fn custom_spawner_tick(&self, host: &mut dyn mod_api::ServerHost, event: &mut mod_api::CustomSpawnerTickEvent<'_>) {
    if event.key.as_str() == "mymod:rat_swarm" && event.spawn_enemies {
        // spawn through `host`
    }
}
```

Every mod is asked about every key, so a mod ignores keys it does not own. See [the hook notes](../reference/hook-notes.md#custom-spawners).
