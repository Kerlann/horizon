"""Cargo diagnostics stay visible and warnings fail only after Cargo finishes."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class CheckWarningsTests(unittest.TestCase):
    @unittest.skipIf(os.name == "nt", "executes the POSIX self-check command")
    def test_self_check_runs_both_feature_graphs_after_a_warning(self):
        from tools import pr_ready
        with tempfile.TemporaryDirectory() as directory:
            cargo = Path(directory) / "cargo"
            cargo.write_text(f"#!{sys.executable}\n" + '''import json, os, sys
with open(os.environ["CALLS"], "a") as calls:
    calls.write("native\\n" if "--workspace" in sys.argv else "server\\n")
if "--workspace" in sys.argv:
    print(json.dumps({"reason": "compiler-message", "message": {
        "level": "warning", "rendered": "warning: native lint\\n"}}))
''')
            cargo.chmod(0o755)
            env = dict(os.environ, PATH=directory + os.pathsep + os.environ["PATH"], CALLS=str(Path(directory) / "calls"))
            command = (f'status=0\n{pr_ready.CI_CLIPPY} || status=$?\n'
                       f'{pr_ready.CI_CLIPPY_SERVER} || status=$?\nexit "$status"')
            result = subprocess.run(["bash", "-c", command], cwd=ROOT, env=env, capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
            self.assertEqual(Path(env["CALLS"]).read_text(), "native\nserver\n")

    def test_rendering_exit_status_and_flags(self):
        with tempfile.TemporaryDirectory() as directory:
            cargo = Path(directory) / "cargo.py"
            cargo.write_text('''import json, os, sys
print("flags=" + os.environ.get("CARGO_ENCODED_RUSTFLAGS", os.environ.get("RUSTFLAGS", "")))
print("args=" + json.dumps(sys.argv[1:]))
mode = os.environ["MODE"]
if mode == "warning":
    for crate in ("lower", "upper"):
        print(json.dumps({"reason": "compiler-message", "message": {
            "level": "warning", "rendered": "warning: lint in " + crate + "\\n"}}))
if mode == "script":
    print("\\x1b[33mwarning: crate@0.1: C warning\\x1b[0m", file=sys.stderr)
if mode == "error":
    print(json.dumps({"reason": "compiler-message", "message": {
        "level": "error", "rendered": "error: type mismatch\\n"}}))
print(json.dumps({"reason": "compiler-artifact", "fresh": True}))
print("finished all crates", file=sys.stderr)
sys.exit(17 if mode == "error" else 0)
''')
            for encoded in (False, True):
                for mode, code in (("clean", 0), ("warning", 1), ("script", 0), ("error", 17)):
                    with self.subTest(encoded=encoded, mode=mode):
                        env = {key: value for key, value in os.environ.items()
                               if key not in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS")}
                        env.update(MODE=mode, RUSTFLAGS="-C prefer-dynamic")
                        if encoded:
                            env["CARGO_ENCODED_RUSTFLAGS"] = "-C\x1fprefer-dynamic"
                        kind = "check" if mode == "script" else "clippy"
                        tail = ["--", "-Wclippy::all"] if kind == "clippy" else []
                        result = subprocess.run([sys.executable, str(ROOT / "tools/ci/check_warnings.py"),
                                                 sys.executable, str(cargo), kind, "--keep-going", *tail],
                                                env=env, capture_output=True, text=True, timeout=10)
                        self.assertEqual(result.returncode, code, result.stderr + result.stdout)
                        self.assertIn("finished all crates", result.stdout)
                        separator = "\x1f" if encoded else " "
                        flags = next(line[6:] for line in result.stdout.splitlines() if line.startswith("flags="))
                        original = "-C\x1fprefer-dynamic" if encoded else "-C prefer-dynamic"
                        self.assertEqual(flags, original + (f"{separator}--cap-lints=warn" if kind == "check" else ""))
                        args = json.loads(next(line[5:] for line in result.stdout.splitlines() if line.startswith("args=")))
                        self.assertEqual(args, [kind, "--keep-going", "--message-format=json-diagnostic-rendered-ansi",
                                                *tail, *(["--cap-lints=warn"] if kind == "clippy" else [])])
                        self.assertNotIn('"compiler-artifact"', result.stdout)
                        if mode == "warning":
                            self.assertIn("warning: lint in lower", result.stdout)
                            self.assertIn("warning: lint in upper", result.stdout)
                            self.assertLess(result.stdout.index("finished all crates"), result.stdout.index("::error::"))
                        if mode == "error":
                            self.assertIn("error: type mismatch", result.stdout)
