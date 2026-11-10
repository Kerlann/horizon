#!/usr/bin/env python3
"""Thread concrete body providers into the one admission boundary for OWN.

A provider proves an actual matching loan even without a writer frame. OWN
must not inherit a foreign Pending marker. This edits engine call sites only;
no mod API signature changes. Commit the script before applying it.
"""
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[2]
BASE = ROOT / 'crates/horizon-client/src/game/modding'


def apply():
    for path in BASE.rglob('*.rs'):
        source = path.read_text()
        edits = []
        for match in re.finditer(r'self\.entity_read_admission\(', source):
            start = match.end()
            depth, end = 1, start
            while depth:
                if source[end] == '(': depth += 1
                elif source[end] == ')': depth -= 1
                end += 1
            body = source[start:end-1]
            # Do not add another argument on reruns.
            depth, commas = 0, []
            for i, ch in enumerate(body):
                if ch in '([{': depth += 1
                elif ch in ')]}': depth -= 1
                elif ch == ',' and depth == 0: commas.append(i)
            fields = []
            previous = 0
            for comma in commas:
                fields.append(body[previous:comma].strip()); previous = comma+1
            if body[previous:].strip(): fields.append(body[previous:].strip())
            if len(fields) == 3: continue
            assert len(fields) == 2, (path, fields)
            declarations = list(re.finditer(r'\bfn\s+(\w+)\s*[<(]', source[:match.start()]))
            declaration = declarations[-1]
            signature = source[declaration.start():source.index('{', declaration.start())]
            provider = 'provider' if re.search(r'\bprovider\s*:', signature) else 'None'
            insert = start + len(body.rstrip())
            comma = '' if body.rstrip().endswith(',') else ','
            edits.append((insert, comma+' '+provider))
            print(f'{path.relative_to(ROOT)}: {declaration.group(1)} -> {provider}')
        for pos, addition in reversed(edits): source = source[:pos]+addition+source[pos:]
        if edits: path.write_text(source)


if __name__ == '__main__': apply()
