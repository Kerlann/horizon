#!/usr/bin/env python3
"""Extract the game's active-slot declarations verbatim before adding the runtime backend."""
from pathlib import Path
root = Path(__file__).resolve().parents[2]
registry = root / 'crates/horizon-mod-points/src/registry.rs'
source = registry.read_text()
a = source.index('/// Each place\'s slot:')
b = source.index('/// The game\'s table:', a)
block = source[a:b]
target = root / 'crates/horizon-engine-slots/src/lib.rs'
target.parent.mkdir(parents=True, exist_ok=True)
target.write_text('//! Engine-owned injection activity.\nuse std::sync::atomic::AtomicBool;\n\n' + block)
registry.write_text(source[:a] + source[b:])
