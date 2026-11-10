#!/usr/bin/env python3
"""Convert the three original immediate topology contracts, not to queues.

Commit this script before application. Engine body/source corrections and caller
error propagation follow separately. No remove/discard or NBT files are edited.
"""
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[2]
METHODS = ('entity_start_riding', 'entity_stop_riding', 'entity_eject_passengers')
PATHS = (
    'crates/horizon-mod-api/src/host/entities.rs',
    'crates/horizon-client/src/game/modding/native_entity_riding.rs',
)


def apply():
    for name in PATHS:
        path = ROOT / name
        text = path.read_text()
        for method in METHODS:
            pattern = r'(\bfn\s+' + method + r'\s*\([^)]*\)\s*->\s*)bool\b'
            error = 'EntityReadError' if 'horizon-mod-api' in name else 'mod_api::EntityReadError'
            text = re.sub(pattern, lambda match: match[1] + 'Result<bool, ' + error + '>', text)
        path.write_text(text)


if __name__ == '__main__':
    apply()
