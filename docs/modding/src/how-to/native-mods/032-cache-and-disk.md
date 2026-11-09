<!-- chapter: Cache and disk -->
<!-- separator: 1 -->

## Cache and disk

The key hashes the commit, the horizon-mixins commit, Rust's version, the profile and features, and every file of
every bundle. The same set on the same commit is never built twice. A change gives a new build, and
`native/target/` stays, so a rebuild compiles only `horizon-client` and what the mods touched.

**Incremental rebuilds.** With Balanced and Developer, Cargo recompiles only the changed parts of `horizon-client`, not the whole
crate with LTO. Patched source is cached separately by its mixin inputs and pins (the three most
recent snapshots are kept), so editing only a trait crate skips the mixins pass. This holds because
the stage (`native/stage/`) is one folder for every build and the launcher writes
only the files whose content changed (Cargo goes by file times), switching mods on the same commit reuses the same
`native/target/`, and the launcher removes `CARGO_INCREMENTAL` and `CARGO_BUILD_INCREMENTAL` from the build's
environment. Optimized has no incremental compilation (the release profile's own).

**Build status.** Before it starts, the build identifies its plan and profile: up to date, incremental rebuild,
first build, or prebuilt game. While compiling it shows the crate and crates done of the total, from Cargo's own
progress. Afterwards it identifies the completed profile and plan. Neither the window nor `native-build` promises
or displays a build duration. `native/build.log` retains Cargo's diagnostic output, without the progress bar.

Native client builds share the prepared resources of their base version instead of copying them into every build. The launcher passes that base folder to the game as `--resources` and retains the base while an installed native build references it. Remove those builds before removing their base. Dedicated server exports remain standalone and include their own resources.

Disk use varies with the platform, profile and set of mods. Run `horizon-launcher storage` to inspect the actual
cache rather than estimating its size from another computer's build.

Instance references and newest-build retention still select which builds could be disposable, but do not prove
that no game can start using them. While launch routes remain uninstrumented, manual game-cache cleanup,
installed-version removal and automatic runtime eviction retain those files, even after a process scan reports no
game. Rust/Cargo/temp build-tool caches remain clearable with their existing Cargo locks. `native/target/` stays
for the next build rather than being evicted without exclusion.

Private launch records and raw stdout/stderr capture files are collected as one group only after every protocol
owner exits, under an exclusive namespace gate. Malformed records and unknown files remain untouched. The
permanent gate is never deleted; recognized empty registry lock files are collected at the 4,096-entry threshold.
