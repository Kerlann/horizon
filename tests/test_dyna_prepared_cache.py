"""The Dyna preparation cache reuses keys and recovers its lock after a killed job."""
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "tools/ci/vanilla-data.sh"


class PreparedCacheTest(unittest.TestCase):
    def test_fresh_stale_and_dead_lock(self):
        (ROOT / "target").mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=ROOT / "target") as directory:
            root = Path(directory)
            source, cache, work, tmp = [root / x for x in ("source", "cache", "work", "temp")]
            for path in (source / "minecraft/26.3", source / "tools", cache, work, tmp):
                path.mkdir(parents=True)
            (source / "minecraft/26.3/prepared.json").write_text("{}")
            (work / "research-lock.json").write_text('{"version": "26.3"}')
            target = work / "target/release"
            target.mkdir(parents=True)
            prepare = target / "horizon-prepare"
            prepare.write_text('#!/bin/sh\nmkdir -p .local/horizon-client/26.3 .local/datagen\n')
            prepare.chmod(0o755)
            binary = work / "bin"
            binary.mkdir()
            cargo = binary / "cargo"
            cargo.write_text('#!/bin/sh\nexit 0\n')
            cargo.chmod(0o755)
            env = dict(os.environ, PATH=f"{binary}:{os.environ['PATH']}", HORIZON_DYNA_CACHE="true",
                       HORIZON_PREPARED_CACHE=str(cache), HORIZON_SHARED_SOURCE=str(source),
                       HORIZON_PREPARE_KEY="fixture", RUNNER_TEMP=str(tmp), CARGO_TARGET_DIR=str(work / "target"))
            def run():
                return subprocess.run(["bash", SCRIPT], cwd=work, env=env, text=True,
                                      capture_output=True, check=True).stdout
            self.assertIn("prepared key fixture stale", run())
            self.assertEqual(run(), "prepared key fixture fresh\n")
            lock = cache / "prepared/.lock-file"
            holder = subprocess.Popen(["bash", "-c", 'exec 9>"$1"; flock 9; exec sleep 60', "holder", str(lock)])
            time.sleep(0.2)
            waiter = subprocess.Popen(["bash", SCRIPT], cwd=work, env=env, text=True,
                                      stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            time.sleep(0.2)
            self.assertIsNone(waiter.poll())
            holder.send_signal(signal.SIGKILL)
            holder.wait(timeout=5)
            out, err = waiter.communicate(timeout=5)
            self.assertEqual(waiter.returncode, 0, err)
            self.assertEqual(out, "prepared key fixture fresh\n")


if __name__ == "__main__":
    unittest.main()
