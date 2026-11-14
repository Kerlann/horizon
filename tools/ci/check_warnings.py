#!/usr/bin/env python3
"""Run Cargo checks to completion with lint warnings, then reject any warning or compiler failure.

Usage: python3 tools/ci/check_warnings.py cargo clippy --keep-going --workspace --all-targets
Cargo's JSON diagnostics include cached warnings from fresh crates. Render them normally, but enforce their
severity here rather than in rustc: a denied lint otherwise prevents metadata for dependent crates.
"""
import json
import os
import re
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor


ANSI = re.compile(r"\x1b\[[0-9;]*m")


def forward_diagnostics(stream, structured: bool) -> int:
    """Render one Cargo stream and count warnings; keep stderr separate so it cannot corrupt JSON lines."""
    warnings = 0
    for line in stream:
        message = None
        if structured:
            try:
                message = json.loads(line)
            except json.JSONDecodeError:
                pass
        if isinstance(message, dict) and message.get("reason") == "compiler-message":
            diagnostic = message["message"]
            warnings += diagnostic["level"] == "warning"
            print(diagnostic.get("rendered") or diagnostic["message"] + "\n", end="", flush=True)
        elif not isinstance(message, dict) or "reason" not in message:
            print(line, end="", flush=True)
            # Cargo/build-script warnings are plain text, not compiler-message records. Counting a Cargo
            # summary again is harmless: this is a warning-record count, not a distinct-lint count.
            warnings += bool(re.match(r"^warning(?:\[[^]]+\])?:", ANSI.sub("", line)))
    return warnings


def main(command: list[str]) -> int:
    # Cargo flags precede the rustc/clippy argument separator. The gate owns this command, not untrusted input.
    split = command.index("--") if "--" in command else len(command)
    command = [*command[:split], "--message-format=json-diagnostic-rendered-ansi", *command[split:]]
    env = dict(os.environ)
    # Cap explicit crate-level deny/forbid lints too; genuine type/parse/link errors remain compiler errors.
    # Preserve encoded flags' precedence and SDK prefer-dynamic. Dependencies retain Cargo's own lint cap.
    if "CARGO_ENCODED_RUSTFLAGS" in env:
        env["CARGO_ENCODED_RUSTFLAGS"] += "\x1f--cap-lints=warn"
    else:
        env["RUSTFLAGS"] = env.get("RUSTFLAGS", "") + " --cap-lints=warn"
    with subprocess.Popen(command, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                          text=True, errors="replace") as process, ThreadPoolExecutor(1) as pool:
        stderr = pool.submit(forward_diagnostics, process.stderr, False)
        warnings = forward_diagnostics(process.stdout, True) + stderr.result()
        code = process.wait()
    if warnings:
        print(f"::error::{warnings} warning records emitted; warnings fail this check after all crates finish",
              flush=True)
    return code if code else int(bool(warnings))


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
