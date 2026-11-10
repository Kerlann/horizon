"""Dyna's switch routes only supported, same-repository events to its pool."""
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]
BASH = shutil.which("bash")
POSIX_BASH = BASH is not None and not (
    os.name == "nt" and os.path.dirname(os.path.normcase(BASH)).endswith(os.path.normcase(r"\system32")))


class DynaRoutingTests(unittest.TestCase):
    def route(self, event, same_repo="true", enabled="true", lease=None):
        env = dict(os.environ, EVENT=event, SAME_REPO=same_repo, ENABLED=enabled,
                   LEASE=str(int(time.time()) + 3600) if lease is None else lease)
        return subprocess.check_output(["bash", ROOT / "tools/ci/dyna_ci_runner.sh"], env=env, text=True).strip()

    @unittest.skipUnless(POSIX_BASH, "needs a bash that is not WSL's")
    def test_only_allowed_events_and_same_repository(self):
        for event in ("push", "workflow_dispatch", "pull_request"):
            self.assertEqual(self.route(event), "self_hosted=true", event)
        for event in ("pull_request_target", "workflow_run", "issue_comment", "schedule", ""):
            self.assertEqual(self.route(event), "self_hosted=false", event)
        self.assertEqual(self.route("pull_request", same_repo="false"), "self_hosted=false")

    @unittest.skipUnless(POSIX_BASH, "needs a bash that is not WSL's")
    def test_switch_and_lease_fail_closed(self):
        self.assertEqual(self.route("push", enabled="false"), "self_hosted=false")
        for lease in ("", "bad", "0"):
            self.assertEqual(self.route("push", lease=lease), "self_hosted=false")

    @unittest.skipIf(os.name == "nt", "the script installs dependencies on Dyna's Ubuntu runner")
    def test_cached_dependencies_export_persistent_paths(self):
        script = ROOT / "tools/ci/dyna_ci_deps.sh"
        with tempfile.TemporaryDirectory() as directory:
            cache = Path(directory)
            prefix = cache / "apt-prefix"
            prefix.mkdir()
            (prefix / ".dyna-hash").write_text(hashlib.sha256(script.read_bytes()).hexdigest() + "\n")
            envfile, pathfile = cache / "env", cache / "path"
            # Dyna sets CARGO_HOME and CARGO_TARGET_DIR on the runner, as in session boxes, so the
            # host's shared compiler cache matches; the script keeps them and only puts Cargo's bin on PATH.
            cargo_home = cache / "dyna-cargo"
            env = dict(os.environ, DYNA_CI_BUILD=directory, GITHUB_ENV=str(envfile), GITHUB_PATH=str(pathfile),
                       CARGO_HOME=str(cargo_home))
            subprocess.run(["bash", script], env=env, check=True)
            exports = envfile.read_text()
            self.assertIn(f"RUSTUP_HOME={cache / 'rustup'}\n", exports)
            for key in ("CARGO_TARGET_DIR", "CARGO_HOME", "CARGO_INCREMENTAL"):
                self.assertNotIn(key, exports)
            self.assertIn(str(cargo_home / "bin"), pathfile.read_text())

    def test_workflow_wires_route_and_keeps_privileges_off_pool(self):
        from test_ci_runners import jobs
        workflow = (ROOT / ".github/workflows/dyna-ci.yml").read_text()
        blocks = dict(jobs(workflow))
        route = blocks["changes"]
        for item in ("vars.DYNA_CI_SELF_HOSTED", "vars.DYNA_CI_POOL_UNTIL", "github.event_name",
                     "github.event.pull_request.head.repo.full_name == github.repository", "dyna_ci_runner.sh"):
            self.assertIn(item, route)
        for name, label in (("verify", "dyna-large"), ("launcher", "dyna-medium")):
            block = blocks[name]
            self.assertIn("needs: changes", block)
            self.assertIn(f"needs.changes.outputs.self_hosted == 'true' && '{label}'", block)
            self.assertNotIn("secrets.", block)
            self.assertNotIn("permissions:", block)
        self.assertIn("runs-on: ubuntu-latest", blocks["pool-watch"])
        self.assertIn("permissions:\n  contents: read", workflow)

    def test_telemetry_ingest_checks_require_manual_dispatch(self):
        from test_ci_runners import triggers
        workflow = (ROOT / ".github/workflows/telemetry-server.yml").read_text()
        self.assertEqual(triggers(workflow), {"workflow_dispatch"})

    def test_schedule_only_dispatches_and_parity_only_runs_on_dispatch(self):
        from test_ci_runners import jobs, triggers
        workflow = (ROOT / ".github/workflows/dyna-ci.yml").read_text()
        self.assertEqual(triggers(workflow), {"pull_request", "push", "workflow_dispatch", "schedule"})
        blocks = dict(jobs(workflow))
        dispatcher = blocks["nightly-dispatch"]
        self.assertIn("github.event_name == 'schedule'", dispatcher)
        self.assertIn("runs-on: ubuntu-latest", dispatcher)
        self.assertIn('gh workflow run dyna-ci.yml --repo "$GITHUB_REPOSITORY" --ref main', dispatcher)
        self.assertIn("if: github.event_name != 'schedule'", blocks["changes"])
        parity = blocks["vanilla-parity"]
        self.assertIn("github.event_name == 'workflow_dispatch'", parity)
        self.assertIn("github.repository == 'arismoko/horizon'", parity)
        self.assertIn("runs-on: dyna-large", parity)
        self.assertIn("persist-credentials: false", parity)


if __name__ == "__main__":
    unittest.main()
