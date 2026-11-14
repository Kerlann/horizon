"""The verify guard refuses inet operations even when tests swallow their errors."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from tools.ci import network_guard, run_test_binaries


@unittest.skipUnless(sys.platform == "linux", "Linux preload guard")
class NetworkGuard(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.scratch = tempfile.TemporaryDirectory()
        cls.shim = Path(cls.scratch.name, "guard.so")
        source = Path(__file__).resolve().parents[1] / "tools/ci/network_guard.c"
        subprocess.run(["cc", "-shared", "-fPIC", "-Wall", "-Wextra", "-Werror", str(source), "-o", str(cls.shim)], check=True)

    @classmethod
    def tearDownClass(cls):
        cls.scratch.cleanup()

    def invoke(self, source, shim=None):
        binary = {"executable": sys.executable, "cwd": self.scratch.name, "env": {}, "libtest": False,
                  "label": "package target " + "long_test_name_" * 8}
        result = network_guard.run((binary, ["-c", source]), {}, False, shim or self.shim,
                                   run_test_binaries.run_one)
        self.addCleanup(result["output"].close)
        result["output"].seek(0)
        return result, result["output"].read()

    def test_swallowed_socket_error_still_fails_with_full_name(self):
        result, output = self.invoke('import socket\ntry: socket.socket()\nexcept OSError: pass')
        self.assertIn("socket", result["status"])
        self.assertIn("long_test_name_" * 8, output)
        self.assertIn('reason network', output)

    def test_name_resolution_is_forbidden(self):
        result, _ = self.invoke('import socket\ntry: socket.getaddrinfo("localhost", 80)\nexcept OSError: pass')
        self.assertIn("getaddrinfo", result["status"])

    def test_unix_socket_is_allowed(self):
        result, _ = self.invoke('import socket; socket.socket(socket.AF_UNIX).close()')
        self.assertEqual(result["status"], "ok")

    def test_children_inherit_guard(self):
        result, _ = self.invoke('import subprocess, sys; subprocess.run([sys.executable, "-c", '
                                '"import socket; socket.socket()"], capture_output=True)')
        self.assertIn("socket", result["status"])

    def test_missing_library_is_not_a_pass(self):
        result, _ = self.invoke('pass', Path(self.scratch.name, "absent.so"))
        self.assertIn("did not load", result["status"])

    def test_invalid_channel_exits_instead_of_losing_violation(self):
        env = {**os.environ, "LD_PRELOAD": str(self.shim), "HORIZON_NETWORK_GUARD_LOG": "/nonexistent/guard/log"}
        result = subprocess.run([sys.executable, "-c", "pass"], env=env)
        self.assertEqual(result.returncode, 125)

    def test_isolated_selection_omits_ignored_tests(self):
        binary = {"label": "package target", "env": {}}
        def listing(binary, env, args):
            return ["ignored"] if "--ignored" in args else ["ignored", "full::test"]
        tasks = list(network_guard.invocations([binary], {}, [], listing))
        self.assertEqual(len(tasks), 1)
        self.assertEqual(tasks[0][1], ["--exact", "full::test"])
        self.assertEqual(tasks[0][0]["isolated_test"], "full::test")

    def test_unreadable_listing_fails_closed(self):
        with self.assertRaises(ValueError):
            list(network_guard.invocations([{"label": "target", "env": {}}], {}, [], lambda *args: None))
