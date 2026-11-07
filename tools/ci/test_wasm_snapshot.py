"""Snapshot publication must not evict the repository's lint working set."""
import os
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import patch

from tools.ci import wasm_snapshot as snapshot


class SnapshotTests(unittest.TestCase):
    def test_publication_budget_and_failed_occupancy_read(self):
        build = snapshot.ROOT / "target"
        build.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=build) as directory:
            root = Path(directory)
            for name in ("internals-catalog-coverage", "internals-catalog-check"):
                (root / name).mkdir()
            archive = root / "wasm-snapshot/payload.tar.zst"
            env = {"GITHUB_OUTPUT": str(root / "outputs"), "GITHUB_STEP_SUMMARY": str(root / "summary"),
                   "GITHUB_REPOSITORY": "owner/repo", "WITHIN_TIME": "true", "EXACT_HIT": "false"}
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
