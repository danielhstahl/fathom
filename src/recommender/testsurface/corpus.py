"""The labelled corpus: fixtures in five languages, and what the ticket must say about each.

Everything the acceptance criteria claim is here with a hand-written expectation beside it:

| table | what it labels | AC |
| --- | --- | --- |
| `PARSE_CASES` | one revision → the named cases, skips and focus markers found | AC 1 (rust, python, ts, go — plus java, and the no-parser rows) |
| `PAIR_CASES` | a file pair → cases added / removed / skipped / un-skipped / weakened / lost | AC 1, AC 2 |
| `COVERAGE_CASES` | an episode → which new public symbols must fire `untested-public`, and which must not | AC 3, the tunable definition, negatives included |
| `EPISODE_CASES` | an episode → exact counts, ratio, proxy basis and stated confidence | AC 1, AC 4 |

Two rules keep this corpus honest rather than agreeable.

**Every rule has a negative.** A corpus that only contains the cases a detector catches
proves nothing, so the moving-a-test case, the macro swap (`assert_eq!(a, b)` →
`assert!(a == b)`), the covered-by-name symbol, the `pub(crate)` helper, the
below-threshold function and the excluded path are all here deliberately: the finding's
*silence* is half of AC 3, and silence is only tested by a case that expects it.

**Labels are facts about the file, not about the implementation.** Where a limitation makes
an answer a lower bound — a `.each` table whose rows are counted rather than evaluated, a
Python file that will not parse — the label and the `note` say so, so a change that makes
the answer better has to change the label and notice what it claimed.

Fixtures are deliberately small. A fixture you cannot read in one screen is a fixture whose
expected output nobody will re-derive when it changes.
"""

from __future__ import annotations

from dataclasses import dataclass, field

# ---------------------------------------------------------------------------
# Rust
# ---------------------------------------------------------------------------
RUST_SRC = '''use std::collections::BTreeMap;

pub struct Snapshot {
    pub rows: BTreeMap<String, u32>,
}

pub fn load_snapshot(path: &str) -> Snapshot {
    let raw = std::fs::read_to_string(path).unwrap();
    Snapshot { rows: parse_rows(&raw) }
}

fn parse_rows(raw: &str) -> BTreeMap<String, u32> {
    BTreeMap::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_empty_file() {
        let snap = load_snapshot("empty");
        assert_eq!(snap.rows.len(), 0);
    }

    #[test]
    fn loads_real_file() {
        assert_eq!(load_snapshot("real").rows.len(), 3);
    }
}
'''

RUST_SRC_NEW_TEST = '''use std::collections::BTreeMap;

pub struct Snapshot {
    pub rows: BTreeMap<String, u32>,
}

pub fn load_snapshot(path: &str) -> Snapshot {
    let raw = std::fs::read_to_string(path).unwrap();
    Snapshot { rows: parse_rows(&raw) }
}

pub fn write_snapshot(snap: &Snapshot) -> u32 {
    let mut total = 0;
    for (key, value) in &snap.rows {
        total += value;
        log(key, *value);
    }
    total
}

fn parse_rows(raw: &str) -> BTreeMap<String, u32> {
    BTreeMap::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_empty_file() {
        let snap = load_snapshot("empty");
        assert_eq!(snap.rows.len(), 0);
    }

    #[test]
    fn write_snapshot_sums_rows() {
        let snap = Snapshot { rows: BTreeMap::from([("a".to_string(), 2)]) };
        assert_eq!(write_snapshot(&snap), 2);
    }

    #[test]
    #[ignore = "flaky on windows"]
    fn loads_real_file() {
        assert_eq!(load_snapshot("real").rows.len(), 3);
    }
}
'''

RUST_MOVED = RUST_SRC.replace("mod tests {\n    use super::*;",
                              "mod tests {\n    use super::*;\n\n"
                              "    // the whole module shifted down two lines by a comment")

RUST_WEAKEN_BEFORE = '''pub fn report() -> Option<u32> {
    Some(42)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_total() {
        assert_eq!(report(), Some(42));
    }
}
'''

