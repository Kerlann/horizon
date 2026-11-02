#!/usr/bin/env python3
"""Simulate a downloaded SDK by copying it and making original artifacts unavailable."""
import json
import os
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[2]
WORK = ROOT / "target/dylib-spike"
original = WORK / "game"
copy = WORK / "downloaded-game"
mods = WORK / "downloaded-mods"
assert not copy.exists(), "use a fresh destination"
shutil.copytree(original, copy)
mods.mkdir(exist_ok=True)
(mods / "order").write_text("machine_sample\n")
hidden = WORK / "original-game-unavailable"
fast = ROOT / "target/fast"
hidden_fast = ROOT / "target/fast-unavailable"
original.rename(hidden)
try:
    fast.rename(hidden_fast)
    subprocess.run(["python3", str(ROOT / "tools/dylib_spike.py"), "--game", str(copy),
        "build-mod", "machine_sample", str(WORK / "sources/machine/examples/trait-api/machine-hooks"),
        "--mods", str(mods)], check=True)
    sysroot = Path(subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip())
    env = dict(os.environ, LD_LIBRARY_PATH=f"{copy}:{copy / 'deps'}:{sysroot / 'lib/rustlib/x86_64-unknown-linux-gnu/lib'}")
    log = WORK / "relocated-dedicated.log"
    with log.open("w") as output:
        subprocess.run(["python3", str(ROOT / "tools/dylib_spike/dedicated.py"),
            "--label", "downloaded", "--binary", str(copy / "horizon-dylib-game"),
            "--mods", str(mods)], env=env, stdout=output, stderr=subprocess.STDOUT, check=True)
    run_dir = Path(log.read_text().splitlines()[0])
    runtime = (run_dir / "run.log").read_text()
    result = dict(original_game_unavailable=not original.exists(),
                  original_cargo_target_unavailable=not fast.exists(),
                  mod_loaded="DYLIB_SPIKE loaded machine_sample" in runtime,
                  server_hook_ran="MACHINE_SAMPLE server clock: 200 ticks" in runtime,
                  game_rebuilt=False, directory=str(run_dir))
    assert result["mod_loaded"] and result["server_hook_ran"]
    (WORK / "relocated-result.json").write_text(json.dumps(result, indent=2))
    print(json.dumps(result, indent=2))
finally:
    hidden.rename(original)
    if hidden_fast.exists():
        hidden_fast.rename(fast)
