<!-- chapter: State flags on the living handles -->
<!-- separator: 1 -->

### State flags on the living handles

`host.living(entity)` and `host.mob(entity)` make the handles of [#7387](../../reference/hook-notes.md)'s handle design, and
the state flags sit where vanilla keeps them: the ones `LivingEntity` owns read and write through `LivingRef`, the ones
`Mob` owns through `MobRef`. Reads take the shared host, writes the server one; a stale handle or a kind the game keeps
no such state for reads `None` and writes `false`, as every handle does.

Through `LivingRef`: the glide (`is_gliding`, `gliding_ticks`, `stop_gliding`), the sleep of a server player or a
villager (`is_sleeping`, `sleeping_pos`, `start_sleeping`, `stop_sleeping`, `set_sleeping_pos`, `clear_sleeping_pos`),
the jump of the player in place (`is_jumping`, `set_is_jumping`), climbing (`is_climbing`, the same `onClimbable`
movement uses, including the gliding exclusion for vines), the baby read (`baby`) and shield blocking
(`is_blocking`: `LivingEntity.isBlocking`, an item with `blocks_attacks` in use past its block delay, for players
and mobs). Server player sleep writes
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

`EntityRef::leash_holder` takes `&dyn Host` and reads the live lead of a mob or boat. `LeashHolder::Unleashed`
is a definite absence; `Entity` holds an existing entity handle for a player, mob or boat, and `Knot` holds
fence coordinates. `None` means an entity that takes no lead, that the entity or holder is unavailable, that the
level is parked, or that a saved holder has not resolved; this read neither loads a level nor changes the lead.

`EntityRef::leash_to` and `leash_to_knot` take `&mut dyn ServerHost` and hand the lead to a player, mob, boat or
fence as `Leashable.setLeashedTo` does: the entity lets go of any other holder and gets off its vehicle, and neither
`canBeLeashed` nor the distance is asked, so a holder too far away snaps the lead on the next tick. `detach_leash`
drops the lead as an item or removes it; a mob's removal fires `entity_unleash`.

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
