"""The card's fixtures: episodes with hand-authored answers, and the analyses the caller lends.

Fifteen cases, each authored as an episode plus the *expectations* the card must meet on it. The
cases are not evenly spread across "interesting code changes" — they are spread across the ways a
card can be wrong when the analyses around it are uneven, which is what the ticket's third
acceptance criterion is about:

| case | what it holds the card to |
| --- | --- |
| `code_and_tests` | the ordinary case: five sections measured, heuristic blast with a real inventory |
| `bare_index_record` | paths and counts only, no bytes anywhere: every content analysis is *unavailable*, with a reason and a fill-verb, and the card is still complete |
| `no_inventory` | bytes but no repository inventory: the heuristic census is the change set, and the card says its reach is a floor |
| `graph_full` | a module graph covers every changed path: basis `module graph`, one-hop reached through an edge |
| `graph_partial` | a graph covers two of three changed paths: unmapped ones are a stated partial, and confidence is scaled |
| `graph_no_cover` | a graph exists but has no node for anything that changed: heuristics answer, and say the graph was useless here |
| `deps_available` | a dependency delta is supplied: direct/version/transitive/supply figures, and unknown fields printed as unknown |
| `deps_half_empty` | a delta that reports nothing at all: unavailable, not a row of zeros |
| `migration_episode` | the non-code surface dominates: operator actions and blocking count on the card |
| `reformat_only` | `e2.2` collapses the episode: the `what` section reports the collapsed view and says so |
| `empty_episode` | nothing changed: no section invents a number, and blast refuses |
| `noise_and_code` | a real change wearing whitespace beside a real one: folded and semantic counts differ |
| `risk_supplied` | a caller supplies an `e4.4`-shaped score: the attention section relays it with its rank |
| `attention_floor` | every finding is `note`: with the default floor the section counts them and shows none |
| `wide_heuristic` | a 30-file package change: rows are capped and the remainder is named, not cut in silence |

Everything is built from bytes written in this file — no repository is cloned, nothing is
downloaded, and the same fixture gives the same numbers in every process (ADR-002 §4.5).
The expectations are written by hand next to the fixtures, so a card that changes its mind about
one of them has to be *re-labelled*, not just re-blessed.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from .blast import measure as measure_blast
from .compose import Card, compose
from .links import EvidenceMap
from .model import Absent, Analyses, Change, Episode, ModuleGraph, DepsDelta, RiskScore
from .policy import (SECTION_ATTENTION, SECTION_BLAST, SECTION_DEPENDENCIES, SECTION_IDS,
                     SECTION_SURFACE, SECTION_TESTS, SECTION_WHAT, SECTION_WHERE,
                     STATE_MEASURED, STATE_PARTIAL, STATE_UNAVAILABLE, Policy)

# --- source bytes ----------------------------------------------------------
SRC_BEFORE = """\
import json
from dataclasses import dataclass


@dataclass
class Cart:
    items: list


def total(cart):
    return sum(item.price for item in cart.items)


def tax(cart, rate):
    return total(cart) * rate


def dump(cart):
    return json.dumps({"total": total(cart)})
"""

SRC_AFTER = """\
import json
from dataclasses import dataclass


@dataclass
class Cart:
    items: list


def total(cart):
    return sum(item.price for item in cart.items)


def scaled_total(cart, factor):
    sub = total(cart)
    return sub * factor + 1


def tax(cart, rate):
    return total(cart) * rate


def dump(cart):
    return json.dumps({"total": total(cart)})
"""

TEST_BEFORE = """\
from src.cart import total


def test_total():
    assert total(Cart([])) == 0


def test_tax():
    assert tax(Cart([]), 0.2) == 0


def test_removed_case():
    assert total(Cart([Item(3)])) == 3
"""

TEST_AFTER = """\
from src.cart import total


def test_total():
    assert total(Cart([]))
"""

UTIL_BEFORE = """\
def money(x):
    return round(x, 2)


def slug(text):
    return text.lower().replace(' ', '-')
"""

UTIL_AFTER = """\
def money(x):
    return round(x, 2)


def slug(text):
    return text.lower(

        ).replace(' ', '-')
