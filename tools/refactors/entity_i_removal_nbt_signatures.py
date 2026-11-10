#!/usr/bin/env python3
"""Stage the four removal/NBT immediate signatures; hand fixes own semantics.

This migration does not turn old sentinel returns into truthful refusals. Native
bodies, call sites and docs are corrected separately before the branch is ready.
"""
from pathlib import Path
import argparse
import re

ROOT = Path(__file__).resolve().parents[2]
METHODS = {
    "entity_nbt": "Option<Compound>",
    "entity_merge_nbt": "bool",
    "entity_discard": "bool",
    "entity_remove": "bool",
}
FILES = {
    "crates/horizon-mod-api/src/host/entities.rs": tuple(METHODS),
    "crates/horizon-client/src/game/modding/native_entity_data.rs": (
        "entity_nbt", "entity_merge_nbt"
    ),
    "crates/horizon-client/src/game/modding/native_entity_remove.rs": (
        "entity_discard", "entity_remove"
    ),
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    stale = []
    for relative, methods in FILES.items():
        path = ROOT / relative
        original = text = path.read_text()
        for method in methods:
            old = METHODS[method]
            new = f"Result<{old}, EntityReadError>"
            prefix = rf"(\bfn {method}\([^{{}};]*?\)\s*->\s*)"
            pattern = prefix + re.escape(old) + r"(?=\s*[{;])"
            text, count = re.subn(pattern, rf"\g<1>{new}", text)
            if count != 1 and not (count == 0 and re.search(prefix + re.escape(new), text)):
                raise SystemExit(f"unexpected signature: {relative}: {method}")
        if text != original:
            stale.append(relative)
            if not args.check:
                path.write_text(text)
    if args.check and stale:
        raise SystemExit("signature migration pending: " + ", ".join(stale))


if __name__ == "__main__":
    main()
