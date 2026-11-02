#!/usr/bin/env python3
"""Trigger either sample/server or Zoom/client hook panic in an integrated game."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
from dedicated import ROOT, WORK, request, scratch_env

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("kind", choices=("machine_sample", "zoom"))
args = parser.parse_args()
folder = Path(tempfile.mkdtemp(prefix="panic-client-", dir=WORK)).resolve()
mods = folder / "mods"
mods.mkdir()
source = WORK / ("panic-sample" if args.kind == "machine_sample" else "mods")
for suffix in ("so", "json"):
    shutil.copy(source / f"{args.kind}.{suffix}", mods / f"{args.kind}.{suffix}")
(mods / "order").write_text(args.kind + "\n")
env = scratch_env(folder)
env.update(HORIZON_DYLIB_MODS=str(mods), DYLIB_SPIKE_PANIC="1", DISPLAY=":77")
log = folder / "run.log"
with log.open("w") as output:
    process = subprocess.Popen(["python3", str(ROOT / "tools/dylib_spike.py"), "run",
        "--resources", str(ROOT / ".local/horizon-client/26.3"), "--world", "Panic",
        "--seed", "2603", "--creative", "--offscreen", "--control-socket", str(folder / "ctl.sock")],
        env=env, stdout=output, stderr=subprocess.STDOUT)
    try:
        deadline = time.monotonic() + 180
        triggered = False
        reports = []
        hook = "level_tick_pre" if args.kind == "machine_sample" else "key"
        marker = f"in native mod {args.kind}, hook {hook}"
        while time.monotonic() < deadline:
            reports = list(folder.glob("**/crash-reports/*.txt"))
            if reports or marker in log.read_text():
                break
            if args.kind == "zoom" and not triggered and (folder / "ctl.sock").exists():
                state = request(folder / "ctl.sock", "state")
                if state.get("player") is not None and state.get("screen") == "playing":
                    request(folder / "ctl.sock", "key z down")
                    triggered = True
            if process.poll() is not None:
                break
            time.sleep(.05)
        found = [str(path) for path in reports if marker in path.read_text()]
        assert found or marker in log.read_text(), f"missing {marker!r}: {log}"
        result = dict(kind=args.kind, attribution=marker, reports=found, log=str(log))
        (WORK / f"panic-{args.kind}-result.json").write_text(json.dumps(result, indent=2))
        print(json.dumps(result, indent=2))
    finally:
        if process.poll() is None:
            process.terminate()
            process.wait(timeout=30)
