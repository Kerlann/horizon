<!-- chapter: Dimension shape -->
<!-- separator: 1 -->

### Dimension keys

`LevelRef::dimension` returns a `DimensionKey`, a small `Copy` value that names the dimension
for as long as the game runs, custom dimensions included. Compare it with another key or with
`DimensionKey::OVERWORLD`, `NETHER` and `END`; `level.dimension().id(host)` gives the name
(`minecraft:the_nether`). On a client, `host.level(name)` gives the level it shows under the
name the server calls it, whether or not that is a vanilla dimension.

### Dimension shape

`LevelRef::dimension_shape` reads the loaded server level's build range, logical height,
skylight and ceiling from its dimension shape. It accepts `&dyn Host`, including when the
level is parked, and returns `None` for a stale or unavailable level or on a client. It does
not load a level or allocate.

### Dimension type

`level.dimension_type(host)` reads a level's dimension type (`Level.dimensionType()` and its
gameplay attributes) as a `LevelDimensionType`: the build range, `coordinate_scale`, `ambient_light`,
`has_fixed_time`, `infiniburn`, the monster spawn light range, the sky and the gameplay switches. A
level on a vanilla type returns that type's values, a registered dimension returns what its
`DimensionSettings` set, and a data pack's dimension type counts the same way. It returns `None` for
a stale handle, and on a client it reads the level that window shows; a client of another server
reads the Overworld's values for a level that server's mods added.

### Registering a dimension

`register_dimension` gives every world a level of the mod's own. Call it once, at startup, in
`Hooks::register`:

```rust,ignore
#[mod_api::horizon_mod]
impl Hooks for MyMod {
    fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
        r.register_dimension(
            "pocket",
            DimensionSettings { min_y: -128, height: 448, logical_height: 448, ..Default::default() },
        );
    }
}
```

The level is `mymod:pocket`. `DimensionSettings` sets the build height, `has_skylight`,
`has_ceiling`, the `generator` (`minecraft:noise` by default), its `noise_settings` and the level's
one `biome`; the default is the Overworld's shape and terrain over plains. It also sets the rest of
the dimension type the game reads: `has_fixed_time`, `has_ender_dragon_fight`, `coordinate_scale`,
`ambient_light`, `infiniburn`, the monster spawn light range and block light limit, the `skybox`,
the `cardinal_light` and the gameplay switches (`water_evaporates`, `fast_lava`,
`respawn_anchor_works`, `nether_portal_spawns_piglin`, `piglins_zombify`, `snow_golem_melts`,
`can_start_raid`, `sky_light_level`). A field left at its default is the Overworld's, and the game
refuses a value out of range (a `coordinate_scale` of 0, an `ambient_light` above 1, an unknown
`skybox`) at startup with a message naming the mod. The level opens, ticks, saves and closes as the
Nether and the End do, in `dimensions/mymod/pocket/` of the world, and clients receive its
dimension type when they join. A client of a server takes the whole type, not only the build
range: the level's fluids, mobs, beds and time follow the type the server sent, as on the host,
whether or not the client has the mod that registered the level.

`sky_color`, `fog_color` (`#rrggbb`) and `cloud_height` set the sky overhead, the fog at the
horizon and the clouds' height; left as None they are the Overworld's. They apply to the host's own
world: a client of another server that has the mod's dimension shows the Overworld's sky there.

The level's biomes are the one `biome` everywhere unless `biome_source` says otherwise:
`BiomeSourceKind::Checkerboard` alternates the `biomes` in cells `2^biome_scale` chunks wide,
`MultiNoise` uses the Overworld's or the Nether's biomes (`biome_preset`) and `End` the End's islands.
Only `Fixed` suits a chunk generator of the mod's own, which writes terrain only.

A dimension in another mod's namespace, an empty name, a name that is not a folder of the world
(lowercase letters, digits and `_ . -` only, with `/` between segments that are not empty, `.` or
`..`), a vanilla level's id, an id registered twice, a build height the dimension type codec refuses
(`min_y` and `height` are multiples of 16, and the level stays between -2032 and 2031) and a
generator nobody registered stop the game at startup with a message naming the mod. The generator
may come from a mod that loads later.

A registered dimension is the level stem and dimension type a data pack would give, below every
data pack: a pack with `data/mymod/dimension/pocket.json` or `data/mymod/dimension_type/pocket.json`
replaces that half of it, in the worlds that have the pack.

