#!/usr/bin/env python3
"""Extract the game's active-slot declarations verbatim before adding the runtime backend."""
from pathlib import Path
root = Path(__file__).resolve().parents[2]
import subprocess
import sys

if sys.argv[1:] == ["--verify"]:
    # Prove the historical mechanical commit, before the separate hand fixes.
    def at(revision, path):
        return subprocess.check_output(["git", "show", f"{revision}:{path}"], cwd=root).decode()

    before, after = "074a5df2d6", "cdb311a595"
    path = "crates/horizon-mod-points/src/registry.rs"
    source = at(before, path)
    a = source.index("/// Each place's slot:")
    b = source.index("/// The game's table:", a)
    expected = "//! Engine-owned injection activity.\nuse std::sync::atomic::AtomicBool;\n\n" + source[a:b]
    assert at(after, "crates/horizon-engine-slots/src/lib.rs") == expected
    assert at(after, path) == source[:a] + source[b:]
    print(f"Verified verbatim extraction {before} -> {after}")
    sys.exit(0)
if sys.argv[1:]:
    sys.exit("usage: " + sys.argv[0] + " [--verify]")

registry = root / 'crates/horizon-mod-points/src/registry.rs'
source = registry.read_text()
a = source.index('/// Each place\'s slot:')
b = source.index('/// The game\'s table:', a)
block = source[a:b]
target = root / 'crates/horizon-engine-slots/src/lib.rs'
target.parent.mkdir(parents=True, exist_ok=True)
target.write_text('//! Engine-owned injection activity.\nuse std::sync::atomic::AtomicBool;\n\n' + block)
registry.write_text(source[:a] + source[b:])
