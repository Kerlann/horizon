#!/usr/bin/env python3
"""Bounded, slot-independent snapshots of wasm-mods' complete Cargo working set."""
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
TARGET = ROOT / "target"
ARCHIVE = TARGET / "wasm-snapshot/payload.tar.zst"
MAX_ARCHIVE = 2_000_000_000
MAX_TOTAL = 8_000_000_000
MAX_RESTORE_SECONDS = 120


def output(key, value):
    with open(os.environ["GITHUB_OUTPUT"], "a") as stream:
        stream.write(f"{key}={value}\n")


def report(message):
    print(message)
    with open(os.environ["GITHUB_STEP_SUMMARY"], "a") as stream:
        stream.write(message + "\n\n")


def capture(*command):
    return subprocess.check_output(command, cwd=ROOT)


def fits(archive_bytes, occupied_bytes):
    return archive_bytes <= MAX_ARCHIVE and occupied_bytes + archive_bytes <= MAX_TOTAL


def key():
    toolchains = b"".join(capture("rustup", "run", toolchain, "rustc", "-vV")
                         for toolchain in ("stable", "1.99.1"))
    compiler = hashlib.sha256(toolchains).hexdigest()
    lock = hashlib.sha256((ROOT / "Cargo.lock").read_bytes()).hexdigest()
    # Conservative catalog-input hash, not a generated internals.json output hash.
    inputs = hashlib.sha256(capture("git", "ls-files", "-s", "-z")).hexdigest()
    namespace = os.environ.get("CACHE_NAMESPACE", "wasm-snapshot-v1")
    prefix = f"{namespace}-{platform.system()}-{platform.machine()}-{compiler}-{lock}-"
    output("key", prefix + inputs)
    output("prefix", prefix)
    output("fallback", f"{namespace}-{platform.system()}-{platform.machine()}-{compiler}-")
    output("started", int(time.time()))
    report(f"Catalog-input Git-object hash: `{inputs}`; toolchain: `{compiler}`; lock: `{lock}`.")


def restore():
    matched = os.environ.get("MATCHED_KEY", "")
    started = time.monotonic()
    if ARCHIVE.exists():
        subprocess.run(["tar", "--zstd", "-xf", str(ARCHIVE), "-C", str(TARGET)], check=True)
    extraction = time.monotonic() - started
    elapsed = time.time() - int(os.environ["RESTORE_STARTED"])
    report(f"Snapshot matched key: `{matched or 'miss'}`. Restore including extraction: "
           f"{elapsed:.2f} s; extraction: {extraction:.2f} s; "
           f"compressed bytes: {ARCHIVE.stat().st_size if ARCHIVE.exists() else 0}.")
    output("within-time", str(not matched or elapsed <= MAX_RESTORE_SECONDS).lower())
    if matched and elapsed > MAX_RESTORE_SECONDS:
        report("Warm target unmet: restore exceeded 120 seconds. Checks still run normally.")


def pack():
    output("publish", "false")
    # Do not republish an exact hit or reinforce a restore that missed the warm budget.
    if os.environ.get("EXACT_HIT") == "true" or os.environ.get("WITHIN_TIME") != "true":
        return
    usage = subprocess.run(["gh", "api", f"repos/{os.environ['GITHUB_REPOSITORY']}/actions/cache/usage"],
                           capture_output=True, text=True)
    if usage.returncode:
        report("Snapshot not published: cache occupancy unavailable; protect lint's cache budget.")
        return
    occupied = json.loads(usage.stdout)["active_caches_size_in_bytes"]
    if type(occupied) is not int or occupied < 0:
        raise ValueError("invalid GitHub cache occupancy")
    total = capture("du", "-sb", str(TARGET)).decode().split()[0]
    parts = capture("du", "-sb", str(TARGET / "internals-catalog-coverage"),
                    str(TARGET / "internals-catalog-check")).decode().strip()
    ARCHIVE.parent.mkdir(parents=True, exist_ok=True)
    started = time.monotonic()
    subprocess.run(["tar", "--exclude=./wasm-snapshot", "-I", "zstd -T2 -3", "-cf", str(ARCHIVE),
                    "-C", str(TARGET), "."], check=True)
    size = ARCHIVE.stat().st_size
    report(f"Target apparent bytes: {total}. Coverage subtree apparent bytes:\n\n```\n{parts}\n```\n\n"
           f"Compressed snapshot bytes: {size}; compression: {time.monotonic() - started:.2f} s; "
           f"existing repository cache bytes: {occupied}.")
    if not fits(size, occupied):
        report("Warm target unmet: snapshot exceeds 2 GB compressed or 8 GB combined occupancy. "
               "Not publishing or deleting other caches; checks remain unchanged.")
        return
    output("publish", "true")


if __name__ == "__main__":
    {"key": key, "restore": restore, "pack": pack}[sys.argv[1]]()
