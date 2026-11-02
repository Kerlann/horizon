#!/usr/bin/env python3
"""Linux-only #26 spike. Not a replacement for launcher/native_mods staging.

Build the game once with Cargo; compile trait-only mods with rustc against its
exact metadata. This deliberately rejects additional dependencies instead of
silently compiling a second copy of a shared crate. private.py separately proves
a mod-private rlib; full launcher dependency policy and mixins are outside this experiment.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import time
import tomllib

ROOT = Path(__file__).resolve().parents[1]
WORK = ROOT / "target/dylib-spike"
FLAGS = ["-C", "prefer-dynamic", "-C", "opt-level=3", "-C", "codegen-units=16"]


def run(argv, **kwargs):
    print("+", " ".join(map(str, argv)), flush=True)
    return subprocess.run(list(map(str, argv)), check=True, **kwargs)


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def rustc_version():
    return subprocess.check_output(["rustc", "-Vv"], text=True)


def read_game(game):
    identity = json.loads((game / "identity.json").read_text())
    if identity["toolchain"] != rustc_version():
        raise SystemExit("toolchain mismatch: rustc differs from game build")
    if identity["profile"] != "fast" or identity["flags"] != FLAGS:
        raise SystemExit("profile mismatch: game must use fast/prefer-dynamic")
    for name, expected in identity["artifacts"].items():
        if digest(game / name) != expected:
            raise SystemExit(f"crate/game build mismatch: {name}")
    if digest(game / "Cargo.lock") != identity["lock"]:
        raise SystemExit("crate lock mismatch: Cargo.lock")
    return identity


def compiler(game):
    return ["nice", "-n", "19", "rustc", "--edition=2024", *FLAGS,
            "-L", f"dependency={game / 'deps'}", "--extern",
            f"horizon_client={game / 'libhorizon_client.so'}"]


def loader(game):
    staged = game / "loader"
    staged.mkdir(exist_ok=True)
    shutil.copy(ROOT / "tools/dylib_spike/loader.rs", staged / "main.rs")
    shutil.copy(game / "identity.json", staged / "identity.json")
    libraries = list((game / "deps").glob("liblibloading-*.rlib"))
    if len(libraries) != 1:
        raise SystemExit("expected exactly one game build of libloading")
    run([*compiler(game), staged / "main.rs", "--extern", f"libloading={libraries[0]}",
         "-o", game / "horizon-dylib-game"])


def build_game(args):
    WORK.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, RUSTFLAGS="-C prefer-dynamic")
    run(["nice", "-n", "19", "cargo", "rustc", "-j", "6", "--profile", "fast",
         "-p", "horizon-client", "--no-default-features", "--features", "dylib-spike",
         "--lib", "--crate-type", "dylib"], cwd=ROOT, env=env)
    # A relocatable SDK snapshot; no Cargo fingerprints or original source needed by rustc.
    game = args.game
    game.mkdir(parents=True, exist_ok=True)
    shutil.copytree(ROOT / "target/fast/deps", game / "deps", dirs_exist_ok=True)
    shutil.copy(ROOT / "target/fast/libhorizon_client.so", game / "libhorizon_client.so")
    shutil.copy(ROOT / "Cargo.lock", game / "Cargo.lock")
    artifacts = {p.relative_to(game).as_posix(): digest(p)
                 for p in sorted((game / "deps").iterdir())
                 if p.suffix in (".so", ".rlib", ".rmeta")}
    artifacts["libhorizon_client.so"] = digest(game / "libhorizon_client.so")
    identity = dict(toolchain=rustc_version(), profile="fast", flags=FLAGS,
                    lock=digest(game / "Cargo.lock"), artifacts=artifacts)
    (game / "identity.json").write_text(json.dumps(identity, sort_keys=True, indent=2) + "\n")
    loader(game)


def build_mod(args):
    identity = read_game(args.game)
    if args.expect:
        expected = json.loads(args.expect.read_text())
        for field in ("toolchain", "profile", "flags", "lock", "artifacts"):
            if expected[field] != identity[field]:
                raise SystemExit(f"{args.id}: {field} mismatch with requested game build")
    package = tomllib.loads((args.package / "Cargo.toml").read_text())
    for name in package.get("dependencies", {}):
        if name != "horizon-client":
            raise SystemExit(f"crate mismatch: {name}: spike supports only horizon-client dependencies")
    if not args.id or any(not (c.isascii() and (c.isalnum() or c in "_-")) for c in args.id):
        raise SystemExit("invalid mod id")
    args.mods.mkdir(parents=True, exist_ok=True)
    stage = args.mods / f"source-{args.id}"
    stage.mkdir(exist_ok=True)
    if args.zoom:
        shutil.copy(ROOT / "tools/dylib_spike/zoom.rs", stage / "lib.rs")
        shutil.copy(args.package / "mixins/zoom/runtime.rs", stage / "runtime.rs")
    else:
        source = args.package / package.get("lib", {}).get("path", "src/lib.rs")
        # The sample trait package is a single-file crate. Do not pretend this handles
        # arbitrary Cargo packages, build scripts, features or dependency resolution.
        text = source.read_text()
        if args.panic:
            text = text.replace("let dimension = event.dimension;",
                                'panic!("machine_sample spike panic");\nlet dimension = event.dimension;', 1)
        (stage / "original.rs").write_text(text)
        (stage / "lib.rs").write_text('mod original;\n#[unsafe(no_mangle)]\n'
            'pub fn horizon_mod_entry() -> Box<dyn horizon_client::mod_api::Hooks> { original::hooks() }\n')
    start = time.monotonic()
    run([*compiler(args.game), "--crate-type=dylib", "--crate-name", args.id.replace("-", "_"),
         stage / "lib.rs", "-o", args.mods / f"{args.id}.so"])
    shutil.copy(args.game / "identity.json", args.mods / f"{args.id}.json")
    print(f"mod_compile_seconds={time.monotonic() - start:.6f}")
    print("Set the mods/order file to newline-separated IDs in launcher-resolved dependency order.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--game", type=Path, default=WORK / "game")
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("build-game")
    sub.add_parser("build-loader")
    mod = sub.add_parser("build-mod")
    mod.add_argument("id")
    mod.add_argument("package", type=Path)
    mod.add_argument("--mods", type=Path, default=WORK / "mods")
    mod.add_argument("--expect", type=Path, help="required game identity supplied by caller")
    mod.add_argument("--zoom", action="store_true", help="adapt official mixin Zoom runtime")
    mod.add_argument("--panic", action="store_true", help="throwaway sample panic fixture")
    sub.add_parser("run")
    args, remaining = parser.parse_known_args()
    args.game = args.game.resolve()
    if args.command != "run" and remaining:
        parser.error(f"unrecognized arguments: {remaining}")
    if args.command == "build-game":
        build_game(args)
    elif args.command == "build-loader":
        read_game(args.game)
        loader(args.game)
    elif args.command == "build-mod":
        args.package = args.package.resolve()
        args.mods = args.mods.resolve()
        build_mod(args)
    elif args.command == "run":
        read_game(args.game)
        sysroot = Path(subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip())
        stdlib = sysroot / "lib/rustlib/x86_64-unknown-linux-gnu/lib"
        os.environ["LD_LIBRARY_PATH"] = f"{args.game}:{args.game / 'deps'}:{stdlib}"
        os.execv(args.game / "horizon-dylib-game", ["horizon-dylib-game", *remaining])


if __name__ == "__main__":
    main()
