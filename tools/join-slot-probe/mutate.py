#!/usr/bin/env python3
"""Kill location-index faults using the production-source focused tests.

Run with no other build editing join_seq.rs. Always restores the source.
"""
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "crates/horizon-client/src/join_seq.rs"
original = SOURCE.read_text()
# Each edit is confined to a named production method, never a test body.
faults = [
    ("join_slot", "self.1.get(&join).copied()", "None"),
    ("repair", "self.1.clear();", "return;"),
    ("append", "(join, offset + i)", "(join, i)"),
    ("remove", "self.1.remove(&entity.join_seq());", ""),
    ("remove", "*slot -= 1;", "*slot -= 0;"),
    ("extract_if", "slots.remove(&entity.join_seq());", ""),
    ("extract_if", "*slot -= 1;", "*slot -= 0;"),
    ("swap_remove", "self.1.remove(&entity.join_seq());", ""),
    ("swap_remove", "self.1.insert(moved.join_seq(), index);", ""),
    ("pop", "self.1.remove(&entity.join_seq());", ""),
    ("clear", "self.1.clear();", ""),
    ("truncate", "self.1.retain(|_, i| *i < len);", ""),
    ("retain", "self.repair();", ""),
    ("drain", "return false;", "return true;"),
    ("drain", "*i -= end - start;", "*i -= 0;"),
    ("swap", "self.1.insert(self.0[a].join_seq(), a);", ""),
    ("swap", "self.1.insert(self.0[b].join_seq(), b);", ""),
    ("retain_mut", "self.repair();", ""),
    ("push", "self.1.insert(entity.join_seq(), self.len());", ""),
    ("insert", "self.repair();", ""),
]
cmd = ["cargo", "test", "--manifest-path", str(Path(__file__).with_name("Cargo.toml")),
       "--lib", "join_seq::tests::"]
try:
    subprocess.run(cmd, check=True, cwd=ROOT, capture_output=True)
    for number, (method, old, new) in enumerate(faults, 1):
        # Skip ResidentList's same-named methods.
        start = original.index("    pub fn " + method + "(", original.index("fn repair(")) if method not in ("repair", "join_slot") else original.index("    " + ("fn " if method == "repair" else "pub fn ") + method + "(")
        end = original.find("\n    pub fn ", start + 1)
        if end < 0:
            end = original.index("\n#[cfg(test)]", start)
        block = original[start:end]
        assert block.count(old) == 1, (method, old)
        SOURCE.write_text(original[:start] + block.replace(old, new) + original[end:])
        result = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)
        output = result.stdout + result.stderr
        assert "test result: FAILED" in output, output
        print(f"{number}: {method}: {old} -> {new!r}: killed", flush=True)
    extras = [
        ("clone", "#[derive(Clone, Debug)]\npub struct JoinList", "#[derive(Debug)]\npub struct JoinList"),
        ("bound update", "before.is_none_or(|old| entity", "true || before.is_none_or(|old| entity"),
    ]
    for name, old, new in extras:
        mutant = original.replace(old, new)
        if name == "clone":
            mutant += "\nimpl<T: Clone, const D: bool> Clone for JoinList<T, D> { fn clone(&self) -> Self { Self(self.0.clone(), HashMap::new()) } }\n"
        SOURCE.write_text(mutant)
        result = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)
        assert "test result: FAILED" in result.stdout + result.stderr, result.stdout + result.stderr
        print(f"{name}: killed", flush=True)
finally:
    SOURCE.write_text(original)
