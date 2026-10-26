#!/usr/bin/env python3
"""Author the turn corpus spike `s2` measures against — facts mechanical, truth human.

Spike `s2` (`reco-q6r`) needs hand-annotated episode boundaries. This tool writes the **mechanical half**
of that annotation and refuses, loudly and by construction, to write the half that belongs to a person.

    corpus/turns/fathom-self.json      facts from this repository's real git history   truth: hand-written
    corpus/turns/fixture-<name>.json   facts from `fathom-fixtures --scenario <name>`  truth: `e0.10`'s
                                       commit facts + the hand-authored `corpus/<name>.json` episodes
                                       and findings, carried across verbatim

The two halves are separated by *who may write them*, exactly like a fixture scenario's `determinism` and
`semantics` blocks (docs/concepts/fixtures.md §4): `--self` and `--scenarios` rewrite facts and preserve
`truth` byte-for-byte; `--check` re-derives everything and exits 1 on any difference; nothing here can
invent a `truth` block, and `--new-authored` writes a skeleton whose `truth` is empty and then fails
validation until a human fills it in.

Why the fixture stratum is *extracted* rather than read live: the spike must score the same bytes the
documented corpus says, and `ten_k_commits` costs 21 s and `huge_file` 14 s to build. Each extracted file
records the scenario's blessed `determinism.digest`, so a fixture that moved under the corpus is a stale
snapshot and `--check` says so instead of quietly re-annotating.

Run:
    python3 tools/author_turn_corpus.py --self
    python3 tools/author_turn_corpus.py --scenarios
    python3 tools/author_turn_corpus.py --check
"""

from __future__ import annotations

import argparse
import datetime
import glob
import json
import os
import pathlib
import re
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
TURNS = ROOT / "crates" / "fathom-fixtures" / "corpus" / "turns"
CORPUS = ROOT / "crates" / "fathom-fixtures" / "corpus"

SCHEMA = "fathom.turn-ground-truth/1"

#: Scenarios excluded from the turn corpus, with the reason printed in corpus/turns/README.md §1. The
#: exclusion is a named list rather than a size heuristic so that adding a scenario cannot change the
#: corpus underneath a published measurement by accident.
EXCLUDED_SCENARIOS = {
    "ten_k_commits": "10 003 commits in 103 100-commit bot batches; it would outvote every hand-written "
    "boundary in the corpus 96 to 1",
    "huge_file": "one 4 MiB blob: a size-stress case whose four agent turns are the shape "
    "clean_baseline already carries, for 14 s of generator time",
}

#: The vocabularies a `truth` block may use. Kept here so the writer, the reader and the test share one
#: list rather than three.
TURN_KINDS = ("agent", "human", "bot", "unknown")
JOIN_EVIDENCE = ("T1", "T2", "T3", "T4", "fixture", "authored")


# ---------------------------------------------------------------------------------------------
# git facts (the `real` stratum)
# ---------------------------------------------------------------------------------------------
def _git(repository: pathlib.Path, *args: str) -> str:
    return subprocess.run(
        ["git", *args],
        cwd=repository,
        capture_output=True,
        text=True,
        check=True,
        env={
            **os.environ,
            "GIT_CONFIG_GLOBAL": "/dev/null",
            "GIT_CONFIG_SYSTEM": "/dev/null",
        },
    ).stdout


