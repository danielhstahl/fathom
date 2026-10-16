#!/usr/bin/env python3
"""Author the corpus ground-truth files.

This script is a build-time tool, not part of the crate: it writes the hand-authored half of each
`corpus/<scenario>.json` and leaves the `determinism` block empty for `--bless` to fill in. Keeping the
semantics in a script here (rather than hand-editing fifteen files) is what makes a scenario change
reviewable: the diff is the ground truth's source.
"""

import json
import os

CORPUS = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "crates", "fathom-fixtures", "corpus")

SCHEMA = "fathom.fixture-ground-truth/1"


def episode(id_, rule, commits=None, span=None, author=None, kind=None, session=None, turn=None,
            lineage=None, note=None):
    out = {"id": id_, "rule": rule}
    if span is not None:
        out["span"] = list(span)
    if commits is not None:
        out["commits"] = commits
    for key, value in (
        ("lineage", lineage),
        ("author", author),
        ("kind", kind),
        ("session", session),
        ("turn", turn),
        ("note", note),
    ):
        if value is not None:
            out[key] = value
    return out


def finding(kind, summary, at=None, episode_id=None, paths=None, pairs=None, expect="fire",
            detector=None, detail=None):
    # The first string is the one-line summary; `detail` extends it when the case needs explaining. A finding
    # always carries one of the two, and the failure message quotes whichever it is.
    out = {"kind": kind, "detail": detail if detail is not None else summary}
    if detector is not None:
        out["detector"] = detector
    if at is not None:
        out["at"] = at
    if episode_id is not None:
        out["episode"] = episode_id
    if paths is not None:
        out["paths"] = paths
    if pairs is not None:
        out["pairs"] = [list(pair) for pair in pairs]
    if expect != "fire":
        out["expect"] = expect
    return out


def claim(kind, **fields):
    return {"kind": kind, **fields}


def scenario(name, purpose, consumers, episodes, findings, claims, notes=None):
    return {
        "schema": SCHEMA,
        "scenario": name,
        "purpose": purpose,
        "consumers": consumers,
        "determinism": {},
        "episodes": episodes,
        "findings": findings,
        "claims": claims,
        "notes": notes or [],
    }


DOCS = []

# -------------------------------------------------------------------------------------------- rename_storm
DOCS.append(scenario(
    "rename_storm",
    "Four module paths move with unchanged bytes in one commit, a fifth moves with its content rewritten, "
    "and two later turns do a delayed rename and a reformat that is not a rename. A rename detector has to "
    "report all four moves as moves, must not report any of them as a deletion plus an addition, and must "
    "not mistake whitespace for a move.",
    ["e1.3", "e2.2", "e4.3"],
    [
        episode("ep-scaffold", "root", commits=[0], author="sam", kind="human",
                note="a person scaffolds; no session, no turn"),
        episode("ep-publish", "trailer", commits=[1], author="pi-agent", kind="agent", session="rename",
                turn="1", note="the tree the renames will move"),
        episode("ep-storm", "trailer", commits=[2], author="pi-agent", kind="agent", session="rename",
                turn="1", note="second commit of the same turn: one turn, two commits"),
        episode("ep-router", "trailer", commits=[3], author="pi-agent", kind="agent", session="rename",
                turn="2", note="the rename that also rewrote the file"),
        episode("ep-docs", "trailer", commits=[4], author="pi-agent", kind="agent", session="rename",
                turn="3"),
        episode("ep-ci", "author", commits=[5], author="dana", kind="human",
                note="a person commits between two agent turns"),
        episode("ep-late", "trailer", commits=[6, 7], author="pi-agent", kind="agent", session="rename",
                turn="4", note="a reformat and a delayed rename, same turn"),
    ],
    [
        finding("rename", "four paths moved with identical bytes", at=2, episode_id="ep-storm",
                pairs=[["src/core/store.rs", "src/store/mod.rs"], ["src/api/routes.rs", "src/api/handlers.rs"],
                       ["src/queue.rs", "src/scheduler.rs"], ["tests/api.rs", "tests/query.rs"]],
                paths=["src/store/mod.rs", "src/api/handlers.rs", "src/scheduler.rs", "tests/query.rs"]),
        finding("file_removed", "none of the four moves is a deletion", at=2, expect="silent",
                paths=["src/core/store.rs"], detail="a rename reported as a removal double-counts the change"),
        finding("rename", "the router moved and gained the cache struct", at=3, episode_id="ep-router",
                pairs=[["src/api/handlers.rs", "src/http/handlers.rs"]], paths=["src/http/handlers.rs"]),
        finding("rename", "the delayed move, two turns after the rest", at=7, episode_id="ep-late",
                pairs=[["src/scheduler.rs", "src/scheduler/mod.rs"]], paths=["src/scheduler/mod.rs"]),
        finding("rename", "a reformat is not a move", at=6, expect="silent", paths=["src/http/handlers.rs"],
                detail="the file's bytes changed and its path did not"),
        finding("noise", "the reformat-only commit is the noise case", at=6, episode_id="ep-late",
                paths=["src/http/handlers.rs"]),
    ],
    [
        claim("paths_changed", commit=2,
              paths=["src/api/handlers.rs", "src/scheduler.rs", "src/store/mod.rs", "tests/query.rs"]),
        claim("rename", commit=2, pairs=[["src/core/store.rs", "src/store/mod.rs"],
                                         ["src/api/routes.rs", "src/api/handlers.rs"],
                                         ["src/queue.rs", "src/scheduler.rs"],
                                         ["tests/api.rs", "tests/query.rs"]]),
        claim("whitespace_only_change", commit=6, paths=["src/http/handlers.rs"]),
        claim("text_contains", commit=3, path="src/http/handlers.rs", needle="misses"),
        claim("tip_is", **{"ref": "refs/heads/main", "commit": 7}),
    ],
    notes=[
        "the reformat at commit 6 and the rename at commit 7 are the same turn, so they are one episode: an "
        "episode that is half noise and half a move is the case a rename detector and a noise detector "
        "disagree about, and both expectations are stated here",
    ],
))

