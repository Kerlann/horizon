#!/usr/bin/env python3
"""Remove superseded hand-written GoalCx H bridges after universal generation.

The canonical H inventory comes from API signatures, not a maintained list.
Refuse to remove a bridge still called by retained engine code.
"""
from entity_h_borrowed_routes import GAME, ROOT, body_end, heavy_names
import re


def main():
    names = heavy_names()
    directory = GAME / 'modding/native_goal_ticks'
    changes = {}
    count = 0
    for path in directory.glob('*.rs'):
        source = path.read_text()
        edits = []
        for match in re.finditer(r'^    (?:pub\([^)]*\) )?fn r_(\w+)\s*\(', source, re.M):
            if match[1] not in names:
                continue
            opening = source.index('{', match.end())
            end = body_end(source, opening)
            start = match.start()
            # Remove this method's attached documentation, not its neighbor's.
            while start:
                previous = source.rfind('\n', 0, start - 1) + 1
                line = source[previous:start].strip()
                if line.startswith('///'):
                    start = previous
                else:
                    break
            edits.append((start, end))
        for start, end in reversed(edits):
            source = source[:start] + source[end:]
        if edits:
            count += len(edits)
            changes[path] = source
    remaining = '\n'.join(changes.get(path, path.read_text()) for path in directory.glob('*.rs'))
    dangling = sorted(name for name in names if re.search(r'\br_' + name + r'\s*\(', remaining))
    assert not dangling, f'Retained callers require semantic conversion first: {dangling}'
    host = ROOT / 'crates/horizon-mod-api-gen/src/host.rs'
    source = host.read_text()
    start = source.index('pub(super) const GOAL_VIEW_READS:')
    end = source.index('];', start)
    inventory = source[start:end]
    inventory = re.sub(r'^\s*"(\w+)",\n', lambda m: '' if m[1] in names else m[0], inventory, flags=re.M)
    updated = source[:start] + inventory + source[end:]
    if updated != source:
        changes[host] = updated
    for path, content in changes.items():
        path.write_text(content)
        print(path.relative_to(ROOT))
    print(f'Removed {count} superseded GoalCx H bridges.')


if __name__ == '__main__':
    main()
