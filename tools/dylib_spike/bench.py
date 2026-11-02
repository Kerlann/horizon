#!/usr/bin/env python3
"""Three alternating static/dynamic runs, means over world ticks (1000, 6000]."""
import json
import os
from pathlib import Path
import re
import statistics
import subprocess

ROOT = Path(__file__).resolve().parents[2]
WORK = ROOT / "target/dylib-spike"
mods = WORK / "benchmark-mods"
mods.mkdir(exist_ok=True)
(mods / "order").write_text("machine_sample\n")
for name in ("machine_sample.so", "machine_sample.json"):
    path = mods / name
    if not path.exists():
        path.symlink_to(WORK / "mods" / name)
results = {"static": [], "dynamic": []}
env = dict(os.environ, HORIZON_DYLIB_SPIKE_TIMINGS="1")
for i in range(3):
    for kind in ("static", "dynamic"):
        log = WORK / f"bench-{kind}-{i + 1}.log"
        args = ["python3", str(ROOT / "tools/dylib_spike/dedicated.py"),
                "--label", f"bench-{kind}-{i + 1}"]
        args += (["--binary", str(WORK / "static-machine-game")] if kind == "static"
                 else ["--mods", str(mods)])
        print(" ".join(args), flush=True)
        with log.open("w") as output:
            subprocess.run(args, env=env, stdout=output, stderr=subprocess.STDOUT, check=True)
        folder = Path(log.read_text().splitlines()[0])
        all_samples = [(int(tick), int(ns)) for tick, ns in re.findall(
            r"DYLIB_SPIKE_TICK_NS (\d+) (\d+)", (folder / "run.log").read_text())]
        samples = [ns for tick, ns in all_samples if 1000 < tick <= 6000]
        assert len(samples) == 5000, f"expected 5000 samples, got {len(samples)} in {folder}"
        mean = statistics.mean(samples) / 1e6
        results[kind].append(dict(directory=str(folder), ticks=len(samples), mean_ms=mean))
        print(kind, i + 1, mean, flush=True)
medians = {kind: statistics.median(row["mean_ms"] for row in rows)
           for kind, rows in results.items()}
ratio = medians["dynamic"] / medians["static"]
report = dict(runs=results, median_mean_ms=medians, ratio=ratio,
              relative_difference_percent=(ratio - 1) * 100, pass_within_one_percent=abs(ratio - 1) <= .01)
(WORK / "benchmark-result.json").write_text(json.dumps(report, indent=2))
print(json.dumps(report, indent=2))
