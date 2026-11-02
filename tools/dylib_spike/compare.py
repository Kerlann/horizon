#!/usr/bin/env python3
"""Compare decoded blocks, block/entity NBT and player NBT in two dedicated saves.

Reuse the existing save harness's palette decoding. No gameplay field is
normalized away: missing chunks/entities/players are differences. Region file
layout, compression, timestamps and palette ordering are not gameplay state.
"""
import argparse
from collections import Counter
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))
import anvil_dump
import test_save_roundtrip_parity as saves

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("static", type=Path)
parser.add_argument("dynamic", type=Path)
args = parser.parse_args()


def snapshot(world):
    blocks, block_entities, entities, players = {}, {}, {}, {}
    for region in sorted(world.glob("dimensions/**/region/r.*.*.mca")):
        dimension = str(region.parent.parent.relative_to(world))
        for slot, root in anvil_dump.region_chunks(str(region)).items():
            key = f"{dimension}/{region.name}/{slot}"
            data, sections = saves.normalize_chunk(root)
            blocks[key] = {str(y): section["blocks"] for y, section in sections.items()}
            block_entities[key] = {str(pos): value for pos, value in data["block_entities"].items()}
    for region in sorted(world.glob("dimensions/**/entities/r.*.*.mca")):
        dimension = str(region.parent.parent.relative_to(world))
        for root in anvil_dump.region_chunks(str(region)).values():
            for entity in saves.unwrap(root).get("Entities", []):
                entities[f"{dimension}/{entity.get('UUID')}"] = entity
    for player in sorted(world.glob("players/data/*.dat")):
        players[player.name] = anvil_dump.read_dat(str(player))
    return dict(blocks=blocks, block_entities=block_entities, entities=entities, players=players)


left, right = snapshot(args.static), snapshot(args.dynamic)
differences = []
for key in left:
    saves.diff(left[key], right[key], key, differences)
result = dict(equal=not differences,
              counts={kind: {key: len(data) for key, data in snap.items()}
                      for kind, snap in (("static", left), ("dynamic", right))},
              difference_count=len(differences),
              differences_by_kind=dict(Counter(row[0].split('.')[0] for row in differences)),
              first_differences=differences[:20])
print(json.dumps(result, indent=2, default=str))
