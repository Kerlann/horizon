#!/usr/bin/env python3
"""Apply reviewed mechanical source declarations; leave handwritten inputs alone."""
import argparse
import hashlib
import json
from pathlib import Path

from text_replay import PROTECTED, replay, transform


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--plan", type=Path, required=True)
    parser.add_argument("--log", type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve()
    plan = json.loads(args.plan.read_text())
    # Preflight every declared operation before changing any file. A rerun
    # starts from the immutable input commit, not from partly rewritten output.
    pending = []
    for name, declaration in plan.get("source", {}).items():
        if "handwritten" in declaration:
            continue
        path = (root / name).resolve()
        if not path.is_relative_to(root) or path.name in PROTECTED:
            raise ValueError(f"unsafe/protected source declaration: {name}")
        before = path.read_text() if path.exists() else ""
        replay_input = before
        if "move_from" in declaration:
            origin = (root / declaration["move_from"]).resolve()
            if not origin.is_relative_to(root) or origin.name in PROTECTED:
                raise ValueError(f"unsafe move source: {origin}")
            replay_input = origin.read_text()
        after, _ = transform(name, replay_input)
        for edit in declaration.get("rewrites", []):
            if not edit["old"] or edit["count"] < 1 or after.count(edit["old"]) != edit["count"]:
                raise ValueError(f"stale or ambiguous rewrite: {name}")
            after = after.replace(edit["old"], edit["new"])
        pending.append((path, before, after, declaration))
    records = []
    for path, before, after, declaration in pending:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(after)
        records.append({"path": str(path.relative_to(root)), "before_sha256": hashlib.sha256(before.encode()).hexdigest(),
                        "after_sha256": hashlib.sha256(after.encode()).hexdigest(), "edits": declaration.get("rewrites", [])})
    records += replay(root, True)
    args.log.parent.mkdir(parents=True, exist_ok=True)
    args.log.write_text(json.dumps(records, indent=2) + "\n")
    print(f"Replayed {len(records)} files; handwritten declarations untouched")


if __name__ == "__main__":
    main()
