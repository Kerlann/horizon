#!/usr/bin/env python3
"""Link the same sample source into the static fast game for the timing baseline."""
import argparse
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
WORK = ROOT / "target/dylib-spike"
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("package", type=Path)
args = parser.parse_args()
source = args.package.resolve() / "src/lib.rs"
main = WORK / "static-sample.rs"
main.write_text(f'''#[path = {json.dumps(str(source))}]
mod sample;
#[global_allocator]
static ALLOCATOR: horizon_threads::Allocator = horizon_threads::Allocator;
fn main() {{
    horizon_client::mod_api::name_mods(&["machine_sample"]);
    horizon_client::entry::main_with(vec![sample::hooks()].into());
}}
''')
fast = WORK / "static-target/fast"
threads = list((fast / "deps").glob("libhorizon_threads-*.rlib"))
assert len(threads) == 1
subprocess.run(["nice", "-n", "19", "rustc", "--edition=2024", "-C", "opt-level=3",
                "-C", "codegen-units=16", "-L", f"dependency={fast / 'deps'}",
                "--extern", f"horizon_client={fast / 'libhorizon_client.rlib'}",
                "--extern", f"horizon_threads={threads[0]}", str(main),
                "-o", str(WORK / "static-machine-game")], check=True)
