"""One-off, idempotent extraction of the existing Mob NBT runtime-preservation kernel.

This does not add parked mutation routing. It makes the unchanged runtime load
step reusable by active commands and later dimension-local immediate writes.
"""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / 'crates/horizon-client/src/game/administration/commands/data.rs'
DESTINATION = ROOT / 'crates/horizon-client/src/game/modding/native_mob_nbt_runtime.rs'
CALL = 'Self::reload_mob_runtime(slot, new, &tag, self.world.game_time);'


def main():
    source = SOURCE.read_text()
    if CALL in source:
        if not DESTINATION.exists():
            raise ValueError('runtime call exists without its extracted implementation')
        print('Mob NBT runtime extraction already applied')
        return
    if DESTINATION.exists():
        raise ValueError('refusing to overwrite an existing runtime implementation')
    method = source.index('pub(in crate::game) fn apply_entity_nbt(')
    mob = source.index('            Target::Mob(id) => {', method)
    anchor = '                if let Some(slot) = self.mobs.iter_mut().find(|m| m.id == id) {'
    start = source.index(anchor, mob) + len(anchor)
    depth = 1
    end = start
    while depth:
        if source[end] == '{':
            depth += 1
        elif source[end] == '}':
            depth -= 1
        end += 1
    original = source[start:end - 1]
    body = '\n'.join(line[12:] if line.startswith(' ' * 12) else line
                     for line in original.splitlines()).rstrip()
    body = body.replace('self.world.game_time', 'game_time')
    body = body.replace('entity_nbt::read_mob_home(slot, &tag)',
                        'horizon_client::entity_nbt::read_mob_home(slot, tag)')
    helper = '''//! Runtime state preserved when loading NBT into an existing actual Mob.
use super::Game;
use horizon_client::mobs::Mob;
use horizon_core::nbt::Tag;

impl Game<'_> {
    pub(in crate::game) fn reload_mob_runtime(
        slot: &mut Mob,
        mut new: Mob,
        tag: &Tag,
        game_time: i64,
    ) {'''+ body + '''
    }
}
'''
    prefix = source[:start]
    declaration = prefix.rfind('let mut new = entity_nbt::mob_from_nbt(', mob)
    if declaration < 0:
        raise ValueError('expected the existing Mob decode before runtime restoration')
    prefix = prefix[:declaration] + prefix[declaration:].replace('let mut new =', 'let new =', 1)
    updated = prefix + '\n                    ' + CALL + '\n                ' + source[end - 1:]
    DESTINATION.write_text(helper)
    SOURCE.write_text(updated)
    print('Extracted existing Mob runtime-preservation body without changing its steps')


if __name__ == '__main__':
    main()