# -------------------------------------------------------------------- dependency_bump_storm
DOCS.append(scenario(
    "dependency_bump_storm",
    "Every shape a dependency change arrives in, one per turn: a direct bump, the same bump as a "
    "lockfile-only commit, a front-end bump with its transitive pin, a Python pin, a dependency added, a "
    "dependency removed, a bot's own bump, a human revert of it, and a lockfile regenerated afterwards. "
    "Each is a way for a bump detector to be wrong, and two of them are ways for a source-change detector "
    "to be wrong about a commit that touched no source.",
    ["e1.3", "e2.2", "e4.3"],
    [
        episode("ep-scaffold", "root", commits=[0], author="sam", kind="human"),
        episode("ep-regex", "trailer", commits=[1, 2], author="pi-agent", kind="agent", session="deps",
                turn="1", note="the manifest and its lockfile, one turn apart"),
        episode("ep-express", "trailer", commits=[3, 4], author="pi-agent", kind="agent", session="deps",
                turn="2", note="the front end and its lockfile, including the transitive pin"),
        episode("ep-reporter", "author", commits=[5], author="dana", kind="human",
                note="the Python pin, by hand"),
        episode("ep-add-drop", "trailer", commits=[6, 7], author="pi-agent", kind="agent", session="deps",
                turn="3", note="added and removed, same turn"),
        episode("ep-bot", "trailer", commits=[8], author="deps-bot", kind="bot", session="deps", turn="4",
                note="the bot's own turn: agent by evidence, not by declaration"),
        episode("ep-revert", "author", commits=[9], author="sam", kind="human",
                note="a person undoes the bot's bump"),
        episode("ep-lockfile", "trailer", commits=[10], author="pi-agent", kind="agent", session="deps",
                turn="5"),
    ],
    [
        finding("dependency_bump", "regex 1.10.6 → 1.11.0 in the manifest", at=1, episode_id="ep-regex",
                paths=["Cargo.toml"]),
        finding("dependency_bump", "the lockfile-only half of the same bump", at=2, episode_id="ep-regex",
                paths=["Cargo.lock"]),
        finding("source_change", "a lockfile is not source", at=2, expect="silent", paths=["Cargo.lock"],
                detail="counting the lockfile as source makes every bump look like a code change"),
        finding("dependency_bump", "express 4.x → 5.1.0", at=3, episode_id="ep-express",
                paths=["web/package.json"]),
        finding("dependency_bump", "express and its transitive body-parser in the lockfile", at=4,
                episode_id="ep-express", paths=["web/package-lock.json"]),
        finding("source_change", "neither npm file is source", at=4, expect="silent",
                paths=["web/package-lock.json"]),
        finding("dependency_bump", "the reporter's requirements get pinned", at=5, episode_id="ep-reporter",
                paths=["services/reporter/requirements.txt"]),
        finding("dependency_added", "quick-xml appears", at=6, episode_id="ep-add-drop",
                paths=["Cargo.toml"]),
        finding("dependency_removed", "thiserror disappears", at=7, episode_id="ep-add-drop",
                paths=["Cargo.toml"]),
        finding("dependency_bump", "the bot bumps serde", at=8, episode_id="ep-bot", paths=["Cargo.toml"]),
        finding("dependency_revert", "the revert of the bot's bump", at=9, episode_id="ep-revert",
                paths=["Cargo.toml"], detail="a revert of a bump is a bump; an episode that calls it new "
                                             "work has double-counted the dependency"),
        finding("dependency_bump", "the lockfile regenerated after the revert", at=10,
                episode_id="ep-lockfile", paths=["Cargo.lock"]),
    ],
    [
        claim("paths_changed", commit=1, paths=["Cargo.toml"]),
        claim("paths_changed", commit=2, paths=["Cargo.lock"]),
        claim("paths_changed", commit=3, paths=["web/package.json"]),
        claim("text_contains", commit=1, path="Cargo.toml", needle="regex = \"1.11.0\""),
        claim("text_contains", commit=6, path="Cargo.toml", needle="quick-xml"),
        claim("text_absent", commit=7, path="Cargo.toml", needle="thiserror"),
        claim("text_contains", commit=9, path="Cargo.toml", needle="thiserror"),
        claim("tip_is", **{"ref": "refs/heads/main", "commit": 10}),
    ],
    notes=[
        "commit 9 writes the manifest the agent started from at commit 6, which is why thiserror is back: "
        "the revert restores the file, it does not invert one line",
    ],
))

# -------------------------------------------------------------------- cycle_introduced
DOCS.append(scenario(
    "cycle_introduced",
    "A three-layer project (core → store → api) in which two commits each add one import that points the "
    "wrong way and closes a cycle, and a third deletes a file another module still declares. The cycles are "
    "real text: single functions whose doc comments say why they do not belong there. One of the three "
    "cycles only exists because a file is gone, which is the shape a source-only cycle finder misses.",
    ["e1.3", "e2.2", "e4.3"],
    [
        episode("ep-scaffold", "root", commits=[0], author="sam", kind="human"),
        episode("ep-core", "trailer", commits=[1, 2], author="pi-agent", kind="agent", session="layers",
                turn="1"),
        episode("ep-scheduler", "trailer", commits=[3], author="codex-agent", kind="agent",
                session="layers", turn="2", note="a second agent, so the author rule has work to do"),
        episode("ep-cycles", "trailer", commits=[4, 5], author="pi-agent", kind="agent", session="layers",
                turn="3", note="the two back edges, one per commit"),
        episode("ep-delete", "trailer", commits=[6], author="pi-agent", kind="agent", session="layers",
                turn="4"),
    ],
    [
        finding("cycle_introduced", "core starts importing api", at=4, episode_id="ep-cycles",
                paths=["src/core/mod.rs", "src/api/mod.rs"],
                detail="core → api and api → core: the layering stated in the files is now a lie"),
        finding("cycle_introduced", "store starts importing api too", at=5, episode_id="ep-cycles",
                paths=["src/store/mod.rs", "src/api/mod.rs"]),
        finding("cycle_introduced", "api still declares the deleted routes module", at=6,
                episode_id="ep-delete", paths=["src/api/mod.rs"],
                detail="the edge survives as a dangling declaration; a finder that only reads existing "
                       "files will not see it"),
        finding("rename", "the deletion is a deletion", at=6, expect="silent", paths=["src/api/routes.rs"],
                detail="nothing took the deleted file's place"),
    ],
    [
        claim("paths_changed", commit=4, paths=["src/core/mod.rs"]),
        claim("text_absent", commit=3, path="src/core/mod.rs", needle="crate::api"),
        claim("text_contains", commit=4, path="src/core/mod.rs", needle="use crate::api"),
        claim("text_contains", commit=5, path="src/store/mod.rs", needle="use crate::api"),
        claim("paths_changed", commit=6, paths=["src/api/routes.rs"]),
        claim("text_contains", commit=6, path="src/api/mod.rs", needle="mod routes"),
        claim("tip_is", **{"ref": "refs/heads/main", "commit": 6}),
    ],
))

