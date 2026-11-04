#!/usr/bin/env python3
"""Run every library test, ignored or not, with the prepared vanilla data, against a list of known failures.

Usage: ignored_tests.py PACKAGE...  (HORIZON_TEST_RESOURCES points at the prepared 26.3 data)

Each package's library tests run with `--include-ignored`: the #[ignore]d ones that need the data, and the
ones that return early without it, which `verify` (no data) passes without checking anything. The packages
in INTEGRATION_TESTS also run their `tests/` integration tests. The job fails when a test fails that
tools/ci/ignored-tests-known-failures.txt doesn't list, when a listed test passes (take it off the
list: the list only shrinks), and when a listed test no longer exists. A test marked `flaky` there
may pass or fail; one marked `harness` is a parity harness's step, run by its harness with the
environment it sets, and is skipped here; one marked `gpu` needs a graphics adapter and runs only where
the runner has a Vulkan driver (an ICD file), and must pass there.

Only test names, results and panic locations are printed, never a test's output or panic message:
the prepared data is Mojang's, and a failing assertion could quote it.
"""

import os
import re
import subprocess
import sys
from pathlib import Path

KNOWN = Path(__file__).with_name("ignored-tests-known-failures.txt")
RESULT = re.compile(r"^test (\S+) \.\.\. (ok|FAILED)$")
SUMMARY = re.compile(r"^test result: ")
PANIC = re.compile(r"^thread '([^']+)'.* panicked at (\S+):$")
# Packages whose integration tests (`tests/*.rs`) run too, beside their library tests.
INTEGRATION_TESTS = {"horizon-client", "horizon-world"}
ICD_DIRS = ("/usr/share/vulkan/icd.d", "/etc/vulkan/icd.d")


def has_vulkan():
    """Whether this machine has a Vulkan driver for wgpu to find."""
    return bool(os.environ.get("VK_ICD_FILENAMES")) or any(
        Path(d).is_dir() and any(Path(d).glob("*.json")) for d in ICD_DIRS
    )


def known_failures():
    """(package, test) -> its mark (None, "flaky", "harness" or "gpu"), from the checked-in list."""
    known = {}
    for number, line in enumerate(KNOWN.read_text().splitlines(), 1):
        line = line.split(" # ", 1)[0].split()
        if not line or line[0].startswith("#"):
            continue
        if len(line) < 3 or line[3:] not in ([], ["flaky"], ["harness"], ["gpu"]):
            sys.exit(f"{KNOWN.name}:{number}: expected `PACKAGE TEST REFERENCE [flaky|harness|gpu]`")
        known[(line[0], line[1])] = line[3] if len(line) > 3 else None
    return known


def run(package, skip):
    """Each test's result in `package`'s ignored library tests but `skip`, or None if they didn't build."""
    skips = [arg for test in skip for arg in ("--skip", test)]
    process = subprocess.run(
        [
            "cargo", "test", "-p", package, "--lib", *(["--bins", "--tests"] if package in INTEGRATION_TESTS else []),
            "--", "--include-ignored", "--exact", "--test-threads=4", *skips,
        ],
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, errors="replace",
    )
    results = {}
    summaries = []
    for line in process.stdout.splitlines():
        if found := RESULT.match(line):
            results[found[1]] = found[2] == "ok"
        elif found := PANIC.match(line):
            print(f"  {found[1]} panicked at {found[2]}")
        elif SUMMARY.match(line):
            summaries.append(line)
    if not summaries:
        # A build failure: the compiler's output quotes only this repository's code.
        print(process.stdout[-20000:])
        return None
    print(f"{package}: {summaries[-1]}")
    return results


def main(packages):
    known = known_failures()
    skipped = {"harness"} if has_vulkan() else {"harness", "gpu"}
    if "gpu" in skipped:
        print("no Vulkan driver on this runner: the tests marked `gpu` are skipped")
    problems = []
    for package in packages:
        skip = [test for (listed, test), mark in known.items() if listed == package and mark in skipped]
        results = run(package, skip)
        if results is None:
            problems.append(f"{package}: the tests didn't build")
            continue
        for test, passed in sorted(results.items()):
            listed = (package, test) in known
            if not passed and not listed:
                problems.append(f"{package} {test}: failed, and isn't a known failure")
            elif not passed and known[(package, test)] == "gpu":
                problems.append(f"{package} {test}: failed with a Vulkan driver present")
            elif passed and listed and known[(package, test)] is None:
                problems.append(f"{package} {test}: passes now; take it off {KNOWN.name}")
        for (listed, test), mark in known.items():
            if listed == package and mark not in skipped and test not in results:
                problems.append(f"{package} {test}: listed in {KNOWN.name} but no longer runs")
    for problem in problems:
        print(f"::error::{problem}")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
