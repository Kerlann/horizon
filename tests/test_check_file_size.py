import contextlib
import io
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

from tools import check_file_size


def entry(path, before, now, generated=False):
    return (path, before, now, generated)


class ProblemsTests(unittest.TestCase):
    def test_a_big_file_may_not_grow(self):
        found = check_file_size.problems([entry("crates/a/src/game.rs", 21000, 21001)])
        self.assertEqual(len(found), 1)
        self.assertIn("docs/layers.md#file-size", found[0])

    def test_a_big_file_may_shrink_or_stay(self):
        self.assertEqual(check_file_size.problems([entry("a.rs", 21000, 20000), entry("b.rs", 5000, 5000)]), [])

    def test_small_files_grow_up_to_the_limit(self):
        self.assertEqual(check_file_size.problems([entry("a.rs", 2000, 3000)]), [])
        self.assertEqual(len(check_file_size.problems([entry("a.rs", 2000, 3001)])), 1)

    def test_a_new_big_file_fails(self):
        self.assertIn("a new file", check_file_size.problems([entry("a.rs", 0, 3500)])[0])

    def test_the_message_names_both_ways_out(self):
        found = check_file_size.problems([entry("crates/a/src/mobs/ai.rs", 3100, 3101)])[0]
        self.assertIn('ai_tests.rs (`#[path = "ai_tests.rs"] mod tests;`)', found)
        self.assertIn("pure-move PR first", found)

    def test_third_party_and_generated_files_are_not_checked(self):
        self.assertEqual(check_file_size.problems([entry("third_party/naga/src/x.rs", 8000, 9000),
                                                   entry("a.rs", 4000, 4100, generated=True)]), [])

    def test_a_test_filename_does_not_exempt_non_inline_code(self):
        for path in ("feature_tests.rs", "crates/a/src/mod_content_tests.rs",
                     "crates/b/src/level/tests.rs", "tests.rs"):
            self.assertEqual(len(check_file_size.problems([entry(path, 3100, 3101)])), 1, path)
        self.assertEqual(len(check_file_size.problems([entry("tests.rs", 0, 3500)])), 1)


TEST_MODULE = """#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn braces_in_literals_and_comments() {
        let text = "}\\" {";
        let close = '}';
        let open = '{';
        let quote = '\\'';
        let raw = r#"
}
{"#;
        // }
        /* } /* nested { */ } */
        assert!(text.contains(close) && open != quote && !raw.is_empty());
    }
}
"""


def skipped(text):
    return check_file_size.test_module_lines(text)


class TestModuleTests(unittest.TestCase):
    """Inline test modules don't count; anything else, and anything the rule isn't sure about, does."""

    def test_a_test_module_is_skipped_with_the_braces_in_its_literals(self):
        self.assertEqual(skipped("fn a() {}\n\n" + TEST_MODULE + "\nfn b() {}\n"), TEST_MODULE.count("\n"))

    def test_visibility_and_nesting(self):
        text = ("pub mod outer {\n    #[cfg(test)]\n    pub(crate) mod a {\n        fn f() {\n        }\n    }\n"
                "    fn real() {}\n}\n#[cfg(test)]\npub mod b {\n    #[cfg(test)]\n    mod c {\n    }\n}\n")
        self.assertEqual(skipped(text), 5 + 6)
        self.assertEqual(check_file_size.code_lines(text), 3)

    def test_windows_line_endings(self):
        self.assertEqual(skipped(TEST_MODULE.replace("\n", "\r\n")), TEST_MODULE.count("\n"))

    def test_other_test_items_count(self):
        for text in ("#[cfg(test)]\nfn helper() {\n}\n", "#[cfg(test)]\nmod tests;\n",
                     "#[cfg(test)]\nimpl A {\n}\n", "#[cfg(not(test))]\nmod live {\n}\n"):
            self.assertEqual(skipped(text), 0, text)

    def test_a_module_quoted_in_a_string_counts(self):
        quoted = "const SOURCE: &str = r#\"\n#[cfg(test)]\nmod tests {\n}\n\"#;\n"
        self.assertEqual(skipped(quoted), 0)
        self.assertEqual(skipped("/*\n#[cfg(test)]\nmod tests {\n}\n*/\n"), 0)

    def test_a_module_not_laid_out_by_rustfmt_counts(self):
        for text in ("#[cfg(test)]\nmod tests {\n    fn a() {}\n} // end\nfn real() {}\n}\n",
                     "#[cfg(test)]\nmod tests {\nfn a() {}\n}\n",
                     "#[cfg(test)]\nmod tests {\n    fn a() {}\n    }\nfn real() {\n}\n",
                     "#[cfg(test)]\nmod tests { fn a() {} }\nfn real() {\n}\n",
                     "#[cfg(test)]\nmod tests {\n    fn a() {}\n    } mod live { pub fn real() {}\n}\n",
                     "#[cfg(test)]\nmod tests {\n    fn a() {}\n"):
            self.assertEqual(skipped(text), 0, text)


