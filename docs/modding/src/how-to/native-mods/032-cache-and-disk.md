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

Disk use varies with the platform, profile and set of mods. Run `horizon-launcher storage` to inspect the actual
cache rather than estimating its size from another computer's build.

The native builds an instance plays are kept, and the three newest no instance plays (`native_mods::prune`, by when
each was built or last used): switching a mod off and on again plays the kept build again without compiling.
`native/target/` stays for the next one.
