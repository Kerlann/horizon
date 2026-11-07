#!/usr/bin/env python3
"""Apply explicit token substitutions ONLY at primary rustc error spans.

Accepts cargo JSON diagnostics from CI or a deliberate compiler run. Recheck
and rerun with the next diagnostics file; no cargo/CI suite is run implicitly.
"""
import argparse
from collections import defaultdict
import hashlib
import json
from pathlib import Path
import re

PROTECTED = {"points.rs", "sites.rs", "internals.rs"}


def rewrite(root, diagnostics, names, apply=False):
    edits = defaultdict(set)
    unresolved = []
    for line in diagnostics.splitlines():
        message = json.loads(line)
        if message.get("reason") != "compiler-message":
            continue
        error = message["message"]
        if error["level"] != "error":
            continue
        found = False
        for span in error["spans"]:
            if not span["is_primary"]:
                continue
            path = (root / span["file_name"]).resolve()
            if not path.is_relative_to(root.resolve()) or path.name in PROTECTED:
                continue
            relative = path.relative_to(root.resolve())
            if not str(relative).startswith("crates/"):
                continue
            source = path.read_bytes()
            start, end = span["byte_start"], span["byte_end"]
            old = source[start:end].decode()
            if old not in names:
                continue
            if not re.fullmatch(r"[A-Za-z_]\w*(?:::[A-Za-z_]\w*)*", old):
                raise ValueError("rewrite keys must be complete Rust paths/tokens")
            edits[relative].add((start, end, old, names[old]))
            found = True
        if not found:
            unresolved.append(error["message"])
    records = []
    for relative, changes in sorted(edits.items()):
        path = root / relative
        source = path.read_bytes()
        before = hashlib.sha256(source).hexdigest()
        prior = len(source)
        for start, end, old, new in sorted(changes, reverse=True):
            if end > prior or source[start:end].decode() != old:
                raise ValueError(f"stale or overlapping compiler spans in {relative}")
            source = source[:start] + new.encode() + source[end:]
            prior = start
        records.append({"path": str(relative), "before_sha256": before,
                        "after_sha256": hashlib.sha256(source).hexdigest(),
                        "edits": [{"start": s, "end": e, "old": o, "new": n} for s, e, o, n in sorted(changes)]})
        if apply:
            path.write_bytes(source)
    return {"records": records, "unresolved": unresolved}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("diagnostics", type=Path)
    parser.add_argument("names", type=Path, help="Explicit old-token to new-token JSON object")
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--log", type=Path, required=True)
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()
    result = rewrite(args.root, args.diagnostics.read_text(), json.loads(args.names.read_text()), args.apply)
    args.log.parent.mkdir(parents=True, exist_ok=True)
    with args.log.open("a") as log:
        log.write(json.dumps(result) + "\n")
    print(f"{'Applied' if args.apply else 'Planned'} {len(result['records'])} files; {len(result['unresolved'])} unresolved diagnostics")


if __name__ == "__main__":
    main()
