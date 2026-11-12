"""A contributor's pull request gets only the cheap checks; ours, main and dispatches get the heavy ones."""

import importlib.util
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("pr_gate", ROOT / "tools/ci/pr_gate.py")
pr_gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pr_gate)

SHA = "a" * 40
SAME_REPO = "github.event_name != 'pull_request' || github.event.pull_request.head.repo.full_name == github.repository"


def never_read(_):
    raise AssertionError("only a dispatch with pr reads a pull request")


class GateTests(unittest.TestCase):
    def test_a_fork_pull_request_is_cheap_and_ours_are_heavy(self):
        self.assertEqual(pr_gate.outputs("pull_request", False, "", never_read)["heavy"], "false")
        self.assertEqual(pr_gate.outputs("pull_request", True, "", never_read)["heavy"], "true")
        for event in ("push", "workflow_dispatch"):
            self.assertEqual(pr_gate.outputs(event, False, "", never_read)["heavy"], "true")

    def test_a_dispatch_with_pr_runs_that_head_and_keeps_a_fork_off_our_runners(self):
        fork = pr_gate.outputs("workflow_dispatch", True, "3665", lambda n: {"sha": SHA, "same_repository": False})
        self.assertEqual(fork, {"heavy": "true", "pr_sha": SHA, "trusted": "false"})
        ours = pr_gate.outputs("workflow_dispatch", True, "3672", lambda n: {"sha": SHA, "same_repository": True})
        self.assertEqual(ours["trusted"], "true")

    def test_pr_must_be_a_number(self):
        for bad in ("0", "12a", "1;id", "refs/pull/1/head", "-1"):
            with self.subTest(pr=bad), self.assertRaises(SystemExit):
                pr_gate.outputs("workflow_dispatch", True, bad, never_read)


class WorkflowTests(unittest.TestCase):
    workflow = (ROOT / ".github/workflows/dyna-ci.yml").read_text()

    def job(self, name):
        match = re.search(rf"^  {re.escape(name)}:\n(.*?)(?=^  [a-z0-9_-]+:\n|\Z)", self.workflow, re.M | re.S)
        self.assertIsNotNone(match, name)
        return match[1]

    def test_heavy_jobs_wait_for_the_gate_and_fail_closed(self):
        for name in ("lint", "verify"):
            with self.subTest(job=name):
                self.assertIn("(needs.changes.result != 'success' || needs.changes.outputs.heavy == 'true')",
                              self.job(name))
        self.assertIn("needs.changes.outputs.heavy == 'true'", self.job("launcher"))
        for name in ("ignored-tests-shards", "ignored-tests", "vanilla-parity"):
            self.assertIn("!inputs.pr", self.job(name), name)

    def test_cheap_jobs_run_without_the_heavy_gate(self):
        self.assertNotIn("outputs.heavy", self.job("verify-static"))
        self.assertIn("needs.changes.outputs.heavy != 'true'", self.job("format"))
        self.assertNotRegex(self.job("format"), r"cargo (clippy|build|test|check)")

    def test_a_dispatched_head_is_checked_out_with_a_read_only_token_and_reported_apart(self):
        for name in ("lint", "verify", "verify-static", "launcher"):
            with self.subTest(job=name):
                self.assertIn("ref: ${{ needs.changes.outputs.pr_sha }}", self.job(name))
                self.assertNotIn("statuses: write", self.job(name))
        status = self.job("pr-status")
        self.assertIn("statuses: write", status)
        self.assertNotIn("actions/checkout", status)
        self.assertIn("permissions:\n  contents: read\n", self.workflow)

    def test_no_fork_code_reaches_our_runners(self):
        # The one trust decision: the gate's answer for a dispatched pull request, else the event's own repository.
        self.assertIn(f"trusted: ${{{{ steps.gate.outputs.trusted || (({SAME_REPO}) && 'true' || 'false') }}}}",
                      self.job("changes"))
        self.assertNotIn("github.event.pull_request.head.repo.full_name == github.repository) &&", self.job("lint"))
        for name in ("lint", "verify", "verify-static", "launcher", "boat-watch"):
            body = self.job(name)
            if "head.repo.full_name" in body:
                self.fail(f"{name} decides trust itself instead of needs.changes.outputs.trusted")

    def test_a_contributors_cheap_checks_run_on_githubs_runners(self):
        # format and verify-static are all a fork pull request gets; a paid pool's balance must not fail them.
        self.assertIn("runs-on: ubuntu-latest\n", self.job("format"))
        static = self.job("verify-static")
        self.assertIn("needs.changes.outputs.trusted != 'true' && 'ubuntu-latest' ||", static)
        self.assertIn("RUNNER_KIND: ${{ needs.changes.outputs.trusted != 'true' && 'github' ||", static)

    def test_other_pull_request_workflows_skip_what_compiles_for_forks(self):
        for workflow, jobs in (("launcher-windows-check.yml", ("test", "runtime-leases")),
                               ("cargo-horizon-mod.yml", ("build",)), ("modding-book.yml", ("book",))):
            text = (ROOT / ".github/workflows" / workflow).read_text()
            for name in jobs:
                with self.subTest(workflow=workflow, job=name):
                    body = re.search(rf"^  {name}:\n(.*?)(?=^  [a-z0-9_-]+:\n|\Z)", text, re.M | re.S)[1]
                    self.assertIn(f"if: {SAME_REPO}\n", body)


if __name__ == "__main__":
    unittest.main()