"""

MIGRATION = """\
class AddOrdersToInvoices < ActiveRecord::Migration[7.1]
  def change
    remove_column :invoices, :legacy_total, :decimal
    add_column :invoices, :order_id, :integer
  end
end
"""

ENV_BEFORE = """\
# Service configuration
DATABASE_URL=postgres://localhost/app
SEARCH_INDEX_HOST=search:9200
"""

ENV_AFTER = """\
# Service configuration
PG_DATABASE_URL=postgres://localhost/app
"""

SETTINGS_BEFORE = """\
import os

DATABASE_URL = os.environ["DATABASE_URL"]
SEARCH_INDEX_HOST = os.environ["SEARCH_INDEX_HOST"]
"""

SETTINGS_AFTER = """\
import os

PG_DATABASE_URL = os.environ["PG_DATABASE_URL"]
SEARCH_INDEX_HOST = os.environ["SEARCH_INDEX_HOST"]
"""

CI_BEFORE = """\
name: release
on:
  push:
    branches: [main]
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"""

CI_AFTER = """\
name: release
on:
  push:
    branches: [main]
jobs:
  build:
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - run: sign
"""

MANIFEST = """\
def manifest_version():
    return 3
"""

HELPER = """\
def helper(x):
    return x + 1
"""

FORMATTED_BEFORE = """\
use std::collections::HashMap;

pub fn thing(a: i32, b: i32) -> i32 {
    let total = a + b;
    total
}
"""

FORMATTED_AFTER = """\
use std::collections::HashMap;

pub fn thing(
    a: i32,
    b: i32,
) -> i32 {
    let total = a + b;
    total
}
"""

REAL_RUST_BEFORE = """\
pub fn parse(input: &str) -> String {
    input.trim().to_string()
}
"""

REAL_RUST_AFTER = """\
pub fn parse(input: &str) -> String {
    let trimmed = input.trim();
    trimmed.to_uppercase()
}
"""


# ---------------------------------------------------------------------------
# the module graphs and dependency deltas the caller can lend
# ---------------------------------------------------------------------------
def code_graph() -> ModuleGraph:
    """A graph whose edges say who uses whom: `(dependent, dependency)`."""
    return ModuleGraph(
        nodes=("src/cart.py", "src/util.py", "src/report.py", "tests/test_cart.py",
               "src/api.py"),
        edges=(("src/report.py", "src/cart.py"), ("tests/test_cart.py", "src/cart.py"),
               ("src/api.py", "src/report.py"), ("src/util.py", "src/cart.py")),
        basis="resolved imports (e3.2 stand-in)")


def partial_graph() -> ModuleGraph:
    """A graph missing one of the changed paths, so part of the answer is unknowable."""
    return ModuleGraph(nodes=("src/cart.py", "src/report.py"),
                       edges=(("src/report.py", "src/cart.py"),),
                       basis="resolved imports (partial index)")


def unrelated_graph() -> ModuleGraph:
    """A graph that has never heard of this episode's files."""
    return ModuleGraph(nodes=("docs/index.md", "tools/build.py"),
                       edges=(("tools/build.py", "docs/index.md"),))


def cargo_delta() -> DepsDelta:
    return DepsDelta(
        headline="2 new direct dependencies, 3 upgrades, 12 transitive-only moves",
        ecosystems=("cargo", "npm"), direct_added=2, direct_removed=1, upgraded=3,
        downgraded=0, transitive_only=12, source_switched=0, integrity_changed=1,
        licence_changed=1, conflicts=0, unparseable=0,
        evidence=["Cargo.toml: `serde_yaml 0.9` added (line 24)",
                  "Cargo.lock: `hyper 0.14.27 -> 0.14.31` (manifest did not move)"],
        source="fathom-analyze::deps::dependency_episode")


def empty_delta() -> DepsDelta:
    return DepsDelta(basis="no manifests were parsed", source="caller")


def risk() -> RiskScore:
    return RiskScore(score=82.0, rank=4, of=7,
                     reasons=["migration drops a column", "untested new logic"],
                     blind_spots=["no blast radius available"], source="spine-1")