RUST_WEAKEN_AFTER = '''pub fn report() -> Option<u32> {
    Some(42)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_total() {
        assert!(report().is_some());
    }
}
'''

RUST_MACRO_SWAP = '''pub fn report() -> Option<u32> {
    Some(42)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_total() {
        let value = report();
        assert_eq!(value.is_some(), true);
    }
}
'''

RUST_TOKIO_GROUP = '''#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn fetches_remote() {
        assert!(fetch().await.is_ok());
    }

    #[test_case(1, 2 ; "small")]
    #[test_case(2, 4 ; "medium")]
    #[test_case(3, 6 ; "large")]
    fn doubles_input(input: i32, expected: i32) {
        assert_eq!(doubles(input), expected);
    }
}
'''

RUST_PLAIN_SOURCE = "pub fn only_logic(x: i32) -> i32 {\n    x + 1\n}\n"

# ---------------------------------------------------------------------------
# Python
# ---------------------------------------------------------------------------
PY_SUITE = '''import pytest


@pytest.fixture
def store():
    return {}


def test_standalone():
    assert load() == []


class TestCart:
    def test_adds(self, store):
        store["a"] = 1
        assert store["a"] == 1

    @pytest.mark.skip(reason="flaky")
    def test_removes(self, store):
        assert store == {}

    def test_checks(self, store):
        assert store.get("a") == 1
'''

PY_SUITE_NEW = '''import pytest


@pytest.fixture
def store():
    return {}


def test_standalone():
    assert load() == []


@pytest.mark.parametrize("n,expected", [(1, 1), (2, 4), (3, 9)])
def test_square(n, expected):
    assert n * n == expected


class TestCart:
    def test_adds(self, store):
        store["a"] = 1
        assert store["a"] == 1

    def test_checks(self, store):
        assert store.get("a")
'''

PY_SKIPS = '''import unittest

import pytest


@pytest.mark.skipif(os.environ.get("CI"), reason="no network in CI")
def test_remote():
    assert fetch()


@pytest.mark.xfail(reason="known bug 421")
def test_bug():
    assert 0


def test_runtime_skip():
    pytest.skip("needs docker")


class TestUnittest(unittest.TestCase):
    @unittest.skip("legacy")
    def test_legacy(self):
        self.assertEqual(1, 1)

    @unittest.expectedFailure
    def test_broken(self):
        self.assertEqual(fetch(), 3)
'''

PY_BROKEN = "def test_unclosed(:\n    pass\n"

PY_LOST_ASSERT = '''def test_total():
    assert order.total == 3
    assert order.rows == []
'''

PY_LOST_ASSERT_NEW = '''def test_total():
    assert order.rows == []
'''

# ---------------------------------------------------------------------------
# Go
# ---------------------------------------------------------------------------
GO_SUITE = '''package store

import (
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestLoad(t *testing.T) {
	got, err := Load("x")
	if err != nil {
		t.Fatalf("load: %v", err)
	}
	if got != want {
		t.Errorf("got %v want %v", got, want)
	}
	assert.Equal(t, got, 3)
	t.Run("nested", func(t *testing.T) {})
}

func TestNeedsNetwork(t *testing.T) {
	assert.NotNil(t, x)
}

func TestRemovedLater(t *testing.T) {
	assert.Equal(t, a, 1)
}

func BenchmarkLoad(b *testing.B) {
	for i := 0; i < b.N; i++ {
		Load("x")
	}
}
'''

GO_SUITE_NEW = '''package store

import (
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestLoad(t *testing.T) {
	got, err := Load("x")
	if err != nil {
		t.Fatalf("load: %v", err)
	}
	assert.NotNil(t, got)
	t.Run("nested", func(t *testing.T) {})
}

func TestNeedsNetwork(t *testing.T) {
	t.Skip("needs network")
	assert.NotNil(t, x)
}

func TestFresh(t *testing.T) {
	assert.Equal(t, b, 2)
}

func BenchmarkLoad(b *testing.B) {
	for i := 0; i < b.N; i++ {
		Load("x")
	}
}
'''

