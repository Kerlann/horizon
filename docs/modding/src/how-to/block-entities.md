# Declaring a machine's menu data

A mod block entity has two different kinds of storage: external item access uses
transactional Parts, while internal slots use the handle methods below; integer menu data is read with
`BlockEntityRef::data_slots` and changed on a server Host. Menu data is transient:
creation and loading both start with an empty data vector, even when the saved
payload still contains the machine's progress.

## Size the data vector

Call `entity.set_data_slots(host, initial_values)` with the complete initial
vector when preparing a newly created or loaded machine. For a progress display,
use one element initialized from the progress value in its saved payload. This is
the live-handle replacement for assigning a mutable tick payload's `data` field.
Do not reset the vector to its initial values on every tick.

The method accepts at most the game's mod-menu data-slot limit (currently 256).
It returns `Ok(true)` when it changes the vector, `Ok(false)` for identical values,
or `BlockEntityDataError::TooManySlots` with the submitted count and limit. It
never silently clamps a vector. `BlockEntityDataError::Unavailable` means the
handle cannot reach a writable mod holder in the server level currently in place.

## Update individual values

After sizing, call `entity.set_data_slot(host, index, value)` for scalar updates.
This method does not resize: an absent index returns false. Both methods use the
same write target, journal and `setChanged` path. Nested transactions retain their
ordinary behavior; an aborted outer transaction restores the vector that existed
before its first write, including writes committed by children. The normal menu
broadcast delivers committed changes to viewers.

The public handle still resolves a level and position. Removal makes it
unavailable; replacement at that position is followed, as with the other handle
methods. This is not an instance-token or borrowed-payload API.

## Internal items versus automation

For a configured mod container, `item_slot(host, slot)` snapshots an internal slot;
`set_item_slot(host, tx, slot, stack)` replaces it within an existing server
transaction. Any held mod block-entity handle can be used; there is no caller
ownership check. This deliberately bypasses inputs, outputs and face policy, so a
recipe can consume its input and produce its output atomically, including output
slots that automation cannot insert into. Use the `ITEMS` capability for external
insertion; its policy is unchanged. Vanilla containers are unavailable here.

Indices use the current container's declared count. Lazy slots read empty; saved
excess slots are neither exposed nor truncated. None clears, identical values
return `Ok(false)`, and real changes send Changed at root commit. Nested abort
restores ordinary before-images, including committed-child writes. Invalid
indices, unknown/air/zero-count stacks and counts above the registered or
component-overridden maximum return `BlockEntityItemError`, never a partial write
or silent count clamp. A positional handle follows a replacement and validates
its current container; this is distinct from an identity-strict automatic restore.

## Retaining a handle

A `BlockEntityRef` names one live entity, not every entity later placed at its position. Removal,
replacement and chunk reload invalidate it; reacquire with `host.block_entity(level, position)`.
In-place data edits and temporary tick storage preserve identity. Option reads return `None` for
stale handles; writes return their error's `Stale` variant, never writing to a replacement.
`set_lock`, `set_name`, `merge_data`, `replace_data`, `set_changed` and `set_data_slot`
return `Result<bool, BlockEntityError>`: handle `Err(BlockEntityError::Stale)` by dropping
or reacquiring the handle. `Ok(false)` means unchanged data or an unsupported operation,
not a stale identity. Existing item and data-vector methods retain their specialized errors.
