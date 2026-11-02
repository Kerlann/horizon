#!/usr/bin/env python3
"""Real dedicated-server fixed seed/script, 6000 running ticks, frozen at both ends.

No player joins this fixture. The dedicated save nevertheless writes one default
player record; that record is compared, not populated-player inventory parity.
The helper uses existing console/control APIs, not engine fixtures.
"""
import argparse
import json
import os
from pathlib import Path
import re
import socket
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))
from lab import game_env

WORK = ROOT / "target/dylib-spike"
SCRIPT = [
    "fill 0 80 0 15 80 15 minecraft:stone",
    "setblock 4 81 4 minecraft:chest",
    "item replace block 4 81 4 container.0 with minecraft:cobblestone 32",
    "summon minecraft:pig 8 81 8 {NoAI:1b,PersistenceRequired:1b,UUID:[I;1,2,3,4]}",
    "time set 0", "weather clear",
]


def request(path, line):
    with socket.socket(socket.AF_UNIX) as connection:
        connection.settimeout(180)
        connection.connect(str(path))
        connection.sendall((line + "\n").encode())
        return json.loads(connection.makefile().readline())


def scratch_env(folder):
    env = os.environ.copy()
    for var, name in (("HORIZON_HOME", "home"), ("HORIZON_LAUNCHER_HOME", "launcher"),
                      ("XDG_DATA_HOME", "xdg-data"), ("XDG_CONFIG_HOME", "xdg-config"),
                      ("HORIZON_LAUNCHER_TRASH", "trash"),
                      ("HORIZON_LAUNCHER_CLIPBOARD", "clipboard"),
                      ("HORIZON_LAUNCHER_DESKTOP", "desktop")):
        env[var] = str(folder / name)
    return game_env(env)


def wait_for(process, log, needle, after=0, timeout=600):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        text = log.read_text(errors="replace")
        if needle in text[after:]:
            return len(text)
        if process.poll() is not None:
            raise RuntimeError(f"server exited {process.returncode}: {text[-3000:]}")
        time.sleep(.05)
    raise TimeoutError(f"waiting for {needle}: {log}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, help="static executable; default is dynamic")
    parser.add_argument("--label", required=True)
    parser.add_argument("--mods", type=Path)
    parser.add_argument("--ticks", type=int, default=6000)
    parser.add_argument("--settle-seconds", type=float, default=0,
                        help="optional frozen startup delay for the divergence investigation")
    args = parser.parse_args()
    if args.ticks < 0:
        parser.error("--ticks must be nonnegative")
    WORK.mkdir(exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix=args.label + "-", dir=WORK)).resolve()
    env = scratch_env(folder)
    if args.mods:
        env["HORIZON_DYLIB_MODS"] = str(args.mods.resolve())
    else:
        env.pop("HORIZON_DYLIB_MODS", None)
    (folder / "eula.txt").write_text("eula=true\n")
    (folder / "server.properties").write_text(
        "level-seed=2603\nonline-mode=false\nserver-port=0\nview-distance=2\n"
        "simulation-distance=2\npause-when-empty-seconds=-1\nmax-tick-time=0\n")
    command = ([str(args.binary.resolve())] if args.binary else
               ["python3", str(ROOT / "tools/dylib_spike.py"), "run"])
    command += ["--resources", str(ROOT / ".local/horizon-client/26.3"),
                "--game-dir", str(folder), "--control-socket", str(folder / "ctl.sock"),
                "--dedicated", "--universe", str(folder), "--world", "Spike", "--nogui"]
    log = folder / "run.log"
    print(folder, flush=True)
    with log.open("w") as output:
        process = subprocess.Popen(command, cwd=folder, env=env, stdin=subprocess.PIPE,
                                   stdout=output, stderr=subprocess.STDOUT, text=True)
        try:
            process.stdin.write("tick freeze\nforceload add 0 0 15 15\n")
            process.stdin.flush()
            wait_for(process, log, "The game is frozen")
            sock = folder / "ctl.sock"
            def console(line):
                answer = request(sock, "console " + line)
                print(line, answer, flush=True)
                return "\n".join(answer["output"])
            deadline = time.monotonic() + 180
            while "Changed the block" not in console("setblock 0 80 0 minecraft:stone"):
                if time.monotonic() > deadline:
                    raise TimeoutError("force-loaded chunk never arrived")
                time.sleep(.1)
            time.sleep(args.settle_seconds)
            for line in SCRIPT:
                console(line)
            commands = ["tick freeze", "forceload add 0 0 15 15",
                        "setblock 0 80 0 minecraft:stone", *SCRIPT]
            def game_time():
                return int(re.findall(r"\d+", console("time query gametime"))[-1])
            initial = game_time()
            for ticks in (min(1000, args.ticks), max(0, args.ticks - 1000)):
                if ticks == 0:
                    continue
                target = initial + ticks
                # The console route advances one extra running tick on both Horizon and
                # vanilla 26.3. Pin observable world time, not the requested sprint count.
                sprint = f"tick sprint {ticks - 1}" if ticks > 1 else "tick step 1"
                commands.append(sprint)
                console(sprint)
                deadline = time.monotonic() + 180
                while game_time() < target:
                    if time.monotonic() > deadline:
                        raise TimeoutError(f"sprint did not reach {target}")
                    time.sleep(.1)
                assert game_time() == target, "sprint overshot frozen boundary"
                initial = target
            (folder / "status.json").write_text(json.dumps(request(sock, "server-status"), indent=2))
            console("save-all")
            console("stop")
            (folder / "commands.txt").write_text("\n".join([*commands, "save-all", "stop"]) + "\n")
            process.wait(timeout=120)
        finally:
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=30)
    print(f"result={folder} exit={process.returncode}", flush=True)


if __name__ == "__main__":
    main()
