#!/usr/bin/env python3
"""Record official server duplicate-UUID passenger lifecycle, without a Rust build.

Uses the dedicated-server parity console transport and verified official jars.
Output is private vanilla data; never commit the output directory.
"""
import argparse
import json
import os
from pathlib import Path
import re
import time

import lab
import test_rust_parity as base
from test_dedicated_server_parity import Server, free_port, game_time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True, help='new private output directory')
    parser.add_argument('--cases', nargs='+', choices=('root_child', 'siblings', 'leaf', 'loaded'),
                        default=('root_child', 'siblings', 'leaf', 'loaded'))
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    lock, jars, libraries = base.verify_artifacts()
    command = ['java', '-Xmx2g', '-cp', os.pathsep.join(map(str, [jars['server'], *libraries])),
               'net.minecraft.server.Main', 'nogui']
    world = out / 'server'
    world.mkdir()
    (world / 'eula.txt').write_text('eula=true\n')
    (world / 'server.properties').write_text(
        f'server-port={free_port()}\nonline-mode=false\nlevel-type=minecraft:flat\n'
        'generate-structures=false\nview-distance=2\nsimulation-distance=2\n'
        'pause-when-empty-seconds=-1\nmax-tick-time=-1\n')
    records = []
    serial = 0
    server = None

    def reply(text):
        nonlocal serial
        serial += 1
        marker = f'uuid_observation_end_{serial}'
        start = len(server.lines)
        server.console(text)
        server.console(f'say {marker}')
        server.wait_for(re.escape(f'[Server] {marker}') + '$')
        lines = [line for line in server.lines[start:] if marker not in line]
        records.append({'command': text, 'lines': lines})
        (out / 'records.json').write_text(json.dumps(records, indent=2))
        return lines

    def advance(ticks):
        before = game_time(server)
        reply(f'tick step {ticks}')
        deadline = time.monotonic() + 180
        while game_time(server) < before + ticks:
            assert time.monotonic() < deadline, 'tick step stalled'

    def observe(dimension=None):
        prefix = f'execute in minecraft:{dimension} run ' if dimension else ''
        for tag in ('root', 'first', 'rejected', 'grandchild', 'leaf'):
            reply(f'{prefix}data get entity @e[tag={tag},distance=0..,limit=1]')

    def start(label):
        nonlocal server
        server = Server(command, world, label)
        server.wait_for(r'Done \(')
        reply('tick freeze')
        reply('forceload add 0 0')
        # Blocking on the console command keeps all observations in server order.
        for _ in range(300):
            if any('Test passed' in line for line in reply('execute if loaded 0 200 0')):
                break
        else:
            raise AssertionError('summon chunk not loaded')

    def entity(tag, uuid, passengers=''):
        return ('{id:"minecraft:pig",UUID:[I;0,0,0,' + str(uuid) + '],Tags:["' + tag
                + '"],NoAI:1b,NoGravity:1b,Age:-1000'
                + (',Passengers:[' + passengers + ']' if passengers else '') + '}')

    try:
        start('initial')
        reply('gamerule minecraft:spawn_mobs false')
        # Each case gets distinct UUIDs so cleanup cannot mask admission behavior.
        for number, kind in enumerate(args.cases, 1):
            records.append({'case': kind})
            reply('kill @e[type=minecraft:pig]')
            u = number * 10
            if kind == 'root_child':
                passengers = entity('rejected', u, entity('grandchild', u + 2))
            elif kind == 'siblings':
                passengers = entity('first', u + 1) + ',' + entity('rejected', u + 1, entity('grandchild', u + 2))
            elif kind == 'leaf':
                passengers = entity('first', u + 1, entity('leaf', u + 1))
            else:
                reply('summon pig 2 200 2 ' + entity('first', u + 1))
                passengers = entity('rejected', u + 1, entity('grandchild', u + 2))
            reply('summon pig 1 200 1 ' + entity('root', u, passengers))
            observe()
            advance(5)
            observe()
            reply('save-all flush')
            server.stop()
            start(f'reload_{kind}')
            observe()
            reply('execute as @e[tag=grandchild] run ride @s dismount')
            advance(5)
            observe()
            reply('execute in minecraft:the_nether run forceload add 0 0')
            for _ in range(300):
                if any('Test passed' in line for line in reply(
                        'execute in minecraft:the_nether if loaded 0 200 0')):
                    break
            else:
                raise AssertionError('destination chunk not loaded')
            reply('execute as @e[tag=root] in minecraft:the_nether run teleport @s 1 200 1')
            observe('the_nether')
            observe()
            advance(5)
            observe('the_nether')
            reply('execute in minecraft:the_nether run kill @e[tag=root,distance=0..]')
            reply('kill @e[tag=root]')
            observe()
        (out / 'version.json').write_text(json.dumps(lock['minecraft'], indent=2))
    finally:
        if server is not None:
            server.stop()


if __name__ == '__main__':
    main()