GO_PROD_NEW = '''package store

// Load reads a snapshot from disk and refuses an empty one.
func Load(path string) (Snapshot, error) {
	raw, err := os.ReadFile(path)
	if err != nil {
		return Snapshot{}, err
	}
	rows := parse(raw)
	if len(rows) == 0 {
		return Snapshot{}, ErrEmpty
	}
	return Snapshot{Rows: rows}, nil
}
'''

# ---------------------------------------------------------------------------
# TypeScript
# ---------------------------------------------------------------------------
# The demo's uncovered half: a new exported function with real logic and no test
# anywhere near it. Deliberately *not* under the directory the episode's Go test lives in,
# and named with tokens no test case in the episode shares — so none of the three coverage
# tests (`test.untested.path_overlap`, `name_overlap`, `mention_in_test`) can rescue it.
# Set against `RUST_SRC_NEW_TEST`, whose new `write_snapshot` *is* named by the test the
# same episode wrote: the same episode covering one new symbol and missing the other is
# the whole point of the finding.
GO_PRICING_NEW = '''package pricing

// ApplyDiscount discounts every row and refuses a percentage outside the unit interval.
func ApplyDiscount(rows []Row, pct float64) (float64, error) {
	if pct < 0 || pct > 1 {
		return 0, ErrPercent
	}
	total := 0.0
	for _, row := range rows {
		total += row.Price * row.Qty
	}
	return total * (1 - pct), nil
}
'''

TS_SUITE = '''import { describe, expect, it, test } from "vitest";

describe("cart", () => {
  it("adds", () => {
    expect(cart.total).toBe(3);
  });

  it.skip("removes", () => {
    expect(cart.items).toEqual([]);
  });

  it("checks", () => {
    expect(cart.find("a")).toBe(item);
  });
});

test("standalone", () => {
  expect(x).toBeDefined();
});
'''

TS_SUITE_NEW = '''import { describe, expect, it, test } from "vitest";

describe("cart", () => {
  it("adds", () => {
    expect(cart.total).toBe(3);
  });

  it("checks", () => {
    expect(cart.find("a")).toBeDefined();
  });

  it.only("focused", () => {
    expect(a).toBe(1);
  });
});

test("standalone", () => {
  expect(x).toBeDefined();
});
'''

TS_JEST_EACH = '''const cases = [
  [1, 2],
  [2, 4],
  [3, 6],
];

describe("doubles", () => {
  it.each(cases)("doubles %i", (input, expected) => {
    expect(doubles(input)).toBe(expected);
  });

  it.todo("negative input");
});
'''

TS_PROD_NEW = '''export function loadSnapshot(path: string): Snapshot {
  const raw = fs.readFileSync(path, "utf8");
  const rows = parseRows(raw);
  if (rows.size === 0) {
    throw new Error("empty");
  }
  return { rows };
}
'''

TS_UNRELATED_TEST = '''import { describe, expect, it } from "vitest";

describe("misc", () => {
  it("reads the file", () => {
    const snap = loadSnapshot("real");
    expect(snap.rows.size).toBe(3);
  });
});
'''

# ---------------------------------------------------------------------------
# Java
# ---------------------------------------------------------------------------
JAVA_SUITE = '''package com.example;

import org.junit.jupiter.api.Test;

class CartTest {
    @Test
    void adds() {
        assertEquals(cart.total(), 3);
    }

    @Test
    void checks() {
        assertEquals(cart.find("a"), item);
    }
}
'''

JAVA_SUITE_NEW = '''package com.example;

import org.junit.jupiter.api.Disabled;
import org.junit.jupiter.api.Test;

class CartTest {
    @Test
    void adds() {
        assertEquals(cart.total(), 3);
    }

    @Disabled("bug 421")
    @Test
    void checks() {
        assertNotNull(cart.find("a"));
    }
}
'''

