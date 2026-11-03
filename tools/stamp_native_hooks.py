#!/usr/bin/env python3
"""Collect pending native hook fragments into the catalog."""
import argparse
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FRAGMENTS = Path("tools/native-hook-fragments")
CATALOG = Path("native-hooks.json")


def collect_fragments(check):
    paths = sorted((ROOT / FRAGMENTS).glob("*.json"))
    if not paths:
        return False
    if check:
        print(f"{len(paths)} pending native hook fragment(s)")
        return True

    catalog = json.loads((ROOT / CATALOG).read_text())
    names = {hook["name"] for hook in catalog["hooks"]}
    helper_names = {helper["name"] for helper in catalog.get("helpers", [])}
    # Match the build-time generator: fragment paths, then their declared hook/helper order.
    for path in paths:
        fragment = json.loads(path.read_text())
        for hook in fragment["hooks"]:
            if hook["name"] in names:
                raise ValueError(f"duplicate hook: {hook['name']}")
            names.add(hook["name"])
            entry = {"name": hook["name"], "version": 1, "target": hook["target"]}
            # Catalog facts about a hook travel with it.
            entry.update({k: hook[k] for k in ("neoforge", "fabric", "paper", "side", "cancellable", "mod_api") if k in hook})
            catalog["hooks"].append(entry)
        for helper in fragment.get("helpers", []):
            if helper["name"] in helper_names:
                raise ValueError(f"duplicate helper: {helper['name']}")
            helper_names.add(helper["name"])
            catalog.setdefault("helpers", []).append(helper)

    (ROOT / CATALOG).write_text(json.dumps(catalog, indent=2, ensure_ascii=False) + "\n")
    for path in paths:
        path.unlink()
    print(f"Collected {len(paths)} catalog fragment(s)")
    return False


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="fail if fragments are pending")
    args = parser.parse_args()
    try:
        return int(collect_fragments(args.check))
    except (ValueError, KeyError, OSError) as error:
        print(f"stamp_native_hooks: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