class GitTests(unittest.TestCase):
    """Against a real repository: renames compare with the old name, the working tree counts."""

    def test_rename_and_growth_in_a_repository(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            env = dict(os.environ, GIT_AUTHOR_NAME="t", GIT_AUTHOR_EMAIL="t@t.invalid",
                       GIT_COMMITTER_NAME="t", GIT_COMMITTER_EMAIL="t@t.invalid")

            def run(*args):
                subprocess.run(["git", *args], cwd=root, check=True, capture_output=True, env=env)

            run("init", "-q", "-b", "main")
            (root / "big.rs").write_text("x\n" * 3100)
            # UTF-8 that Windows' code page cannot decode (0x9d): `git show` output is read as UTF-8.
            (root / "small.rs").write_bytes("// ”\n".encode() * 10)
            run("add", ".")
            run("commit", "-q", "-m", "base")
            run("checkout", "-q", "-b", "work")
            run("mv", "big.rs", "moved.rs")
            run("commit", "-q", "-m", "rename")
            (root / "small.rs").write_text("y\n" * 20)
            with mock.patch.object(check_file_size, "ROOT", root), contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(check_file_size.main(["--base", "main"]), 0)
                (root / "moved.rs").write_text("x\n" * 3101)
                self.assertEqual(check_file_size.main(["--base", "main"]), 1)

    def test_test_modules_dont_count_on_either_side(self):
        """The base (git show) and the working tree count the same way: a file over the limit may grow inside its test
        module, but not outside it."""
        def source(code, tests):
            return "x\n" * code + "#[cfg(test)]\nmod tests {\n" + "    t();\n" * tests + "}\n"

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            env = dict(os.environ, GIT_AUTHOR_NAME="t", GIT_AUTHOR_EMAIL="t@t.invalid",
                       GIT_COMMITTER_NAME="t", GIT_COMMITTER_EMAIL="t@t.invalid")

            def run(*args):
                subprocess.run(["git", *args], cwd=root, check=True, capture_output=True, env=env)

            run("init", "-q", "-b", "main")
            (root / "big.rs").write_text(source(3100, 200))
            run("add", ".")
            run("commit", "-q", "-m", "base")
            run("checkout", "-q", "-b", "work")
            out = io.StringIO()
            with mock.patch.object(check_file_size, "ROOT", root), contextlib.redirect_stdout(out):
                (root / "big.rs").write_text(source(3100, 300))
                run("commit", "-q", "-am", "more tests")
                self.assertEqual(check_file_size.main(["--base", "main"]), 0)
                (root / "big.rs").write_text(source(3101, 100))
                self.assertEqual(check_file_size.main(["--base", "main"]), 1)
            self.assertIn("big.rs: 3101 lines without test modules (3100 on the base)", out.getvalue())



class SyncBaselineTests(unittest.TestCase):
    def test_the_cap_allows_equal_or_lower_but_not_growth(self):
        self.assertEqual(check_file_size.problems([entry("a.rs", 3100, 3200)], baseline={"a.rs": 3200}), [])
        self.assertEqual(check_file_size.problems([entry("a.rs", 3100, 3199)], baseline={"a.rs": 3200}), [])
        self.assertTrue(check_file_size.problems([entry("a.rs", 3100, 3201)], baseline={"a.rs": 3200}))

    def test_only_a_sync_declaration_allows_new_or_raised_entries(self):
        sizes, previous, counts = {"a.rs": 3200, "b.rs": 3300}, {"a.rs": 3100}, {"a.rs": 3200, "b.rs": 3300}
        self.assertEqual(len(check_file_size.baseline_problems(sizes, previous, counts, False)), 2)
        self.assertEqual(check_file_size.baseline_problems(sizes, previous, counts, True), [])
        self.assertEqual(check_file_size.baseline_problems({"a.rs": 3000}, previous, {"a.rs": 3000}, False), [])

    def test_stale_high_and_deleted_entries_fail_even_in_syncs(self):
        found = check_file_size.baseline_problems({"a.rs": 3200, "gone.rs": 3500}, {}, {"a.rs": 3199}, True)
        self.assertEqual(len(found), 2)
        self.assertTrue(all("stale-high" in problem for problem in found))

    def test_the_sync_marker_is_a_full_sha_and_entries_are_unique(self):
        marker = "a" * 40
        self.assertEqual(check_file_size.parse_baseline(f"# sync-baseline: {marker}\na.rs 3200\n"),
                         (marker, {"a.rs": 3200}))
        with self.assertRaises(ValueError):
            check_file_size.parse_baseline("# sync-baseline: not-a-sha")
        with self.assertRaises(ValueError):
            check_file_size.parse_baseline("a.rs 3200\na.rs 3300")


class SyncProvenanceTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.run_git("init", "-q", "-b", "main")
        self.run_git("config", "user.name", "Fixture")
        self.run_git("config", "user.email", "fixture@example.invalid")
        self.commit("base")
        self.base = self.run_git("rev-parse", "HEAD").strip()
        self.run_git("checkout", "-q", "-b", "upstream")
        self.commit("upstream")
        self.upstream = self.run_git("rev-parse", "HEAD").strip()
        self.remote = self.root / "upstream.git"
        self.run_git("init", "-q", "--bare", str(self.remote))
        self.run_git("push", "-q", str(self.remote), "HEAD:refs/heads/main")
        environment = mock.patch.dict(os.environ, {"HORIZON_UPSTREAM_URL": str(self.remote)})
        environment.start()
        self.addCleanup(environment.stop)
        self.run_git("checkout", "-q", "-b", "work", self.base)
        self.commit("fork")
        patch = mock.patch.object(check_file_size, "ROOT", self.root)
        patch.start()
        self.addCleanup(patch.stop)

    def run_git(self, *args):
        return subprocess.run(["git", *args], cwd=self.root, check=True,
                              capture_output=True, text=True).stdout

    def commit(self, name):
        (self.root / name).write_text(name)
        self.run_git("add", name)
        self.run_git("commit", "-q", "-m", name)

    def test_real_sync_parent_passes(self):
        self.run_git("merge", "-q", "--no-edit", "upstream")
        self.assertEqual(check_file_size.sync_provenance(self.upstream, self.base), [])

    def test_fake_sha_missing_header_and_base_ancestor_fail(self):
        for marker in ["b" * 40, None, self.base]:
            with self.subTest(marker=marker):
                self.assertTrue(check_file_size.sync_provenance(marker, self.base))

    def test_an_upstream_base_ancestor_that_is_a_merge_parent_is_rejected(self):
        # A forced merge makes the old base an exact merge parent, and it is
        # also in upstream main. Only the outside-base rule rejects this SHA.
        self.run_git("checkout", "-q", "-b", "already-based", self.base)
        self.run_git("merge", "-q", "--no-ff", "--no-edit", "upstream")
        result = check_file_size.sync_provenance(self.base, self.base)
        self.assertIn("already an ancestor of the base", result[0])

    def test_ancestor_without_merge_parent_is_not_a_sync(self):
        self.run_git("checkout", "-q", "upstream")
        self.commit("child")
        self.assertTrue(check_file_size.sync_provenance(self.upstream, self.base))

    def test_a_fork_commit_is_not_an_upstream_main_ancestor(self):
        fork = self.run_git("rev-parse", "HEAD").strip()
        self.assertIn("not an ancestor of upstream main",
                      check_file_size.sync_provenance(fork, self.base)[0])

    def test_unreachable_upstream_fails_even_with_a_cached_ref(self):
        self.run_git("merge", "-q", "--no-edit", "upstream")
        self.assertEqual(check_file_size.sync_provenance(self.upstream, self.base), [])
        with mock.patch.dict(os.environ, {"HORIZON_UPSTREAM_URL": str(self.root / "missing.git")}):
            result = check_file_size.sync_provenance(self.upstream, self.base)
        self.assertIn("upstream main fetch failed", result[0])

    def test_fork_side_branch_merge_cannot_exempt_a_new_production_file(self):
        self.run_git("checkout", "-q", "-b", "forgery", self.base)
        self.commit("forged-parent")
        forged = self.run_git("rev-parse", "HEAD").strip()
        self.run_git("checkout", "-q", "work")
        self.run_git("merge", "-q", "--no-edit", "forgery")
        self.assertIn("not an ancestor of upstream main",
                      check_file_size.sync_provenance(forged, self.base)[0])
        (self.root / "tools").mkdir()
        (self.root / "tools/file_size_baseline.txt").write_text(
            f"# sync-baseline: {forged}\nproduction.rs 3101\n")
        (self.root / "production.rs").write_text("fn production() {}\n" * 3101)
        self.run_git("add", "tools/file_size_baseline.txt", "production.rs")
        self.run_git("commit", "-q", "-m", "forge size exemption")
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.assertNotEqual(check_file_size.main(["--base", self.base]), 0)
        self.assertIn("not an ancestor of upstream main", output.getvalue())
        self.assertIn("baseline added or raised", output.getvalue())
        checker = self.root / "tools/check_file_size.py"
        checker.write_text(Path(check_file_size.__file__).read_text())
        result = subprocess.run([sys.executable, str(checker), "--base", self.base],
                                cwd=self.root, capture_output=True, text=True)
        self.assertEqual(result.returncode, 1)
        self.assertIn("not an ancestor of upstream main", result.stdout)

    def test_shallow_history_is_an_explicit_failure(self):
        self.run_git("merge", "-q", "--no-edit", "upstream")
        with tempfile.TemporaryDirectory() as clone:
            subprocess.run(["git", "clone", "-q", "--depth", "1", self.root.as_uri(), clone], check=True)
            with mock.patch.object(check_file_size, "ROOT", Path(clone)):
                result = check_file_size.sync_provenance(self.upstream, self.base)
                self.assertIn("unshallow", result[0])

if __name__ == "__main__":
    unittest.main()