# ---------------------------------------------------------------------------
# no test framework this tool can read (AC 4)
# ---------------------------------------------------------------------------
FEATURE_OLD = """Feature: checkout
  Scenario: card payment
    When I pay with "card"
    Then the total is 42
"""

FEATURE_NEW = """Feature: checkout
  Scenario: card payment
    When I pay with "card"
    Then the total is 42

  Scenario: gift card
    When I pay with "gift"
    Then the total is 0
"""

BASH_TEST = '''#!/usr/bin/env bash
set -e

run_test "cli prints version" 'fathom --version | grep -q 0.'
run_test "help works" 'fathom --help | grep -q index'
'''

# ---------------------------------------------------------------------------
# public-symbol fixtures for the coverage table
# ---------------------------------------------------------------------------
SRC_BEFORE = '''pub fn existing(x: i32) -> i32 {
    x
}
'''

SRC_AFTER = '''pub fn existing(x: i32) -> i32 {
    x
}

pub fn compute_total(rows: &[Row]) -> u32 {
    let mut total = 0;
    for row in rows {
        total += row.value;
    }
    total
}

pub struct Banner {
    pub text: String,
}

pub(crate) fn crate_helper(x: u8) -> u8 {
    let mut total = x;
    for shift in 0..3 {
        total = total.rotate_left(shift);
    }
    total
}

pub fn tiny(x: u8) -> u8 {
    x.checked_add(1).unwrap_or(0)
}
'''

RUST_TEST_COMPUTE_TOTAL = '''#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_total_sums_rows() {
        assert_eq!(compute_total(&rows()), 4);
    }
}
'''

BANNER_TYPE_ONLY = "pub struct Banner {\n    pub text: String,\n}\n"

# ---------------------------------------------------------------------------
# the labelled tables
# ---------------------------------------------------------------------------
@dataclass(frozen=True)
class ParseCase:
    """One revision, and every case name the parser must produce for it."""

    name: str
    path: str
    text: str
    cases: tuple[str, ...]
    skipped: tuple[str, ...] = ()
    focused: tuple[str, ...] = ()
    state: str = ""
    note: str = ""


@dataclass(frozen=True)
class PairCase:
    """A file pair, and the case-level deltas expected from it."""

    name: str
    path: str
    before: str
    after: str
    added: tuple[str, ...] = ()
    removed: tuple[str, ...] = ()
    skipped: tuple[str, ...] = ()
    unskipped: tuple[str, ...] = ()
    weakened: tuple[str, ...] = ()
    lost: tuple[str, ...] = ()
    focused: tuple[str, ...] = ()
    note: str = ""


@dataclass(frozen=True)
class CoverageCase:
    """An episode, its new public symbols, and which side of the line each falls on."""

    name: str
    files: dict[str, tuple[str | None, str | None]]
    untested: tuple[str, ...] = ()
    covered: tuple[str, ...] = ()
    knobs: dict = field(default_factory=dict)
    note: str = ""


@dataclass(frozen=True)
class EpisodeCase:
    """An episode with exact expectations about counts, ratio and stated confidence."""

    name: str
    files: dict[str, tuple[str | None, str | None]]
    counts: dict = field(default_factory=dict)
    test_lines: int | None = None
    prod_lines: int | None = None
    ratio: float | None = None
    basis: str = ""
    confidence: float | None = None
    must_say: tuple[str, ...] = ()
    note: str = ""


