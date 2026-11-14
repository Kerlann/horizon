"""Gate failure reporting and the setup/check dependency contract."""
import re
import unittest

from tests.test_ci_concurrency import ROOT, yaml
from tools.ci.gate_summary import summary


class GateFailuresTests(unittest.TestCase):
    def test_summary_lists_failures_not_skips_or_successes(self):
        self.assertEqual(summary({"lint": {"outcome": "failure"}, "test": {"outcome": "success"},
                                  "docs": {"outcome": "skipped"}}),
                         "### Failed gate steps\n\n- `lint`\n")

    def test_independent_checks_continue_but_failed_setup_stops_them(self):
        workflow = yaml.load((ROOT / ".github/workflows/dyna-ci.yml").read_text(), Loader=yaml.BaseLoader)
        for job in ("lint", "verify", "verify-static", "launcher"):
            steps = workflow["jobs"][job]["steps"]
            ids = {s.get("id") for s in steps}
            for step in steps:
                for dependency in re.findall(r"steps\.([\w-]+)\.", step.get("if", "")):
                    self.assertIn(dependency, ids, (job, step))
            checks = [s for s in steps[steps.index(next(s for s in steps if s.get("id") == "setup"))+1:] if s.get("name", "").startswith(("Lint ", "Test Rust", "Test launcher"))]
            for check in checks:
                self.assertNotIn("success()", check["if"])
                self.assertIn("!cancelled()", check["if"])
                self.assertIn(".outcome == 'success'", check["if"])
                self.assertNotIn("continue-on-error", check)
