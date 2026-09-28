"""Self-tests for check_tests.py: `python3 -m unittest discover -s .github/scripts`."""

import os
import subprocess
import tempfile
import textwrap
import unittest

from check_tests import (
    BYPASS_LABEL,
    FileChange,
    collect_changes,
    evaluate,
    is_test_path,
    parse_added_lines,
    test_regions,
)

SOURCE = textwrap.dedent(
    """\
    pub fn add(a: i32, b: i32) -> i32 {
        let _brace = '{';
        let _s = "}}} not a brace";
        a + b
    }

    #[cfg(not(test))]
    fn real_only() {}

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn adds() {
            assert_eq!(add(1, 2), 3);
        }
    }

    pub fn after() {}
    """
)


def lines_of(source, *needles):
    """1-based line numbers of the lines containing each needle."""
    lines = source.split("\n")
    return [next(i + 1 for i, l in enumerate(lines) if n in l) for n in needles]


class RegionTests(unittest.TestCase):
    def test_finds_the_cfg_test_module_and_ignores_braces_in_literals(self):
        (start,) = lines_of(SOURCE, "#[cfg(test)]")
        end = SOURCE.split("\n").index("}", start) + 1  # the module's closing brace
        self.assertIn((start, end), test_regions(SOURCE))
        (code,) = lines_of(SOURCE, "a + b")
        self.assertFalse(any(a <= code <= b for a, b in test_regions(SOURCE)))

    def test_not_test_cfg_is_not_a_test_region(self):
        (line,) = lines_of(SOURCE, "fn real_only")
        self.assertFalse(any(a <= line <= b for a, b in test_regions(SOURCE)))

    def test_attribute_forms(self):
        src = textwrap.dedent(
            """\
            #[tokio::test(flavor = "multi_thread")]
            async fn a() {
                let _ = r#"{ unbalanced"#;
            }
            fn outside() {}
            #[cfg(all(test, unix))]
            use std::os::unix::net::UnixStream;
            fn also_outside() {}
            """
        )
        regions = test_regions(src)
        self.assertEqual(regions, [(1, 4), (6, 7)])

    def test_bodiless_test_module(self):
        self.assertEqual(test_regions("#[cfg(test)]\nmod tests;\nfn x() {}\n"), [(1, 2)])

    def test_test_paths(self):
        for path in ["src/app/tests.rs", "src/player/mpv_tests.rs", "src/ui/tests/layout.rs", "tests/cli.rs"]:
            self.assertTrue(is_test_path(path), path)
        for path in ["src/app/mod.rs", "src/test_utils_are_not_tests/../latest.rs", "src/contest.rs"]:
            self.assertFalse(is_test_path(path), path)


class DiffTests(unittest.TestCase):
    def test_parse_added_lines(self):
        diff = textwrap.dedent(
            """\
            diff --git a/src/a.rs b/src/a.rs
            --- a/src/a.rs
            +++ b/src/a.rs
            @@ -3,0 +4,2 @@ fn x() {
            +    one();
            +    two();
            @@ -10 +12 @@
            -old
            +new
            """
        )
        self.assertEqual(parse_added_lines(diff), {4: "    one();", 5: "    two();", 12: "new"})


class VerdictTests(unittest.TestCase):
    def change(self, *needles, path="src/lib.rs"):
        return FileChange(path, {n: SOURCE.split("\n")[n - 1] for n in lines_of(SOURCE, *needles)}, SOURCE)

    def test_code_without_tests_fails(self):
        verdict = evaluate([self.change("a + b")], [])
        self.assertFalse(verdict.ok)
        self.assertIn(BYPASS_LABEL, verdict.message)
        self.assertEqual(verdict.code_files, ["src/lib.rs"])

    def test_code_with_tests_passes(self):
        self.assertTrue(evaluate([self.change("a + b", "assert_eq!")], []).ok)

    def test_tests_in_another_file_count(self):
        test_file = FileChange("src/app/tests.rs", {1: "fn t() {}"}, "fn t() {}\n")
        self.assertTrue(evaluate([self.change("a + b"), test_file], []).ok)

    def test_label_bypasses(self):
        self.assertTrue(evaluate([self.change("a + b")], ["docs", BYPASS_LABEL]).ok)

    def test_comments_and_blank_lines_are_not_code(self):
        change = FileChange("src/lib.rs", {1: "    // just a comment", 2: "", 3: "/// docs", 4: " * more"}, SOURCE)
        self.assertTrue(evaluate([change], []).ok)

    def test_deref_assignment_is_code(self):
        change = FileChange("src/lib.rs", {1: "    *count = 0;"}, SOURCE)
        self.assertFalse(evaluate([change], []).ok)

    def test_no_rust_changes_pass(self):
        self.assertTrue(evaluate([], []).ok)


class GitTests(unittest.TestCase):
    """End to end against a throwaway repository."""

    def git(self, *args):
        subprocess.run(["git", "-C", self.repo, *args], check=True, capture_output=True)

    def write(self, path, text):
        full = os.path.join(self.repo, path)
        os.makedirs(os.path.dirname(full), exist_ok=True)
        with open(full, "w") as f:
            f.write(text)

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.repo = self.tmp.name
        self.git("init", "-q", "-b", "main")
        self.git("config", "user.email", "ci@example.invalid")
        self.git("config", "user.name", "CI")
        self.git("config", "commit.gpgsign", "false")
        self.write("src/main.rs", SOURCE)
        self.write("README.md", "hi\n")
        self.git("add", ".")
        self.git("commit", "-q", "-m", "base")
        self.git("checkout", "-q", "-b", "feature")
        self.cwd = os.getcwd()
        os.chdir(self.repo)

    def tearDown(self):
        os.chdir(self.cwd)
        self.tmp.cleanup()

    def commit(self):
        self.git("commit", "-q", "-am", "change")

    def test_code_change_without_tests_fails(self):
        self.write("src/main.rs", SOURCE.replace("a + b", "b + a"))
        self.commit()
        self.assertFalse(evaluate(collect_changes("main"), []).ok)

    def test_code_change_with_a_new_test_passes(self):
        new_test = "    #[test]\n    fn commutes() {\n        assert_eq!(add(2, 1), 3);\n    }\n\n"
        changed = SOURCE.replace("a + b", "b + a").replace("    #[test]\n", new_test + "    #[test]\n")
        self.assertNotEqual(changed.count("#[test]"), SOURCE.count("#[test]"))
        self.write("src/main.rs", changed)
        self.commit()
        self.assertTrue(evaluate(collect_changes("main"), []).ok)

    def test_non_rust_change_passes(self):
        self.write("README.md", "hello\n")
        self.commit()
        self.assertTrue(evaluate(collect_changes("main"), []).ok)

    def test_deleting_code_passes(self):
        self.write("src/main.rs", SOURCE.replace("pub fn after() {}\n", ""))
        self.commit()
        self.assertTrue(evaluate(collect_changes("main"), []).ok)


if __name__ == "__main__":
    unittest.main()