PARSE_CASES: tuple[ParseCase, ...] = (
    ParseCase("rust-inline-tests", "crates/store/src/snapshot.rs", RUST_SRC,
              cases=("tests::loads_empty_file", "tests::loads_real_file"), state="parsed",
              note="Rust tests live inside the source file: the region is test surface even "
                   "though the path taxonomy calls the file `source`"),
    ParseCase("rust-async-and-table", "crates/store/src/doubles.rs", RUST_TOKIO_GROUP,
              cases=("tests::fetches_remote", "tests::doubles_input#1",
                     "tests::doubles_input#2", "tests::doubles_input#3"),
              note="one case per `#[test_case]` row, named `fn#k`"),
    ParseCase("rust-no-tests", "crates/store/src/lib.rs", RUST_PLAIN_SOURCE, cases=(),
              state="empty", note="a source file with no test item is an honest zero"),
    ParseCase("python-pytest", "tests/test_cart.py", PY_SUITE,
              cases=("test_standalone", "TestCart::test_adds", "TestCart::test_removes",
                     "TestCart::test_checks"),
              skipped=("TestCart::test_removes",), state="parsed"),
    ParseCase("python-skips", "tests/test_skips.py", PY_SKIPS,
              cases=("test_remote", "test_bug", "test_runtime_skip",
                     "TestUnittest::test_legacy", "TestUnittest::test_broken"),
              skipped=("test_remote", "test_bug", "test_runtime_skip",
                       "TestUnittest::test_legacy", "TestUnittest::test_broken"),
              state="parsed",
              note="`skip`, `skipif`, `xfail`, `unittest.skip`, `expectedFailure` and a "
                   "runtime `pytest.skip()` all mean 'does not run'"),
    ParseCase("go-suite", "store/load_test.go", GO_SUITE,
              cases=("TestLoad", "TestNeedsNetwork", "TestRemovedLater", "BenchmarkLoad"),
              state="parsed",
              note="`t.Run` sub-tests are evidence on the parent case, not separate cases"),
    ParseCase("ts-vitest", "src/cart.test.ts", TS_SUITE,
              cases=("cart::adds", "cart::removes", "cart::checks", "standalone"),
              skipped=("cart::removes",), state="parsed"),
    ParseCase("ts-jest-each", "src/doubles.test.ts", TS_JEST_EACH,
              cases=("doubles::doubles %i", "doubles::negative input"),
              skipped=("doubles::negative input",), state="parsed",
              note="`it.todo` is a declared case that cannot fail, so it is listed skipped"),
    ParseCase("java-junit", "src/test/java/CartTest.java", JAVA_SUITE,
              cases=("CartTest::adds", "CartTest::checks"), state="parsed"),
    ParseCase("no-framework-feature", "tests/checkout.feature", FEATURE_NEW, cases=(),
              state="unsupported",
              note="Gherkin has no parser here: the row is a refusal, not a zero"),
    ParseCase("no-framework-bash", "tests/cli.sh", BASH_TEST, cases=(),
              state="unsupported", note="a bash test harness is a framework this tool "
                                        "cannot read"),
    ParseCase("python-unparseable", "tests/test_broken.py", PY_BROKEN, cases=(),
              state="failed", note="a file that will not parse refuses; it is not '0 tests'"),
)

