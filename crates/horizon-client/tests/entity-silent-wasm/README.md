# Queued entity-write execution fixture

This guest is loaded twice as independent wasm packages by the real Game test.
Each package queues writes from A's Pre, queries Pending, receives only its own
terminals, appends work from its first completion, and verifies all three terminals
before B's Pre. Independent no-gravity/glowing markers prove both guests reached
the end of their assertions. A native observer must receive neither guest's event
nor query authority.

From the repository root, with the pinned wasm toolchain available:

```sh
cargo build -p cargo-horizon-mod
tool="$PWD/target/debug/cargo-horizon-mod"
std="$PWD/target/entity-silent-std.wasm"
(cd crates/horizon-client/tests/entity-silent-wasm && \
  "$tool" horizon-mod build --wasm && \
  "$tool" horizon-mod wasm-std "$std")
export HORIZON_SILENT_WASM="$PWD/crates/horizon-client/tests/entity-silent-wasm/target/horizon-mod/entity_silent_fixture.hmod"
export HORIZON_WASM_STD="$std"
export HORIZON_TEST_RESOURCES="$PWD/.local/horizon-client/26.3"
scratch=$(mktemp -d)
export HORIZON_HOME="$scratch/home" HORIZON_LAUNCHER_HOME="$scratch/launcher"
export XDG_DATA_HOME="$scratch/data" XDG_CONFIG_HOME="$scratch/config"
export HORIZON_LAUNCHER_NO_SELF_UPDATE=1 HORIZON_NO_AUDIO=1
cargo test -p horizon-client --lib --features testing,system-allocator \
  native_entity_command_ -- --ignored
```

The existing `testing` feature opts into the separately built wasm fixture; the
normal ignored-test sweep does not require wasm inputs. The existing
`system-allocator` feature disables mimalloc so the test-only safe
allocation counter can measure warm converted writes, actual apply/completion
dispatch and queries. Both measured regions must allocate zero times after the
issuer and queue capacities are warmed. No production allocator is changed.
Prepared resources and built wasm members are private local inputs, not fixtures
to commit.
