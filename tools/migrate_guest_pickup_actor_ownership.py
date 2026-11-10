#!/usr/bin/env python3
"""Mechanical stable-join nominations and per-family actor implementations.

Run once from the checkout root, before the separate semantic callback/loan
migration. No pickup amounts, RNG calls, effects or callback order change here.
"""
from pathlib import Path

path = Path("crates/horizon-client/src/game/network/lan_actions/pickups.rs")
text = path.read_text()
for old, new in [
    (".map(|d| d.id)\n            .collect();", ".map(|d| (d.id, d.join))\n            .collect();"),
    ("for item_id in near.into_iter().rev() {", "for (item_id, item_join) in near.into_iter().rev() {"),
    ("let Some(item) = self.dropped.iter().find(|d| d.id == item_id) else {", "let Some(index) = self.dropped.locate(item_join) else {\n                continue;\n            };\n            let item = &self.dropped[index];\n            if item.id != item_id {"),
    ("let Some(i) = self.dropped.iter().position(|d| d.id == item_id) else {", "let Some(i) = self.dropped.locate(item_join) else {"),
    ("let touching: Vec<usize> = self", "let touching: Vec<_> = self"),
    (".map(|(i, _)| i)\n            .collect();", ".map(|(_, orb)| orb.join)\n            .collect();"),
    ("let index = touching[pick];", "let join = touching[pick];\n        let Some(index) = self.orbs.locate(join) else { return };"),
]:
    assert text.count(old) == 1, old
    text = text.replace(old, new)
path.write_text(text)
for module, type_name, kind, field in [
    ("item", "ItemEntity", "Item", "dropped"),
    ("orb", "ExperienceOrb", "Orb", "orbs"),
]:
    Path(f"crates/horizon-client/src/game/entities/{module}_actor.rs").write_text(
        "//! The real pickup body's owner, using the shared actor carrier.\n"
        "use super::{Game, entity_actor_loan::EntityActor};\n"
        f"use horizon_client::entities::{type_name};\n"
        "use horizon_client::join_seq::{JoinList, residency::Kind};\n"
        f"impl EntityActor for {type_name} {{\n"
        "    type Storage = JoinList<Self>;\n"
        f"    const KIND: Kind = Kind::{kind};\n"
        "    fn id(&self) -> u32 { self.id }\n"
        f"    fn target(&self) -> super::commands::Target {{ super::commands::Target::{kind}(self.id) }}\n"
        "    fn owner<'a, 'g>(game: &'a mut Game<'g>) -> &'a mut Self::Storage {\n"
        f"        &mut game.{field}\n"
        "    }\n}\n"
    )