# ---------------------------------------------------------------------------
# cases
# ---------------------------------------------------------------------------
@dataclass
class Case:
    """One fixture: an episode, what the caller lends, and what the card must say."""

    name: str
    episode: Episode
    analyses: Analyses = field(default_factory=Analyses)
    expect_states: dict[str, str] = field(default_factory=dict)
    expect_in_card: tuple[str, ...] = ()
    forbid_in_card: tuple[str, ...] = ()
    expect_blast: tuple[int, int] | None = None
    #: A policy a case is authored under (the attention-floor case raises the floor). A policy
    #: passed to :meth:`card` still wins, so `--policy-file` reaches every case.
    policy: Policy | None = None
    doc: str = ""

    def card(self, policy: Policy | None = None) -> Card:
        return compose(self.episode, self.analyses, policy or self.policy or Policy.built_in())


def _code_and_tests() -> Case:
    episode = Episode.from_pairs(
        "ep-code",
        {"src/cart.py": (SRC_BEFORE, SRC_AFTER), "tests/test_cart.py": (TEST_BEFORE, TEST_AFTER),
         "src/util.py": (UTIL_BEFORE, UTIL_AFTER)},
        title="add scaled_total, drop a test",
        before_paths=("src/cart.py", "src/util.py", "src/report.py", "src/api.py",
                      "tests/test_cart.py", "docs/readme.md"),
        after_paths=("src/cart.py", "src/util.py", "src/report.py", "src/api.py",
                     "tests/test_cart.py", "tests/test_scaled.py", "docs/readme.md"))
    return Case(
        "code_and_tests", episode, Analyses(deps=None, graph=None),
        expect_states={SECTION_WHAT: STATE_MEASURED, SECTION_WHERE: STATE_MEASURED,
                       SECTION_DEPENDENCIES: STATE_UNAVAILABLE,
                       SECTION_SURFACE: STATE_MEASURED, SECTION_TESTS: STATE_MEASURED,
                       SECTION_BLAST: STATE_MEASURED, SECTION_ATTENTION: STATE_MEASURED},
        expect_in_card=("12 of 15 changed line(s) survive noise suppression",
                        "path heuristics", "no dependency delta was supplied"),
        forbid_in_card=("2 new direct dependencies",),
        doc="the ordinary case: five sections measured, the dependency section honestly empty")


def _bare_index_record() -> Case:
    """Paths and counts only — the shape a cold spine hands a card."""
    episode = Episode(id="ep-record", files=(
        Change(path="src/cart.py", change_type="modified"),
        Change(path="src/util.py", change_type="modified"),
        Change(path="db/migrate/20261017120000_add_orders_to_invoices.rb",
               change_type="added")), title="record-only episode")
    absent = {name: Absent(name, "the index stores no content for this episode "
                                  "(paths and counts only)",
                           run="fathom show ep-record --patch")
              for name in ("noise", "structure", "test surface", "non-code surface",
                           "classes", "dependencies")}
    return Case("bare_index_record", episode, Analyses(absent=absent),
                expect_states={SECTION_WHAT: STATE_UNAVAILABLE,
                               SECTION_WHERE: STATE_UNAVAILABLE,
                               SECTION_DEPENDENCIES: STATE_UNAVAILABLE,
                               SECTION_SURFACE: STATE_UNAVAILABLE,
                               SECTION_TESTS: STATE_UNAVAILABLE,
                               SECTION_BLAST: STATE_PARTIAL,
                               SECTION_ATTENTION: STATE_UNAVAILABLE},
                expect_in_card=("the index stores no content for this episode",
                                "fathom show ep-record --patch"),
                doc="every content analysis missing: the card stays complete and says so 5 times")


def _no_inventory() -> Case:
    episode = Episode.from_pairs("ep-noinv", {"src/cart.py": (SRC_BEFORE, SRC_AFTER)},
                                 title="one file, no census")
    return Case("no_inventory", episode, Analyses(),
                expect_states={SECTION_WHAT: STATE_MEASURED, SECTION_WHERE: STATE_PARTIAL,
                               SECTION_DEPENDENCIES: STATE_UNAVAILABLE,
                               SECTION_SURFACE: STATE_MEASURED, SECTION_TESTS: STATE_PARTIAL,
                               SECTION_BLAST: STATE_PARTIAL,
                               SECTION_ATTENTION: STATE_MEASURED},
                expect_in_card=("no repository inventory", "this episode could be counted"),
                doc="heuristics can only see the episode, and the card prices that")


