#!/usr/bin/env python3
"""Make the shared Mob save kernel fallible; keep engine wrappers Infallible.

One-shot signature migration, committed before its mechanical application.
"""
from pathlib import Path
import argparse


def replace(text, old, new, count=1):
    assert text.count(old) == count, (old, text.count(old))
    return text.replace(old, new)


def migrate(root, check):
    base = root / "crates/horizon-client/src/game"
    paths = [base / p for p in ("modding/native_mob_nbt_save.rs", "world/persist.rs", "world/levels.rs", "entities/leads.rs")]
    mob, persist, levels, leads = [p.read_text() for p in paths]
    mob = replace(mob, "pub(super) trait MobNbtOwner {", "pub(super) trait MobNbtOwner {\n    type Error;")
    mob = replace(mob, "fn uuid_for(&mut self, id: u32) -> [i32; 4]", "fn uuid_for(&mut self, id: u32) -> Result<[i32; 4], Self::Error>", 3)
    mob = replace(mob, "fn write_leash(&mut self, tag: &mut Tag, index: usize)", "fn write_leash(&mut self, tag: &mut Tag, index: usize) -> Result<(), Self::Error>", 3)
    mob = replace(mob, "fn mob_tree_nbt(&mut self, i: usize) -> Tag", "fn mob_tree_nbt(&mut self, i: usize) -> Result<Tag, Self::Error>")
    for name in ("id", "suspect", "target", "attacker"):
        mob = replace(mob, f"self.uuid_for({name});", f"self.uuid_for({name})?;")
    mob = replace(mob, "self.write_leash(&mut tag, i);", "self.write_leash(&mut tag, i)?;")
    mob = replace(mob, "self.put_mob_passengers(&mut tag, id);\n        tag", "self.put_mob_passengers(&mut tag, id)?;\n        Ok(tag)")
    mob = replace(mob, "fn put_mob_passengers(&mut self, tag: &mut Tag, id: u32) {", "fn put_mob_passengers(&mut self, tag: &mut Tag, id: u32) -> Result<(), Self::Error> {")
    for name in ("pos", "j"):
        mob = replace(mob, f"riders.push(self.mob_tree_nbt({name}));", f"riders.push(self.mob_tree_nbt({name})?);")
    mob = replace(mob, "        add_passengers(tag, riders);", "        add_passengers(tag, riders);\n        Ok(())")
    for owner in ("Game<'_>", "ParkedMobNbt<'_>"):
        mob = replace(mob, f"impl MobNbtOwner for {owner} {{", f"impl MobNbtOwner for {owner} {{\n    type Error = std::convert::Infallible;")
    mob = replace(mob, "        Game::uuid_for(self, id)", "        Ok(Game::uuid_for(self, id))")
    mob = replace(mob, "        Game::write_leash(self, tag, index);", "        Game::write_leash(self, tag, index);\n        Ok(())")
    mob = replace(mob, "            return uuid;", "            return Ok(uuid);")
    mob = replace(mob, "        self.assignments.push((id, uuid));\n        uuid", "        self.assignments.push((id, uuid));\n        Ok(uuid)")
    mob = replace(mob, "            self.uuid_for(id)\n        });", "            self.uuid_for(id)\n        })")
    for call in ("put_mob_passengers(self, tag, id)", "mob_tree_nbt(self, i)"):
        persist = replace(persist, "MobNbtOwner::" + call, "MobNbtOwner::" + call + ".unwrap_or_else(|never| match never {})")
    levels = replace(levels, "owner.mob_tree_nbt(index)", "owner.mob_tree_nbt(index).unwrap_or_else(|never| match never {})")
    leads = replace(leads, "pub(super) fn write_mob_leash_tag(", "pub(super) fn write_mob_leash_tag<E>(")
    leads = replace(leads, "    mob_uuid: impl FnOnce(u32) -> [i32; 4],\n) {", "    mob_uuid: impl FnOnce(u32) -> Result<[i32; 4], E>,\n) -> Result<(), E> {")
    start = leads.index("pub(super) fn write_mob_leash_tag<E>")
    end = leads.index("\n}\n", start) + len("\n}\n")
    helper = leads[start:end]
    changed = helper.replace("else { return };", "else { return Ok(()) };").replace("Some(mob_uuid(id))", "Some(mob_uuid(id)?)")
    changed = changed[:-2] + "    Ok(())\n}\n"
    leads = leads[:start] + changed + leads[end:]
    leads = replace(leads, "            |id| self.mob_uuid(id),\n        );", "            |id| Ok::<_, std::convert::Infallible>(self.mob_uuid(id)),\n        ).unwrap_or_else(|never| match never {});")
    leads = replace(leads, "                |_| [0; 4],\n            );", "                |_| Ok::<_, std::convert::Infallible>([0; 4]),\n            ).unwrap_or_else(|never| match never {});")
    if not check:
        for path, content in zip(paths, (mob, persist, levels, leads)):
            path.write_text(content)
    print("Validated error migration" if check else "Migrated shared Mob save errors")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    migrate(args.root, args.check)
