#!/usr/bin/env python3
"""Build a read-only observer in both linking modes; locate first divergence."""
from collections import defaultdict
import json
from pathlib import Path
import shutil
import subprocess
from dedicated import ROOT, WORK
import sys
sys.path.insert(0, str(ROOT / "tools"))
from dylib_spike import compiler, run

game = WORK / "game"
mods = WORK / "trace-mods"
mods.mkdir(exist_ok=True)
run([*compiler(game), "--crate-type=dylib", "--crate-name", "trace_observer",
     ROOT / "tools/dylib_spike/trace.rs", "-o", mods / "trace.so"])
shutil.copy(game / "identity.json", mods / "trace.json")
(mods / "order").write_text("trace\n")
fast = WORK / "static-target/fast"
threads = list((fast / "deps").glob("libhorizon_threads-*.rlib"))
assert len(threads) == 1
main = WORK / "static-trace.rs"
main.write_text(f'''#[path = {json.dumps(str(ROOT / 'tools/dylib_spike/trace.rs'))}]
mod trace;
#[global_allocator]
static ALLOCATOR: horizon_threads::Allocator = horizon_threads::Allocator;
fn main() {{
    horizon_client::mod_api::name_mods(&["trace"]);
    horizon_client::entry::main_with(vec![Box::new(trace::Trace::new()) as Box<dyn horizon_client::mod_api::Hooks>].into());
}}
''')
run(["nice", "-n", "19", "rustc", "--edition=2024", "-C", "opt-level=3", "-C", "codegen-units=16",
     "-L", f"dependency={fast / 'deps'}", "--extern", f"horizon_client={fast / 'libhorizon_client.rlib'}",
     "--extern", f"horizon_threads={threads[0]}", main, "-o", WORK / "static-trace-game"])
logs = []
for kind in ("static", "dynamic"):
    log = WORK / f"trace-{kind}.log"
    args = ["python3", str(ROOT / "tools/dylib_spike/dedicated.py"), "--label", f"trace-{kind}"]
    args += (["--binary", str(WORK / "static-trace-game")] if kind == "static" else ["--mods", str(mods)])
    with log.open("w") as output:
        subprocess.run(args, stdout=output, stderr=subprocess.STDOUT, check=True)
    folder = Path(log.read_text().splitlines()[0])
    logs.append(folder / "run.log")


def records(log, prefix):
    result = defaultdict(list)
    for line in log.read_text().splitlines():
        if line.startswith(prefix):
            tick, payload = line.removeprefix(prefix).split(" ", 1)
            result[int(tick)].append(payload)
    return result


report = dict(logs=[str(log) for log in logs])
for prefix in ("TRACE_END ", "TRACE_RNG "):
    a, b = [records(log, prefix) for log in logs]
    if prefix == "TRACE_END ":
        assert len(a) == len(b) == 6000, "missing running-tick observations"
    else:
        assert a and b, "missing random-tick observations"
    first = next((tick for tick in sorted(set(a) | set(b)) if a[tick] != b[tick]), None)
    report[prefix.strip()] = dict(first_differing_tick=first,
                                 static=a.get(first, [])[:10], dynamic=b.get(first, [])[:10])
(WORK / "trace-result.json").write_text(json.dumps(report, indent=2))
print(json.dumps(report, indent=2))