def git_log(repository: pathlib.Path, head: str) -> list[dict]:
    """Commit facts for `head`'s ancestry, oldest first, read the way `e1.1` reads them.

    Two passes rather than one: `--numstat` prints its lines *after* the format string, so a message body
    (which may contain anything, including tab-separated lines) and the stat lines cannot be told apart in
    a single stream. Pass one is the fields plus the stats — a record starts at `COMMIT\x1f` and every
    other line is a numstat line — pass two is the bodies, delimited by a byte no commit message carries.
    """
    stats = _git(
        repository,
        "log",
        "--reverse",
        "--numstat",
        "--format=COMMIT\x1f%H\x1f%an\x1f%ae\x1f%at\x1f%P\x1f%s",
        head,
    )
    bodies = {}
    for chunk in _git(
        repository, "log", "--reverse", "--format=%x1e%H\x1f%b", head
    ).split("\x1e"):
        if chunk.strip():
            oid, _, body = chunk.partition("\x1f")
            bodies[oid.strip()] = body
    commits: list[dict] = []
    current = None
    for line in stats.splitlines():
        if line.startswith("COMMIT\x1f"):
            oid, name, email, when, parents, subject = line[len("COMMIT\x1f") :].split(
                "\x1f", 5
            )
            current = {
                "oid": oid,
                "parents": parents.split(),
                "author_name": name,
                "author_email": email,
                "author_millis": int(when) * 1000,
                "at": datetime.datetime.fromtimestamp(
                    int(when), datetime.timezone.utc
                ).strftime("%Y-%m-%dT%H:%M:%SZ"),
                "subject": subject,
                "trailers": parse_trailers(bodies.get(oid, "")),
                "paths": [],
                "n_files": 0,
                "n_add": 0,
                "n_del": 0,
                "n_binary": 0,
                "session_id": None,
                "turn_id": None,
                "merge": len(parents.split()) > 1,
            }
            commits.append(current)
            continue
        parts = line.split("\t")
        if len(parts) != 3 or current is None:
            continue
        adds, dels, path = parts
        current["paths"].append(normalise_rename(path))
        current["n_files"] += 1
        if adds == "-" or dels == "-":
            current["n_binary"] += 1
        else:
            current["n_add"] += int(adds)
            current["n_del"] += int(dels)
    return commits


def normalise_rename(path: str) -> str:
    """`git`'s numstat spells a rename `a/{old => new}/b`; the path a reviewer reads is the destination."""
    if " => " in path:
        path = re.sub(r"\{[^{}]* => ([^{}]*)\}", r"\1", path).replace("//", "/")
        path = path.replace("/}", "}").replace("{/", "{")
        return path.strip("/") or path
    return path


TRAILER = re.compile(r"^([A-Za-z][A-Za-z0-9_-]*):\s*(.+?)\s*$")


def parse_trailers(body: str) -> dict:
    """Git-trailers: the last paragraph of the message, `Key: value` lines only, keys lower-cased."""
    paragraphs = [p for p in body.strip().split("\n\n") if p.strip()]
    if not paragraphs:
        return {}
    trailers = {}
    for line in paragraphs[-1].splitlines():
        match = TRAILER.match(line.strip())
        if match:
            trailers[match.group(1).lower()] = match.group(2)
    return trailers


