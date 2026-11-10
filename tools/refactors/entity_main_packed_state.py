#!/usr/bin/env python3
"""Port entity-visibility consumers to main's already-merged residency packing.

Run after resolving merge structure. Keep the physics watcher at tuple slot 4
and the independently owned pass lease at slot 5; never clone a pass lease.
"""
from pathlib import Path
import argparse
import re

ROOT = Path(__file__).resolve().parents[2]
SRC = ROOT / "crates/horizon-client/src"


def replace_once(text, old, new):
    if old not in text and new in text:
        return text
    if text.count(old) != 1:
        raise SystemExit(f"unexpected port seam: {old[:100]}")
    return text.replace(old, new)


def main():
    argparse.ArgumentParser(description=__doc__).parse_args()
    state = SRC / "game/entities/resident_state.rs"
    if not state.exists():
        raise SystemExit("merge main and resolve source structure before this port")
    for path in [SRC / "game.rs", *sorted((SRC / "game").rglob("*.rs"))]:
        text = path.read_text()
        for old, new in [
            (r"\.entity_residency\b", ".resident_entities.table"),
            (r"\.local_player_residency\b", ".resident_entities.local_player"),
            (r"(?<!resident_entities)\.knots\b", ".resident_entities.knots"),
            (r"(?<!residency)\.part_uuids\b", ".residency.part_uuids"),
        ]:
            text = re.sub(old, new, text)
        path.write_text(text)
    text = state.read_text()
    text = text.replace(
        "residency::table::EntityTable>>",
        "residency::table::EntityTable<super::native_entity_read_cache::NativeReadPublication>>>",
    )
    state.write_text(text)
    path = SRC / "join_seq.rs"
    text = path.read_text()
    for old, new in [
        ("    Option<physics_mutations::Binding<T>>,\n);", "    Option<physics_mutations::Binding<T>>,\n    Option<visibility::PassLease>,\n);"),
        ("Self(self.0.clone(), None, ListStamp::new(), None, None)", "Self(self.0.clone(), None, ListStamp::new(), None, None, None)"),
        ("Self(Vec::new(), None, ListStamp::new(), None, None)", "Self(Vec::new(), None, ListStamp::new(), None, None, None)"),
        ("assert!(taken.4.is_none(), \"lifting an already owned pass\");", "assert!(taken.5.is_none(), \"lifting an already owned pass\");"),
        ("taken.4 = Some(visibility::PassLease::enter());", "taken.5 = Some(visibility::PassLease::enter());"),
        ("let pass_lease = taken.4.take();", "let pass_lease = taken.5.take();"),
        ("_pass: self.4.take(),", "_pass: self.5.take(),"),
        ("            Vec::with_capacity(capacity),\n            None,\n            ListStamp::new(),\n            None,\n            None,\n", "            Vec::with_capacity(capacity),\n            None,\n            ListStamp::new(),\n            None,\n            None,\n            None,\n"),
    ]:
        text = replace_once(text, old, new)
    path.write_text(text)


if __name__ == "__main__":
    main()
