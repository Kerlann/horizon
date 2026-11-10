#!/usr/bin/env python3
"""Expose engine-owned types for the registered mod host provider.

Fields remain private. Run from the repository root; --check verifies the result.
"""
import argparse
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    path = Path('crates/horizon-client/src/game.rs')
    text = path.read_text()
    for before, after in (
        ('mod host;', 'pub mod host;'),
        ("struct GameLevel<'w> {", "pub struct GameLevel<'w> {"),
        ("struct Game<'a> {", "pub struct Game<'a> {"),
    ):
        if after in text:
            continue
        if args.check:
            raise SystemExit(f'{path}: missing {after}')
        if text.count(before) != 1:
            raise SystemExit(f'{path}: expected one {before}')
        text = text.replace(before, after)
    if not args.check:
        path.write_text(text)


if __name__ == '__main__':
    main()
