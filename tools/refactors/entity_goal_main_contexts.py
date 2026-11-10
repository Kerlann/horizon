#!/usr/bin/env python3
"""Port four newly merged real selector fixtures to the source carrier.

The active selector supplies its view inside tick(); the inactive target
selector is the actual taken object, not an invented empty read slice. Safe to
run on an older tree where these tests have not arrived yet.
"""
from pathlib import Path
import argparse

TESTS = (
    ('aquatic', 'a_spectator_does_not_puff_a_pufferfish', 'fish'),
    ('enderman', 'a_spectator_staring_does_not_anger_an_enderman', 'm'),
    ('raider', 'a_banner_chase_stops_when_the_raid_ends', 'm'),
    ('tame', 'a_spectator_holding_a_bone_does_not_make_a_wolf_beg', 'm'),
)


def apply(root):
    for file, name, mob in TESTS:
        path = root / 'crates/horizon-client/src/mobs' / (file + '.rs')
        text = path.read_text()
        needle = 'fn ' + name + '('
        if needle not in text: continue
        start = text.index(needle)
        brace = text.index('{', start)
        depth, end = 1, brace + 1
        while depth:
            if text[end] == '{': depth += 1
            elif text[end] == '}': depth -= 1
            end += 1
        body = text[start:end]
        if 'entity_inactive_targets' in body: continue
        take = f'let mut goals = std::mem::take(&mut {mob}.ai.goals);'
        assert body.count(take) == 1, (file, name)
        body = body.replace(take, '')
        old = 'let mut cx = Cx {'
        indent = body[body.rfind('\n', 0, body.index(old))+1:body.index(old)]
        new = (f'let mut goals = {mob}.ai.goals.take();\n{indent}'
               f'let entity_inactive_targets = {mob}.ai.targets.take();\n{indent}'
               'let mut cx = Cx {\n' + indent + '    selectors: crate::mobs::goals::GoalSelectorSources {\n'
               + indent + '        goals: None,\n'
               + indent + '        targets: entity_inactive_targets.read_source(),\n'
               + indent + '        active: crate::mobs::goals::SelectorKind::Goals,\n'
               + indent + '    },')
        body = body.replace(old, new, 1)
        restore = f'{mob}.ai.goals = goals;'
        body = body.replace(restore, f'{mob}.ai.targets = entity_inactive_targets;\n{indent}' + restore)
        path.write_text(text[:start] + body + text[end:])
        print(file + ': ' + name)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[2])
    apply(parser.parse_args().root)
