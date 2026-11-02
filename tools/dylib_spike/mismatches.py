#!/usr/bin/env python3
"""Exercise refusal diagnostics with real build-driver invocations, before rustc."""
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
WORK = ROOT / "target/dylib-spike"
game = WORK / "game"
identity = json.loads((game / "identity.json").read_text())
package = WORK / "sources/machine/examples/trait-api/machine-hooks"
results = []
for field, wrong in (("toolchain", "other rustc"), ("profile", "dev"),
                     ("lock", "wrong crate lock"), ("artifacts", {})):
    expected = WORK / f"wrong-{field}.json"
    expected.write_text(json.dumps(dict(identity, **{field: wrong})))
    completed = subprocess.run(["python3", str(ROOT / "tools/dylib_spike.py"), "build-mod",
        "machine_sample", str(package), "--expect", str(expected)], capture_output=True, text=True)
    output = completed.stdout + completed.stderr
    assert completed.returncode != 0 and f"{field} mismatch" in output
    assert "rustc --edition" not in output, "refused too late: compiler ran"
    results.append(dict(field=field, status=completed.returncode, output=output.strip()))
# Wrong identity must be refused before dlopen: no library even exists in this folder.
mods = WORK / "wrong-runtime"
mods.mkdir(exist_ok=True)
(mods / "order").write_text("machine_sample\n")
(mods / "machine_sample.json").write_text("wrong game identity\n")
import os
env = dict(os.environ, HORIZON_DYLIB_MODS=str(mods))
completed = subprocess.run(["python3", str(ROOT / "tools/dylib_spike.py"), "run"],
                           env=env, capture_output=True, text=True)
output = completed.stdout + completed.stderr
assert completed.returncode != 0 and "build mismatch" in output
assert "cannot open shared object" not in output, "loader attempted dlopen"
results.append(dict(field="runtime", status=completed.returncode, output=output.strip()))
(WORK / "mismatch-results.json").write_text(json.dumps(results, indent=2))
print(json.dumps(results, indent=2))