# -------------------------------------------------------------------- test_deletion
DOCS.append(scenario(
    "test_deletion",
    "Five tests become three, three become `#[ignore]`d stubs with their assertions gone, an integration "
    "test file is deleted outright, a person puts all of it back, and an agent adds a test beside the "
    "module it covers. Four shapes, only two of which are deletions, and one of which is the restoration a "
    "deletion detector must not report a second time.",
    ["e1.3", "e2.2", "e4.3"],
    [
        episode("ep-scaffold", "root", commits=[0], author="sam", kind="human"),
        episode("ep-publish", "trailer", commits=[1], author="pi-agent", kind="agent", session="tests",
                turn="1"),
        episode("ep-trim", "trailer", commits=[2], author="pi-agent", kind="agent", session="tests",
                turn="2", note="two tests removed to make the suite green"),
        episode("ep-skip", "trailer", commits=[3], author="pi-agent", kind="agent", session="tests",
                turn="3", note="the remainder skipped: weakened, not deleted"),
        episode("ep-delete", "trailer", commits=[4], author="codex-agent", kind="agent", session="tests",
                turn="4"),
        episode("ep-restore", "author", commits=[5], author="dana", kind="human"),
        episode("ep-add", "trailer", commits=[6], author="pi-agent", kind="agent", session="tests",
                turn="5"),
    ],
    [
        finding("test_deletion", "two core tests removed", at=2, episode_id="ep-trim",
                paths=["tests/core.rs"], detail="downsample_keeps_the_mean and epoch_round_trip are gone"),
        finding("test_weakened", "the rest became #[ignore] stubs", at=3, episode_id="ep-skip",
                paths=["tests/core.rs"],
                detail="the file still has #[test] lines and no assertions left in them"),
        finding("test_deletion", "the api integration tests are deleted", at=4, episode_id="ep-delete",
                paths=["tests/api.rs"]),
        finding("test_deletion", "the restoration is not a deletion", at=5, expect="silent",
                paths=["tests/core.rs", "tests/api.rs"],
                detail="reporting the restore as a deletion is the false positive this file exists to catch"),
        finding("test_added", "a person put the coverage back", at=5, episode_id="ep-restore",
                paths=["tests/core.rs", "tests/api.rs"]),
        finding("test_added", "a test beside the module it covers", at=6, episode_id="ep-add",
                paths=["src/store/tests.rs"]),
    ],
    [
        claim("paths_changed", commit=4, paths=["tests/api.rs"]),
        claim("text_absent", commit=3, path="tests/core.rs", needle="fn downsample_keeps_the_mean"),
        claim("text_contains", commit=3, path="tests/core.rs", needle="#[ignore"),
        claim("text_contains", commit=5, path="tests/core.rs", needle="fn timing_is_monotonic"),
        claim("text_contains", commit=6, path="src/store/tests.rs", needle="#[test]"),
        claim("tip_is", **{"ref": "refs/heads/main", "commit": 6}),
    ],
))

# -------------------------------------------------------------------- reformat_only
DOCS.append(scenario(
    "reformat_only",
    "One turn that reformats five files, reorders the items in a sixth and regenerates the lockfile; a "
    "person's unrelated commit five hours later; and one turn that fixes a real parsing bug while also "
    "reformatting a file. The last one is why a noise detector has to be per-change rather than per-episode: "
    "call that episode noise and a bug fix disappears with it.",
    ["e1.3", "e2.2", "e4.3"],
    [
        episode("ep-scaffold", "root", commits=[0], author="sam", kind="human"),
        episode("ep-publish", "trailer", commits=[1], author="pi-agent", kind="agent", session="publish",
                turn="1"),
        episode("ep-format", "trailer", commits=[2, 3, 4], author="pi-agent", kind="agent",
                session="format", turn="1",
                note="three commits, one turn, nothing but churn"),
        episode("ep-typo", "time-gap", commits=[5], author="sam", kind="human",
                note="five hours after the format turn: the gap, not the author, is what opens this one"),
        episode("ep-fix", "trailer", commits=[6], author="pi-agent", kind="agent", session="format",
                turn="2", note="one whitespace-only file and one real fix, same turn"),
    ],
    [
        finding("noise", "five files reformatted and nothing else", at=2, episode_id="ep-format",
                paths=["src/api/mod.rs", "src/api/routes.rs", "src/core/mod.rs", "src/parse.rs",
                       "src/render.rs"]),
        finding("whitespace_only_change", "every line moved, no token moved", at=2, episode_id="ep-format",
                paths=["src/api/mod.rs", "src/api/routes.rs", "src/core/mod.rs", "src/parse.rs",
                       "src/render.rs"]),
        finding("whitespace_only_change", "a reorder is not a reformat", at=3, expect="silent",
                paths=["src/parse.rs"],
                detail="the items moved, so the bytes differ beyond whitespace; a whitespace check says no "
                       "and a token-multiset check says the behaviour is unchanged"),
        finding("noise", "a regenerated lockfile is churn", at=4, episode_id="ep-format",
                paths=["Cargo.lock"]),
        finding("noise", "the fix episode is not noise", at=6, expect="silent", paths=["src/parse.rs"],
                detail="the episode also contains a real change, and the finding would hide it"),
        finding("bug_fix", "the trailing comma the review caught", at=6, episode_id="ep-fix",
                paths=["src/parse.rs"]),
        finding("whitespace_only_change", "the same commit reformatted main.rs", at=6,
                episode_id="ep-fix", paths=["src/main.rs"]),
    ],
    [
        claim("whitespace_only_change", commit=2,
              paths=["src/api/mod.rs", "src/api/routes.rs", "src/core/mod.rs", "src/parse.rs",
                     "src/render.rs"]),
        claim("whitespace_only_change", commit=6, paths=["src/main.rs"]),
        claim("text_absent", commit=1, path="src/parse.rs", needle="trim_end_matches"),
        claim("text_contains", commit=6, path="src/parse.rs", needle="trim_end_matches"),
        claim("paths_changed", commit=6, paths=["src/main.rs", "src/parse.rs"]),
        claim("tip_is", **{"ref": "refs/heads/main", "commit": 6}),
    ],
    notes=[
        "the gap between commit 4 and commit 5 is five hours and the gap between 5 and 6 is one hour, so "
        "the time-gap rule is exercised against a threshold rather than against an obvious infinity",
    ],
))

