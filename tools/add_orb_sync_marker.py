#!/usr/bin/env python3
"""Add the real orb sync marker and initialize engine orb constructors.

Run from the repository root. This mechanical migration precedes hand-written
NoGravity publication and dirty-marker consumption; mod_api value DTOs are not
engine bodies and are deliberately excluded.
"""
from pathlib import Path
import re

root = Path("crates/horizon-client/src")
for path in sorted(root.rglob("*.rs")):
    text = path.read_text()
    positions = set()
    for match in re.finditer(r"\bExperienceOrb\s*\{", text):
        if text[max(0, match.start() - 9):match.start()].endswith("mod_api::"):
            continue
        depth = 1
        end = match.end()
        while depth and end < len(text):
            depth += (text[end] == "{") - (text[end] == "}")
            end += 1
        for field in re.finditer(r"(?m)^(\s*)no_gravity:[^\n]*,\n", text[match.end():end]):
            positions.add((match.end() + field.end(), field.group(1)))
    for position, indent in sorted(positions, reverse=True):
        text = text[:position] + f"{indent}needs_sync: false,\n" + text[position:]
    if path == root / "entities.rs":
        anchor = "pub struct ExperienceOrb {\n"
        assert text.count(anchor) == 1
        text = text.replace(anchor, anchor + "    /// Synched data changed outside normal ticking; cleared after tracking.\n    pub needs_sync: bool,\n")
    if text != path.read_text():
        path.write_text(text)
        print(path)
