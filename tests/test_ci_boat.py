"""Boat routing must distinguish runner outages from real test failures."""
import importlib.util
from pathlib import Path
import unittest
import sys
from unittest.mock import patch
import os
import tempfile
import io
import json
import zipfile
import zlib
import subprocess
from types import SimpleNamespace
from datetime import datetime, timezone

spec = importlib.util.spec_from_file_location("boat_health", Path(__file__).parents[1] / "tools/ci/boat_health.py")
boat = importlib.util.module_from_spec(spec)
spec.loader.exec_module(boat)
sys.modules["boat_health"] = boat
watch_spec = importlib.util.spec_from_file_location("boat_watch", Path(__file__).parents[1] / "tools/ci/boat_watch.py")
watch = importlib.util.module_from_spec(watch_spec)
watch_spec.loader.exec_module(watch)
controller_spec = importlib.util.spec_from_file_location("boat_controller", Path(__file__).parents[1] / "tools/ci/boat_controller.py")
controller = importlib.util.module_from_spec(controller_spec)
controller_spec.loader.exec_module(controller)
NOW = datetime(2026, 11, 10, 12, tzinfo=timezone.utc)


def job(**changes):
    return dict(id=42, labels=["boat-large"], status="queued", conclusion=None,
                created_at="2026-11-10T11:49:59Z", steps=[], **changes)


