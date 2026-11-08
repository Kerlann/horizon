# Drawing resolved models

Register standalone models during `standalone_models`, then resolve their `ModelRef`
through `RenderAssets::model` after resources are published. World render hooks receive
`WorldDrawContext`: submit transient copies with `draw_model_instances`, or retain
copies in a group. GUI scenes, mobs and kept/spinning renderers use their existing
CPU routes; groups are a world-drawing facility, not another renderer.

## Persistent groups

- `host.create_group(model, capacity)` reserves up to 16,384 copies of a solid model.
  Initial drawn length is zero. Translucent models are refused.
- `group.write(host, first, copies)` atomically overwrites a range and extends drawn
  length if needed. Unwritten slots start as white, full-bright identity copies at
  world origin. Invalid coordinates or light, or a range beyond capacity, change nothing.
- `group.set_len(host, len)` changes drawn length without changing reserved capacity.
  Shrinking to zero draws nothing; growing restores retained copies, including hidden
  slots written earlier.
- `group.set_group_transform(host, transform)` sets an identity-by-default row-major
  3×4 affine pose. For each local vertex the composition is exactly
  `world = origin + group_transform * (copy_transform * local)`.
  The world-space f64 origin is not rotated or scaled. Non-finite pose values change nothing.
- `draw.draw_instance_group(group)` draws the live length once per view. Repeated
  submissions in that view are ignored. Culling uses the composed pose.
- `group.destroy(host)` retires the handle and releases capacity and CPU caches.
  Copies already accepted for a view remain drawn; subsequent calls return `Stale`.

Each mod may reserve 65,536 copies across all its groups; a zero-capacity group
reserves one slot. Destroying a group releases that reservation immediately. A view
accepts 16,384 visible copies shared across transient batches, groups and listeners,
and at most 16,384 group submissions. Culled copies do not consume the visible-copy
budget. Refusal is atomic: no prefix of a refused submission draws.

Groups cache converted records on the CPU. Unchanged groups avoid pose, normal and
tint conversion, but still cull and stage visible copies each frame. Every visible
GPU-routed copy uploads each frame; groups do not retain a private GPU buffer.
Frequent whole-group pose changes rebuild every live record and can cost more than
uncached transient copies. Shader-pack selection, missing prepared GPU records and
overlays use the same CPU baking path as transient batches, including tint and light.
There is no simulation-tick work.

Only the creating mod can access a group. A foreign handle returns `GroupError::Stale`,
as does a destroyed group, a group after resource reload or world close, or a group
whose mod was disabled. Re-enabling starts without groups: recreate them from the
newly published model handles. Slot reuse never revives an old handle. Native and
wasm use the same ownership, budgets, lifetime and rendering implementation.
