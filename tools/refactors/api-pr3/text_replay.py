#!/usr/bin/env python3
"""Replay the mechanical Text root cut; never infer a semantic adapter."""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import subprocess

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
PROTECTED = {"points.rs", "sites.rs", "internals.rs"}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def transform(path, source):
    """Declared edits, shared by replay and the independent base comparison."""
    edits = []

    def replace(old, new, rule):
        nonlocal source
        count = source.count(old)
        if count:
            edits.append({"rule": rule, "old": old, "new": new, "count": count})
            source = source.replace(old, new)

    if path == "crates/horizon-mod-api/src/exports/horizon_client_core.rs":
        replace("#[horizon_mod_api_macros::wasm(via = horizon_client_core::text::TextForm)]\npub use horizon_client_core::text::Text;\n", "", "remove-engine-text-export")
    if path == "crates/horizon-mod-api/src/exports/text.rs":
        if "pub use text::Text;" not in source:
            replace("pub mod text;\n", "pub mod text;\npub use text::Text;\n", "publish-owned-text")
    if path.startswith("crates/horizon-mod-api/src/") and "/exports/" not in path:
        replace("use horizon_client_core::text::Text;", "use crate::Text;", "api-text-import")
    return source, edits


def inventory(root):
    """Unresolved exports require handwritten prerequisites, not blind renaming."""
    pending = []
    for path in sorted((root / "crates/horizon-mod-api/src").rglob("*.rs")):
        if path.name in PROTECTED:
            continue
        for number, line in enumerate(path.read_text().splitlines(), 1):
            if any(token in line for token in (
                "horizon_client_core::text::", "horizon_client_core::particle_type::",
                "loot::{LootStack, NativeLootContext}",
            )):
                pending.append({"path": str(path.relative_to(root)), "line": number, "source": line})
    return pending


def replay(root, apply):
    records = []
    for path in sorted((root / "crates/horizon-mod-api/src").rglob("*.rs")):
        if path.name in PROTECTED:
            continue
        relative = str(path.relative_to(root))
        before = path.read_bytes()
        after, edits = transform(relative, before.decode())
        if edits:
            records.append({"path": relative, "before_sha256": digest(before),
                            "after_sha256": digest(after.encode()), "edits": edits})
            if apply:
                path.write_text(after)
    return records


def check(root, base):
    """Compare actual output against the declared transform of immutable Git input.

    Logs are evidence only, never an allow-list. Unknown edits remain failures.
    Formatting is normalized using rustfmt on both sides, not discarded lines.
    """
    names = subprocess.check_output(
        ["git", "diff", "--name-only", base, "--", "crates", "docs/modding"], cwd=root, text=True
    ).splitlines()
    removed, added, failures = Counter(), Counter(), []
    for name in names:
        if Path(name).name in PROTECTED:
            failures.append(f"protected file changed: {name}")
            continue
        original = subprocess.run(["git", "show", f"{base}:{name}"], cwd=root,
                                  capture_output=True, check=False)
        if original.returncode:
            failures.append(f"new source file is not a declared move: {name}")
            continue
        expected, _ = transform(name, original.stdout.decode())
        path = root / name
        actual = path.read_text() if path.exists() else ""
        if name.endswith(".rs"):
            def formatted(value):
                return subprocess.check_output(
                    ["rustfmt", "--edition", "2024", "--config", "skip_children=true", "--emit", "stdout"],
                    input=value, text=True, cwd=root,
                )
            expected, actual = formatted(expected), formatted(actual)
        if expected != actual:
            failures.append(f"undeclared source change: {name}")
        removed.update(expected.splitlines())
        added.update(actual.splitlines())
    return {"files_checked": len(names), "normalized_removed_only": dict(removed - added),
            "normalized_added_only": dict(added - removed), "failures": failures,
            "passed": not failures and removed == added}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--apply", action="store_true")
    parser.add_argument("--base", help="Immutable pre-replay commit for moved-lines/equivalence check")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    records = replay(args.root, args.apply)
    (args.output / "rewrite-log.json").write_text(json.dumps(records, indent=2) + "\n")
    pending = inventory(args.root)
    (args.output / "pending-sites.json").write_text(json.dumps(pending, indent=2) + "\n")
    print(f"{'Applied' if args.apply else 'Planned'} {len(records)} files; {len(pending)} pending engine references")
    if args.base:
        report = check(args.root, args.base)
        (args.output / "moved-lines.json").write_text(json.dumps(report, indent=2) + "\n")
        if not report["passed"]:
            parser.exit(1, "Equivalence check failed; see moved-lines.json\n")


if __name__ == "__main__":
    main()