`register_dimension` is the same call in a mod built to `mod.wasm`, as the settings are plain
data. A wasm mod's dimension can name only the game's own generator, `minecraft:noise`: a chunk
generator is code, and `register_chunk_generator` is native only.

### Sending a player to a dimension

There is no portal to a registered level. A mod sends an entity there with
`entity.teleport_to_level(host, dimension, pos, rotation)` from a server hook, the call vanilla has
as `Entity.teleportTo(ServerLevel, ..)`. It works for a player, a mob, a boat and a minecart (a
vehicle takes its passengers along).

```rust,ignore
fn run_mod_command(&self, host: &mut dyn ServerHost, event: &mut RunModCommandEvent<'_>) -> Flow {
    let CommandSource::Player { player, .. } = event.source else {
        return Flow::Continue;
    };
    let entity = player.entity();
    entity.teleport_to_level(host, "mymod:pocket", [8.5, 100.0, 8.5], None);
    Flow::Cancel
}
```

`pos` is in the destination's own coordinates: nothing scales it by the level's
`coordinate_scale`. A level nobody has entered opens first, and a player arrives after the current
tick, when `player_teleported` and `player_changed_dimension` report the move. The call
answers whether the move was taken, not whether the player arrived: a level that won't open, such
as one naming a generator nobody registered, logs an error and leaves the player where they were.
It is false for an unregistered dimension and for an entity that cannot change level (an item, an
orb, a projectile); a fake player stays where it is. Nothing builds a platform at the destination, so look for the
ground first, or place one.

### Saving a level

`level.request_save(host)` asks the server to save a level, as the autosave and `/save-all` do, with its
chunks on disk when it ends. The save runs at the start of the next server tick, not inside the call (a hook
may be running in the tick that is saving), and a level asked for twice before then is saved once. The call
answers whether the request was taken. `level_save` fires with the level's dimension once it was written; a
save that fails fires nothing and is logged.

### Chunk generators


A chunk generator is a mod's code for a level's terrain: vanilla's `ChunkGenerator`, and
`build_terrain` its `buildTerrain`. Register it once, at startup, and name it from a dimension,
or from a data pack's level stem as `"generator": {"type": "mymod:void"}`:

```rust,ignore
struct Floor;

impl ChunkGenerator for Floor {
    fn build_terrain(&self, chunk: &mut dyn ChunkAccess) {
        let stone = chunk.state("stone");
        let [x, z] = [chunk.pos().x * 16, chunk.pos().z * 16];
        for (dx, dz) in (0..16).flat_map(|dx| (0..16).map(move |dz| (dx, dz))) {
            chunk.set_block([x + dx, chunk.min_y(), z + dz], stone);
        }
    }
}

#[mod_api::horizon_mod]
impl Hooks for MyMod {
    fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
        r.register_chunk_generator("void", Floor);
        r.register_dimension(
            "void",
            DimensionSettings { generator: "mymod:void".into(), ..Default::default() },
        );
    }
}
```

There is no `Host` in world generation: chunks fill on the worker threads, so a generator sees only
the `ChunkAccess` of the chunk it fills, is `Send + Sync`, and keeps no mutable state, because two
workers can fill neighbouring chunks at once. The same seed and chunk give the same blocks.
`ChunkAccess` has the chunk's `pos`, the world's `seed`, the level's `min_y` and `height`, `state`
and `state_with` to resolve a block by name into a `TerrainState` (the handle `set_block` takes),
and `set_block`, which skips a position outside the chunk or the build height and answers whether
it wrote. The chunk starts as air.

The generator gives terrain only. The level has no surface, carvers or features, and every chunk
has the stem's one biome. `noise_settings` means nothing to it.

A generator in another mod's namespace, an empty name, a name that is not a folder of the world (as
for a dimension), an id registered twice and `minecraft:noise` stop the game at startup with a
message naming the mod. It is native only: a mod built to `mod.wasm` that calls
`register_chunk_generator` fails when it registers, with a message saying why.

A level stem naming a generator nobody registered, say after its mod was removed, is an error for
whoever opens that level, not for the world: a teleport there fails and is logged, and a player
saved there joins a server in the Overworld. Single player refuses to open a world whose player is
saved in such a level.
