#!/usr/bin/env python3
"""One-shot mechanical extraction of the existing non-Mob command save arms.

Run against the pre-extraction tree, then review the generated move separately
from owner-local serializer fixes. --check validates the input without writing.
"""
from pathlib import Path
import argparse


def extract(root: Path, check: bool) -> None:
    source = root / "crates/horizon-client/src/game/administration/commands/data.rs"
    output = root / "crates/horizon-client/src/game/modding/native_non_mob_nbt_save.rs"
    text = source.read_text()
    start = text.index("    fn entity_nbt_in_level(")
    end = text.index("    /// `Entity.load`", start)
    function = text[start:end]
    arms_start = function.index("            Target::Item(id) => {")
    arms_end = function.rindex("\n        })")
    arms = function[arms_start:arms_end]
    assert arms.count("Target::Cloud(id)") == 1
    assert arms.count("Target::Knot(id)") == 1
    prefix = function[:arms_start].replace(
        "        let player_uuid = self.saved_player_uuid();\n", ""
    )
    replacement = prefix + "            _ => return self.non_mob_nbt_in_level(t),\n        })\n    }\n"
    helper_start = text.index("fn set_key(")
    helper_end = text.index("/// `EntityType.loadEntityRecursive`", helper_start)
    helper = text[helper_start:helper_end]
    module = (
        "//! Existing non-Mob save behavior, mechanically extracted before owner-local adaptation.\n"
        "use super::{Game, commands::Target};\n"
        "use horizon_client::{entity_nbt, save::random_uuid};\n"
        "use horizon_core::nbt::{JavaString, Tag};\n\n"
        + helper.replace("fn set_key(", "pub(in crate::game) fn set_key(", 1)
        + "\nimpl Game<'_> {\n"
        "    pub(super) fn non_mob_nbt_in_level(&mut self, t: Target) -> Option<Tag> {\n"
        "        let player_uuid = self.saved_player_uuid();\n"
        "        Some(match t {\n"
        + arms.replace("super::OtherEntity {", "super::commands::OtherEntity {")
        + "\n            _ => return None,\n        })\n    }\n}\n"
    )
    revised = text[:start] + replacement + text[end:]
    revised = revised.replace(helper, "", 1)
    revised = revised.replace(
        "use super::{Arg, Context, Game, R, Target, err};",
        "use super::{Arg, Context, Game, R, Target, err};\n"
        "use super::super::native_non_mob_nbt_save::set_key;",
        1,
    )
    revised = revised.replace(
        "    fn saved_player_uuid(&self)",
        "    pub(in crate::game) fn saved_player_uuid(&self)",
        1,
    )
    if revised.count("JavaString") == 1:
        revised = revised.replace("Compound, JavaString, Tag", "Compound, Tag")
    if revised.count("random_uuid") == 1:
        revised = revised.replace("save::{self, random_uuid}", "save")
    assert not output.exists(), "mechanical extraction output already exists"
    if not check:
        source.write_text(revised)
        output.write_text(module)
    print(f"{'Validated' if check else 'Extracted'} {arms.count('Target::')} non-Mob save arms")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    extract(args.root, args.check)