PAIR_CASES: tuple[PairCase, ...] = (
    PairCase("rust-new-test-and-new-skip", "crates/store/src/snapshot.rs",
             RUST_SRC, RUST_SRC_NEW_TEST,
             added=("tests::write_snapshot_sums_rows",),
             skipped=("tests::loads_real_file",),
             note="a new test inside the same file's test module, and a case that gained "
                  "`#[ignore]`; the new `pub fn write_snapshot` is covered by the new case"),
    PairCase("rust-weakened-assert", "crates/store/src/report.rs",
             RUST_WEAKEN_BEFORE, RUST_WEAKEN_AFTER,
             weakened=("tests::reports_total",),
             note="assert_eq!(report(), Some(42)) → assert!(report().is_some()): the "
                  "expected value was thrown away, the subject is the same"),
    PairCase("rust-macro-swap-is-not-weakening", "crates/store/src/report.rs",
             RUST_WEAKEN_BEFORE, RUST_MACRO_SWAP,
             note="`assert_eq!(value, Some(42))` → `assert_eq!(value.is_some(), true)` keeps "
                  "an exact claim about a (differently keyed) subject; nothing is weakened"),
    PairCase("rust-moved-case-is-not-a-deletion", "crates/store/src/snapshot.rs",
             RUST_SRC, RUST_MOVED,
             note="two lines inserted above the cases: identity is the name, not the line"),
    PairCase("python-removes-and-weakens", "tests/test_cart.py", PY_SUITE, PY_SUITE_NEW,
             added=("test_square",), removed=("TestCart::test_removes",),
             weakened=("TestCart::test_checks",),
             note="the deleted case was also the skipped one: a deletion outranks the skip"),
    PairCase("python-assertions-lost", "tests/test_total.py", PY_LOST_ASSERT,
             PY_LOST_ASSERT_NEW, lost=("test_total",),
             note="the surviving case lost an exact assertion and gained none"),
    PairCase("go-skip-weaken-and-add", "store/load_test.go", GO_SUITE, GO_SUITE_NEW,
             added=("TestFresh",), removed=("TestRemovedLater",),
             skipped=("TestNeedsNetwork",), weakened=("TestLoad",),
             note="one pair, four answers: added, deleted, newly `t.Skip`'d, and "
                  "`Equal` → `NotNil`"),
    PairCase("ts-focus-and-weaken", "src/cart.test.ts", TS_SUITE, TS_SUITE_NEW,
             added=("cart::focused",), removed=("cart::removes",),
             weakened=("cart::checks",), focused=("cart::focused",),
             note="`it.only` is a finding of its own: the file's other tests stopped "
                  "running, and the new focused case is also a new case"),
    PairCase("java-disable-and-weaken", "src/test/java/CartTest.java", JAVA_SUITE,
             JAVA_SUITE_NEW, skipped=("CartTest::checks",), weakened=("CartTest::checks",),
             note="@Disabled and `assertEquals` → `assertNotNull` on the same method"),
    PairCase("go-production-only", "store/load.go", "package store\n", GO_PROD_NEW,
             note="a production file with no test file in the pair: this is the shape the "
                  "coverage finding exists for"),
)

COVERAGE_CASES: tuple[CoverageCase, ...] = (
    CoverageCase("untested-new-public-fn",
                 {"src/store.rs": (SRC_BEFORE, SRC_AFTER)},
                 untested=("compute_total",),
                 covered=("existing", "Banner", "crate_helper", "tiny"),
                 note="`Banner` has no logic, `crate_helper` is `pub(crate)`, `tiny` is "
                      "below the body-line floor, `existing` is not new: `compute_total` is "
                      "the only firing symbol"),
    CoverageCase("covered-by-new-test-name",
                 {"src/store.rs": (SRC_BEFORE, SRC_AFTER),
                  "tests/store.rs": (None, RUST_TEST_COMPUTE_TOTAL)},
                 covered=("compute_total",),
                 note="the new case `compute_total_sums_rows` shares a name token with the "
                      "new symbol, so the episode is not silent about it"),
    CoverageCase("covered-by-test-path",
                 {"store/load.go": (None, GO_PROD_NEW),
                  "store/load_test.go": (None, GO_SUITE)},
                 covered=("Load",),
                 note="the changed test file's name relates to the source file's path"),
    CoverageCase("covered-by-mention",
                 {"src/store/snapshot.ts": (None, TS_PROD_NEW),
                  "tests/misc.test.ts": (None, TS_UNRELATED_TEST)},
                 covered=("loadSnapshot",),
                 note="no name overlap and no path overlap: the mention of `loadSnapshot` in "
                      "the added test lines is what covers it"),
    CoverageCase("type-only-addition",
                 {"src/banner.rs": (None, BANNER_TYPE_ONLY)},
                 covered=("Banner",),
                 note="require_logic is on by default: a type with no body is not logic"),
    CoverageCase("type-only-with-require-logic-off",
                 {"src/banner.rs": (None, BANNER_TYPE_ONLY)},
                 untested=("Banner",), knobs={"require_logic": False, "min_body_lines": 1},
                 note="two knobs flipped together: with `require_logic` off a type's field "
                      "list is enough, and the body floor has to drop to clear it"),
    CoverageCase("body-floor-raised",
                 {"src/store.rs": (SRC_BEFORE, SRC_AFTER)},
                 untested=("compute_total", "tiny"), covered=("Banner", "crate_helper"),
                 knobs={"min_body_lines": 1},
                 note="lowering the floor to 1 promotes `tiny` from invisible to a candidate "
                      "that is untested like its bigger sibling"),
    CoverageCase("excluded-path",
                 {"examples/demo.rs": (None, SRC_AFTER)},
                 covered=("compute_total", "tiny"),
                 knobs={"exclude": ["**/examples/**"]},
                 note="a path `test.untested.exclude` claims never fires"),
    CoverageCase("all-rust-visibilities",
                 {"src/store.rs": (SRC_BEFORE, SRC_AFTER)},
                 untested=("compute_total", "crate_helper"), covered=("Banner", "tiny"),
                 knobs={"visibility": "crate"},
                 note="`test.untested.visibility = crate` promotes the crate-internal helper "
                      "into the finding, which is what a crate that treats `pub(crate)` as "
                      "its public surface asks for"),
)

