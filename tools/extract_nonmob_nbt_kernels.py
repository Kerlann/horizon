#!/usr/bin/env python3
"""Extract actual Other/display/Boat-leash kernels from their Game wrappers.

This one-shot move precedes the owner-local NBT adapter. No body is copied.
--check validates all anchors without writing.
"""
from pathlib import Path
import argparse


def method(text, name):
    start = text.index(f"    pub(super) fn {name}(")
    end = text.index("\n    }\n", start) + len("\n    }\n")
    source = text[start:end]
    body = source[source.index("{") + 1:source.rindex("}")]
    return source, body


def extract(root, check):
    base = root / "crates/horizon-client/src/game"
    persist_path = base / "world/persist.rs"
    display_path = base / "world/persist/display.rs"
    boat_path = base / "entities/boat_leads.rs"
    persist, display, boat = (p.read_text() for p in (persist_path, display_path, boat_path))
    other_method, other_body = method(persist, "other_to_nbt")
    display_method, display_body = method(display, "write_display_nbt")
    boat_method, boat_body = method(boat, "write_boat_leash")
    old_raw = "let mut tag = self\n            .persist\n            .raw\n            .get(&o.id)"
    assert old_raw in other_body
    other_body = other_body.replace(old_raw, "let mut tag = raw")
    other_body = other_body.replace("self.write_display_nbt(o, &mut tag)", "display::write_display_nbt(o, &mut tag, registry, items)")
    other_kernel = (
        "pub(super) fn other_to_nbt(o: &super::commands::OtherEntity, uuid: [i32; 4], raw: Option<&Tag>, registry: &horizon_world::registry::BlockRegistry, items: &horizon_items::items::Items) -> Tag {"
        + other_body + "}\n\n"
    )
    wrapper = "    pub(super) fn other_to_nbt(&self, o: &super::commands::OtherEntity, uuid: [i32; 4]) -> Tag {\n        other_to_nbt(o, uuid, self.persist.raw.get(&o.id), self.pack.assets.registry, self.items())\n    }\n"
    persist = persist.replace(other_method, wrapper, 1)
    persist = persist.replace("impl Game<'_> {", other_kernel + "impl Game<'_> {", 1)
    persist = persist.replace("fn anon_raw(", "pub(super) fn anon_raw(", 1)
    display_body = display_body.replace("self.state_from_nbt(value)", "entity_nbt::state_from_nbt(registry, value)")
    display_body = display_body.replace("self.state_to_nbt(s)", "entity_nbt::state_to_nbt(registry, s)")
    display_body = display_body.replace("self.items()", "items")
    display_kernel = "pub(in crate::game) fn write_display_nbt(entity: &OtherEntity, tag: &mut Tag, registry: &horizon_world::registry::BlockRegistry, items: &horizon_items::items::Items) {" + display_body + "}\n"
    start = display.index("impl Game<'_> {")
    end = display.index("\n}\n", start) + len("\n}\n")
    assert display_method in display[start:end]
    display = display[:start] + display_kernel + display[end:]
    display = display.replace("{Game, commands::OtherEntity,", "{commands::OtherEntity,", 1)
    match_start = boat_body.index("        let value = match")
    leash_body = boat_body[match_start:].replace("self.mob_uuid(id)", "uuid_for(id)").replace("self.uuid_for(id)", "uuid_for(id)")
    leash_kernel = "pub(super) fn write_boat_leash_tag(tag: &mut Tag, holder: Option<Holder>, pending: Option<Pending>, player: Option<[i32; 4]>, mut uuid_for: impl FnMut(u32) -> [i32; 4]) {\n        let Tag::Compound(map) = tag else { return };\n        map.remove(&\"leash\".into());\n" + leash_body + "}\n\n"
    boat_wrapper = "    pub(super) fn write_boat_leash(&mut self, tag: &mut Tag, index: usize) {\n        let b = &self.boats[index];\n        let (holder, pending, player) = (b.leash, b.leash_pending, self.boat_leash_player(b));\n        write_boat_leash_tag(tag, holder, pending, player, |id| self.uuid_for(id));\n    }\n"
    boat = boat.replace(boat_method, boat_wrapper, 1)
    boat = boat.replace("impl Game<'_> {", leash_kernel + "impl Game<'_> {", 1)
    if not check:
        for path, content in ((persist_path, persist), (display_path, display), (boat_path, boat)):
            path.write_text(content)
    print("Validated kernels" if check else "Extracted actual body-only kernels")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    extract(args.root, args.check)
