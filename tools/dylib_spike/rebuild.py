#!/usr/bin/env python3
"""Time a one-line sample edit, compile, restart and observable in-world hook."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import time
from dedicated import ROOT, WORK, request, scratch_env

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("package", type=Path)
parser.add_argument("--run-dir", type=Path, required=True)
args = parser.parse_args()
source = args.package.resolve() / "src/lib.rs"
original = source.read_text()
mods = WORK / "rebuild-mods"
mods.mkdir(exist_ok=True)
(mods / "order").write_text("machine_sample\n")
game = WORK / "game/libhorizon_client.so"
before = hashlib.sha256(game.read_bytes()).hexdigest()
env = scratch_env(args.run_dir.resolve())
env.update(HORIZON_DYLIB_MODS=str(mods), DISPLAY=":77")
start = time.monotonic()
assert "const CLOCK_INTERVAL: u64 = 200;" in original
try:
    source.write_text(original.replace("const CLOCK_INTERVAL: u64 = 200;", "const CLOCK_INTERVAL: u64 = 201;", 1))
    subprocess.run(["python3", str(ROOT / "tools/dylib_spike.py"), "build-mod", "machine_sample",
                    str(args.package.resolve()), "--mods", str(mods)], check=True)
    compiled = time.monotonic()
    log = args.run_dir.resolve() / "rebuild.log"
    with log.open("w") as output:
        process = subprocess.Popen(["python3", str(ROOT / "tools/dylib_spike.py"), "run",
            "--resources", str(ROOT / ".local/horizon-client/26.3"), "--world", "Spike",
            "--seed", "2603", "--creative", "--offscreen", "--control-socket",
            str(args.run_dir.resolve() / "rebuild.sock")], env=env, stdout=output, stderr=subprocess.STDOUT)
        try:
            deadline = time.monotonic() + 180
            state = None
            sock = args.run_dir.resolve() / "rebuild.sock"
            while time.monotonic() < deadline and process.poll() is None:
                if sock.exists():
                    state = request(sock, "state")
                    if (state.get("world") == "Spike" and state.get("screen") == "playing"
                            and not state.get("busy") and state.get("player") is not None
                            and state.get("ticks", 0) > 0):
                        break
                time.sleep(.05)
            else:
                raise RuntimeError(f"world failed to become ready: {log}")
            ready = time.monotonic()
            # Show that the edited hook is also running, not merely mapped into the process.
            deadline = time.monotonic() + 60
            while "server clock: 201 ticks" not in log.read_text() and time.monotonic() < deadline:
                time.sleep(.05)
            hook_seen = "server clock: 201 ticks" in log.read_text()
            result = dict(build_and_preflight_seconds=compiled-start,
                          edit_to_world_seconds=ready-start,
                          edit_to_edited_hook_seconds=time.monotonic()-start,
                          edited_hook_seen=hook_seen,
                          game_hash_unchanged=before == hashlib.sha256(game.read_bytes()).hexdigest(),
                          state=state)
            (WORK / "rebuild-result.json").write_text(json.dumps(result, indent=2))
            print(json.dumps(result, indent=2))
            request(sock, "quit")
            process.wait(timeout=90)
        finally:
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=30)
finally:
    source.write_text(original)