EPISODE_CASES: tuple[EpisodeCase, ...] = (
    EpisodeCase("python-tests-only",
                {"tests/test_cart.py": (PY_SUITE, PY_SUITE_NEW)},
                counts={"tests_added": 1, "tests_removed": 1, "tests_skipped": 0,
                        "tests_unskipped": 0, "assertions_weakened": 1},
                prod_lines=0, basis="parsed-cases", confidence=0.9,
                must_say=("pytest",)),
    EpisodeCase("go-tests-and-production",
                {"store/load_test.go": (GO_SUITE, GO_SUITE_NEW),
                 "store/load.go": (None, GO_PROD_NEW)},
                counts={"tests_added": 1, "tests_removed": 1, "tests_skipped": 1},
                basis="parsed-cases", confidence=0.9,
                must_say=("1 : ",)),
    EpisodeCase("rust-inside-one-file",
                {"crates/store/src/snapshot.rs": (RUST_SRC, RUST_SRC_NEW_TEST)},
                counts={"tests_added": 1, "tests_removed": 0, "tests_skipped": 1},
                basis="parsed-cases", confidence=0.9,
                must_say=("test code inside the file",)),
    EpisodeCase("no-test-framework-at-all",
                {"tests/checkout.feature": (FEATURE_OLD, FEATURE_NEW)},
                basis="lines-only", confidence=0.3,
                must_say=("line count only", "a line count is not a coverage number")),
    EpisodeCase("partly-parseable-test-surface",
                {"tests/test_cart.py": (PY_SUITE, PY_SUITE_NEW),
                 "tests/checkout.feature": (FEATURE_OLD, FEATURE_NEW)},
                basis="partial", confidence=0.6,
                must_say=("lower bound",)),
    EpisodeCase("production-only-episode",
                {"store/load.go": (None, GO_PROD_NEW)},
                basis="no-test-change", confidence=1.0,
                must_say=("no test surface changed",)),
)


def demo_episode() -> dict[str, tuple[str | None, str | None]]:
    """The docs/CLI demo episode: four languages, seven files, every finding kind."""
    return {
        "crates/store/src/snapshot.rs": (RUST_SRC, RUST_SRC_NEW_TEST),
        "tests/test_cart.py": (PY_SUITE, PY_SUITE_NEW),
        "tests/test_total.py": (PY_LOST_ASSERT, PY_LOST_ASSERT_NEW),
        "store/load_test.go": (GO_SUITE, GO_SUITE_NEW),
        "src/cart.test.ts": (TS_SUITE, TS_SUITE_NEW),
        # a suite that was deleted outright: the file-level finding exists for this shape
        "tests/test_flaky.py": ('def test_flaky_on_windows():\n    assert load() == 1\n',
                                None),
        # new exported logic with no test of its own, in the same episode that tested the
        # Rust function above: the finding is the *difference* between the two
        "internal/pricing/quote.go": (None, GO_PRICING_NEW),
    }
