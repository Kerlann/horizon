"""Stage existing Mob save methods into one owner-local storage adapter.

Mechanical first stage only: subsequent hand edits replace Game field access
with typed owner accessors and implement the real parked adapter. No body is
cloned and no dimension switch is introduced by this extraction.
"""
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / 'crates/horizon-client/src/game/world/persist.rs'
DESTINATION = ROOT / 'crates/horizon-client/src/game/modding/native_mob_nbt_save.rs'
METHODS = {
    'mob_tree_nbt': 'i',
    'put_last_hurt_by_mob': 'm, tag',
    'put_mob_passengers': 'tag, id',
}


def main():
    source = SOURCE.read_text()
    marker = 'super::native_mob_nbt_save::MobNbtOwner::'
    if marker in source:
        if not DESTINATION.exists():
            raise ValueError('owner adapter calls exist without their implementation')
        print('Mob NBT save extraction already staged')
        return
    if DESTINATION.exists():
        raise ValueError('refusing to overwrite an existing save implementation')
    extracted = []
    replacements = []
    for name, arguments in METHODS.items():
        matches = list(re.finditer(r'    pub\(super\) fn ' + name + r'\(', source))
        if len(matches) != 1:
            raise ValueError(f'expected exactly one existing {name} method')
        start = matches[0].start()
        opening = source.index('{', matches[0].end())
        depth = 1
        end = opening + 1
        while depth:
            if source[end] == '{':
                depth += 1
            elif source[end] == '}':
                depth -= 1
            end += 1
        method = source[start:end].replace('pub(super) ', '', 1)
        extracted.append(method)
        call = marker + name + '(self, ' + arguments + ')'
        replacements.append((opening + 1, end - 1, '\n        ' + call + '\n    '))
    for start, end, replacement in sorted(replacements, reverse=True):
        source = source[:start] + replacement + source[end:]
    header = '''//! Actual Mob save behavior shared by active and parked owner storage.
use super::{Game, persist::{EntityMemory, PassengerSlot}};
use horizon_client::{entity_nbt, join_seq::JoinList, mobs::{Mob, Who}};
use horizon_core::nbt::Tag;
use std::collections::HashSet;

pub(super) trait MobNbtOwner {
'''
    DESTINATION.write_text(header + '\n\n'.join(extracted) + '\n}\n')
    SOURCE.write_text(source)
    print('Staged original save bodies; owner-access hand corrections are required')


if __name__ == '__main__':
    main()
