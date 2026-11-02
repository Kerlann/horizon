"""Preflight checks must refuse mismatches before invoking the compiler."""
import importlib.util
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "dylib_spike", Path(__file__).resolve().parents[1] / "tools/dylib_spike.py")
spike = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(spike)


class DylibPreflight(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.game = self.root / "game"
        self.game.mkdir()
        (self.game / "Cargo.lock").write_text("locked crates")
        (self.game / "libhorizon_client.so").write_bytes(b"exact game build")
        self.identity = dict(toolchain="pinned rustc", profile="fast", flags=spike.FLAGS,
                             lock=spike.digest(self.game / "Cargo.lock"),
                             artifacts={"libhorizon_client.so": spike.digest(
                                 self.game / "libhorizon_client.so")})
        (self.game / "identity.json").write_text(json.dumps(self.identity))
        self.package = self.root / "package"
        self.package.mkdir()
        (self.package / "Cargo.toml").write_text(
            '[package]\nname="test-mod"\n[dependencies]\nhorizon-client="*"\n')
        self.args = SimpleNamespace(game=self.game, package=self.package, id="test",
                                    expect=None, mods=self.root / "mods")

    def test_toolchain_refused_before_compilation(self):
        with patch.object(spike, "rustc_version", return_value="other rustc"), \
             patch.object(spike, "run") as compiler:
            with self.assertRaisesRegex(SystemExit, "toolchain mismatch"):
                spike.build_mod(self.args)
            compiler.assert_not_called()
        self.assertFalse(self.args.mods.exists())

    def test_changed_shared_artifact_refused_before_compilation(self):
        (self.game / "libhorizon_client.so").write_bytes(b"different build")
        with patch.object(spike, "rustc_version", return_value="pinned rustc"), \
             patch.object(spike, "run") as compiler:
            with self.assertRaisesRegex(SystemExit, "crate/game build mismatch: libhorizon_client"):
                spike.build_mod(self.args)
            compiler.assert_not_called()

    def test_required_game_identity_mismatches_name_the_field(self):
        for field, value in (("toolchain", "another compiler"), ("profile", "dev"),
                             ("lock", "another lock"), ("artifacts", {})):
            with self.subTest(field=field):
                self.args.expect = self.root / "expected.json"
                self.args.expect.write_text(json.dumps(dict(self.identity, **{field: value})))
                with patch.object(spike, "rustc_version", return_value="pinned rustc"), \
                     patch.object(spike, "run") as compiler:
                    with self.assertRaisesRegex(SystemExit, f"test: {field} mismatch"):
                        spike.build_mod(self.args)
                    compiler.assert_not_called()

    def test_unknown_dependency_refused_not_compiled_independently(self):
        (self.package / "Cargo.toml").write_text('[dependencies]\nserde="1"\n')
        with patch.object(spike, "rustc_version", return_value="pinned rustc"), \
             patch.object(spike, "run") as compiler:
            with self.assertRaisesRegex(SystemExit, "crate mismatch: serde"):
                spike.build_mod(self.args)
            compiler.assert_not_called()

    def test_artifacts_survive_relocation(self):
        import shutil
        moved = self.root / "downloaded"
        shutil.copytree(self.game, moved)
        shutil.rmtree(self.game)
        with patch.object(spike, "rustc_version", return_value="pinned rustc"):
            self.assertEqual(spike.read_game(moved), self.identity)


if __name__ == "__main__":
    unittest.main()