# -------------------------------------------------------------------- scope_creep
DOCS.append(scenario(
    "scope_creep",
    "Two turns that arrived at more than the intent they declared. The first fixes one parser and in the "
    "same commit reindents an unrelated renderer, adds two hundred and forty lines of notes, leaves a "
    "four-hundred-line debug log behind, turns a CI job on and edits .envrc. The second re-cleans the "
    "reporter and commits a notebook and a scheduler it was not asked about. A person then deletes the "
    "debris.",
    ["e1.3", "e2.2", "e4.3"],
    [
        episode("ep-scaffold", "root", commits=[0], author="sam", kind="human"),
        episode("ep-publish", "trailer", commits=[1], author="pi-agent", kind="agent", session="creep",
                turn="1"),
        episode("ep-creep-fix", "trailer", commits=[2], author="pi-agent", kind="agent", session="creep",
                turn="2", note="Agent-Intent named the parser; six paths arrived"),
        episode("ep-creep-report", "trailer", commits=[3], author="codex-agent", kind="agent",
                session="creep", turn="3", note="Agent-Intent named the reporter; five paths arrived"),
        episode("ep-clean", "author", commits=[4], author="dana", kind="human"),
    ],
    [
        finding("scope_creep", "the intent was one parser line", at=2, episode_id="ep-creep-fix",
                paths=["src/render.rs", "docs/notes.md", "debug.log", ".envrc",
                       ".github/workflows/ci.yml"],
                detail="Agent-Intent: 'fix the trailing comma in parse'"),
        finding("scratch_committed", "the debug log and the notes are not deliverables", at=2,
                episode_id="ep-creep-fix", paths=["debug.log", "docs/notes.md"]),
        finding("scope_creep", "the intent was the nightly report", at=3, episode_id="ep-creep-report",
                paths=["src/scheduler.rs", "notebooks/exploration.ipynb", "config/scheduler.toml"],
                detail="Agent-Intent: 'update the nightly report'"),
        finding("scope_creep", "a person deleting debris is not scope creep", at=4, expect="silent",
                paths=["debug.log", "notebooks/exploration.ipynb", "docs/notes.md"]),
    ],
    [
        claim("paths_changed", commit=2,
              paths=[".envrc", ".github/workflows/ci.yml", "debug.log", "docs/notes.md", "src/parse.rs",
                     "src/render.rs"]),
        claim("paths_changed", commit=4,
              paths=["debug.log", "docs/notes.md", "notebooks/exploration.ipynb"]),
        claim("file_at_least", commit=2, path="debug.log", bytes=10_000),
        claim("file_at_least", commit=2, path="docs/notes.md", bytes=2_000),
        claim("text_contains", commit=2, path=".envrc", needle="LUMEN_WINDOW"),
    ],
))

# -------------------------------------------------------------------- rebase_rewrite
DOCS.append(scenario(
    "rebase_rewrite",
    "The same work twice. A branch adds a retry budget, a policy and a doc page and is left where it is; "
    "the same three changes are then made again on `main` with different parents and different SHAs, and "
    "the original branch is merged anyway — which brings nothing, because `main` already has the bytes. "
    "Both lineages are reachable, which is the state a repository is really in after a force-push, and the "
    "state a history walk double-counts unless it compares content rather than ids.",
    ["e1.3", "e2.2", "e4.3"],
    [
        episode("ep-scaffold", "root", commits=[0], author="sam", kind="human", lineage="main"),
        episode("ep-orig-budget", "branch", commits=[1, 2], author="pi-agent", kind="agent",
                session="orig", turn="1", lineage="orig",
                note="the branch opens here: the author is the same, the lineage is not"),
        episode("ep-orig-docs", "trailer", commits=[3], author="pi-agent", kind="agent", session="orig",
                turn="2", lineage="orig"),
        episode("ep-arch", "branch", commits=[4], author="sam", kind="human", lineage="main",
                note="back on main, by a different author; the branch rule fires first"),
        episode("ep-rewrite-budget", "trailer", commits=[5, 6], author="pi-agent", kind="agent",
                session="rewrite", turn="3", lineage="main",
                note="the same two commits, different parents"),
        episode("ep-rewrite-docs", "trailer", commits=[7], author="pi-agent", kind="agent",
                session="rewrite", turn="4", lineage="main"),
        episode("ep-merge", "author", commits=[8], author="sam", kind="human", lineage="main",
                note="the merge of a branch whose content is already present"),
    ],
    [
        finding("duplicate_change", "the retry budget already exists on `orig`", at=5,
                episode_id="ep-rewrite-budget", paths=["src/scheduler.rs"],
                detail="same subject, same tree content, different parent and SHA: an id comparison misses "
                       "it, a patch-id comparison finds it"),
        finding("duplicate_change", "the doc page is the same bytes on both lineages", at=7,
                episode_id="ep-rewrite-docs", paths=["docs/retries.md"]),
        finding("rebase_rewrite", "the merge brings nothing", at=8, expect="collapse",
                episode_id="ep-merge", paths=["src/scheduler.rs", "docs/retries.md"],
                detail="the merge commit is empty: main already contains everything the branch has"),
        finding("duplicate_change", "the original commits are the ones to keep", at=1, expect="silent",
                paths=["src/scheduler.rs"], detail="the rewrite is the duplicate, not the original"),
    ],
    [
        claim("ref_exists", **{"ref": "refs/heads/orig"}),
        claim("tip_is", **{"ref": "refs/heads/main", "commit": 8}),
        claim("tip_is", **{"ref": "refs/heads/orig", "commit": 3}),
        claim("paths_changed", commit=8, paths=[]),
        claim("text_contains", commit=6, path="src/scheduler.rs", needle="fn policy"),
        claim("text_contains", commit=3, path="docs/retries.md", needle="Retries"),
        claim("text_contains", commit=7, path="docs/retries.md", needle="Retries"),
    ],
    notes=[
        "every episode names its lineage, because a single walk order cannot tile two live histories at "
        "once: commits 1-3 are on `orig` and interleave with `main` only in creation order",
    ],
))

