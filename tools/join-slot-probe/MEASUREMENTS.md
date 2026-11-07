# Joined-list location measurements

Draft evidence, not a game-tick benchmark or a speedup claim for `find_join`.
The index supplies list-location authority for the later entity table. **Blocked on 1f0**:
rocket, trident and falling ticks temporarily remove/reinsert every live entity; see AUDIT.md.

## Method

20 logical cores available in the box, shared host; `fast` profile (release-derived,
opt-level 2, no LTO). The probe includes the actual production `join_seq.rs`; it does
not reimplement eager indexing. Fixture entities are 64 bytes, not real mob/boat kinds.
Five process runs; numbers below are medians of the per-process means. Host contention,
allocator layout and randomized map seeds cause noise; these are not confidence intervals.
`old_find` is the deleted binary-search plus linear fallback, kept only in the probe.

Commands:

```sh
cargo test --manifest-path tools/join-slot-probe/Cargo.toml --lib join_seq::
python3 tools/join-slot-probe/mutate.py
cargo run --manifest-path tools/join-slot-probe/Cargo.toml --profile fast
cargo run --manifest-path tools/join-slot-probe/Cargo.toml --profile fast -- --mixed
```

200,000 queries per phase, cycling by a step of 97 over live keys. Removal phases have
N - 1 entities; restore adds one spawned owner. Removal timing excludes cloning and
teardown (200 samples). Idle timing runs 10,000 retain-all passes. Both idle paths
allocate **zero** times; the production retain checks length and does no index repair
when membership is unchanged. This does not measure allocations in an entire Game tick.

## Lookup latency (ns per query, indexed / old)

| Initial N | After push | After ordered remove | After swap | After take/restore | Out of order |
|---|---|---|---|---|---|
| 100 | 12.12 / 9.84 | 11.67 / 9.57 | 14.23 / 15.04 | 17.84 / 17.27 | 17.25 / 48.72 |
| 1,000 | 19.12 / 26.90 | 16.51 / 21.16 | 17.03 / 27.63 | 19.11 / 23.75 | 18.13 / 345.32 |
| 10,000 | 21.02 / 57.48 | 18.93 / 44.21 | 19.43 / 50.68 | 19.51 / 35.21 | 18.90 / 2,932.37 |

| Initial N | Middle removal ns, indexed / Vec | Idle retain ns, indexed / Vec |
|---|---|---|
| 100 | 182.88 / 58.85 | 60.85 / 79.48 |
| 1,000 | 2,049.82 / 439.31 | 787.62 / 965.48 |
| 10,000 | 28,700.57 / 8,862.05 | 9,587.68 / 8,822.49 |

## Mixed workload (ms per synthetic tick)

N successful surviving-key lookups and 1, 10 or 100 middle removals. Burst performs
all removals before lookups; interleaved distributes N lookups evenly among removals.
Each process averages 20 synthetic ticks. The experimental lazy variant records the
lowest shifted position, then repairs its suffix at the next lookup; it is **not**
production storage and was rejected because interleaved lookups repeatedly pay repair.

| N | Removals | Schedule | Eager branchless | Lazy | Vec plus old lookup |
|---|---|---|---|---|---|
| 1,000 | 1 | burst | 0.016 | 0.024 | 0.018 |
| 1,000 | 1 | interleaved | 0.012 | 0.017 | 0.014 |
| 1,000 | 10 | burst | 0.032 | 0.023 | 0.021 |
| 1,000 | 10 | interleaved | 0.025 | 0.058 | 0.014 |
| 1,000 | 100 | burst | 0.189 | 0.042 | 0.035 |
| 1,000 | 100 | interleaved | 0.177 | 0.604 | 0.042 |
| 10,000 | 1 | burst | 0.175 | 0.251 | 0.257 |
| 10,000 | 1 | interleaved | 0.142 | 0.204 | 0.220 |
| 10,000 | 10 | burst | 0.270 | 0.246 | 0.269 |
| 10,000 | 10 | interleaved | 0.279 | 0.720 | 0.268 |
| 10,000 | 100 | burst | 1.858 | 0.889 | 0.891 |
| 10,000 | 100 | interleaved | 2.232 | 8.781 | 1.035 |

Thus eager can lose about 2x against Vec at 10k and 100 individual middle removals.
Burst rows still use individual removes, not production retain/drain; do not cite
those rows as a measured bulk-retain result. No variant universally beats old lookup.

Not measured: full Game ticks, real-kind 1k rocket/trident/falling ticks, boat clone
versus store_back, bound-slot idle edits, cold loads, or lazy retain/drain. No in-game
proof was run: these drafts change lookup mechanics, not intended firing order.
