#!/usr/bin/env python3
"""Defer pending selection until existing H projectors select actual storage.

Only methods already using an explicit source projector are rewritten. Legacy
current-level-only bodies are deliberately left for semantic conversion.
"""
from entity_h_borrowed_routes import GAME, body_end
import re


def main():
    changed = 0
    for path in GAME.rglob('*.rs'):
        source = path.read_text()
        edits = []
        for match in re.finditer(r'pub\([^)]*\) fn read_\w+\s*\(', source):
            opening = source.index('{', match.end())
            end = body_end(source, opening)
            body = source[opening + 1:end - 1]
            guard = 'self.entity_read_dependency::<core::convert::Infallible>('
            if guard not in body or not any(call in body for call in [
                'self.with_live_entity_source(', 'self.with_live_minecart_read('
            ]):
                continue
            body = body.replace(guard, 'self.entity_read_admission(')
            body = body.replace('Some(read) => read?,', 'Some(read) => read,')
            if 'self.with_live_entity_source(' in body:
                body = body.replace('self.with_live_entity_source(read?,', 'self.with_admitted_entity_source(read,')
                body, count = re.subn(r'\s*\.ok_or\(mod_api::EntityReadError::Lifted\)\s*$', '\n    ', body)
                assert count == 1, path
            edits.append((opening + 1, end - 1, body))
        for start, end, replacement in reversed(edits):
            source = source[:start] + replacement + source[end:]
        if edits:
            path.write_text(source)
            changed += len(edits)
            print(path)
    print(f'Updated {changed} existing source projectors.')


if __name__ == '__main__':
    main()