def bead_evidence() -> dict:
    """The real harness's own turn record: which bead was claimed, when, and when it released.

    This is the `hook` evidence ADR-003 §9.1 describes, sitting in the tracker instead of on the commit:
    the loop claims a bead, works it, commits, closes it. Nothing on the commit says any of it, which is
    why `created_source` is `index` for every real episode in this repository and why the `hook` rule
    cannot fire today.
    """
    try:
        out = subprocess.run(
            ["bd", "list", "--all", "--json"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=120,
        ).stdout
        issues = json.loads(out)
    except Exception as error:  # noqa: BLE001 - a spike degrades
        print(f"  note: `bd list` unavailable ({error}); no harness evidence recorded")
        return {}
    return {
        i["id"]: {
            "status": i.get("status"),
            "started_at": i.get("started_at"),
            "closed_at": i.get("closed_at"),
            "assignee": i.get("assignee"),
            "closed_by_session": i.get("closed_by_session"),
        }
        for i in issues
    }


BEAD_IN_SUBJECT = re.compile(r"(reco-[a-z0-9]{3})")


def extract_self(head: str | None = None) -> dict:
    """Build `fathom-self.json`'s facts half, preserving whatever `truth` half is already in the file."""
    commits = git_log(ROOT, head or "HEAD")
    board = bead_evidence()
    for commit in commits:
        bead = BEAD_IN_SUBJECT.search(commit["subject"])
        turn = bead.group(1) if bead else None
        record = board.get(turn or "", {})
        commit["harness"] = {
            "turn_id": f"beads:{turn}" if turn else None,
            "session_id": "looprs" if record.get("closed_by_session") else None,
            "bead": turn,
            "claim_utc": record.get("started_at"),
            "release_utc": record.get("closed_at"),
            "closed_by_session": record.get("closed_by_session"),
        }
        # What the *commit* carries is the only thing the shipped `hook`/`trailer` rules can see.
        commit["turn_id"] = None
        commit["session_id"] = None
    path = TURNS / "fathom-self.json"
    existing = json.loads(path.read_text()) if path.exists() else None
    truth = (
        existing["truth"]
        if existing
        else empty_truth(
            "fill this in by hand: one turn per object, every commit claimed exactly once"
        )
    )
    if not existing:
        print(
            "  wrote a skeleton; the truth half is empty and validation will refuse it until a "
            "person fills it in",
            file=sys.stderr,
        )
    doc = {
        "schema": SCHEMA,
        "history": "fathom-self",
        "provenance": "real",
        "authoring": {
            "annotated_by": "danielhstahl, from the beads board and `git log`; protocol in "
            "crates/fathom-fixtures/corpus/turns/README.md §2",
            "protocol": "corpus/turns/README.md §2 (T1 harness turn, T2 named unit, T3 continuation of "
            "the same act, T4 otherwise separate)",
            "annotated_on": "2026-10-17",
            "derived_from": "Fathom's own history: every commit this repository has, produced by the "
            "looprs + beads + pi harness on this machine. The harness's own claim/close "
            "windows are carried in `lineage[].harness` and are a second, independent "
            "derivation of the same turn boundaries.",
        },
        "source": {
            "kind": "git",
            "repository": "fathom",
            "head": commits[-1]["oid"] if commits else None,
            "head_short": commits[-1]["oid"][:12] if commits else None,
            "commit_count": len(commits),
            "extracted_by": "tools/author_turn_corpus.py --self",
        },
        "clock": "real",
        "lineage": commits,
        "truth": truth,
    }
    validate(doc)
    return doc


# ---------------------------------------------------------------------------------------------
# fixture facts (the `fixture` stratum)
# ---------------------------------------------------------------------------------------------
def fixture_scenarios() -> list[str]:
    names = []
    for path in sorted(CORPUS.glob("*.json")):
        name = json.loads(path.read_text())["scenario"]
        if name not in EXCLUDED_SCENARIOS:
            names.append(name)
    return names


def generate_scenario(scenario: str) -> dict:
    """Build the scenario with the crate's own generator and read the manifest it wrote."""
    with tempfile.TemporaryDirectory(prefix=f"s2-{scenario}-") as destination:
        subprocess.run(
            [
                "cargo",
                "run",
                "-q",
                "--offline",
                "-p",
                "fathom-fixtures",
                "--",
                "--scenario",
                scenario,
                destination,
            ],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=True,
            timeout=600,
        )
        return json.loads((pathlib.Path(destination) / "fixture.json").read_text())


def iso_to_millis(stamp: str) -> int:
    return int(
        datetime.datetime.strptime(stamp, "%Y-%m-%dT%H:%M:%S.%fZ")
        .replace(tzinfo=datetime.timezone.utc)
        .timestamp()
        * 1000
    )


def extract_scenario(scenario: str) -> dict:
    """Carry a scenario's hand-authored episodes and findings into turn-corpus shape, verbatim."""
    truth_file = json.loads((CORPUS / f"{scenario}.json").read_text())
    manifest = generate_scenario(scenario)
    commits = manifest["commits"]
    lineage = []
    for commit in commits:
        trailers = {
            str(k).lower(): v for k, v in (commit.get("trailers") or {}).items()
        }
        # Exactly what `crates/fathom-analyze/src/episodes.rs:79` does today: the turn and session a commit
        # can be *read* as carrying, taken from the trailer, with no session-qualifying — which is the thing
        # ADR-003 §9.1 says a `turn_id` already is. Kept as-is so the spike measures the shipped behaviour
        # and not the spike author's opinion of it.
        session_id = trailers.get("agent-session") or trailers.get("session")
        turn_id = trailers.get("agent-turn") or trailers.get("turn")
        harness_turn = (
            "%s:%s" % (commit["session"], commit["turn"])
            if commit.get("session") and commit.get("turn")
            else None
        )
        lineage.append(
            {
                "oid": commit["sha"],
                "parents": commit["parents"],
                "author_name": commit["author"],
                "author_email": "",
                "author_millis": iso_to_millis(commit["timestamp"]),
                "at": commit["timestamp"],
                "subject": commit["subject"],
                "trailers": trailers,
                "paths": [change["path"] for change in commit["changes"]],
                "n_files": commit["n_files"],
                "n_add": commit["n_add"],
                "n_del": commit["n_del"],
                "n_binary": commit["n_binary"],
                "session_id": session_id,
                "turn_id": turn_id,
                "merge": commit.get("merge", False),
                "harness": {
                    "turn_id": harness_turn,
                    "session_id": commit.get("session"),
                },
            }
        )
    by_index = {index: commit["sha"] for index, commit in enumerate(commits)}
    turns = []
    for episode in truth_file["episodes"]:
        indices = episode["commits"] or list(
            range(episode["span"][0], episode["span"][0] + episode["span"][1])
        )
        turns.append(
            {
                "id": episode["id"],
                "commits": [by_index[i][:12] for i in indices],
                "author": episode["author"],
                "kind": episode["kind"] or "unknown",
                "turn": episode["turn"],
                "session": episode["session"],
                "expected_rule": episode["rule"],
                "joined_by": "fixture",
                "note": episode["note"],
            }
        )
    pathologies = []
    for finding in truth_file["findings"]:
        if finding["expect"] != "fire" or finding["at"] is None:
            continue
        pathologies.append(
            {
                "kind": finding["kind"],
                "at": by_index[finding["at"]][:12],
                "episode": finding["episode"],
                "paths": finding["paths"],
                "why": finding["detail"],
                "source": f"corpus/{scenario}.json findings (hand-authored by e0.10)",
            }
        )
    return {
        "schema": SCHEMA,
        "history": f"fixture-{scenario}",
        "provenance": "fixture",
        "authoring": {
            "annotated_by": "e0.10 (`reco-whw`), hand-authored in tools/author_corpus.py",
            "protocol": "crates/fathom-fixtures/corpus/<scenario>.json `episodes` — the semantics half "
            "of a scenario file, which `--bless` may not touch (docs/concepts/fixtures.md §4)",
            "annotated_on": "2026-10-16",
            "derived_from": f"the {scenario} scenario: a deterministic repository the fixture generator "
            "writes object-by-object, with the episode truth a person wrote for it",
        },
        "source": {
            "kind": "fixture",
            "scenario": scenario,
            "blessed_digest": truth_file["determinism"]["digest"],
            "commit_count": len(commits),
            "extracted_by": "tools/author_turn_corpus.py --scenarios",
        },
        "clock": "synthetic: every commit sits `seq/1000` seconds after the last, so a gap number here "
        "measures the scenario author's clock, not anybody's",
        "lineage": lineage,
        "truth": {
            "turns": turns,
            "pathologies": pathologies,
            "notes": [truth_file.get("purpose") or ""],
        },
    }


# ---------------------------------------------------------------------------------------------
# validation: the truth half has to be coherent before a measurement can mean anything
# ---------------------------------------------------------------------------------------------
def empty_truth(note: str) -> dict:
    return {"turns": [], "pathologies": [], "notes": [note]}


def resolve(prefix: str, oids: list[str]) -> list[str]:
    return [oid for oid in oids if oid.startswith(prefix)]


def validate(doc: dict) -> list[str]:
    """Every structural promise of the format; returns the problems rather than dying on the first."""
    problems: list[str] = []
    if doc.get("schema") != SCHEMA:
        problems.append(f"schema is {doc.get('schema')!r}, expected {SCHEMA!r}")
    lineage = doc.get("lineage") or []
    truth = doc.get("truth") or {}
    turns = truth.get("turns") or []
    oids = [commit["oid"] for commit in lineage]
    if not lineage:
        problems.append("no commits")
    if not turns:
        problems.append(
            "truth.turns is empty: a skeleton was never annotated by a person"
        )
    claimed: list[str] = []
    for turn in turns:
        matches = []
        for prefix in turn["commits"]:
            found = resolve(prefix, oids)
            if len(found) != 1:
                problems.append(
                    f"turn {turn['id']}: commit prefix {prefix!r} matches {len(found)} "
                    "commits, expected exactly 1"
                )
            matches.extend(found)
        indices = sorted(oids.index(oid) for oid in matches if oid in oids)
        if indices and indices != list(range(indices[0], indices[-1] + 1)):
            problems.append(
                f"turn {turn['id']}: commits are not contiguous on the lineage "
                f"({indices}); an episode is a contiguous run (ADR-003 E1)"
            )
        if turn.get("kind") not in TURN_KINDS:
            problems.append(
                f"turn {turn['id']}: kind {turn.get('kind')!r} not in {TURN_KINDS}"
            )
        if turn.get("joined_by") == "T3" and not (turn.get("note") or "").strip():
            problems.append(
                f"turn {turn['id']}: joined_by T3 is the judgement call and must carry a "
                "note saying what it judged"
            )
        if turn.get("joined_by") not in JOIN_EVIDENCE:
            problems.append(
                f"turn {turn['id']}: joined_by {turn.get('joined_by')!r} not in "
                f"{JOIN_EVIDENCE}"
            )
        claimed.extend(matches)
    for oid in oids:
        count = claimed.count(oid)
        if count != 1:
            problems.append(
                f"commit {oid[:12]} is claimed by {count} turns; every commit belongs to "
                "exactly one (ADR-003 E3)"
            )
    ids = [turn["id"] for turn in turns]
    if len(ids) != len(set(ids)):
        problems.append("turn ids are not unique")
    by_id = {turn["id"]: turn for turn in turns}
    for pathology in truth.get("pathologies") or []:
        at = resolve(pathology["at"], oids)
        if len(at) != 1:
            problems.append(
                f"pathology {pathology.get('kind')}: `at` {pathology['at']!r} is ambiguous"
            )
            continue
        if pathology["episode"] not in by_id:
            problems.append(
                f"pathology {pathology.get('kind')}: episode {pathology['episode']!r} is "
                "not a turn"
            )
        elif at[0] not in by_id[pathology["episode"]]["commits"] and not any(
            at[0].startswith(p) for p in by_id[pathology["episode"]]["commits"]
        ):
            problems.append(
                f"pathology {pathology.get('kind')}: its `at` commit is not a member of the "
                "episode it names — a pathology is annotated at a commit *and* an episode, and "
                "the two must agree"
            )
        for other in pathology.get("requires") or []:
            if not resolve(other, oids):
                problems.append(
                    f"pathology {pathology.get('kind')}: requires {other!r} matches nothing"
                )
    authoring = doc.get("authoring") or {}
    if not (authoring.get("annotated_by") or "").strip():
        problems.append(
            "authoring.annotated_by is empty: a corpus nobody signed is a corpus nobody trusts"
        )
    if doc.get("provenance") not in ("real", "fixture", "authored"):
        problems.append(f"provenance {doc.get('provenance')!r} unknown")
    return problems


# ---------------------------------------------------------------------------------------------
# writing
# ---------------------------------------------------------------------------------------------
def dump(path: pathlib.Path, doc: dict) -> str:
    text = json.dumps(doc, indent=1, ensure_ascii=False) + "\n"
    old = path.read_text() if path.exists() else None
    if old != text:
        path.write_text(text)
        return "wrote"
    return "unchanged"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--self",
        action="store_true",
        dest="self_history",
        help="refresh fathom-self.json's facts half from git",
    )
    parser.add_argument(
        "--head", default=None, help="freeze the real stratum at this head"
    )
    parser.add_argument(
        "--scenarios",
        action="store_true",
        help="re-extract the fixture stratum (runs the fixture generator)",
    )
    parser.add_argument("--scenario", default=None, help="one scenario only")
    parser.add_argument(
        "--check",
        action="store_true",
        help="re-derive everything and exit 1 on any drift or incoherence",
    )
    parser.add_argument(
        "--validate-only",
        action="store_true",
        help="validate the committed files without regenerating anything",
    )
    args = parser.parse_args()

    problems: list[str] = []
    written = 0
    if args.validate_only:
        for path in sorted(TURNS.glob("*.json")):
            doc = json.loads(path.read_text())
            for problem in validate(doc):
                problems.append(f"{path.name}: {problem}")
        if not problems:
            files = sorted(TURNS.glob("*.json"))
            turns = sum(len(json.loads(p.read_text())["truth"]["turns"]) for p in files)
            commits = sum(len(json.loads(p.read_text())["lineage"]) for p in files)
            print(
                f"turn corpus OK: {len(files)} histories, {commits} commits, {turns} annotated turns"
            )
        for problem in problems:
            print("turn corpus:", problem, file=sys.stderr)
        return 1 if problems else 0

    if args.self_history:
        doc = extract_self(args.head)
        print(
            f"  {dump(TURNS / 'fathom-self.json', doc)} fathom-self.json "
            f"({len(doc['lineage'])} commits, {len(doc['truth']['turns'])} turns)"
        )
        written += 1
    if args.scenarios or args.scenario:
        for scenario in [args.scenario] if args.scenario else fixture_scenarios():
            doc = extract_scenario(scenario)
            for problem in validate(doc):
                problems.append(f"fixture-{scenario}: {problem}")
            print(
                f"  {dump(TURNS / f'fixture-{scenario}.json', doc)} fixture-{scenario}.json "
                f"({len(doc['lineage'])} commits, {len(doc['truth']['turns'])} turns)"
            )
            written += 1
    if args.check:
        # Re-derive and diff: the committed bytes must be what these commands produce right now.
        current = json.loads((TURNS / "fathom-self.json").read_text())
        fresh = extract_self(args.head)
        if current != fresh:
            problems.append(
                "fathom-self.json is not what `--self` produces: the real history moved, or "
                "the extractor changed. Refresh, review, commit together."
            )
        for path in sorted(TURNS.glob("authored-*.json")):
            for problem in validate(json.loads(path.read_text())):
                problems.append(f"{path.name}: {problem}")
        for path in sorted(TURNS.glob("fixture-*.json")):
            name = path.name[len("fixture-") : -len(".json")]
            if name in EXCLUDED_SCENARIOS:
                problems.append(
                    f"{path.name} is an excluded scenario; delete it or the exclusion"
                )
                continue
            fresh = extract_scenario(name)
            if json.loads(path.read_text()) != fresh:
                problems.append(
                    f"{path.name} is not what `--scenarios` produces: the fixture moved "
                    "under the corpus (re-extract deliberately, never silently)"
                )
    if not written:
        parser.print_help()
        return 2
    for problem in problems:
        print("turn corpus:", problem, file=sys.stderr)
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