def _graph_full() -> Case:
    episode = Episode.from_pairs(
        "ep-graph",
        {"src/cart.py": (SRC_BEFORE, SRC_AFTER), "src/util.py": (UTIL_BEFORE, UTIL_AFTER)},
        title="change a module the graph knows",
        after_paths=("src/cart.py", "src/util.py", "src/report.py", "src/api.py",
                     "tests/test_cart.py"))
    return Case("graph_full", episode, Analyses(graph=code_graph()),
                expect_states={SECTION_WHAT: STATE_MEASURED, SECTION_WHERE: STATE_PARTIAL,
                               SECTION_DEPENDENCIES: STATE_UNAVAILABLE,
                               SECTION_SURFACE: STATE_MEASURED, SECTION_TESTS: STATE_PARTIAL,
                               SECTION_BLAST: STATE_MEASURED,
                               SECTION_ATTENTION: STATE_PARTIAL},
                expect_in_card=("module graph", "blast.confidence.graph"),
                expect_blast=(2, 1),
                doc="the graph answers: direct dependents from edges, one hop through them")


def _graph_partial() -> Case:
    episode = Episode.from_pairs(
        "ep-partial",
        {"src/cart.py": (SRC_BEFORE, SRC_AFTER), "src/other.py": (MANIFEST, HELPER)},
        title="two changed paths, one of them unknown to the graph")
    return Case("graph_partial", episode, Analyses(graph=partial_graph()),
                expect_states={SECTION_WHAT: STATE_MEASURED, SECTION_WHERE: STATE_PARTIAL,
                               SECTION_DEPENDENCIES: STATE_UNAVAILABLE,
                               SECTION_SURFACE: STATE_MEASURED, SECTION_TESTS: STATE_PARTIAL,
                               SECTION_BLAST: STATE_PARTIAL,
                               SECTION_ATTENTION: STATE_MEASURED},
                expect_in_card=("changed paths", "unknown, not zero"),
                doc="a graph that covers half the change set is a partial, not an answer")


def _graph_no_cover() -> Case:
    episode = Episode.from_pairs("ep-nocover", {"src/cart.py": (SRC_BEFORE, SRC_AFTER)},
                                 title="graph with nothing to say",
                                 after_paths=("src/cart.py", "src/util.py"))
    return Case("graph_no_cover", episode, Analyses(graph=unrelated_graph()),
                expect_states={SECTION_WHAT: STATE_MEASURED, SECTION_WHERE: STATE_PARTIAL,
                               SECTION_DEPENDENCIES: STATE_UNAVAILABLE,
                               SECTION_SURFACE: STATE_MEASURED, SECTION_TESTS: STATE_PARTIAL,
                               SECTION_BLAST: STATE_MEASURED,
                               SECTION_ATTENTION: STATE_MEASURED},
                expect_in_card=("no node for any", "path heuristics"),
                doc="a supplied graph that covers nothing must not be believed, only mentioned")


def _deps_available() -> Case:
    episode = Episode.from_pairs("ep-deps", {"src/cart.py": (SRC_BEFORE, SRC_AFTER),
                                             "Cargo.lock": (MANIFEST, HELPER)},
                                 title="a dependency change",
                                 after_paths=("src/cart.py", "Cargo.lock"))
    return Case("deps_available", episode, Analyses(deps=cargo_delta()),
                expect_states={SECTION_WHAT: STATE_MEASURED, SECTION_WHERE: STATE_PARTIAL,
                               SECTION_DEPENDENCIES: STATE_MEASURED,
                               SECTION_SURFACE: STATE_MEASURED, SECTION_TESTS: STATE_PARTIAL,
                               SECTION_BLAST: STATE_MEASURED,
                               SECTION_ATTENTION: STATE_MEASURED},
                expect_in_card=("2 new direct dependencies", "transitive-only moves 12",
                                "fathom analyze --deps"),
                doc="a real dependency headline, with the view that owns it named")


