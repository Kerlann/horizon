#!/usr/bin/env python3
"""Strict API 3 closeout gates. Missing evidence fails; logs never authorize edits."""
import argparse
from collections import Counter
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import subprocess

HERE = Path(__file__).resolve().parent


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


manifest = module("manifest", HERE.parent / "api-pr3a/manifest_gate.py")
wit = module("wit", HERE.parent / "api-pr1/wit_order.py")
text_replay = module("text_replay", HERE / "text_replay.py")


def substitute(value, substitutions):
    # Simultaneous replacement: a type cannot be renamed twice by a chain.
    if not substitutions:
        return value
    pattern = r"(?<![\w-])(?:" + "|".join(re.escape(k) for k in sorted(substitutions, key=len, reverse=True)) + r")(?![\w-])"
    return re.sub(pattern, lambda m: substitutions[m[0]], value)


def exact_rows(before, after, exceptions, substitutions):
    failures = []
    used = set()
    for key in sorted(before.keys() | after.keys() | exceptions.keys()):
        old, new = before.get(key), after.get(key)
        if key in exceptions:
            if exceptions[key] != {"old": old, "new": new} or old == new:
                failures.append(f"{key}: stale or mismatched exact exception")
        elif old is None or new is None or substitute(old, substitutions) != new:
            failures.append(f"{key}: undeclared delta")
        elif old != new:
            used.update(k for k in substitutions if substitute(old, {k: substitutions[k]}) != old)
    return failures, used


def host_gate(before, after, plan):
    # Unlike 3a's comparison, signatures are NOT ignored.
    left = {k: v[0] for k, v in manifest.parse(before).items()}
    right = {k: v[0] for k, v in manifest.parse(after).items()}
    return exact_rows(left, right, plan.get("host_exceptions", {}), plan.get("host_substitutions", {}))[0]


def wit_rows(source):
    normalized, groups = wit.canonical_wit(source)
    rows = {}
    # Whole type/function blocks retain their case/field order. The skeleton
    # also checks hook interfaces, world exports and all unparsed WIT bytes.
    for (interface, kind), blocks in groups.items():
        if kind.startswith("use-items:"):
            joined = ", ".join(sorted(blocks))
            normalized = normalized.replace("  use types.{" + joined + "};", "  use types.{};")
            rows[f"{interface}/{kind}"] = joined
        else:
            for block in blocks:
                name = re.search(r"^  (?:(?:type|record|variant|enum|flags|resource) )?([\w%-]+)", block)[1]
                rows[f"{interface}/{kind}/{name}"] = block
                normalized = normalized.replace(block, "")
    rows["document-skeleton"] = normalized
    return rows


def source_gate(root, base, plan):
    changes = subprocess.check_output(["git", "diff", "--name-only", base, "--", "crates", "docs/modding"], cwd=root, text=True).splitlines()
    failures, report = [], []
    declarations = plan.get("source", {})
    for name in sorted(set(changes) | declarations.keys()):
        if not (root / name).resolve().is_relative_to(root.resolve()):
            failures.append(f"{name}: path escapes checkout")
            continue
        if Path(name).name in text_replay.PROTECTED:
            failures.append(f"{name}: protected source changed")
            continue
        old = subprocess.run(["git", "show", f"{base}:{name}"], cwd=root, capture_output=True, check=False)
        before = old.stdout if old.returncode == 0 else b""
        path = root / name
        after = path.read_bytes() if path.exists() else b""
        rule = declarations.get(name, {})
        if "handwritten" in rule:
            expected = {"before_sha256": hashlib.sha256(before).hexdigest(),
                        "after_sha256": hashlib.sha256(after).hexdigest()}
            if rule["handwritten"] != expected or before == after:
                failures.append(f"{name}: handwritten hash declaration mismatched or unused")
            # Handwritten changes are counted, NOT claimed equivalent/moved.
            removed = Counter(before.decode().splitlines()) - Counter(after.decode().splitlines())
            added = Counter(after.decode().splitlines()) - Counter(before.decode().splitlines())
            report.append({"path": name, "kind": "handwritten", "lines": sum(removed.values()) + sum(added.values())})
            continue
        replay_input = before
        if "move_from" in rule:
            origin = rule["move_from"]
            if Path(origin).name in text_replay.PROTECTED or not (root / origin).resolve().is_relative_to(root.resolve()):
                failures.append(f"{name}: unsafe move source")
                continue
            if (root / origin).exists() and (root / origin).read_bytes():
                failures.append(f"{name}: whole-file move source was not removed")
            replay_input = subprocess.check_output(["git", "show", f"{base}:{origin}"], cwd=root)
        expected, _ = text_replay.transform(name, replay_input.decode())
        for rewrite in rule.get("rewrites", []):
            if not rewrite["old"] or rewrite["count"] < 1 or expected.count(rewrite["old"]) != rewrite["count"]:
                failures.append(f"{name}: rewrite count mismatched")
                continue
            expected = expected.replace(rewrite["old"], rewrite["new"])
        if name.endswith(".rs"):
            def formatted(source):
                return subprocess.check_output(["rustfmt", "--edition", "2024", "--config", "skip_children=true", "--emit", "stdout"], input=source, text=True, cwd=root)
            expected, actual = formatted(expected), formatted(after.decode())
        else:
            actual = after.decode()
        removed, added = Counter(expected.splitlines()), Counter(actual.splitlines())
        if expected != actual:
            failures.append(f"{name}: output differs from declared replay")
        report.append({"path": name, "kind": "scripted", "removed_only": dict(removed - added), "added_only": dict(added - removed)})
    return failures, report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=HERE.parents[2])
    parser.add_argument("--base", default="origin/main")
    parser.add_argument("--plan", type=Path, required=True)
    parser.add_argument("--before-host", type=Path, required=True)
    parser.add_argument("--after-host", type=Path, required=True)
    parser.add_argument("--before-wit", type=Path, required=True)
    parser.add_argument("--after-wit", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    plan = json.loads(args.plan.read_text())
    failures, moved = source_gate(args.root, args.base, plan)
    failures += host_gate(args.before_host.read_text(), args.after_host.read_text(), plan)
    delta, _ = exact_rows(wit_rows(args.before_wit.read_text()), wit_rows(args.after_wit.read_text()),
                          plan.get("wit_exceptions", {}), plan.get("wit_substitutions", {}))
    failures += ["WIT " + item for item in delta]
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps({"base": args.base, "failures": failures, "source": moved}, indent=2) + "\n")
    print(f"API 3 {'FAIL' if failures else 'PASS'}: {len(moved)} source files, {len(failures)} failures; {args.output}")
    return bool(failures)


if __name__ == "__main__":
    raise SystemExit(main())
