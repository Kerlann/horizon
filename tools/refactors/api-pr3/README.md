# API 3 integration replay and gates

Part of #1608. These tools support the integration PR, not a replacement for its
handwritten semantic conversion layers. #1769 is an input, not an encoded patch.
No generated game/API output is committed. Nothing here runs workspace CI.

## Commands

From a clean checkout at the **pre-replay integration input commit**:

```sh
python3 tools/refactors/api-pr3/replay.py --root . --plan review-plan.json --log target/api3/rewrite-log.json
```

This applies reviewed exact rewrites and the Text export/import cut. Every exact
rewrite has a required occurrence count. Run from the immutable input commit
again to reproduce; arbitrary exact rewrites deliberately fail if already applied.
The built-in Text cut itself is idempotent. Never hand-edit replay output.

For compiler-guided work, capture cargo JSON diagnostics in CI or a deliberate
compiler invocation permitted by the room's local-check rules. Seed definition
renames in the reviewed plan, then run:

```sh
python3 tools/refactors/api-pr3/compiler_rewrite.py diagnostics.jsonl names.json --root . --log target/api3/compiler-edits.jsonl --apply
```

`names.json` is an explicit old-token to new-token map. Only primary error spans
whose current bytes exactly equal a complete old token/path are edited. This
reuses PR 2c's span approach, with path containment, overlap checks, protected
files and before/after hashes. It does not apply arbitrary compiler suggestions.
It reports unresolved diagnostics rather than guessing semantic adapters. Recheck
and repeat until compilation is clean. Review the retained edits, then declare
their exact source rewrites in the plan. The log alone never grants an exception.

Generate each checkout's WIT and full host manifest with **that checkout's own**
`api-layout ROOT OUT` and `host-manifest` binaries from `horizon-mod-api-gen`.
Include `OUT/mod_api_generated_host.rs`, `OUT/mod_api_host_areas.rs`, and all
`crates/horizon-mod-api/src/host/*.rs` when producing the manifest. These are the
same generation entry points used by the existing PR 3a proof. Do not substitute
one checkout's generator for the other. Then the single closeout command is:

```sh
python3 tools/refactors/api-pr3/gate.py --root . --base origin/main \
  --plan review-plan.json \
  --before-host evidence/base-host.txt --after-host evidence/head-host.txt \
  --before-wit evidence/base/mod-api.wit --after-wit evidence/head/mod-api.wit \
  --output target/api3/gate.json
```

Exit 0 and `API 3 PASS` mean all three gates passed; otherwise exit 1 and
`API 3 FAIL`. The JSON report lists every failing row/file and source accounting.
Missing/malformed evidence is an error, never a pass. Pin `--base` to main's exact
commit when retaining final evidence so a later main fetch cannot move it.

## Reviewed plan format

Start with `{}`: it allows **no** API delta except built-in Text source rewrites.
The final integration author fills the plan from the intended changes; independent
reviewers read it. Do not auto-bless current output into an exceptions file.

```json
{
  "host_substitutions": {"ParticleType": "ParticleKey"},
  "host_exceptions": {},
  "wit_substitutions": {"particle-type": "particle-key"},
  "wit_exceptions": {},
  "source": {
    "crates/example/src/file.rs": {
      "rewrites": [{"old": "old exact source", "new": "new exact source", "count": 1}]
    },
    "crates/example/src/semantic.rs": {
      "handwritten": {"before_sha256": "EXACT BASE FILE HASH", "after_sha256": "EXACT FINAL FILE HASH"}
    }
  }
}
```

- Host parsing reuses PR 3a's manifest parser, but unlike that historical gate
  **signatures are checked**, not ignored. Substitutions are simultaneous and
  token-bounded. Names, owners, docs and hidden flags remain checked.
- Exact exceptions map each method key (`Owner::method`) to complete `old` and
  `new` rows, including null for deliberate additions/removals. Stale/unused or
  mismatched exceptions fail. Declaration/typed-result doc changes need exact
  exceptions, not blanket signature waivers.
- WIT reuses PR 1's structural parser. Keys are `types/types/record-name`,
  `host/functions/method-name`, `host/use-items:0`, etc. Exact exceptions carry
  the complete old/new block. Type fields and variant/enum cases remain ordered.
  `document-skeleton` checks every unparsed byte, including hook interfaces and
  the world. A function rename fails unless explicitly declared. The allow-list
  is not a reason to permit accidental reordering: review every WIT exception.
- Source comparison covers **all tracked changes** under `crates/` and
  `docs/modding/`, against the immutable base. It replays declared substitutions
  over base content, then rustfmt-normalizes both sides. It requires exact final
  text equality as well as reporting line multisets: balanced line shuffles
  cannot pass accidentally. A pure move can be declared with exact removal and
  insertion blocks in existing files. A whole-file move uses `"move_from":
  "crates/old/src/file.rs"` on the destination declaration plus a full-content
  removal rewrite on the source; the gate requires that source to be gone/empty.
  No broad `use`/comment/whitespace line exclusion exists.
- Handwritten file hashes identify the exact separately reviewed semantic input.
  Those files are counted and are **not** called equivalent or pure moves. Split
  scripts and handwritten inputs by commit/file where possible; a mixed file's
  whole delta is conservatively handwritten. New/deleted source needs an explicit
  declaration. Tool files outside these source roots remain ordinary code review.
- `points.rs`, `sites.rs`, and `internals.rs` are protected, even in declarations.

This does not prove semantic parity or that Wasmtime accepts an intentionally
changed interface. Focused conversion tests, CI, review and compatibility analysis
remain required. It cannot authenticate artifacts: retain base/head SHAs and
commands beside the generated files.

## Verified here

```sh
python3 -m unittest discover -s tools/refactors/api-pr3 -p 'test_*.py' -v
```

Seven focused tests cover signature/docs/hidden changes, exact exceptions, WIT
world changes and case order, compiler primary spans and stale diagnostics,
Text replay idempotence, a balanced-line-shuffle negative source check, and
whole-file moves with a changed-body negative check.

At chain top `c92325020ca1f1db923ff78314b37001a6d33153`, Text replay plans
**0 files**. Its source equivalence gate passes with no changes. The chain's own
generator and manifest binary were built and run; passing the identical exact-head
artifacts through the integration gate gives `API 3 PASS: 0 source files,
0 failures`. This is a no-op smoke check, **not** the final main-to-integration
proof. That proof must be populated at the final integration head.

Remaining inventory: no independent mechanical Text/placement/rules/HUD cut.
Loot has one engine export row for two types; ParticleOptions has one engine
field and five typed constructor parameters (six `ty: ParticleType` occurrences
in total), plus nine fixed variants and one export. Those swaps depend on the
integration's handwritten semantic converters and are not blindly renamed here.
