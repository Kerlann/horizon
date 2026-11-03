import importlib.util
import json
import shutil
import tempfile
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location(
    "stamp_native_hooks", Path(__file__).resolve().parents[1] / "tools/stamp_native_hooks.py"
)
stamp = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(stamp)


class StampTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.original = stamp.ROOT
        stamp.ROOT = self.root
        self.addCleanup(setattr, stamp, "ROOT", self.original)
        (self.root / stamp.CATALOG).write_text('{"hooks": []}\n')
        (self.root / stamp.FRAGMENTS).mkdir(parents=True)

    def fragment(self, name):
        path = self.root / stamp.FRAGMENTS / f"{name}.json"
        path.write_text(json.dumps({"hooks": [{"name": name, "target": f"target::{name}", "side": "server"}]}))
        return path

    def test_check_collection_order_and_idempotence(self):
        catalog = self.root / stamp.CATALOG
        catalog.write_text('{"hooks": [{"name": "existing", "version": 3, "side": "server"}]}\n')
        z = self.fragment("z")
        a = self.fragment("a")
        before = catalog.read_bytes()
        self.assertTrue(stamp.collect_fragments(True))
        self.assertEqual(catalog.read_bytes(), before)
        self.assertTrue(a.exists())
        self.assertTrue(z.exists())
        self.assertFalse(stamp.collect_fragments(False))
        hooks = json.loads(catalog.read_text())["hooks"]
        self.assertEqual([(h["name"], h["version"]) for h in hooks],
                         [("existing", 3), ("a", 1), ("z", 1)])
        self.assertFalse(a.exists())
        self.assertFalse(z.exists())
        after = catalog.read_bytes()
        self.assertFalse(stamp.collect_fragments(False))
        self.assertEqual(catalog.read_bytes(), after)

    def test_helper_only_fragment_is_collected(self):
        path = self.fragment("helper")
        helpers = [{"name": "helper", "target": "target::helper", "side": "server"}]
        path.write_text(json.dumps({"hooks": [], "helpers": helpers}))
        self.assertFalse(stamp.collect_fragments(False))
        catalog = json.loads((self.root / stamp.CATALOG).read_text())
        self.assertEqual(catalog, {"hooks": [], "helpers": helpers})
        self.assertFalse(path.exists())

    def test_catalog_facts_travel_with_the_hook(self):
        path = self.fragment("served")
        fragment = json.loads(path.read_text())
        contract = {"kind": "listen", "receiver": "game"}
        fragment["hooks"][0].update(
            {"neoforge": "SomeEvent", "fabric": ["SomeCallback", "OtherCallback"], "paper": "SomePaperEvent",
             "side": "server", "cancellable": True, "mod_api": contract, "other": 1})
        path.write_text(json.dumps(fragment))
        self.assertFalse(stamp.collect_fragments(False))
        hooks = json.loads((self.root / stamp.CATALOG).read_text())["hooks"]
        self.assertEqual(hooks, [{"name": "served", "version": 1, "target": "target::served",
                                  "neoforge": "SomeEvent", "fabric": ["SomeCallback", "OtherCallback"],
                                  "paper": "SomePaperEvent", "side": "server", "cancellable": True, "mod_api": contract}])

    def test_collection_does_not_read_or_write_source_and_docs(self):
        path = self.fragment("drops")
        fragment = json.loads(path.read_text())
        fragment.update({"rust_summary": "pending source note", "doc_summary": "pending docs note",
                         "comments": ["notes.rs"]})
        path.write_text(json.dumps(fragment))
        notes = self.root / "notes.rs"
        notes.write_text("/// A hook's documentation.\n")
        before = notes.read_bytes()
        self.assertFalse(stamp.collect_fragments(False))
        self.assertEqual(notes.read_bytes(), before)
        self.assertEqual({p.relative_to(self.root).as_posix() for p in self.root.rglob("*") if p.is_file()},
                         {"notes.rs", stamp.CATALOG.as_posix()})

    def test_duplicates_fail_before_writes(self):
        catalog_path = self.root / stamp.CATALOG
        for kind in ("hooks", "helpers"):
            for origin in ("catalog", "fragment"):
                with self.subTest(kind=kind, origin=origin):
                    for path in (self.root / stamp.FRAGMENTS).glob("*.json"):
                        path.unlink()
                    entry = {"name": "same", "target": "target::same", "side": "server"}
                    fragment = {"hooks": []}
                    fragment[kind] = [entry]
                    first = self.root / stamp.FRAGMENTS / "first.json"
                    first.write_text(json.dumps(fragment))
                    catalog = {"hooks": []}
                    if origin == "catalog":
                        catalog[kind] = [entry]
                    else:
                        (self.root / stamp.FRAGMENTS / "second.json").write_text(json.dumps(fragment))
                    catalog_path.write_text(json.dumps(catalog))
                    paths = [catalog_path, *(self.root / stamp.FRAGMENTS).glob("*.json")]
                    before = {path: path.read_bytes() for path in paths}
                    with self.assertRaisesRegex(ValueError, "duplicate"):
                        stamp.collect_fragments(False)
                    self.assertEqual({path: path.read_bytes() for path in paths}, before)

    def test_repository_catalog_collection_needs_no_source_tree(self):
        repo = self.original
        fragments = sorted((repo / stamp.FRAGMENTS).glob("*.json"))
        expected = json.loads((repo / stamp.CATALOG).read_text())
        expected_hooks = [(h["name"], h["side"]) for h in expected["hooks"]]
        expected_helpers = expected.get("helpers", []).copy()
        for path in fragments:
            fragment = json.loads(path.read_text())
            expected_hooks.extend((h["name"], h["side"]) for h in fragment["hooks"])
            expected_helpers.extend(fragment.get("helpers", []))
            shutil.copyfile(path, self.root / stamp.FRAGMENTS / path.name)
        shutil.copyfile(repo / stamp.CATALOG, self.root / stamp.CATALOG)
        self.assertFalse(stamp.collect_fragments(False))
        catalog = json.loads((self.root / stamp.CATALOG).read_text())
        self.assertEqual([(h["name"], h["side"]) for h in catalog["hooks"]], expected_hooks)
        self.assertEqual(catalog.get("helpers", []), expected_helpers)
        self.assertFalse(list((self.root / stamp.FRAGMENTS).glob("*.json")))


if __name__ == "__main__":
    unittest.main()