def _deps_half_empty() -> Case:
    episode = Episode.from_pairs("ep-emptydeps", {"src/cart.py": (SRC_BEFORE, SRC_AFTER)},
                                 title="a delta that measured nothing")
    return Case("deps_half_empty", episode, Analyses(deps=empty_delta()),
                expect_states={SECTION_WHAT: STATE_MEASURED, SECTION_WHERE: STATE_PARTIAL,
                               SECTION_DEPENDENCIES: STATE_UNAVAILABLE,
                               SECTION_SURFACE: STATE_MEASURED, SECTION_TESTS: STATE_PARTIAL,
                               SECTION_BLAST: STATE_PARTIAL,
                               SECTION_ATTENTION: STATE_MEASURED},
                expect_in_card=("reports no counts at all",),
                forbid_in_card=("direct dependencies   0 added, 0 removed",),
                doc="an empty delta is unavailable, not a row of zeros")


def _migration_changes() -> dict:
    """A release that changes production as well as code: env, a migration, and the code."""
    return {"db/migrate/20261017120000_add_orders_to_invoices.rb": (None, MIGRATION),
            ".env.example": (ENV_BEFORE, ENV_AFTER),
            "src/settings.py": (SETTINGS_BEFORE, SETTINGS_AFTER)}


def _migration_episode() -> Case:
    episode = Episode.from_pairs("ep-migration", _migration_changes(),
                                 title="drop a column, rename a variable")
    return Case("migration_episode", episode, Analyses(),
                expect_states={SECTION_WHAT: STATE_MEASURED, SECTION_WHERE: STATE_PARTIAL,
                               SECTION_DEPENDENCIES: STATE_UNAVAILABLE,
                               SECTION_SURFACE: STATE_MEASURED, SECTION_TESTS: STATE_PARTIAL,
                               SECTION_BLAST: STATE_PARTIAL,
                               SECTION_ATTENTION: STATE_PARTIAL},
                expect_in_card=("5 action(s), 4 blocking",
                                "cannot be reviewed from the diff alone"),
                doc="the surfaces that break production, and the actions they require")


def _reformat_only() -> Case:
    episode = Episode.from_pairs("ep-reformat", {"src/util.py": (UTIL_BEFORE, UTIL_AFTER)},
                                 title="black happened",
                                 after_paths=("src/util.py",))
    return Case("reformat_only", episode, Analyses(),
                expect_states={SECTION_WHAT: STATE_PARTIAL, SECTION_WHERE: STATE_PARTIAL,
                               SECTION_DEPENDENCIES: STATE_UNAVAILABLE,
                               SECTION_SURFACE: STATE_MEASURED, SECTION_TESTS: STATE_PARTIAL,
                               SECTION_BLAST: STATE_MEASURED,
                               SECTION_ATTENTION: STATE_MEASURED},
                expect_in_card=("noise-only", "collapse"),
                doc="`e2.2` collapses it and the card reports the collapsed view, saying so")


def _empty_episode() -> Case:
    return Case("empty_episode", Episode(id="ep-empty", title="nothing here"), Analyses(),
                expect_states={name: STATE_UNAVAILABLE for name in SECTION_IDS},
                expect_in_card=("changes no paths",),
                forbid_in_card=("0 file(s), 0 semantic", "ratio 0 test lines"),
                doc="an episode with nothing in it must not be given invented numbers")


def _noise_and_code() -> Case:
    episode = Episode.from_pairs(
        "ep-mixed",
        {"src/formatted.rs": (FORMATTED_BEFORE, FORMATTED_AFTER),
         "src/real.rs": (REAL_RUST_BEFORE, REAL_RUST_AFTER)},
        title="a reformat beside a real change",
        after_paths=("src/formatted.rs", "src/real.rs"))
    return Case("noise_and_code", episode, Analyses(),
                expect_states={SECTION_WHAT: STATE_MEASURED, SECTION_WHERE: STATE_PARTIAL,
                               SECTION_DEPENDENCIES: STATE_UNAVAILABLE,
                               SECTION_SURFACE: STATE_MEASURED, SECTION_TESTS: STATE_PARTIAL,
                               SECTION_BLAST: STATE_MEASURED,
                               SECTION_ATTENTION: STATE_MEASURED},
                expect_in_card=("reformat 1",),
                doc="folded and semantic lines differ, and the card prints both")


