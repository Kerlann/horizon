# Installing and using mods

Two kinds of mod run in Horizon:

| Kind | File | What it is | Where it runs |
|---|---|---|---|
| **Content mod** | `.hmod` | Data and assets: blocks, items, recipes, loot, tags, models, textures, structures. No code. | Loaded at startup; assets-and-data-only switches can reload live |
| **Native mod** | `.hmod` | A flat Rust project using the typed hook API, with optional raw injections. | In a game the launcher builds for the player ([native mods](../how-to/native-mods.md)) |

Fabric and NeoForge jars do **not** run as they are: their code calls and patches Minecraft's Java classes, and
Horizon has none. What carries over is the data and assets inside a jar ([modpacks.md](../../../modpacks.md)).

Mods are **off by default**. A fresh install has no `horizon-mods` folder and plays exactly like vanilla 26.3.

## Playing with mods

1. Install a mod from the launcher's Mods page ([mod-browser.md](../../../mod-browser.md)) or the game's Options > Mods >
   **Browse Mods** (content-only packages, checked against the same registry), or put its files by hand in the
   game folder (the one holding `options.txt`): a content mod's `.hmod` (or a folder holding `horizon-mod.toml`) in
   `horizon-mods/`. Native mods go in `horizon-native-mods/`, and the launcher builds them
   ([native mods](../how-to/native-mods.md)).
2. Options > **Mods** lists them with what each may do ("May add blocks, items, mob effects and entity types to your
   worlds") and a switch. They start off. The choice is kept in `horizon-mods.txt` (`mod.<id>:true`).
3. A content mod with only assets and reloadable data (textures, models, sounds, languages, recipes, tags, loot,
   advancements and functions) switches live: the game reloads its resources, then its world's data as `/reload`
   does, including for LAN guests. Content that adds registry entries or changes world generation still needs a
   restart, and its switch is frozen while a world is open. Packages with jar layers also wait for a restart,
   because those layers may contain world generation. On another server all content switches wait: the server
   owns its data. Code mods keep their startup content; pausing their dispatch is not a content unload.
4. A mod that can't load shows why, in red, on its row.

A live selection becomes applied only after its resource reload succeeds. A failed stage keeps the previous
resource selection and does not ask the integrated server to load the failed selection's data. The requested
switches remain saved, so a later reload can retry them. A switch made while another reload is running waits
for that reload and then uses the latest requested selection.

The game and launcher serialize package changes, profile removal and switch/key saves per game folder. An
unrelated game settings save merges only the player's changed switches and keys into the latest on-disk list;
it does not restore an old copy over launcher changes. An install that finishes while a world is open waits
until the world closes before the game enables its content. These locks coordinate Horizon processes, not
manual edits or another launcher's writes.

Two turned-on content mods never own the same namespace: the first by file name loads, and the other stays out with
the reason in the log.

Benchmarks (`--benchmark`) never load mods.

## Horizon's official mods

Horizon's own mods are native. They live in
[horizon-rust/horizon-mods](https://github.com/horizon-rust/horizon-mods) under `native/<name>/`:

| Mod | Folder | Replaces |
|---|---|---|
| FPS Counter | `native/fps-overlay` | Extras > FPS counter |
| Keystrokes | `native/keystrokes` | Extras > Keystrokes |
| Zoom | `native/zoom` | Extras > Zoom |
| Minimap | `native/minimap` | Extras > Minimap |
| Recipes | `native/recipes` | The Recipes mod (item panel and recipe pages, on hook points version 2) |
| Coordinates HUD | `native/coords-hud` | A small example mod (position and facing, `/where`, `/coords`) |

The game keeps the parts that must be fast, such as drawing the minimap's map, and a native mod picks the options
([extras.md](../../../extras.md)). Far View is not a mod any more: its distance and detail are Graphics > LOD Distance and Far
Detail ([graphics.md](../../../graphics.md)).
