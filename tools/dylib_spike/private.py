#!/usr/bin/env python3
"""Prove a crate absent from the game's lock links privately into a mod dylib.

This intentionally bypasses the spike driver's restricted Cargo-manifest policy.
The private crate has no dependencies except the shared std; no second game crate
or Cargo resolution is introduced. No launcher policy is changed.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tomllib
from dedicated import ROOT, WORK

import sys
sys.path.insert(0, str(ROOT / "tools"))
from dylib_spike import FLAGS, compiler, digest, read_game, run

game = WORK / "game"
read_game(game)
before = digest(ROOT / "Cargo.lock")
locked = tomllib.loads((ROOT / "Cargo.lock").read_text())["package"]
assert not any(package["name"] == "spike_private" for package in locked)
mods = WORK / "private-mod"
mods.mkdir(exist_ok=True)
source = mods / "private.rs"
source.write_text('''#[inline(never)]
pub fn message(pid: u32) -> String { format!("spike_private in process {pid}") }
''')
run(["nice", "-n", "19", "rustc", "--edition=2024", *FLAGS, "--crate-type=rlib",
     "--crate-name", "spike_private", source, "-o", mods / "libspike_private.rlib"])
sample = WORK / "sources/machine/examples/trait-api/machine-hooks/src/lib.rs"
wrapper = mods / "mod.rs"
wrapper.write_text(f'''#[path = {json.dumps(str(sample))}]
mod sample;
#[unsafe(no_mangle)]
pub fn horizon_mod_entry() -> Box<dyn horizon_client::mod_api::Hooks> {{
    eprintln!("DYLIB_SPIKE private: {{}}", spike_private::message(std::process::id()));
    sample::hooks()
}}
''')
run([*compiler(game), "--crate-name", "private_sample", "--crate-type=dylib", wrapper,
     "--extern", f"spike_private={mods / 'libspike_private.rlib'}", "-o", mods / "machine_sample.so"])
shutil.copy(game / "identity.json", mods / "machine_sample.json")
(mods / "order").write_text("machine_sample\n")
with (WORK / "private-dedicated.log").open("w") as log:
    subprocess.run(["python3", str(ROOT / "tools/dylib_spike/dedicated.py"), "--label", "private",
                    "--mods", str(mods)], stdout=log, stderr=subprocess.STDOUT, check=True)
folder = Path((WORK / "private-dedicated.log").read_text().splitlines()[0])
needed = subprocess.check_output(["readelf", "-d", str(mods / "machine_sample.so")], text=True)
runtime = (folder / "run.log").read_text()
symbols = subprocess.check_output(["nm", "--defined-only", str(mods / "machine_sample.so")], text=True)
result = dict(crate="spike_private", absent_from_game_lock=True,
              game_lock_unchanged=before == digest(ROOT / "Cargo.lock"),
              separate_dso_needed="libspike_private" in needed,
              private_symbol_in_mod="spike_private" in symbols,
              private_code_ran="DYLIB_SPIKE private: spike_private in process" in runtime,
              sample_hook_ran="MACHINE_SAMPLE server clock: 200 ticks" in runtime)
assert result["game_lock_unchanged"] and not result["separate_dso_needed"]
assert result["private_symbol_in_mod"] and result["private_code_ran"] and result["sample_hook_ran"]
(WORK / "private-result.json").write_text(json.dumps(result, indent=2))
print(json.dumps(result, indent=2))
