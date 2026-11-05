# Where each doc lives

The book follows [Diátaxis](https://diataxis.fr): a page is one of four kinds, and does only that job.

| Kind | Job | Lives in |
|---|---|---|
| Tutorial | Take a newcomer to a working result | Start |
| Explanation | Say how the pieces fit, with no steps | Concepts |
| How-to | One task, steps only | How-to, Injections |
| Reference | Facts, complete and dry | Reference, and the rustdoc of `horizon-mod-api` |

- **A hook, event or `Host` method:** its `///` lines in the declaration under `crates/horizon-mod-api/src/hooks/`.
  The generator writes it as `///` on the generated item; `cargo doc -p horizon-mod-api` shows it. A hook without
  a `doc` fails the build.
- **A hook whose game target has many parameters:** a `native_*` target takes at most 7 parameters (receiver included;
  `MAX_TARGET_PARAMETERS` in `crates/horizon-mod-api-gen/src/dispatch.rs`). A hook with more declares its parameters
  on the hook (`fn update_shape(dimension: LevelDimension, ...);`, as written in `hooks/*.rs`) and the target takes them
  as one generated `<Name>Call` (`native_update_shape(&self, ticks, call: UpdateShapeCall<'_>)`),
  handed to `dispatch_<name>(host, call)`. The generator refuses a flat target over the limit, and a target never carries
  `#[allow(clippy::too_many_arguments)]`. The hooks that were already over the limit are listed in
  `FLAT_TARGETS_OVER_THE_LIMIT` in the same file; that list only shrinks (convert a hook, delete its line) and a new hook
  is never added to it. The carrier is host-internal plain data, never on wasm, and no modder sees it.
- **A guide page:** `docs/modding/src/`. Code in a page is `{{#include}}`d from `docs/modding/examples`, which
  compiles, so a change to the API that breaks an example fails the build.
- **A mod API change** updates its fragment `doc` and any page it affects in the same PR.
- `src/llms.txt` is the index for agents; keep it in step with `SUMMARY.md`.

Build the book with `mdbook build docs/modding` and the reference with `cargo doc -p horizon-mod-api --no-deps`.

# What a PR needs to show

Code review, CI and your own focused tests are the proof. You don't need to run the game.

A reviewer runs your change in game at your head only when a test can't cover one of these, and says which:

- rendering or UI, what the player sees;
- client-server sync: LAN guests, a dedicated server, packets;
- when or in what order something happens compared with vanilla, such as a hook's firing point.

If your PR changes or removes something mods already use, the reviewer also runs
[horizon-machine-sample](https://github.com/arismoko/horizon-machine-sample) against it. A new item's `///` doc is its
reference.