# -------------------------------------------------------------------- churn_loop
DOCS.append(scenario(
    "churn_loop",
    "One turn, six commits, the same four lines in the same two files argued over each time: the window "
    "size and the spin behaviour, round after round, and then a seventh commit that puts back what the "
    "first round had. The gross change across the turn is twenty-four lines added and twenty-four removed "
    "and the net is nothing, which is the difference between a churn detector that measures edits and one "
    "that measures outcomes.",
    ["e1.3", "e2.2", "e4.3"],
    [
        episode("ep-scaffold", "root", commits=[0], author="sam", kind="human"),
        episode("ep-add", "trailer", commits=[1], author="pi-agent", kind="agent", session="churn",
                turn="1"),
        episode("ep-churn", "trailer", commits=[2, 3, 4, 5, 6, 7], author="pi-agent", kind="agent",
                session="churn", turn="2", note="six rounds of the same argument, one turn"),
        episode("ep-settle", "trailer", commits=[8], author="pi-agent", kind="agent", session="churn",
                turn="3", note="back to the first round's content"),
        episode("ep-note", "author", commits=[9], author="dana", kind="human"),
    ],
    [
        finding("churn_loop", "six edits to the same region in one turn", at=7, episode_id="ep-churn",
                paths=["src/scheduler.rs", "src/queue.rs"],
                detail="each round rewrites the window and the spin; the per-commit diffs are small and the "
                       "turn's total is not"),
        finding("reverted_change", "the turn undoes itself", at=8, episode_id="ep-settle",
                paths=["src/scheduler.rs", "src/queue.rs"],
                detail="the file at commit 8 has the content it had at commit 1"),
        finding("churn_loop", "the first commit is not churn", at=1, expect="silent",
                paths=["src/scheduler.rs"]),
        finding("churn_loop", "the note is not churn", at=9, expect="silent", paths=["docs/drain.md"]),
    ],
    [
        claim("paths_changed", commit=4, paths=["src/queue.rs", "src/scheduler.rs"]),
        claim("text_contains", commit=3, path="src/scheduler.rs", needle="Round 2 of the same argument"),
        claim("text_absent", commit=8, path="src/scheduler.rs", needle="Round 6 of the same argument"),
        claim("text_contains", commit=8, path="src/scheduler.rs", needle="Round 0 of the same argument"),
    ],
))

# -------------------------------------------------------------------- huge_file
DOCS.append(scenario(
    "huge_file",
    "A vendored API description that is 11 MB of generated JSON, a 600 KB minified bundle, a binary logo "
    "blob, one summary fixed by hand in the description, and the description regenerated properly "
    "afterwards. Four things: the big file is over `fathom-ingest`'s default 8 MiB in-memory blob cap, the "
    "bundle and the blob are files no text pipeline may read, and the hand edit and the regeneration are the "
    "pair a generated-file detector has to tell apart. The event log covers the first three commits only.",
    ["e1.3", "e2.2", "e6.1"],
    [
        episode("ep-scaffold", "root", commits=[0], author="sam", kind="human"),
        episode("ep-description", "trailer", commits=[1], author="pi-agent", kind="agent", session="vendor",
                turn="1"),
        episode("ep-bundle", "trailer", commits=[2], author="pi-agent", kind="agent", session="vendor",
                turn="2"),
        episode("ep-checksums", "trailer", commits=[3], author="deps-bot", kind="bot", session="vendor",
                turn="3"),
        episode("ep-hand-edit", "trailer", commits=[4], author="codex-agent", kind="agent",
                session="vendor", turn="4", note="one line changed by hand in a generated file"),
        episode("ep-regenerated", "trailer", commits=[5], author="codex-agent", kind="agent",
                session="vendor", turn="5", note="the generator's own output, identical to commit 1's"),
    ],
    [
        finding("generated_file", "the description declares itself generated", at=1,
                episode_id="ep-description", paths=["openapi/openapi.json"]),
        finding("over_blob_memory_cap", "11 MB against an 8 MiB cap", at=1, episode_id="ep-description",
                paths=["openapi/openapi.json"],
                detail="fathom-ingest's Limits::blob_memory_cap default; the diff must be counted and "
                       "skipped, not read into memory"),
        finding("binary_path", "the logo is not text", at=2, episode_id="ep-bundle",
                paths=["web/assets/logo.bin"]),
        finding("generated_file", "the bundle carries an @generated header", at=2, episode_id="ep-bundle",
                paths=["web/dist/app.min.js"]),
        finding("regenerated_file", "the hand edit is not a regeneration", at=4, expect="silent",
                paths=["openapi/openapi.json"],
                detail="the marker is still there and the bytes are not the generator's: this is the case "
                       "that separates 'generated' from 'regenerated'"),
        finding("regenerated_file", "back to the generator's output", at=5, episode_id="ep-regenerated",
                paths=["openapi/openapi.json"]),
    ],
    [
        claim("file_at_least", commit=1, path="openapi/openapi.json", bytes=9_000_000),
        claim("file_at_least", commit=2, path="web/dist/app.min.js", bytes=500_000),
        claim("binary_at", commit=2, path="web/assets/logo.bin"),
        claim("text_contains", commit=1, path="openapi/openapi.json", needle="x-generated"),
        claim("text_contains", commit=2, path="web/dist/app.min.js", needle="@generated by esbuild"),
        claim("text_contains", commit=4, path="openapi/openapi.json",
              needle="touched by hand in the editor"),
        claim("text_absent", commit=5, path="openapi/openapi.json",
              needle="touched by hand in the editor"),
        # The stream stops where `Events::First(3)` says it does; this is the assertion that it did, and the
        # only event count in the corpus that is a fact a human cares about rather than one bless owns.
        claim("event_count", events=23),
    ],
    notes=[
        "the event log covers the first three commits only (`Events::First(3)`): this is where the corpus "
        "exercises a stream that stops before the history does, rather than in `ten_k_commits`, where a "
        "complete stream would be the largest artifact in CI. ADR-005 §3.6's 4 MiB line ceiling is the "
        "reason a single enormous generated file is a harness question at all",
    ],
))

