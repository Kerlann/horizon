<!-- chapter: Tool transformation previews -->
<!-- separator: 1 -->

### Tool transformation previews

`LevelRef::block_transform` previews a tool against a loaded block in the player's server level,
including a parked level. Pass the player, hand, stack, position and hit; the owned result describes the new state,
relative durability cost, sound, loot and particle. The call does not apply the result. It fires
`block_tool_modification` with `ToolUse::simulated` set to `true`: listeners must not change the
level during a simulation. A stale player, unavailable level, unloaded position, invalid face,
or tool with no transformation returns `Ok(None)`. A physically lifted foreign source refuses
`EntityReadError::Lifted`; a relevant pending write refuses `EntityReadError::Pending`.
An explicit OWN callback uses its actual body loan, and the simulation hook receives that same
read host. The real action fires the same hook with `simulated` set to `false`.
