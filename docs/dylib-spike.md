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

Pending: inspect upstream/fork open PRs, issues, branches and recent main; read #5109 and #7386 comments before implementation. The fork lead reported no competing work on November 2; this spike independently checks it.

## Results

Pending implementation. No pass has been claimed.
