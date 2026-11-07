#!/usr/bin/env python3
"""Bounded, slot-independent snapshots of wasm-mods' complete Cargo working set."""
from datetime import datetime
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


def footprint():
    paths = [TARGET, TARGET / "internals-catalog-coverage", TARGET / "internals-catalog-check"]
    present = [str(path) for path in paths if path.exists()]
    report("Target/subtree apparent bytes:\n\n```\n" +
           capture("du", "-sbl", *present).decode().strip() + "\n```")


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
    if ARCHIVE.exists():
        footprint()
    report(f"Snapshot matched key: `{matched or 'miss'}`. Restore including extraction: "
           f"{elapsed:.2f} s; extraction: {extraction:.2f} s; "
           f"compressed bytes: {ARCHIVE.stat().st_size if ARCHIVE.exists() else 0}.")
    output("within-time", str(not matched or elapsed <= MAX_RESTORE_SECONDS).lower())
    if matched and elapsed > MAX_RESTORE_SECONDS:
        report("Warm target unmet: restore exceeded 120 seconds. Checks still run normally.")


def cache_metadata(endpoint):
    response = subprocess.run(["gh", "api", f"repos/{os.environ['GITHUB_REPOSITORY']}/actions/{endpoint}"],
                              capture_output=True, text=True)
    if response.returncode:
        report("Snapshot not published: cache metadata unavailable; protect lint's cache budget.")
        return None
    return json.loads(response.stdout)


def pack():
    output("publish", "false")
    # Do not republish an exact hit or reinforce a restore that missed the warm budget.
    if os.environ.get("EXACT_HIT") == "true" or os.environ.get("WITHIN_TIME") != "true":
        return
    if os.environ.get("GITHUB_REF") == "refs/heads/main":
        recent = cache_metadata("caches?ref=refs%2Fheads%2Fmain&key=wasm-snapshot-v1-"
                                "&sort=created_at&direction=desc&per_page=1")
        if recent is None:
            return
        caches = recent["actions_caches"]
        if caches and time.time() - datetime.fromisoformat(caches[0]["created_at"].replace("Z", "+00:00")).timestamp() < 21600:
            report("Snapshot publication limited to once per six hours; checks still run normally.")
            return
    usage = cache_metadata("cache/usage")
    if usage is None:
        return
    occupied = usage["active_caches_size_in_bytes"]
    if type(occupied) is not int or occupied < 0:
        raise ValueError("invalid GitHub cache occupancy")
    footprint()
    ARCHIVE.parent.mkdir(parents=True, exist_ok=True)
    started = time.monotonic()
    # Cargo compares subsecond mtimes; GNU's default tar format loses that precision.
    subprocess.run(["tar", "--format=pax", "--pax-option=delete=atime,delete=ctime",
                    "--exclude=./wasm-snapshot", "-I", "zstd -T2 -3", "-cf", str(ARCHIVE),
                    "-C", str(TARGET), "."], check=True)
    size = ARCHIVE.stat().st_size
    report(f"Compressed snapshot bytes: {size}; compression: {time.monotonic() - started:.2f} s; "
           f"existing repository cache bytes: {occupied}.")
    if not fits(size, occupied):
        report("Warm target unmet: snapshot exceeds 2 GB compressed or 8 GB combined occupancy. "
               "Not publishing or deleting other caches; checks remain unchanged.")
        return
    output("publish", "true")


if __name__ == "__main__":
    {"key": key, "restore": restore, "pack": pack}[sys.argv[1]]()
