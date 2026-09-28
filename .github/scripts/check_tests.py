#!/usr/bin/env python3
"""Fail a pull request that changes Rust code under src/ without touching tests.

Shellify's unit tests live next to the code in `#[cfg(test)]` modules, so
"touching tests" means that at least one added line of the PR falls inside
test code:

- a `#[cfg(test)]` item (usually `mod tests { ... }`),
- a function marked `#[test]` or `#[<anything>::test]` (e.g. `#[tokio::test]`),
- or a test-only file: anything under a `tests/` directory, or a file named
  `tests.rs`, `test.rs`, `*_tests.rs` or `*_test.rs`.

Changes that are only deletions, blank lines or comments don't count as code
changes. A PR can opt out with the `no-tests-needed` label (docs, CI or
changes that genuinely can't be tested).

Usage (CI passes these through the environment):

    BASE_REF=origin/main PR_LABELS='["bug"]' python3 check_tests.py

Run the self-tests with `python3 -m unittest discover -s .github/scripts`.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import PurePosixPath

BYPASS_LABEL = "no-tests-needed"

# `#[cfg(test)]`, `#[cfg(all(test, unix))]`, but not `#[cfg(not(test))]`.
CFG_TEST = re.compile(r"#\[\s*cfg\s*\((?P<pred>.*)\)\s*\]")
# `#[test]`, `#[tokio::test]`, `#[tokio::test(flavor = "multi_thread")]`.
TEST_ATTR = re.compile(r"#\[\s*(?:[A-Za-z_][A-Za-z0-9_]*\s*::\s*)*test\s*(?:\(.*\))?\s*\]")
# `r"..."`, `r#"..."#`, `br"..."`.
RAW_STRING = re.compile(r'b?r(#*)"')


def is_test_path(path: str) -> bool:
    """True for files that only hold test code."""
    p = PurePosixPath(path)
    if "tests" in p.parts[:-1]:
        return True
    return p.name in ("tests.rs", "test.rs") or p.stem.endswith(("_tests", "_test"))


def _is_test_cfg(line: str) -> bool:
    m = CFG_TEST.search(line)
    if not m:
        return False
    pred = m.group("pred")
    return re.search(r"\btest\b", pred) is not None and "not(test)" not in pred.replace(" ", "")


def _code_mask(source: str) -> list[str]:
    """Return the source with comments, strings and char literals blanked out.

    Newlines are kept, so line numbers still line up. This is only precise
    enough to match braces, which is all the region finder needs.
    """
    out = []
    i, n = 0, len(source)
    while i < n:
        c = source[i]
        nxt = source[i + 1] if i + 1 < n else ""
        if c == "/" and nxt == "/":  # line comment
            while i < n and source[i] != "\n":
                i += 1
            continue
        if c == "/" and nxt == "*":  # block comment, may nest
            depth = 0
            while i < n:
                if source.startswith("/*", i):
                    depth += 1
                    i += 2
                elif source.startswith("*/", i):
                    depth -= 1
                    i += 2
                    if depth == 0:
                        break
                else:
                    out.append("\n" if source[i] == "\n" else " ")
                    i += 1
            continue
        raw = RAW_STRING.match(source, i) if c in "br" else None
        if raw and (i == 0 or not (source[i - 1].isalnum() or source[i - 1] == "_")):
            end = source.find('"' + raw.group(1), raw.end())
            end = n if end == -1 else end + 1 + len(raw.group(1))
            out.extend("\n" if ch == "\n" else " " for ch in source[i:end])
            i = end
            continue
        if c == '"':
            j = i + 1
            while j < n and source[j] != '"':
                j += 2 if source[j] == "\\" else 1
            out.extend("\n" if ch == "\n" else " " for ch in source[i : j + 1])
            i = j + 1
            continue
        if c == "'":
            # A char literal ('x', '\n', '\u{1F600}', '{') rather than a lifetime.
            if nxt == "\\":
                j = source.find("'", i + 3)
                j = n - 1 if j == -1 else j
                out.append(" " * (j + 1 - i))
                i = j + 1
                continue
            if i + 2 < n and source[i + 2] == "'":
                out.append("   ")
                i += 3
                continue
        out.append(c)
        i += 1
    return "".join(out).split("\n")


def test_regions(source: str) -> list[tuple[int, int]]:
    """1-based inclusive line ranges of test code in a Rust file."""
    code = _code_mask(source)
    regions = []
    for start, line in enumerate(code):
        if not (_is_test_cfg(line) or TEST_ATTR.search(line)):
            continue
        # The item runs from the attribute to the brace that closes its body,
        # or to a `;` for a bodiless item (`mod tests;`, `use ...;`). Start
        # scanning after the attribute so its own brackets don't count.
        column = max(m.end() for m in (CFG_TEST.search(line), TEST_ATTR.search(line)) if m)
        depth, opened, end = 0, False, len(code) - 1
        for number in range(start, len(code)):
            text = code[number][column:] if number == start else code[number]
            depth, opened, ended = _scan(text, depth, opened)
            if ended:
                end = number
                break
        regions.append((start + 1, end + 1))
    return regions


def _scan(text: str, depth: int, opened: bool) -> tuple[int, bool, bool]:
    """Track brace depth across one line; report whether the item ended."""
    for ch in text:
        if ch == "{":
            depth += 1
            opened = True
        elif ch == "}":
            depth -= 1
            if opened and depth == 0:
                return depth, opened, True
        elif ch == ";" and not opened and depth == 0:
            return depth, opened, True
    return depth, opened, False


def _in_regions(line: int, regions: list[tuple[int, int]]) -> bool:
    return any(a <= line <= b for a, b in regions)


def _is_code(text: str) -> bool:
    stripped = text.strip()
    return bool(stripped) and not re.match(r"(//|/\*|\*/|\*(\s|$))", stripped)


@dataclass
class FileChange:
    path: str
    added: dict[int, str]  # new-file line number -> text
    source: str  # the file as of the PR head


@dataclass
class Verdict:
    ok: bool
    message: str
    code_files: list[str] = field(default_factory=list)
    test_files: list[str] = field(default_factory=list)


def evaluate(changes: list[FileChange], labels: list[str]) -> Verdict:
    code_files, test_files = [], []
    for change in changes:
        if is_test_path(change.path):
            if any(_is_code(t) for t in change.added.values()):
                test_files.append(change.path)
            continue
        regions = test_regions(change.source)
        has_code = has_test = False
        for number, text in change.added.items():
            if not _is_code(text):
                continue
            if _in_regions(number, regions):
                has_test = True
            else:
                has_code = True
        if has_code:
            code_files.append(change.path)
        if has_test:
            test_files.append(change.path)

    if BYPASS_LABEL in labels:
        return Verdict(True, f"Skipped: the PR has the `{BYPASS_LABEL}` label.", code_files, test_files)
    if not code_files:
        return Verdict(True, "No Rust code under src/ changed, so no tests are required.", code_files, test_files)
    if test_files:
        return Verdict(
            True,
            "Rust code changed and tests were added or updated in: " + ", ".join(test_files),
            code_files,
            test_files,
        )
    return Verdict(
        False,
        "This PR changes Rust code under src/ but adds or changes no unit tests.\n"
        "Changed code: " + ", ".join(code_files) + "\n"
        "Add or update a test in a `#[cfg(test)] mod tests` block (a `#[test]` or "
        "`#[tokio::test]` function) that covers the change. If the change genuinely "
        f"can't be tested, add the `{BYPASS_LABEL}` label to the PR and say why in "
        "the description; the check re-runs when the label is added.",
        code_files,
        test_files,
    )


def _git(*args: str) -> str:
    return subprocess.run(["git", *args], check=True, capture_output=True, text=True).stdout


HUNK = re.compile(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@")


def parse_added_lines(diff: str) -> dict[int, str]:
    """Map new-file line numbers to text for every added line of a -U0 diff."""
    added, line = {}, 0
    for raw in diff.split("\n"):
        m = HUNK.match(raw)
        if m:
            line = int(m.group(1))
            continue
        if raw.startswith("+++") or raw.startswith("---"):
            continue
        if raw.startswith("+"):
            added[line] = raw[1:]
            line += 1
        elif raw.startswith(" "):
            line += 1
    return added


def collect_changes(base: str, head: str = "HEAD") -> list[FileChange]:
    names = _git("diff", "--name-only", "--diff-filter=AMRC", f"{base}...{head}", "--", "src", "tests")
    changes = []
    for path in filter(None, names.split("\n")):
        if not path.endswith(".rs"):
            continue
        diff = _git("diff", "-U0", "--no-color", f"{base}...{head}", "--", path)
        source = _git("show", f"{head}:{path}")
        changes.append(FileChange(path, parse_added_lines(diff), source))
    return changes


def main() -> int:
    base = os.environ.get("BASE_REF") or (sys.argv[1] if len(sys.argv) > 1 else "origin/main")
    labels = json.loads(os.environ.get("PR_LABELS") or "[]")
    verdict = evaluate(collect_changes(base), labels)

    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as f:
            f.write("## Unit tests required\n\n")
            f.write(("Passed. " if verdict.ok else "**Failed.** ") + verdict.message.replace("\n", "\n\n") + "\n")
    if verdict.ok:
        print(verdict.message)
        return 0
    first, _, rest = verdict.message.partition("\n")
    in_actions = os.environ.get("GITHUB_ACTIONS") == "true"
    print(f"::error title=Unit tests required::{first}" if in_actions else first)
    print(rest)
    return 1


if __name__ == "__main__":
    sys.exit(main())
