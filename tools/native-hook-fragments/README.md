# Native hook catalog fragments

A hook PR adds a uniquely named JSON file here instead of editing
`native-hooks.json`. Helper-only additions use an empty `hooks` array and list
new Host helpers in `helpers` as catalog entries. Example `example.json`:

```json
{
  "hooks": [
    {"name": "example", "target": "horizon_client::game::native_hooks::Game<'_>::native_example", "side": "server"}
  ]
}
```

Every hook and helper requires a `side`. A hook may also carry the catalog's optional
`neoforge`, `fabric`, `paper`, `cancellable` and `mod_api` keys; the collector copies them into
`native-hooks.json`. Counterpart keys take a string or list of strings and may be
omitted. New catalog contracts start at version 1; existing per-hook contract
versions are unchanged by collection. These versions describe individual
contracts, not minimum game versions.

Document the hook or helper in its source and docs in the same PR. The collector
does not read or write source, docs or Git history.

After hook PRs merge, run `python3 tools/stamp_native_hooks.py` on current main
and open a PR with the result. The collector uses sorted fragment paths, then
each fragment's declared hook/helper order, matching the build-time generator.
Duplicate names fail before the catalog is written or fragments are removed.
It consumes the collected fragments and updates only the catalog. `--check`
exits nonzero when any fragments are pending; running collection again does
nothing. For local testing, collect on a hook branch without committing the
result. If another collection PR merges first, regenerate from current main.
