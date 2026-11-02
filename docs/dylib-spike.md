# Rust dylib spike (#26 stage 3)

This is an experiment, not a merge recommendation. Linux first. The pass mark below is copied verbatim from [fork issue #26](https://github.com/arismoko/horizon/issues/26), November 2, 2026, before implementation. A failure or unmeasured item stays visible.

## The spike's pass mark

1. **Same results.** On the dedicated server, a world with no mods, from a fixed seed, driven by the same command script for 6000 ticks, ends with the same blocks, entities and player data in today's modded build and in the `dylib` build.
2. **Tick time within 1%.** With horizon-machine-sample loaded, the median mean ms per tick over ticks 1000 to 6000, across three runs, is within 1% of today's static `profile.fast` build.
3. **It works.** `zoom` and horizon-machine-sample load from their `dylib`s with no game rebuild, on the server thread and on a dedicated server. A panic in either names the mod and the hook.
4. **Fast mod rebuilds.** Changing one line in horizon-machine-sample and getting back into the world takes under 30 seconds, against about 3 minutes today.
5. **Windows builds** under the 65,535-export limit.
6. **Mismatches are refused** before the build, naming the crate, toolchain or game build.
7. **A mod builds against a downloaded game build**, not only one built locally. This is the step toward shipping the game prebuilt.

## Measurement plan (written before code)

This section preserves the pre-code plan, including planned CLI spellings. The implemented helper commands and final outcomes are below; the initial-status column is historical, not the current result.

All build commands use `nice -n 19`, Cargo `-j 6`, `profile.fast`, this checkout's `target/`, the pinned rustc and Cargo.lock. `tools/dylib_spike.py` will drive reproducible builds/runs; raw logs and generated artifacts go in `target/dylib-spike/`, never committed. Commands below define its intended subcommands; implemented spellings and exact invocations will be recorded with results.

| Point | Command / measurement | Result destination | Initial status |
| --- | --- | --- | --- |
| 1 | `python3 tools/dylib_spike.py parity --ticks 6000 --seed 2603`: static and dynamic dedicated servers, identical command script; normalize time/session metadata and compare saved blocks, entities and player NBT. Record command script and normalization, not just a hash. | Results below; raw outputs in `target/dylib-spike/parity/` | Not measured: implementation pending |
| 2 | `python3 tools/dylib_spike.py ticks --runs 3 --warmup 1000 --ticks 6000`: fixed seed, machine sample loaded in static and dynamic `fast`; report each run's mean tick duration and ratio of medians, excluding startup and sleep. | Results below; samples in `target/dylib-spike/ticks/` | Not measured: implementation pending |
| 3 | `python3 tools/dylib_spike.py build-game`, `build-mod <sample>`, `build-mod <zoom>`, then `run` on integrated server thread and `run --dedicated`. Check unchanged game artifact hashes/mtime, both mod hooks and render registration, and separate throwaway panic runs with mod/hook attribution. Post a screenshot of the running sample. | Results below; build/runtime logs and screenshot in `target/dylib-spike/works/` | Not measured: implementation pending |
| 4 | `python3 tools/dylib_spike.py rebuild-time <sample>`: one-line source change, monotonic timer before mod build until world-ready/hook evidence on restart. Restore source afterward. | Results below; timestamps in `target/dylib-spike/rebuild/` | Not measured: implementation pending |
| 5 | `rustup target list --installed`; cross-build `--target x86_64-pc-windows-gnu` if available and count PE exports. Otherwise `nm -D --defined-only target/fast/libhorizon_client.so` (export symbols only), and explicitly state Windows was not built. | Results below; toolchain/build logs and export counts in `target/dylib-spike/exports/` | Not measured: implementation pending |
| 6 | Build driver preflight against manifests altered independently for rustc, profile, Cargo.lock/shared crate identity and game build; command `python3 tools/dylib_spike.py mismatch-test`. Check diagnostic names mismatch and no rustc/mod entry executes. | Results below; diagnostics in `target/dylib-spike/mismatch/` | Not measured: implementation pending |
| 7 | Copy built game artifacts and identity manifest to an independent directory, build sample only with `rustc --extern` and `-L dependency=` (or copied Cargo fingerprints, if demonstrated), load in copied game; command `python3 tools/dylib_spike.py relocated-test <sample>`. No dependency on original target path allowed. This simulates a downloaded build; it is not a cross-host claim. | Results below; build/runtime logs in `target/dylib-spike/relocated/` | Not measured: implementation pending |

## Prior work check

Checked November 2 before implementation: upstream and fork open-PR searches for `dylib`, `plugin`, dynamic/runtime loading found no competing implementation; issue searches found the proposal horizon-rust/horizon#5109. Read that proposal and horizon-rust/horizon#7386 plus their discussion: the later proposal explicitly replaced the earlier C-table/cdylib design with Rust dylibs, launcher builds, shared crates and no unload. `git ls-remote --heads` on both repositories found no plugin/dylib branch (apart from this spike); the only `dynamic` branch was unrelated dynamic-light screenshots. Upstream main was `68ff469984`, fork baseline `0e28b46b69`; recent main commits concerned cosmetics, shaders and parity, not this loader.

The launcher's existing `launcher/src/native_mods/{api,crates,dependencies,inject}.rs` remains authoritative for source staging, crate policy, dependency resolution and mixin injection. This spike reuses `entry::main_with`, `mod_api::name_mods`, `Hooks::render` and all existing dispatch/panic guards unchanged. The folder's `order` file is already-resolved dependency order, **not** a new resolver or filename sort. Both test mods have no mod dependencies; dependency graph integration is deferred to stage 4. The normal build driver refuses dependencies beyond the game's `horizon-client` instead of compiling a conflicting shared crate. A separate private-crate experiment below demonstrates linking a mod-private dependency absent from the game lock; general launcher dependency policy remains out of scope.

`cargo rustc --lib --crate-type dylib -C prefer-dynamic` builds the game once. Subsequent mod builds invoke **rustc directly**, with `--extern horizon_client=...so` and `-L dependency=.../deps`. This avoids a second Cargo graph and feature/fingerprint mismatch. The artifact snapshot includes matching metadata, rlibs, Cargo.lock and an identity manifest; it is deliberately larger than a production SDK. The allocator is owned by the shared gameplay library under the spike feature; std is the toolchain's shared dylib. No C ABI or SDK facade is introduced.

Sample: https://github.com/arismoko/horizon-machine-sample at `cea9005be69f9c71fd3dba53ce5939d28ab06462` (MIT), `examples/trait-api/machine-hooks`. Its source is unchanged in normal runs; a wrapper only exports `hooks()`. This is the existing four-hook trait example, **not** the sample's mixin-based machine GUI port.

Zoom: https://github.com/horizon-rust/horizon-mods at `e5d840dabf3f8d21d6f15b793cd1fb83ed79db17` (MIT). `tools/dylib_spike/zoom.rs` is thin adapted hook glue around its unchanged runtime. Current `Host` lacks `native_playing`; the adapter tests only in-world keys and is **not** a complete parity port for key handling on menus. The original runtime and source are fetched into ignored build output; no assets are copied.

## Results

Measured on Linux x86_64, rustc 1.99.1, November 2, 2026. **Not an overall acceptance pass:** point 1 passes the settled-fixture method below, not bit-identical entity UUIDs; Windows was not measured. Raw logs, worlds, timings and JSON reports are retained under `target/dylib-spike/`; the durable results are recorded here. No game data or downloaded mod source is committed.

| Point | Outcome | Evidence / limit |
| --- | --- | --- |
| 1 | **PASS with a stated settling method** | With a 2 s frozen startup-settling delay, static/dylib match in blocks over 6000 ticks on **two pairs**; both have 32 random-UUID entity differences, also present in static-v-static. Block entities/default player records match. The original no-delay fixture has 15 additional blocks and is startup-timing-sensitive; identical binaries can differ between runs at tick 1533. Not a bit-identical save claim. |
| 2 | **PASS, measured +0.726%** | Median of three run means: static 0.1213035474 ms, dynamic 0.1221841934 ms; 5000 tick samples per run. Shared-host noise is substantial. |
| 3 | **PASS within the trait-example scope** | Both separate dylibs load without game rebuild. Integrated server clock/recipe tooltip and Zoom key/4x/reset work; dedicated sample server hook runs, both modules load. Panics retain machine_sample/level_tick_pre and zoom/key attribution. Not the full mixin-based machine sample or a complete Zoom menu port. |
| 4 | **MEASURED, below 30 seconds** | One-line sample edit: build+preflight 5.876688 s, joined world-ready 15.724876 s, edited server hook 25.133767 s. No engine rebuild. |
| 5 | **NOT MEASURED** | Windows target installation failed; no MinGW compiler available. Linux has 37,281 defined dynamic exports, but that is not a Windows export count. A Windows host is needed. |
| 6 | **PASS for this driver's identity fence** | Real and unit-test toolchain/profile/lock/artifact mismatches refused before rustc; runtime mismatch refused before opening a nonexistent mod library. This is not the full launcher crate-policy implementation. |
| 7 | **PASS for relocated-build simulation** | Copied SDK builds/loads/runs the sample while both original SDK and original `target/fast` are renamed unavailable. Same host and toolchain; not a real downloaded release or cross-host compatibility claim. |

### Build and run reproduction

Run `sh /box/shared/horizon-setup.sh` for locally supplied vanilla resources in this room. Outside the room, prepare the player's own resources normally; do not redistribute them. Use the pinned toolchain on PATH. Source checkouts below are external inputs that were read before execution.

```sh
mkdir -p target/dylib-spike/sources
# Machine sample trait-api-example at cea9005be69f9c71fd3dba53ce5939d28ab06462
# Official horizon-mods at e5d840dabf3f8d21d6f15b793cd1fb83ed79db17
# Place those read/reviewed checkouts at sources/machine and sources/official.
python3 tools/dylib_spike.py build-game
python3 tools/dylib_spike.py build-mod machine_sample \
  target/dylib-spike/sources/machine/examples/trait-api/machine-hooks
python3 tools/dylib_spike.py build-mod zoom \
  target/dylib-spike/sources/official/native/zoom --zoom
printf 'machine_sample\nzoom\n' > target/dylib-spike/mods/order
# Set scratch game/launcher/XDG homes before any interactive run (see AGENTS.md).
HORIZON_DYLIB_MODS=$PWD/target/dylib-spike/mods \
  python3 tools/dylib_spike.py run --resources "$PWD/.local/horizon-client/26.3"
```

`build-game` invokes `cargo rustc -j 6 --profile fast -p horizon-client --no-default-features --features dylib-spike --lib --crate-type dylib`, with `RUSTFLAGS='-C prefer-dynamic'` and low priority. `build-mod` compiles a single-file trait package, not arbitrary Cargo packages/build scripts. `order` is launcher-resolved order supplied by the caller. All modules and their initializers are fully trusted native code; the manifest fence is a compatibility check, not a security boundary. Rust ABI is tied to the exact compiler and crate artifacts; no unload or hot reload is attempted.

Final shared game SHA256: `ed548f04ae8afa5fa9f23a3f5e24fab0cc5b0e1aff0d566ecfc7d4a69dd4ccdd`. The shared library is approximately 138 MiB and loader 105 KiB. The SDK intentionally includes more artifacts than a production distribution needs. The toolchain's shared std remains required.

### 1. Save comparison and bounded divergence investigation

Actual driver: `python3 tools/dylib_spike/dedicated.py --label dynamic-parity`; add `--binary target/dylib-spike/static-target/fast/horizon-client` for the static comparator. Build that comparator with `CARGO_TARGET_DIR=$PWD/target/dylib-spike/static-target nice -n 19 cargo build -j 6 --profile fast -p horizon-client --no-default-features`. Compare the two saved `Spike` folders with `python3 tools/dylib_spike/compare.py STATIC_WORLD DYNAMIC_WORLD`.

Seed **2603**, no loaded mods, one forced chunk, view/simulation distance 2. Freeze before simulation; fill a 16x16 stone platform at Y=80, place a chest with 32 cobblestone, summon a NoAI persistent pig with fixed UUID, set time 0 and clear weather; sprint to observed world times exactly 1000 then 6000, save and stop. Commands are in `dedicated.py` and each run's `commands.txt`. The console sprint route advances one extra running tick in **both Horizon and vanilla 26.3**, so requested counts are 999 and 4999; no clock change was made for the experiment.

The comparator decodes palettes using the existing save-roundtrip helpers and compares full block-state names/properties, block-entity NBT, entity NBT by UUID and player NBT. It ignores no gameplay data. Both runs have 81 chunk records, 17 entity records and one default player record. No real player joins this fixture: default player NBT equality is not populated-player inventory parity. Block-entity and default player records match.

- Original static vs dylib: **47 differences**: 15 vine/mushroom block differences in chunk (0,0), plus 32 unmatched random entity-identity keys. Retained reports: `parity-result.json`, original `static-parity`/`dynamic-parity` logs.
- Static vs repeated static: **32 entity-identity differences**, no blocks (`static-control-result.json`).
- Dylib vs repeated dylib: **32 entity-identity differences**, no blocks (`dynamic-control-result.json`). Thus repeated dylib **block** determinism passes this fixture, but exact complete saves do not.
- Tick-zero static vs dylib: no block differences; two random-identity differences (`zero-result.json`). The 15 additional differences arise during simulation, not terrain generation.
- Read-only `trace.rs`, built against each mode's exact game library, logs every actual random-tick hook's position, block-state ID and the level RNG's state without drawing from it. It also observes the fifteen differing block positions after every running tick. **6000 end-of-tick observations and 1635 random-tick records per mode match exactly**. This observer's static build matches the original dylib's blocks, not the original static executable's. The observer changes the build/workload, so this does not establish a compiler defect or explain the original divergence.

Both Cargo fingerprints use the same fast profile hash: optimized release inheritance, **LTO off, 16 codegen units, incremental on**. Neither sets `target-cpu=native` or any explicit target CPU. Static features are `[]`, dynamic features `["dylib-spike"]`; static rustflags `[]`, dynamic rustflags `[-C, prefer-dynamic]`. The feature only moves allocator ownership to the shared gameplay library. The static observer uses the **same prebuilt static gameplay rlib**, `opt-level=3` and 16 codegen units. No fat/thin LTO mismatch was found.

Read `game.rs::tick_blocks`, `level/tick.rs::{tick_chunk_random_masked,mushroom_random_tick,vine_random_tick}`, `level/ticks.rs::block_random_pos`, `random_tick_sections.rs`, `chunk_map.rs` and the Fx hasher. Chunk order is map order over deterministic Fx integer-coordinate keys, not pointer hashes or `RandomState`; section order is ascending Y. Vine/mushroom local loops use fixed integer ranges/direction arrays, with no unstable sort, uninitialized-byte read or unsafe block found there. The random-section cache does compare Arc addresses, but retains Weak references specifically to prevent address reuse; no stale-cache defect was demonstrated. An exact causal RNG-draw or iteration-order difference in the original uninstrumented executable is **not established**. Do not normalize away the 15 blocks.

The bounded checkpoint search found equal blocks at 1532 but a vine difference plus 15 lava differences at 1533. **Repeating the same binaries at 1533 produced zero block differences**, so bisection across fresh runs cannot establish the first divergent tick: the fixture itself is timing-sensitive. Adding `--settle-seconds 2` pauses while frozen after the forced chunk first arrives and before the script. At 6000 ticks, **two independent static/dylib pairs each show zero block or block-entity/player differences and 32 random-UUID entity differences** (`startup-result.json`). The original fixture starts after just the central chunk arrives, not after a complete streaming/scheduling barrier. The delay suppresses its observed differences without changing either binary. This supports a startup/streaming-scheduling sensitivity, not a proved compiler or dylib defect; no compiler or upstream parity issue is filed.

Reproduce each settled pair with `dedicated.py --label static-settled --binary target/dylib-spike/static-target/fast/horizon-client --settle-seconds 2` and `dedicated.py --label dynamic-settled --settle-seconds 2`, then `compare.py` on their saved worlds. This is the point-1 PASS method. The fixed delay is not a production readiness barrier, and the original no-delay failure remains recorded above. Investigation stopped at this narrowed boundary rather than altering random ticks or hiding the data.

### 2. Tick performance

`python3 tools/dylib_spike/static_sample.py target/dylib-spike/sources/machine/examples/trait-api/machine-hooks` builds the existing trait sample into a static driver. `python3 tools/dylib_spike/bench.py` alternates static/dynamic runs three times using `dedicated.py`; both use the identical optional `HORIZON_DYLIB_SPIKE_TIMINGS` instrumentation around the entire authoritative `tick_server` call, excluding sleeps, startup, console polling and the timing print itself. Select world times **1001 through 6000**, 5000 samples per run after 1000 warmup ticks.

| Run | Static mean ms | Dynamic mean ms |
| --- | ---: | ---: |
| 1 | 0.1213035474 | 0.1246111436 |
| 2 | 0.1011712978 | 0.1221841934 |
| 3 | 0.1344175356 | 0.1159564886 |
| Median of means | **0.1213035474** | **0.1221841934** |

Dynamic/static = **1.0072598536**, or **+0.725985%**, meeting the 1% criterion for this run. Individual run variance is much larger than 1%; this single shared-host experiment is not a reliable general performance bound. Raw samples and means: `benchmark-result.json` and six `bench-*` runs.

### 3. Modules, panic attribution and mod-private dependency

Unchanged sample trait package compile: **1.535574 s**; Zoom adapter: **0.667875 s** (compile-only, not complete edit/restart). Both modules load separately without rebuilding the game. Real integrated-server proof: sample recipe tooltip visible in `works/sample-tooltip.png` (posted to the room), clock lines on the Server thread, and Zoom key hook reporting 4x/reset on release. Dedicated sample clock and both module-load entries also observed. The loader forwards hooks to `entry::main_with`; render registration and dispatch are unchanged. These two trait examples do not provide custom geometry, so this is not a proof of arbitrary custom-renderer behavior.

Throwaway `panic.py` runs show **`in native mod machine_sample, hook level_tick_pre`** on both the integrated server and dedicated server; the dedicated crash report is `panic-sample-2i1zn5pt/crash-reports/crash-2026-11-02_21.08.39-server.txt`. Zoom's client key panic reports **`in native mod zoom, hook key`**, `panic-client-slu53tyi/home/crash-reports/crash-2026-11-02_21.11.42-client.txt`. Zoom has no dedicated key hook; merely loading it there is not claimed as a Zoom server behavior test.

`python3 tools/dylib_spike/private.py` builds `spike_private` as an rlib, verifies its name is absent from the game Cargo.lock, then passes it through `--extern spike_private=...rlib` when building a sample mod dylib. Its exported private function is present in that dylib, no separate private DSO appears in `DT_NEEDED`, and the function plus the sample's server hook execute in the real dedicated game. The game lock hash is unchanged (`private-result.json`). This proves a private crate need not be in the game's lock; it does not implement Cargo resolution or a production shared/private dependency policy.

### 4. One-line rebuild

`python3 tools/dylib_spike/rebuild.py target/dylib-spike/sources/machine/examples/trait-api/machine-hooks --run-dir "$(mktemp -d "$PWD/target/dylib-spike/rebuild-XXXXXX")"` (with Xvfb :77 running) temporarily changes `CLOCK_INTERVAL: 200 → 201`, restores source afterward, builds only the sample and restarts the integrated game. Build plus identity preflight **5.876688 s**; full world-ready with a **joined, non-null player** **15.724876 s**; edited server clock hook **25.133767 s**. The game hash is unchanged. An earlier attempt accepted `player=null`; it was discarded. This measurement predates the later timing instrumentation rebuild, so compare hashes within the measured rebuild run, not against the final timing-enabled artifact (`rebuild-result.json`).

### 5. Windows / exports

A bounded Linux cross-target attempt could not install `x86_64-pc-windows-gnu`. With explicit `RUSTUP_HOME=/opt/rustup` and toolchain 1.99.1, rustup failed while preparing cached downloads: **`No such file or directory (os error 2)`**. There is no MinGW compiler in the box. Therefore Windows **was not built** and the 65,535-export criterion is **not measured**, not passed. Final Linux `nm -D --defined-only` count: **37,281** (`exports.txt`); ELF exports cannot establish PE success. Needs a Windows host/toolchain before acceptance.

### 6. Refused mismatches

`python3 -m unittest discover -s tests -p test_dylib_spike.py`: **five tests pass**. `python3 tools/dylib_spike/mismatches.py` additionally alters the actual SDK expectation for `toolchain`, `profile`, `lock`, and `artifacts`: all four are refused before compilation with the field named. The loader's wrong-identity fixture contains no `.so`, yet reports **crate/toolchain/profile/game build mismatch**, not a library-open error: refusal precedes dlopen and its initializers. Exact artifact hashes include the shared gameplay library and matching dependency metadata. Rust ABI compatibility beyond these exact artifacts is not promised.

### 7. Copied SDK / downloaded-build simulation

`python3 tools/dylib_spike/relocated.py` copies the SDK and loader, temporarily renames **both** the original SDK and original `target/fast` unavailable, compiles the sample using only copied artifact paths, then loads it in the copied game. Dedicated server hook runs through 6000 ticks, with no game rebuild (`relocated-result.json`: all four checks true). Paths are restored in a `finally` block. Do not run this helper concurrently with builds: it temporarily renames `target/fast`. This demonstrates the artifact mechanism needed for downloaded builds on this same host/compiler; it does not test release packaging, actual HTTP downloads, another OS or an unavailable toolchain.
