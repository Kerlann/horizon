# Joined-list location audit

Draft prerequisite: 1f0 must remove the hot temporary-extraction pattern before this index lands.
The index is residency/location authority for the future id-to-slot table, not a universal
`find_join` speedup. Its costs are paid by every JoinList structural edit, including clients.

## Production site census

Bounded census of named joined fields in `game.rs` and `game/`, excluding test scopes:
45 `remove`, 59 `retain`, 3 `retain_mut`, 2 `drain`, and 21 direct whole-list takes.
This is a count of static sites, not calls per tick. Generic list operations and macro-expanded
entity removals are additional sites; no claim of a complete alias analysis is made.
Paths below are relative to `crates/horizon-client/src/` at the draft base.

| Source | Operations and lines |
|---|---|
| `game.rs` | remove: 4377, 4383, 6448; retain: 15383; retain_mut: 15541 |
| `game/administration/command_harness.rs` | retain: 1064 |
| `game/administration/commands/mod.rs` | retain: 2431, 2433, 2439, 2441, 2452, 2461, 2462, 2463, 2464, 2469; remove: 2446 |
| `game/administration/commands/world_cmds.rs` | remove: 448 |
| `game/blocks/dispenser.rs` | remove: 984 |
| `game/blocks/nether_portal.rs` | remove: 996, 1007, 1018, 1029, 1076, 1107, 1142; retain: 1046, 1168 |
| `game/entities/armor_stands.rs` | remove: 853, 867, 1109 |
| `game/entities/boats/mod.rs` | remove: 1549 |
| `game/entities/cushions.rs` | remove: 388 |
| `game/entities/explosion.rs` | drain: 822, 823 |
| `game/entities/fireworks.rs` | remove: 392 |
| `game/entities/frames.rs` | remove: 380, 509, 536 |
| `game/entities/lightning.rs` | remove: 683, 726 |
| `game/entities/minecarts.rs` | retain: 1535; remove: 1898, 2028, 2421, 2476 |
| `game/entities/paintings.rs` | remove: 349 |
| `game/entities/tnt.rs` | remove: 332 |
| `game/entities/vehicle_teleport.rs` | retain: 295, 296, 297 |
| `game/items/deflect.rs` | remove: 143 |
| `game/items/trident.rs` | remove: 674 |
| `game/mobs/beehives.rs` | remove: 53 |
| `game/mobs/creatures/mod.rs` | retain: 1557, 4574, 4867 |
| `game/mobs/dragon.rs` | remove: 1428, 4522 |
| `game/modding/native_entities.rs` | remove: 296, 297, 298, 299 |
| `game/modding/native_entity_remove.rs` | retain: 155 |
| `game/modding/native_projectiles.rs` | remove: 248 |
| `game/network/lan_actions/packets.rs` | retain: 809 |
| `game/network/lan_actions/pickups.rs` | retain: 122 |
| `game/network/remote/mod.rs` | retain: 2885, 2911, 3195, 3196, 3197; remove: 4330; retain_mut: 7368 |
| `game/network/remote_objects.rs` | retain: 511, 512, 513, 525, 526, 527 |
| `game/network/remote_projectiles.rs` | retain_mut: 658; retain: 830, 831, 832, 833, 834, 835 |
| `game/network/remote_vehicles.rs` | remove: 582 |
| `game/world/end.rs` | remove: 1551, 1556 |
| `game/world/ordered_tick.rs` | remove: 228 |
| `game/world/persist.rs` | retain: 1121, 1125, 1126, 1536, 1543, 1549, 1570, 1577, 1589, 1596, 1615, 1622, 1629, 1633, 1637, 1638, 1639, 1640 |

### Hot paths and known blocker

- `entities/fireworks.rs:392–475`, `items/trident.rs:674–1047`, and
  `game.rs:6448–6457` temporarily remove/reinsert each live owner. Index maintenance would
  add quadratic work even without physical departures. These are **blocking 1f0 work**, not
  acceptable production costs hidden by this draft.
- `entities/tnt.rs:332` and `world/ordered_tick.rs:228`: individual expiry during ordered
  ticking. Do not batch without preserving visibility of each departure to later ticks.
- `entities/lightning.rs:683,726`: repeated removals during a strike; may be bursty.
- `modding/native_projectiles.rs:248`: callback replay removal; peers see the departure.
- `game.rs:4377,4383`: shared pickup/discard entry points. Called from player pickups,
  mob looting, transactions, hoppers, portals and commands; multiple calls can occur in a tick.
- Mob sweep (`mobs/creatures/mod.rs:1253` onward) and dropped-item sweep (`game.rs:4685`)
  already take entire lists and retain survivors. Chunk unload uses stable retain in
  `world/persist.rs:1536–1640`. Explosion uses two drains (`entities/explosion.rs:822–823`).
- Direct takes include mob navigation/flags/position/goals, projectile replay, mob and
  projectile passes, dropped items, orbs, pickups and remote mirrors. A take moves the map
  unchanged; its replacement spawn buffer has independent location authority.

## Mutation proof

Run `python3 tools/join-slot-probe/mutate.py`. It changes only the production source,
runs focused tests, requires an actual failed test (not a compiler failure), and restores
source in `finally`. Baseline: all 10 focused tests pass. All 22 mutants were killed.

| Site | Fault | Killing test |
|---|---|---|
| join_slot | always miss | locations_follow_every_membership_operation_and_lifted_owner |
| repair | omit rebuilding | locations_follow_every_membership_operation_and_lifted_owner |
| append | omit receiver offset | locations_follow_every_membership_operation_and_lifted_owner |
| remove | omit deletion; omit suffix shift | locations_follow_every_membership_operation_and_lifted_owner |
| extract_if | omit deletion; omit shift | locations_follow_every_membership_operation_and_lifted_owner |
| swap_remove | omit deletion; omit moved-slot repair | locations_follow_every_membership_operation_and_lifted_owner |
| pop | omit deletion | locations_follow_every_membership_operation_and_lifted_owner |
| clear | omit map clear | locations_follow_every_membership_operation_and_lifted_owner |
| truncate | retain stale slots | locations_follow_every_membership_operation_and_lifted_owner |
| retain | omit repair | locations_follow_every_membership_operation_and_lifted_owner |
| drain | retain removed slot; omit suffix shift | locations_follow_every_membership_operation_and_lifted_owner |
| swap | omit either slot repair | locations_follow_every_membership_operation_and_lifted_owner |
| retain_mut | omit repair | removals_and_retention_preserve_survivor_numbers_and_order |
| push | omit registration | locations_follow_every_membership_operation_and_lifted_owner |
| insert | omit repair | locations_follow_every_membership_operation_and_lifted_owner |
| clone | detached map with cloned entities | locations_follow_every_membership_operation_and_lifted_owner |
| checked_edit | bypass registration invariant | bound_update_rejects_whole_entity_overwrite |

The fixtures exercise production JoinList code, including the real attachment registration
and `same_registration` check. They are not real-game callback proofs. Gaps: no mutant of the
ordered-tick Game call sites themselves; no per-kind extraction fixture yet (belongs to 1f0).
The existing out-of-order ordered-tick fixture now calls the same indexed lookup.