# -------------------------------------------------------------------- unicode_paths
(cafe_nfc, cafe_nfd, japanese, dashes, cyrillic) = [
    "docs/caf\u00e9.md",
    "docs/cafe\u0301.md",
    "docs/\u65e5\u672c\u8a9e.md",
    "docs/na\u00efve\u2013notes.md",
    "src/api/\u043c\u043e\u0434\u0443\u043b\u044c.rs",
]
DOCS.append(scenario(
    "unicode_paths",
    "Paths that are not ASCII, and a rename from the composed spelling of a name to its decomposed "
    "spelling. The two café pages differ by nothing a normalising filesystem can see, so they are never both "
    "in the tree at once; the Cyrillic module name and the en dash in a note are the two a Windows console "
    "and a shell quoting layer have to survive. The paths are written with escapes in the generator so that "
    "no editor or git filter gets to choose the normalisation form.",
    ["e1.3", "e4.3"],
    [
        episode("ep-scaffold", "root", commits=[0], author="sam", kind="human"),
        episode("ep-translated", "trailer", commits=[1], author="pi-agent", kind="agent", session="i18n",
                turn="1"),
        episode("ep-cyrillic", "trailer", commits=[2], author="pi-agent", kind="agent", session="i18n",
                turn="2"),
        episode("ep-rename", "trailer", commits=[3], author="pi-agent", kind="agent", session="i18n",
                turn="3", note="composed → decomposed, content unchanged"),
        episode("ep-notes", "author", commits=[4], author="dana", kind="human"),
        episode("ep-rewrite", "trailer", commits=[5], author="codex-agent", kind="agent", session="i18n",
                turn="4"),
    ],
    [
        finding("non_ascii_path", "two translated pages arrive", at=1, episode_id="ep-translated",
                paths=[cafe_nfc, japanese]),
        finding("non_ascii_path", "a module named in Cyrillic", at=2, episode_id="ep-cyrillic",
                paths=[cyrillic]),
        finding("rename", "composed to decomposed, one path, same bytes", at=3, episode_id="ep-rename",
                pairs=[[cafe_nfc, cafe_nfd]], paths=[cafe_nfd],
                detail="on a normalising filesystem this commit changes nothing at all, which is the trap"),
        finding("non_ascii_path", "an en dash in a file name", at=4, episode_id="ep-notes",
                paths=[dashes]),
        finding("rename", "a rewrite is not a rename", at=5, expect="silent", paths=[japanese],
                detail="the page's content changed and its name did not"),
    ],
    [
        claim("rename", commit=3, pairs=[[cafe_nfc, cafe_nfd]]),
        claim("paths_changed", commit=3, paths=[cafe_nfd]),
        claim("text_contains", commit=2, path=cyrillic, needle="cyrillic is here"),
        claim("text_contains", commit=5, path=japanese, needle="新版"),
        claim("tip_is", **{"ref": "refs/heads/main", "commit": 5}),
    ],
))

