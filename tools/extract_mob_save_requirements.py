#!/usr/bin/env python3
"""Share actual Mob save branch predicates and passenger order with preflight.

Commit this script before its mechanical extraction. --check writes nothing.
"""
from pathlib import Path
import argparse
import re


def extract(root, check):
    path = root / "crates/horizon-client/src/game/modding/native_mob_nbt_save.rs"
    text = path.read_text()
    start = text.index("    fn put_mob_passengers(")
    end = text.index("\n    }\n", start) + len("\n    }\n")
    method = text[start:end]
    body = method[method.index("{") + 1:method.rindex("}")]
    cutoff = body.index("        if let Tag::Compound(c) = tag")
    ordered = body[:cutoff]
    ordered = ordered.replace("Vec<Tag>", "Vec<PassengerSlot>")
    ordered = ordered.replace("riders.push(self.mob_tree_nbt(pos)?);", "riders.push(PassengerSlot::Live(self.mobs()[pos].id));")
    ordered = ordered.replace("riders.push(saved_tag.clone());", "riders.push(PassengerSlot::Saved(saved_tag.clone()));")
    ordered = ordered.replace("riders.push(self.mob_tree_nbt(j)?);", "riders.push(PassengerSlot::Live(self.mobs()[j].id));")
    ordered = re.sub(r"self\s*\.mobs\(\)", "mobs", ordered)
    ordered = re.sub(r"self\s*\.persist\(\)", "persist", ordered)
    assert "self" not in ordered
    helper = "pub(super) fn ordered_mob_passengers(mobs: &JoinList<Mob>, persist: &EntityMemory, id: u32) -> Vec<PassengerSlot> {" + ordered + "        riders\n}\n\n"
    new_method = """    fn put_mob_passengers(&mut self, tag: &mut Tag, id: u32) -> Result<(), Self::Error> {
        let slots = ordered_mob_passengers(self.mobs(), self.persist(), id);
        let mut riders = Vec::new();
        for slot in slots {
            match slot {
                PassengerSlot::Live(id) => {
                    if let Some(index) = self.mobs().iter().position(|m| m.id == id) {
                        riders.push(self.mob_tree_nbt(index)?);
                    }
                }
                PassengerSlot::Saved(tag) => riders.push(tag),
            }
        }
        if let Tag::Compound(c) = tag { c.remove(&"Passengers".into()); }
        add_passengers(tag, riders);
        Ok(())
    }
"""
    text = text[:start] + new_method + text[end:]
    anger_start = text.index("        self.mobs_mut()[i].extra.angry_at = if anger > 0")
    anger_end = text.index(";\n        // A mob gets its UUID", anger_start) + 1
    anger = text[anger_start:anger_end]
    expression = anger.split(" = ", 1)[1][:-1].replace("if anger > 0", "if m.extra.anger > 0").replace("self.mobs()[i]", "m")
    helper += "pub(super) fn saved_angry_at(m: &Mob) -> Option<Who> {\n" + expression + "\n}\n\n"
    text = text[:anger_start] + "        self.mobs_mut()[i].extra.angry_at = saved_angry_at(&self.mobs()[i]);" + text[anger_end:]
    ward_start = text.index("        // A warden's anger names")
    ward_end = text.index("        // `NeutralMob.addPersistentAngerSaveData`", ward_start)
    warden = text[ward_start:ward_end]
    suspects_start = warden.index("        let suspects: Vec<u32>")
    suspects_end = warden.index("        for suspect in suspects")
    suspects = warden[suspects_start:suspects_end].replace("self.mobs()[i]", "m")
    helper += """pub(super) struct MobSaveUuidRequests {
    pub(super) suspects: Vec<u32>,
    pub(super) target: Option<u32>,
    pub(super) attacker: Option<u32>,
}
pub(super) fn mob_save_uuid_requests(m: &Mob, mobs: &JoinList<Mob>) -> MobSaveUuidRequests {
""" + suspects + """    let target = match saved_angry_at(m) {
        Some(Who::Mob(id)) if id != 0 && mobs.iter().any(|m| m.id == id) => Some(id),
        _ => None,
    };
    let attacker = match m.ai.last_hurt_by {
        Some(Who::Mob(id)) if mobs.iter().any(|m| m.id == id) => Some(id),
        _ => None,
    };
    MobSaveUuidRequests { suspects, target, attacker }
}

"""
    text = text[:ward_start] + "        let references = mob_save_uuid_requests(&self.mobs()[i], self.mobs());\n        for suspect in references.suspects { self.uuid_for(suspect)?; }\n" + text[ward_end:]
    targets_start = text.index("        // A mob gets its UUID")
    targets_end = text.index("        let mut tag = entity_nbt::mob_to_nbt", targets_start)
    text = text[:targets_start] + "        for reference in [references.target, references.attacker].into_iter().flatten() {\n            self.uuid_for(reference)?;\n        }\n" + text[targets_end:]
    anchor = "pub(super) trait MobNbtOwner {"
    assert text.count(anchor) == 1
    text = text.replace(anchor, helper + anchor, 1)
    if not check:
        path.write_text(text)
    print("Validated shared save predicates" if check else "Extracted shared save predicates")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    extract(args.root, args.check)