def _risk_supplied() -> Case:
    episode = Episode.from_pairs(
        "ep-risk",
        {"src/cart.py": (SRC_BEFORE, SRC_AFTER), "tests/test_cart.py": (TEST_BEFORE, TEST_AFTER)},
        title="ranked by the queue")
    return Case("risk_supplied", episode, Analyses(risk=risk()),
                expect_states={SECTION_ATTENTION: STATE_MEASURED,
                               SECTION_BLAST: STATE_PARTIAL},
                expect_in_card=("risk score 82.0", "rank 4 of 7"),
                doc="an `e4.4`-shaped score is relayed, not re-invented")


def _attention_floor() -> Case:
    """A crowded episode with the attention floor raised: the card must count what it hid."""
    episode = Episode.from_pairs("ep-notes", _migration_changes(),
                                 title="five findings, one above the floor")
    return Case("attention_floor", episode, Analyses(),
                policy=Policy.built_in().with_knobs(attention_severity="high"),
                expect_states={SECTION_ATTENTION: STATE_PARTIAL},
                expect_in_card=("card.attention.severity",),
                forbid_in_card=("nothing has been left off the card",),
                doc="findings below the attention floor are counted, not silently dropped")


def _migration_only() -> Case:
    """Migrations and nothing else: non-code decides the episode needs deploy coordination."""
    episode = Episode.from_pairs(
        "ep-migonly", {"db/migrate/20261017120000_add_orders_to_invoices.rb": (None, MIGRATION)},
        title="a migration on its own",
        after_paths=("db/migrate/20261017120000_add_orders_to_invoices.rb",))
    return Case("migration_only", episode, Analyses(),
                expect_states={SECTION_SURFACE: STATE_MEASURED,
                               SECTION_ATTENTION: STATE_PARTIAL},
                expect_in_card=("needs deploy coordination", "forward-only"),
                doc="a claim that only exists because two analyses compose it: `e2.8` knows the "
                    "episode is migration-only, and the card says what that means for the release")


def _wide_heuristic() -> Case:
    """Thirty changed files in one package, and forty neighbours that did not change.

    The neighbours are the point: a reach estimate over a change set that is the whole package
    would be zero, which is the correct answer to a different question.
    """
    pairs = {}
    paths = []
    for index in range(30):
        path = "pkg/wide/module%02d.py" % index
        pairs[path] = ("%d\n" % index, "%d\n%d\n" % (index, index + 1))
        paths.append(path)
    for index in range(40):
        paths.append("pkg/wide/untouched%02d.py" % index)
    episode = Episode.from_pairs("ep-wide", pairs, title="30 files in one package",
                                 before_paths=tuple(paths), after_paths=tuple(paths))
    return Case("wide_heuristic", episode, Analyses(),
                expect_states={SECTION_BLAST: STATE_MEASURED},
                expect_in_card=("blast.max_rows",),
                doc="a wide reach is capped in rows and the remainder is named")


CASES: tuple[Case, ...] = (
    _code_and_tests(), _bare_index_record(), _no_inventory(), _graph_full(), _graph_partial(),
    _graph_no_cover(), _deps_available(), _deps_half_empty(), _migration_episode(),
    _migration_only(), _reformat_only(), _empty_episode(), _noise_and_code(), _risk_supplied(),
    _attention_floor(), _wide_heuristic())

CASE_INDEX: dict[str, Case] = {case.name: case for case in CASES}
NAMES: tuple[str, ...] = tuple(CASE_INDEX)


def fixture(name: str, *, policy: Policy | None = None) -> Card:
    """The blessed card for one case, built with the built-in policy."""
    case = CASE_INDEX[name]
    return case.card(policy or Policy.built_in())


def card(name: str, policy: Policy | None = None) -> Card:
    return fixture(name, policy=policy)


def blast_of(name: str, policy: Policy | None = None):
    """Just the blast answer for a case, for the blast-specific gates."""
    case = CASE_INDEX[name]
    active = policy or Policy.built_in()
    return measure_blast(case.episode, active, graph=case.analyses.graph)