# -------------------------------------------------------------------- many_merges
DOCS.append(scenario(
    "many_merges",
    "Five feature branches, five merges into `main`, one conflict resolved by hand in the merge commit, and "
    "one branch nobody ever merged. Merge commits are where an episode's author is least trustworthy: the "
    "author of a merge is whoever typed `git merge`, and two of these merges are a person landing an agent's "
    "work. The conflict resolution is the opposite case — lines a person really wrote, in a merge commit, "
    "appearing nowhere else — and `feat/never` is the history walk that silently drops a tip.",
    ["e1.3", "e2.2", "e4.3"],
    [
        episode("ep-scaffold", "root", commits=[0], author="sam", kind="human", lineage="main"),
        episode("ep-publish", "trailer", commits=[1], author="pi-agent", kind="agent", session="publish",
                turn="1", lineage="main"),
        episode("ep-feat-a", "branch", commits=[2], author="pi-agent", kind="agent", session="feat-a",
                turn="1", lineage="feat/a"),
        episode("ep-feat-b", "branch", commits=[3], author="codex-agent", kind="agent", session="feat-b",
                turn="1", lineage="feat/b"),
        episode("ep-merge-a", "author", commits=[4], author="sam", kind="human", lineage="main",
                note="a person lands an agent's branch: the merge's author is not the work's author"),
        episode("ep-merge-b", "author", commits=[5], author="sam", kind="human", lineage="main"),
        episode("ep-feat-c", "branch", commits=[6], author="pi-agent", kind="agent", session="feat-c",
                turn="1", lineage="feat/c"),
        episode("ep-feat-d", "branch", commits=[7], author="codex-agent", kind="agent", session="feat-d",
                turn="1", lineage="feat/d", note="the same lines feat/c changed, from the same base"),
        episode("ep-merge-c", "trailer", commits=[8], author="pi-agent", kind="agent", session="feat-c",
                turn="2", lineage="main", note="an agent landing its own branch"),
        episode("ep-merge-d", "author", commits=[9], author="dana", kind="human", lineage="main",
                note="the conflict resolution: these lines exist only here"),
        episode("ep-feat-e", "branch", commits=[10], author="sam", kind="human", lineage="feat/e"),
        episode("ep-merge-e", "author", commits=[11], author="sam", kind="human", lineage="main"),
        episode("ep-feat-never", "branch", commits=[12], author="pi-agent", kind="agent",
                session="feat-never", turn="1", lineage="feat/never",
                note="a tip nothing reaches, and an episode a default-branch walk would never see"),
    ],
    [
        finding("merge_commit", "the retry budget lands", at=4, episode_id="ep-merge-a",
                paths=["src/scheduler.rs"]),
        finding("merge_commit", "the queue bound lands", at=5, episode_id="ep-merge-b",
                paths=["src/queue.rs"]),
        finding("merge_commit", "the wider window lands", at=8, episode_id="ep-merge-c",
                paths=["src/scheduler.rs"]),
        finding("merge_commit", "the spin change lands, with a conflict", at=9, episode_id="ep-merge-d",
                paths=["src/scheduler.rs"]),
        finding("conflict_resolution", "the resolution is hand-written and appears nowhere else", at=9,
                episode_id="ep-merge-d", paths=["src/scheduler.rs"]),
        finding("merge_commit", "the doc page lands", at=11, episode_id="ep-merge-e",
                paths=["docs/merging.md"]),
        finding("merge_commit", "a branch commit is not a merge", at=2, expect="silent",
                paths=["src/scheduler.rs"]),
        finding("branch_never_merged", "feat/never is a tip with no path to main", at=12,
                episode_id="ep-feat-never", paths=["src/export.rs"]),
    ],
    [
        claim("ref_exists", **{"ref": "refs/heads/feat/never"}),
        claim("ref_exists", **{"ref": "refs/heads/feat/a"}),
        claim("tip_is", **{"ref": "refs/heads/main", "commit": 11}),
        claim("tip_is", **{"ref": "refs/heads/feat/never", "commit": 12}),
        claim("text_contains", commit=9, path="src/scheduler.rs", needle="Round 4 of the same argument"),
        claim("paths_changed", commit=11, paths=["docs/merging.md"]),
    ],
    notes=[
        "merges are commits too: the merge of feat/a reports the merged file as its changed path, which is "
        "how a consumer that attributes work by commit author ends up crediting the wrong person",
    ],
))

# -------------------------------------------------------------------- ten_k_commits
groups = [
    episode(f"ep-import-g{group}", "trailer", span=[2 + 100 * group, 100], author="deps-bot", kind="bot",
            session="import", turn=f"g{group}", lineage="main",
            note="one hundred commits, one turn: the scale case, not the hand-written case")
    for group in range(100)
]
DOCS.append(scenario(
    "ten_k_commits",
    "Ten thousand commits in one hundred bot turns, each turn a batch of one hundred edits to the same "
    "fixed-size import table, plus a human scaffold, one agent commit and one human check at the end. It "
    "exists for the performance budget and for the two cases only scale produces: a turn that spans a "
    "hundred commits, and a segmentation that has to hold ten thousand episodes without falling over. It "
    "writes no event log at all.",
    ["e1.3", "e4.3"],
    [
        episode("ep-scaffold", "root", commits=[0], author="sam", kind="human", lineage="main"),
        episode("ep-publish", "trailer", commits=[1], author="pi-agent", kind="agent", session="import",
                turn="0", lineage="main"),
        *groups,
        episode("ep-check", "author", commits=[10002], author="sam", kind="human", lineage="main"),
    ],
    [
        finding("turn_spanning_many_commits", "one bot turn, one hundred commits", at=101,
                episode_id="ep-import-g0", paths=["src/imported.rs"],
                detail="the widest hand-written scenario reaches three"),
        finding("episode_count_over_scale_threshold", "ten thousand two commits, one hundred three episodes",
                detail="the budget is the whole corpus's, so this scenario is the majority of it"),
        finding("test_deletion", "nothing is deleted here", expect="silent",
                detail="a scale scenario that also changed tests would confound the timing with the finding"),
        finding("rename_storm", "nothing is renamed here", expect="silent"),
    ],
    [
        claim("tip_is", **{"ref": "refs/heads/main", "commit": 10002}),
        claim("text_contains", commit=10001, path="src/imported.rs", needle="slot_15()"),
        claim("event_count", events=0),
    ],
    notes=[
        "each commit rewrites the same fixed-size file rather than appending to a growing one, and that is "
        "not laziness: a fixture whose tree grows with its history costs quadratic time and memory to "
        "build, and a corpus that takes two gigabytes to generate is a corpus nobody runs",
        "no event stream: see `docs/concepts/fixtures.md` for why the truncation rule is exercised in "
        "`huge_file` instead",
    ],
))

