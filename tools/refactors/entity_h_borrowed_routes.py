#!/usr/bin/env python3
"""Add explicit borrowed-source parameters to signed H implementations.

The script changes plumbing only: public signatures and existing bodies stay
unchanged. Semantic body/source conversions are reviewed in later commits.
Run at the repository root; rerunning is a no-op.
"""
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[2]
HOST = ROOT / 'crates/horizon-mod-api/src/host'
GAME = ROOT / 'crates/horizon-client/src/game/modding'


def body_end(source, opening):
    """Find an item's closing brace, ignoring strings and Rust comments."""
    i, depth = opening + 1, 1
    while depth:
        if source.startswith('//', i):
            i = source.index('\n', i) + 1
            continue
        if source.startswith('/*', i):
            comments, i = 1, i + 2
            while comments:
                if source.startswith('/*', i):
                    comments += 1
                    i += 2
                elif source.startswith('*/', i):
                    comments -= 1
                    i += 2
                else:
                    i += 1
            continue
        if source[i] == '"':
            i += 1
            while source[i] != '"':
                i += 2 if source[i] == '\\' else 1
            i += 1
            continue
        depth += (source[i] == '{') - (source[i] == '}')
        i += 1
    return i


def arguments(text):
    chunks, start, depth = [], 0, 0
    for i, c in enumerate(text):
        if c in '(<[':
            depth += 1
        elif c in ')>]':
            depth -= 1
        elif c == ',' and depth == 0:
            chunks.append(text[start:i].strip())
            start = i + 1
    chunks.append(text[start:].strip())
    return [part for part in chunks if part]


def main():
    names = set()
    for path in HOST.rglob('*.rs'):
        source = path.read_text()
        for match in re.finditer(r'\bfn\s+(\w+)\s*\(', source):
            depth, end = 0, match.start()
            while True:
                c = source[end]
                if c in '([':
                    depth += 1
                elif c in ')]':
                    depth -= 1
                elif c in ';{' and depth == 0:
                    break
                end += 1
            if re.search(r'\bEntityReadError\b', source[match.start():end]):
                names.add(match[1])
    assert len(names) == 66, f'Expected the signed 66 H rows, got {len(names)}'
    files = {path: path.read_text() for path in GAME.glob('native*.rs')}
    edits = {}
    for name in sorted(names):
        helper = 'read_' + name
        if any(re.search(r'\bfn\s+' + helper + r'\s*\(', source) for source in files.values()):
            continue
        pattern = re.compile(r'^    pub\([^)]*\) fn ' + name + r'\s*\(', re.M)
        matches = [(path, match) for path, source in files.items() for match in pattern.finditer(source)]
        assert len(matches) == 1, (name, [str(path) for path, _ in matches])
        path, match = matches[0]
        source = files[path]
        opening = source.index('{', match.end())
        end = body_end(source, opening)
        signature = source[match.start():opening].rstrip()
        parameters_start = signature.index('(' , signature.index('fn '))
        level, closing = 1, parameters_start + 1
        while level:
            level += (signature[closing] == '(') - (signature[closing] == ')')
            closing += 1
        params = arguments(signature[parameters_start + 1:closing - 1])
        assert params[0] == '&self', (name, params)
        args = ', '.join(part.split(':', 1)[0].strip() for part in params[1:])
        call = f'self.{helper}(None' + (f', {args}' if args else '') + ')'
        wrapper = signature + ' {\n        ' + call + '\n    }\n\n'
        private = signature.replace('fn ' + name, 'fn ' + helper, 1)
        receiver = private.index('&self') + len('&self')
        private = private[:receiver] + ', _provider: Option<&dyn super::native_entity_body_provider::EntityBodyProvider>' + private[receiver:]
        replacement = wrapper + private + ' ' + source[opening:end]
        edits.setdefault(path, []).append((match.start(), end, replacement))
    for path, changes in edits.items():
        source = files[path]
        for start, end, replacement in sorted(changes, reverse=True):
            source = source[:start] + replacement + source[end:]
        path.write_text(source)
        print(path.relative_to(ROOT))
    print(f'Added {sum(map(len, edits.values()))} explicit borrowed-source routes.')


if __name__ == '__main__':
    main()
