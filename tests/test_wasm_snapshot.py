"""Snapshot publication must not evict the repository's lint working set."""
import os
from pathlib import Path
import shutil
import subprocess
import time
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import patch

from tools.ci import wasm_snapshot as snapshot


class SnapshotTests(unittest.TestCase):
    @unittest.skipUnless(shutil.which("zstd"), "needs the runner's zstd")
    def test_real_archive_round_trip_preserves_staged_source_timestamps(self):
        build = snapshot.ROOT / "target"
        build.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=build) as directory:
            root = Path(directory)
            source = root / "internals-catalog-coverage/stage/source.rs"
            source.parent.mkdir(parents=True)
            source.write_text("original staged source")
            original_mtime = 1234567890123456789
            os.utime(source, ns=(original_mtime, original_mtime))
            (root / "internals-catalog-check").mkdir()
            archive = root / "wasm-snapshot/payload.tar.zst"
            env = {"GITHUB_OUTPUT": str(root / "outputs"), "GITHUB_STEP_SUMMARY": str(root / "summary"),
                   "GITHUB_REPOSITORY": "owner/repo", "WITHIN_TIME": "true", "EXACT_HIT": "false",
                   "MATCHED_KEY": "fixture", "RESTORE_STARTED": str(int(time.time())),
                   "GITHUB_REF": "refs/heads/proof"}
            real_run = subprocess.run

            def command(args, **kwargs):
                if args[0] == "gh":
                    return SimpleNamespace(returncode=0, stdout='{"active_caches_size_in_bytes": 0}')
                return real_run(args, **kwargs)

            with patch.dict(os.environ, env), patch.object(snapshot, "TARGET", root), \
                 patch.object(snapshot, "ARCHIVE", archive), \
                 patch.object(snapshot.subprocess, "run", side_effect=command):
                snapshot.pack()
                self.assertIn("publish=true", (root / "outputs").read_text())
                source.write_text("changed after snapshot")
                snapshot.restore()
            self.assertEqual(source.read_text(), "original staged source")
            self.assertEqual(source.stat().st_mtime_ns, original_mtime)
            self.assertIn("internals-catalog-check", (root / "summary").read_text())
            self.assertIn("within-time=true", (root / "outputs").read_text())

    def test_publication_budget_and_failed_occupancy_read(self):
        build = snapshot.ROOT / "target"
        build.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=build) as directory:
            root = Path(directory)
            for name in ("internals-catalog-coverage", "internals-catalog-check"):
                (root / name).mkdir()
            archive = root / "wasm-snapshot/payload.tar.zst"
            env = {"GITHUB_OUTPUT": str(root / "outputs"), "GITHUB_STEP_SUMMARY": str(root / "summary"),
                   "GITHUB_REPOSITORY": "owner/repo", "WITHIN_TIME": "true", "EXACT_HIT": "false",
                   "GITHUB_REF": "refs/heads/proof"}
            for occupied, size, status, expected in ((0, 1024, 0, "true"),
                                                    (7_999_999_999, 1024, 0, "false"),
                                                    (0, 2_000_000_001, 0, "false"),
                                                    (0, 1024, 1, "false")):
                (root / "outputs").write_text("")

                def command(args, **kwargs):
                    if args[0] == "gh":
                        return SimpleNamespace(returncode=status,
                                               stdout=f'{{"active_caches_size_in_bytes": {occupied}}}')
                    self.assertEqual(args[0], "tar")
                    # A sparse file supplies the archive size without allocating gigabytes.
                    with archive.open("wb") as stream:
                        stream.truncate(size)

                with patch.dict(os.environ, env), patch.object(snapshot, "TARGET", root), \
                     patch.object(snapshot, "ARCHIVE", archive), \
                     patch.object(snapshot, "capture", return_value=b"1024 fixture\n"), \
                     patch.object(snapshot.subprocess, "run", side_effect=command):
                    snapshot.pack()
                self.assertEqual((root / "outputs").read_text().splitlines()[-1], f"publish={expected}")

    def test_main_publication_is_throttled_without_creating_an_archive(self):
        build = snapshot.ROOT / "target"
        build.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=build) as directory:
            root = Path(directory)
            env = {"GITHUB_OUTPUT": str(root / "outputs"), "GITHUB_STEP_SUMMARY": str(root / "summary"),
                   "GITHUB_REF": "refs/heads/main", "WITHIN_TIME": "true", "EXACT_HIT": "false"}
            recent = {"actions_caches": [{"created_at": "2999-01-01T00:00:00Z"}]}
            with patch.dict(os.environ, env), patch.object(snapshot, "cache_metadata", return_value=recent), \
                 patch.object(snapshot.subprocess, "run") as command:
                snapshot.pack()
            command.assert_not_called()
            self.assertEqual((root / "outputs").read_text(), "publish=false\n")

    def test_changed_catalog_inputs_toolchain_and_lock_invalidate_the_key(self):
        build = snapshot.ROOT / "target"
        build.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=build) as directory:
            root = Path(directory)
            env = {"GITHUB_OUTPUT": str(root / "outputs"), "GITHUB_STEP_SUMMARY": str(root / "summary")}
            keys = set()
            for source, toolchain, lock in ((b"a", b"a", "a"), (b"b", b"a", "a"),
                                            (b"a", b"b", "a"), (b"a", b"a", "b")):
                (root / "Cargo.lock").write_text(lock)
                (root / "outputs").write_text("")
                with patch.dict(os.environ, env), patch.object(snapshot, "ROOT", root), \
                     patch.object(snapshot, "capture", side_effect=lambda *args: toolchain if args[0] == "rustup" else source):
                    snapshot.key()
                keys.add(dict(line.split("=", 1) for line in (root / "outputs").read_text().splitlines())["key"])
            self.assertEqual(len(keys), 4)

    def test_proof_keys_have_no_production_fallback(self):
        build = snapshot.ROOT / "target"
        build.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=build) as directory:
            outputs = Path(directory) / "outputs"
            for namespace in ("wasm-snapshot-v1", "wasm-snapshot-proof-v1-head"):
                outputs.write_text("")
                env = {"GITHUB_OUTPUT": str(outputs), "GITHUB_STEP_SUMMARY": str(Path(directory) / "summary"),
                       "CACHE_NAMESPACE": namespace}
                with patch.dict(os.environ, env), patch.object(snapshot, "capture", return_value=b"identity"):
                    snapshot.key()
                keys = dict(line.split("=", 1) for line in outputs.read_text().splitlines())
                for name in ("key", "prefix", "fallback"):
                    self.assertTrue(keys[name].startswith(namespace + "-"))
                self.assertTrue(keys["key"].startswith(keys["prefix"]))


if __name__ == "__main__":
    unittest.main()
