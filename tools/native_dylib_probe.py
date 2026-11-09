#!/usr/bin/env python3
"""Dev-only exact-build Rust dylib probe. Build SDK with prebuilt_game.py first.

Uses SDK metadata only: never rebuilds mod_api, mod_points or the game.
All compiler commands and native dependency inspection are retained in --output.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

import prebuilt_game as sdk_tools

FIXTURES = Path(__file__).with_suffix("")


def preflight(sdk, compiler):
    identity = json.loads((sdk / sdk_tools.IDENTITY).read_text())
    if compiler.strip() != identity["rustc"].strip():
        raise ValueError("full rustc -vV mismatch")
    if "release: 1.99.1\n" not in compiler:
        raise ValueError("probe requires pinned rustc 1.99.1")
    for name, expected in identity["files"].items():
        path = sdk / name
        if not path.resolve().is_relative_to(sdk.resolve()):
            raise ValueError("SDK artifact escapes root")
        if sdk_tools.sha256(path) != expected:
            raise ValueError(f"exact SDK artifact mismatch: {name}")
    return identity


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sdk", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    sdk, output = args.sdk.resolve(), args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    compiler = subprocess.check_output(["rustc", "-vV"], text=True)
    (output / "rustc.txt").write_text(compiler)
    identity = preflight(sdk, compiler)
    # Negative preflight must refuse before any library can execute.
    try:
        preflight(sdk, compiler + "different build\n")
    except ValueError:
        pass
    else:
        raise AssertionError("compiler mismatch was accepted")
    (output / "sdk-identity.json").write_text(json.dumps(identity, indent=2))
    deps = sdk / "deps"
    windows = sys.platform == "win32"
    extension, prefix = (".dll", "") if windows else (".so", "lib")

    def extern(crate):
        library = sdk_tools.dylib(deps, crate)
        if library is None:
            candidates = list(deps.glob(f"lib{crate}-*.rmeta"))
            if len(candidates) != 1:
                raise ValueError(f"expected exactly one SDK metadata for {crate}: {candidates}")
            library = candidates[0]
        return ["--extern", f"{crate}={library}"]

    def run(command, **kwargs):
        with (output / "commands.log").open("a") as log:
            log.write(json.dumps(list(map(str, command))) + "\n")
            log.flush()
            if command[0] == "rustc":
                for driver in ("rustc", "clippy-driver"):
                    gate = [driver, *command[1:], "--emit=metadata", "-Dwarnings"]
                    gate[gate.index("-o") + 1] = str(output / "gate.rmeta")
                    log.write(json.dumps(gate) + "\n")
                    log.flush()
                    subprocess.run(gate, check=True, stdout=log, stderr=subprocess.STDOUT)
            subprocess.run(command, check=True, stdout=log, stderr=subprocess.STDOUT, **kwargs)

    common = ["rustc", "--edition=2024", "-C", "prefer-dynamic", "-L", f"dependency={deps}",
              "-L", f"native={sdk / 'native'}", "-L", f"dependency={output}"]
    for crate, source in [("probe_shared", "shared.rs"), ("probe_a", "mod.rs"), ("probe_b", "mod.rs")]:
        extra = [] if crate == "probe_shared" else ["--extern", f"probe_shared={output / (prefix + 'probe_shared' + extension)}"]
        run([*common, "--crate-type=dylib", "--crate-name", crate, *extern("horizon_client"), *extern("mod_api"), *extern("mod_points"),
             *extra, str(FIXTURES / source), "-o", str(output / (prefix + crate + extension))])
    executable = output / ("probe.exe" if windows else "probe")
    run([*common, "--crate-name=probe", *extern("horizon_client"), *extern("mod_api"), *extern("mod_points"),
         *extern("libloading"), "--extern", f"probe_shared={output / (prefix + 'probe_shared' + extension)}",
         str(FIXTURES / "main.rs"), "-o", str(executable)])
    sysroot = Path(subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip())
    host = next(line.split(": ", 1)[1] for line in compiler.splitlines() if line.startswith("host: "))
    runtime = sysroot / "lib/rustlib" / host / "lib"
    env = dict(os.environ)
    variable = "PATH" if windows else "LD_LIBRARY_PATH"
    env[variable] = os.pathsep.join(map(str, [output, deps, runtime])) + os.pathsep + env.get(variable, "")
    for artifact in [executable, *output.glob("*" + extension), *deps.glob("*" + extension)]:
        inspector = ["objdump", "-p"] if windows else ["readelf", "-d"]
        run([*inspector, str(artifact)])
    if not windows:
        # Inspect actual exported allocator definitions, not prefer-dynamic intent.
        images = [executable, *output.glob("*.so"), *deps.glob("*.so")]
        allocator_rows = {}
        definitions = []
        for artifact in images:
            symbols = subprocess.check_output(["nm", "-D", str(artifact)], text=True)
            rows = [line for line in symbols.splitlines() if "___rust_alloc" in line or "___rust_dealloc" in line]
            allocator_rows[str(artifact)] = rows
            if any(" T " in row and "___rust_alloc" in row for row in rows):
                definitions.append(artifact)
        (output / "allocator-symbols.json").write_text(json.dumps(allocator_rows, indent=2))
        if definitions != [sdk_tools.dylib(deps, "horizon_client")]:
            raise ValueError(f"expected game as sole allocator definition: {definitions}")
    run([str(executable), str(output / (prefix + "probe_a" + extension)), str(output / (prefix + "probe_b" + extension))], env=env)
    (output / "result.json").write_text(json.dumps({"result": "pass", "artifacts": {
        p.name: sdk_tools.sha256(p) for p in output.iterdir() if p.suffix in (extension, ".exe") or p == executable
    }}, indent=2))
    print(f"PASS; evidence in {output}")


if __name__ == "__main__":
    main()