class BoatHealthTests(unittest.TestCase):
    def test_stale_queue_falls_back(self):
        label, reason = boat.select_label("on", ["alice"], "alice", "alice", [job()], NOW)
        self.assertEqual(label, "ubuntu-latest")
        self.assertIn("42", reason)

    def test_threshold_is_strict(self):
        sample = job()
        sample["created_at"] = "2026-11-10T11:50:00Z"
        self.assertEqual(boat.health_reason([sample], NOW), "")

    def test_setup_failure(self):
        sample = job()
        sample.update(status="completed", conclusion="failure", steps=[
            dict(name="Set up job", started_at="2026-11-10T11:51:00Z", conclusion="failure"),
            dict(name="Complete job", started_at="2026-11-10T11:51:01Z")])
        self.assertIn("first step", boat.health_reason([sample], NOW))

    def test_real_failure_is_not_retried(self):
        sample = job()
        sample.update(status="completed", conclusion="failure", steps=[
            dict(name="Run tests", started_at="2026-11-10T11:51:00Z", conclusion="failure"),
            dict(name="Complete job", started_at="2026-11-10T11:51:01Z")])
        self.assertEqual(boat.health_reason([sample], NOW), "")

    def test_other_runners_do_not_poison_boat(self):
        sample = job()
        sample["labels"] = ["dyna-large"]
        self.assertEqual(boat.health_reason([sample], NOW), "")

    def test_mixed_real_failure_does_not_replace_run(self):
        queued = job()
        queued["name"] = "verify"
        failed = job()
        failed.update(name="lint", status="completed", conclusion="failure", steps=[
            dict(name="Run tests", started_at="2026-11-10T11:51:00Z")])
        self.assertEqual(watch.recovery_reason([queued, failed], NOW), "")
        failed["name"] = "launcher"
        self.assertEqual(watch.recovery_reason([queued, failed], NOW), "")
        failed["conclusion"] = "timed_out"
        self.assertEqual(watch.recovery_reason([queued, failed], NOW), "")
        self.assertEqual(boat.previous_attempt_reason([queued, failed], NOW), "")
        self.assertIn("queued", watch.recovery_reason([queued], NOW))

    def test_previous_cancelled_queue_retains_outage(self):
        sample = job()
        sample.update(name="lint", status="completed", conclusion="cancelled", completed_at="2026-11-10T12:00:01Z")
        self.assertIn("previous attempt", boat.previous_attempt_reason([sample], NOW))
        sample["completed_at"] = "2026-11-10T11:50:00Z"
        self.assertEqual(boat.previous_attempt_reason([sample], NOW), "")
        sample["completed_at"] = "2026-11-10T12:00:01Z"
        sample["steps"] = [dict(name="Run tests", started_at="2026-11-10T11:51:00Z")]
        self.assertEqual(boat.previous_attempt_reason([sample], NOW), "")

    def test_controller_limits_to_first_fork_attempt(self):
        run = dict(workflow_id=123, event="pull_request", run_attempt=1, status="queued", head_repository=dict(full_name="other/horizon"))
        self.assertTrue(controller.eligible(run, "arismoko/horizon", 123))
        for key, value in [("run_attempt", 2), ("event", "push"), ("workflow_id", 999), ("head_repository", dict(full_name="arismoko/horizon"))]:
            self.assertFalse(controller.eligible(dict(run, **{key: value}), "arismoko/horizon", 123))

    def test_controller_cancel_then_rerun(self):
        sample = job()
        sample["name"] = "lint"
        cancelled = dict(sample, status="completed", conclusion="cancelled", completed_at="2026-11-10T12:00:01Z")
        with patch.object(controller, "api", side_effect=[{"jobs": [sample]}, {"status": "completed"}, {"jobs": [cancelled]}]), patch.object(controller, "post") as post, patch.object(controller, "log"), patch.object(controller, "datetime") as clock:
            clock.now.return_value = NOW
            controller.recover("arismoko/horizon", dict(id=123, status="in_progress"))
        self.assertEqual([call.args[0] for call in post.call_args_list], [
            "repos/arismoko/horizon/actions/runs/123/cancel", "repos/arismoko/horizon/actions/runs/123/rerun"])

    def test_controller_does_not_rerun_late_real_failure(self):
        sample = job()
        sample["name"] = "lint"
        failed = dict(sample, status="completed", conclusion="failure", steps=[dict(name="Run tests", started_at="2026-11-10T11:51:00Z")])
        with patch.object(controller, "api", side_effect=[{"jobs": [sample]}, {"status": "completed"}, {"jobs": [failed]}]), patch.object(controller, "post") as post, patch.object(controller, "log"), patch.object(controller, "datetime") as clock:
            clock.now.return_value = NOW
            controller.recover("arismoko/horizon", dict(id=123, status="in_progress"))
        self.assertEqual(len(post.call_args_list), 1)
        self.assertTrue(post.call_args.args[0].endswith("/cancel"))

    def test_controller_completed_setup_failure_needs_no_cancel(self):
        sample = job()
        sample.update(name="verify", status="completed", conclusion="failure")
        with patch.object(controller, "api", return_value={"jobs": [sample]}), patch.object(controller, "post") as post, patch.object(controller, "log"), patch.object(controller, "datetime") as clock:
            clock.now.return_value = NOW
            controller.recover("arismoko/horizon", dict(id=123, status="completed"))
        post.assert_called_once_with("repos/arismoko/horizon/actions/runs/123/rerun")

    def test_controller_real_failure_never_calls_api_writes(self):
        sample = job()
        sample.update(name="lint", status="completed", conclusion="failure", steps=[dict(name="Run tests", started_at="2026-11-10T11:51:00Z")])
        with patch.object(controller, "api", return_value={"jobs": [sample]}), patch.object(controller, "post") as post, patch.object(controller, "log"), patch.object(controller, "datetime") as clock:
            clock.now.return_value = NOW
            controller.recover("arismoko/horizon", dict(id=123, status="completed"))
        post.assert_not_called()

    def test_rerun_gate_outputs_forced_github_and_summary(self):
        sample = job()
        sample.update(name="lint", status="completed", conclusion="cancelled", completed_at="2026-11-10T12:00:01Z")
        with tempfile.TemporaryDirectory() as directory:
            output, summary = Path(directory) / "output", Path(directory) / "summary"
            with patch.dict(os.environ, dict(GITHUB_REPOSITORY="arismoko/horizon", GITHUB_RUN_ID="123", GITHUB_RUN_ATTEMPT="2", BOAT_CI="off", AUTHOR="alice", ACTOR="alice", GITHUB_OUTPUT=str(output), GITHUB_STEP_SUMMARY=str(summary))), patch.object(boat, "api", return_value={"jobs": [sample]}):
                boat.main()
            self.assertIn("label=ubuntu-latest", output.read_text())
            self.assertIn("forced_github=true", output.read_text())
            self.assertIn("previous attempt", summary.read_text())

    def test_support_requires_original_trusted_gate_and_unmodified_pr(self):
        run = dict(pull_requests=[dict(base=dict(sha="base"))], head_sha="head")
        with patch.object(controller, "api", side_effect=[dict(sha="same")] * 4 + [dict(files=[], merge_base_commit=dict(sha="base"))]):
            self.assertEqual(controller.unsupported_reason("repo", run, "trusted"), "")
        with patch.object(controller, "api", side_effect=[dict(sha="same")] * 4 + [dict(files=[], merge_base_commit=dict(sha="older"))]):
            self.assertIn("not an ancestor", controller.unsupported_reason("repo", run, "trusted"))
        with patch.object(controller, "api", side_effect=[dict(sha="old"), dict(sha="new")]):
            self.assertIn("original base", controller.unsupported_reason("repo", run, "trusted"))
        for files in ([dict(filename="tools/ci/boat_health.py")], [dict(filename="renamed", previous_filename=".github/workflows/dyna-ci.yml")], [dict(filename="other")] * 300):
            with patch.object(controller, "api", side_effect=[dict(sha="same")] * 4 + [dict(files=files, merge_base_commit=dict(sha="base"))]):
                self.assertTrue(controller.unsupported_reason("repo", run, "trusted"))
        with patch.object(controller, "api") as api, patch.object(controller, "recorded_base", return_value=""):
            self.assertIn("unavailable", controller.unsupported_reason("repo", dict(pull_requests=[]), "trusted"))
            api.assert_not_called()

    def test_active_candidates_are_paginated_separately_from_failures(self):
        first_page = [dict(id=index) for index in range(100)]
        with patch.object(controller, "api", side_effect=[dict(workflow_runs=first_page), dict(workflow_runs=[dict(id=100)]), dict(workflow_runs=[]), dict(workflow_runs=[])]) as api:
            result = list(controller.candidates("repo", 123))
        self.assertEqual(len(result), 101)
        urls = [call.args[0] for call in api.call_args_list]
        self.assertIn("status=queued", urls[0])
        self.assertIn("page=2", urls[1])
        self.assertIn("status=in_progress", urls[2])
        self.assertIn("status=failure", urls[3])

    def test_real_fork_shape_uses_validated_recorded_base(self):
        # Observed API shape: run 42566379987 (KBroneZ/horizon), no PR association.
        run = dict(id=42566379987, head_sha="72d5f90f5aacc33db72a0592ffd7c58ace36dbf0", pull_requests=[])
        metadata = dict(run_id=str(run["id"]), attempt="1", head=run["head_sha"], base="b" * 40)
        for field, value in [(None, None), ("run_id", "123"), ("head", "c" * 40), ("base", "not-a-sha"), ("attempt", "2")]:
            record = dict(metadata)
            if field:
                record[field] = value
            archive = io.BytesIO()
            with zipfile.ZipFile(archive, "w") as zipped:
                zipped.writestr("boat-base.json", json.dumps(record))
            artifact = dict(id=99, name="boat-base-1", expired=False, size_in_bytes=len(archive.getvalue()))
            with patch.object(controller, "api", return_value=dict(artifacts=[artifact])), patch.object(controller.subprocess, "run", return_value=SimpleNamespace(stdout=archive.getvalue())):
                self.assertEqual(controller.recorded_base("repo", run), "" if field else "b" * 40)
        with patch.object(controller, "recorded_base", return_value="b" * 40), patch.object(controller, "api", side_effect=[dict(sha="same")] * 4 + [dict(files=[], merge_base_commit=dict(sha="b" * 40))]):
            self.assertEqual(controller.unsupported_reason("repo", run, "trusted"), "")

    def test_metadata_artifact_is_bounded_and_never_extracted(self):
        run = dict(id=123, head_sha="a" * 40)
        for artifacts in ([], [dict(id=99, name="boat-base-1", expired=False, size_in_bytes=65537)]):
            with patch.object(controller, "api", return_value=dict(artifacts=artifacts)), patch.object(controller.subprocess, "run") as download:
                self.assertEqual(controller.recorded_base("repo", run), "")
                download.assert_not_called()
        for filename, data in [("../boat-base.json", "{}"), ("boat-base.json", "x" * 4097)]:
            archive = io.BytesIO()
            with zipfile.ZipFile(archive, "w") as zipped:
                zipped.writestr(filename, data)
            artifact = dict(id=99, name="boat-base-1", expired=False, size_in_bytes=len(archive.getvalue()))
            with patch.object(controller, "api", return_value=dict(artifacts=[artifact])), patch.object(controller.subprocess, "run", return_value=SimpleNamespace(stdout=archive.getvalue())):
                self.assertEqual(controller.recorded_base("repo", run), "")

    def test_sweep_continues_after_one_candidate_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            event = Path(directory) / "event.json"
            event.write_text("{}")
            with patch.dict(os.environ, dict(GITHUB_REPOSITORY="repo", GITHUB_EVENT_NAME="schedule", GITHUB_EVENT_PATH=str(event), GITHUB_WORKFLOW_SHA="trusted")), patch.object(controller, "api", return_value=dict(id=123)), patch.object(controller, "candidates", return_value=[dict(id=1), dict(id=2)]), patch.object(controller, "consider", side_effect=[zlib.error("corrupt PR data"), None]) as consider, patch.object(controller, "log") as log:
                controller.main()
            self.assertEqual([call.args[1]["id"] for call in consider.call_args_list], [1, 2])
            self.assertIn("Run 1", log.call_args.args[0])

    def test_prior_api_failure_forces_github_instead_of_aborting(self):
        for failure in (subprocess.CalledProcessError(1, "gh"), json.JSONDecodeError("bad", "", 0), KeyError("jobs")):
            with tempfile.TemporaryDirectory() as directory:
                output, summary = Path(directory) / "output", Path(directory) / "summary"
                with patch.dict(os.environ, dict(GITHUB_REPOSITORY="repo", GITHUB_RUN_ID="123", GITHUB_RUN_ATTEMPT="2", BOAT_CI="off", AUTHOR="alice", ACTOR="alice", GITHUB_OUTPUT=str(output), GITHUB_STEP_SUMMARY=str(summary))), patch.object(boat, "api", side_effect=failure):
                    boat.main()
                self.assertIn("forced_github=true", output.read_text())
                self.assertIn("label=ubuntu-latest", output.read_text())
                self.assertIn("history is unavailable", summary.read_text())

    def test_corrupt_deflate_artifact_fails_closed(self):
        archive = io.BytesIO()
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as zipped:
            zipped.writestr("boat-base.json", "{}")
        corrupted = bytearray(archive.getvalue())
        corrupted[30 + len("boat-base.json")] = 0xff  # Reserved deflate block type.
        with zipfile.ZipFile(io.BytesIO(corrupted)) as zipped:
            with self.assertRaises(zlib.error):
                zipped.read("boat-base.json")
        artifact = dict(id=99, name="boat-base-1", expired=False, size_in_bytes=len(corrupted))
        with patch.object(controller, "api", return_value=dict(artifacts=[artifact])), patch.object(controller.subprocess, "run", return_value=SimpleNamespace(stdout=bytes(corrupted))):
            self.assertEqual(controller.recorded_base("repo", dict(id=123, head_sha="a" * 40)), "")

    def test_failure_sweep_only_inspects_recently_updated_runs(self):
        old = dict(id=1, updated_at="2026-11-10T10:59:59Z")
        recent = dict(id=2, updated_at="2026-11-10T11:30:00Z")
        with patch.object(controller, "api", side_effect=[dict(workflow_runs=[]), dict(workflow_runs=[]), dict(workflow_runs=[old, recent])]), patch.object(controller, "datetime", wraps=datetime) as clock:
            clock.now.return_value = NOW
            result = list(controller.candidates("repo", 123))
        self.assertEqual(result, [recent])

    def test_cancel_timeout_never_requests_rerun(self):
        sample = job()
        sample["name"] = "lint"
        with patch.object(controller, "api", side_effect=[dict(jobs=[sample])] + [dict(status="in_progress")] * 20), patch.object(controller, "post") as post, patch.object(controller, "log"), patch.object(controller.time, "sleep"), patch.object(controller, "datetime") as clock:
            clock.now.return_value = NOW
            with self.assertRaises(RuntimeError):
                controller.recover("repo", dict(id=123, status="in_progress"))
        post.assert_called_once_with("repos/repo/actions/runs/123/cancel")

    def test_allowlist_and_switch(self):
        for enabled, author, actor in [("off", "alice", "alice"), ("on", "bob", "alice"), ("on", "alice", "bob")]:
            self.assertEqual(boat.select_label(enabled, ["alice"], author, actor, [], NOW)[0], "ubuntu-latest")
        self.assertEqual(boat.select_label("on", ["alice"], "alice", "alice", [], NOW)[0], "boat-large")
