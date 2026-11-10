#!/usr/bin/env python3
"""Move native-slot exports and the registration loop from engine start to host loading."""
from pathlib import Path
root = Path(__file__).resolve().parents[2]
entry = root / 'crates/horizon-client/src/entry.rs'
host = root / 'crates/horizon-mod-host/src/loading.rs'
lib = root / 'crates/horizon-mod-host/src/lib.rs'
source = entry.read_text()
a = source.index('/// A native function slot,')
b = source.index('/// Code-runtime services', a)
exports = source[a:b]
source = source[:a] + source[b:]
a = source.index('    for id in ids {')
b = source.index('    logging::set_game_home', a)
register = source[a:b]
entry.write_text(source[:a] + source[b:])
lib.write_text(lib.read_text() + '\n' + exports)
text = host.read_text()
assert '            (ids, wasm)' in text
host.write_text(text.replace('            (ids, wasm)', register + '            (ids, wasm)'))