# -------------------------------------------------------------------- submodule
DOCS.append(scenario(
    "submodule",
    "A vendored upstream that is a real repository with its own history, mounted as a gitlink and then "
    "bumped to a released commit. The companion is built first, into a sibling directory, because a gitlink "
    "names a commit and a fixture pointing at one that does not exist is a broken repository rather than a "
    "fixture. `git fsck --strict` inside the result is what proves the gitlink was written the way git wants "
    "it, including the git dir living inside the parent at `.git/modules/<name>`.",
    ["e1.3", "e4.3"],
    [
        episode("ep-scaffold", "root", commits=[0], author="sam", kind="human"),
        episode("ep-publish", "trailer", commits=[1], author="pi-agent", kind="agent", session="sub",
                turn="1"),
        episode("ep-mount", "trailer", commits=[2], author="pi-agent", kind="agent", session="sub",
                turn="2", note="the gitlink appears, with .gitmodules"),
        episode("ep-bump", "trailer", commits=[3], author="pi-agent", kind="agent", session="sub",
                turn="3", note="the pointer moves to the released upstream commit"),
        episode("ep-note", "author", commits=[4], author="dana", kind="human"),
    ],
    [
        finding("submodule_change", "the upstream is mounted", at=2, episode_id="ep-mount",
                paths=["vendor/upstream", ".gitmodules"]),
        finding("submodule_change", "the pointer moves to the release", at=3, episode_id="ep-bump",
                paths=["vendor/upstream"]),
        finding("dependency_bump", "a gitlink is not a package bump", at=3, expect="silent",
                paths=["vendor/upstream"],
                detail="counting it as one would price a vendored checkout as a library upgrade"),
    ],
    [
        claim("gitlink", commit=2, path="vendor/upstream", companion="upstream", at=1),
        claim("gitlink", commit=3, path="vendor/upstream", companion="upstream", at=3),
        claim("paths_changed", commit=2, paths=[".gitmodules", "vendor/upstream"]),
        claim("text_contains", commit=2, path=".gitmodules", needle="vendor/upstream"),
        claim("ref_exists", **{"ref": "refs/heads/main"}),
    ],
))

# -------------------------------------------------------------------- shallow_clone
DOCS.append(scenario(
    "shallow_clone",
    "A depth-three clone of a ten-commit upstream. The three commits that came over were rewritten: the "
    "window's first commit has no parent at all, because there is nothing below the cut to point at, and "
    "`.git/shallow` names the tip. A history walker that walks to the root instead of to the shallow cut "
    "will ask for an object that is not in this repository and fail with a message nobody in the product can "
    "act on, so the corpus contains the shape and names the cut.",
    ["e1.3", "e4.3"],
    [
        episode("ep-window-0", "root", commits=[0], author="sam", kind="human",
                note="the shallow cut: a commit with no parent because the parent was never fetched"),
        episode("ep-window-1", "trailer", commits=[1], author="pi-agent", kind="agent", session="upstream",
                turn="r7"),
        episode("ep-window-2", "trailer", commits=[2], author="pi-agent", kind="agent", session="upstream",
                turn="r8"),
    ],
    [
        finding("history_truncated", "the walk stops at the shallow cut", at=0, episode_id="ep-window-0",
                detail="the boundary commit's parent line names an object that was never fetched; the "
                       "ingest must stop there and say so rather than fail"),
        finding("missing_object", "nothing is missing that should be there", expect="silent",
                detail="a shallow clone is not a corrupt one; fsck says so, and the fixture's own test says "
                       "so"),
    ],
    [
        claim("is_shallow", tips=[0]),
        claim("ref_exists", **{"ref": "refs/heads/main"}),
        claim("ref_exists", **{"ref": "refs/remotes/origin/main"}),
        claim("tip_is", **{"ref": "refs/heads/main", "commit": 2}),
    ],
    notes=[
        "the three visible commits are upstream rounds 6 to 8 with their ids intact, because a rewritten "
        "commit would no longer be a clone of anything; the boundary commit keeps a `parent` line pointing "
        "at round 5, which this repository does not contain, and `.git/shallow` names the boundary — that is "
        "exactly what `git clone --depth 3` leaves behind, and `git fsck --strict` agrees",
    ],
))

# -------------------------------------------------------------------- clean_baseline
DOCS.append(scenario(
    "clean_baseline",
    "The control: nothing is wrong here. A human scaffold, one agent session of three turns that publishes "
    "a module tree and adds a module with its test, one dependency bump done properly with its lockfile, "
    "and a person's commit that touches two unrelated surfaces. Every detector in the product is claimed to "
    "be quiet on a tidy repository, and a corpus made only of pathological cases would let a detector that "
    "fires on everything score perfectly.",
    ["e1.3", "e2.2", "e4.3"],
    [
        episode("ep-scaffold", "root", commits=[0], author="sam", kind="human"),
        episode("ep-publish", "trailer", commits=[1], author="pi-agent", kind="agent", session="clean",
                turn="1"),
        episode("ep-export", "trailer", commits=[2], author="pi-agent", kind="agent", session="clean",
                turn="2", note="a module and its test, the way it is supposed to be done"),
        episode("ep-bump", "trailer", commits=[3], author="pi-agent", kind="agent", session="clean",
                turn="3", note="the manifest and the lockfile in one commit"),
        episode("ep-docs", "author", commits=[4], author="dana", kind="human",
                note="two unrelated surfaces in one commit: the only thing here a detector may call unusual"),
    ],
    [
        finding("dependency_bump", "regex, with its lockfile", at=3, episode_id="ep-bump",
                paths=["Cargo.toml", "Cargo.lock"]),
        finding("test_added", "the export module arrives with its test", at=2, episode_id="ep-export",
                paths=["tests/export.rs"]),
        finding("scope_creep", "two files is not creep", at=4, expect="silent",
                paths=["docs/export.md", "config/scheduler.toml"],
                detail="this is the pair a creep detector must decline to name"),
        finding("noise", "a documentation commit is not noise", at=4, expect="silent",
                paths=["docs/export.md"]),
        finding("test_deletion", "nothing was deleted", expect="silent"),
        finding("churn_loop", "nothing was argued over", expect="silent"),
        finding("rename_storm", "nothing moved", expect="silent"),
        finding("cycle_introduced", "the layering holds", expect="silent"),
    ],
    [
        claim("paths_changed", commit=3, paths=["Cargo.lock", "Cargo.toml"]),
        claim("text_absent", commit=4, path="src/export.rs", needle="crate::api"),
        claim("text_contains", commit=3, path="Cargo.lock", needle="regex"),
        claim("ref_exists", **{"ref": "refs/heads/main"}),
        claim("tip_is", **{"ref": "refs/heads/main", "commit": 4}),
    ],
))


def main():
    target = os.path.normpath(CORPUS)
    for doc in DOCS:
        path = os.path.join(target, doc["scenario"] + ".json")
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(json.dumps(doc, indent=2, ensure_ascii=False) + "\n")
        print("wrote", path)


if __name__ == "__main__":
    main()
