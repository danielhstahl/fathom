"""The labelled corpus `e2.4` is graded on, and the rules that keep the labels honest.

Two sources of truth, and the difference between them is deliberate:

1. **`s4`'s sixteen authored histories** (:mod:`recommender.rename.specs` plus `truth/*.json`). They
   were written for the spike and they are the only fixtures in this repository with
   `split / merge / copy / decoy / evil-merge / squash` cases graded by *representation class*.
   Reusing them instead of authoring a parallel set is the point of having run the spike: the same
   hand-authored rows the recommendation came from now grade the thing the recommendation produced.
2. **`e0.10`'s blessed `rename_storm` ground truth**
   (`crates/fathom-fixtures/corpus/rename_storm.json`), which `e2.4`'s acceptance criterion names by
   name. Its expectations are *read out of that file* rather than retyped here, so the bar cannot
   quietly drift away from the corpus the rest of the project grades against; the repository the
   expectations describe is authored here in Python, because the contracts CI job runs no Rust
   toolchain. A coherence gate then checks the authored repository against the blessed file's own
   claims, so an authored fixture that drifted from the ground truth it claims to reproduce fails
   rather than silently scoring nothing.

The rule both halves obey, from `fathom-fixtures`: **expectations are hand-authored or blessed, never
computed by the detector being graded.** Nothing in this module imports `pair.py`. Evidence tiers are
`strong / moderate / weak / none`, where `none` means "the move happened and the two blobs do not
support it" — a pair above the ceiling, reported as a ceiling rather than counted as a miss or,
worse, quietly dropped from the denominator.
"""

from __future__ import annotations

import json
import time
from pathlib import Path

from . import content as C
from . import repo as R
from . import specs
from .score import EVIDENCE_TIERS, TruthWindow, load_truth

TRUTH_DIR = Path(__file__).parent / "truth"

#: The blessed ground truth this ticket's acceptance criterion names.
FIXTURE_CORPUS = Path(__file__).resolve().parents[3] / \
    "crates" / "fathom-fixtures" / "corpus" / "rename_storm.json"

STORM = "rename_storm"


# -- the fixture e2.4's acceptance criterion names by name ---------------------------------------

def reformat(text: str) -> str:
    """The same statements, wearing different whitespace: a formatter ran and nothing else did.

    This is a *silent* fixture: it exists to prove the detector stays quiet. A reformat is the shape
    that manufactures pairings in a widened candidate set (`s4`: `G32`), so the fixture is graded on
    what it fails to emit.
    """
    out = []
    for line in text.splitlines():
        if line.startswith("    ") and line.strip():
            out.append("        " + line.strip())          # a wrapped continuation
        elif line.strip():
            out.append("   " + line.strip())              # re-indented under a reformatted def
        else:
            out.append("")
    return "\n".join(out) + "\n"


def rename_storm() -> tuple[R.Script, str]:
    """`e0.10`'s `rename_storm` scenario, authored in Python so the contracts job can build it.

    Commit-for-commit the same history the blessed ground truth describes: four byte-identical moves
    in one turn, a move that also rewrote the file, a turn that touched nothing movable, a reformat
    that is not a move, and a delayed move two turns later. The commit *indices* are what line the
    history up with the blessed file's `at:` fields, so they are the contract; the file bodies are
    ours and only need to be the same shape (identical bytes for a clean move, ~40 % replaced for the
    rewritten one, whitespace-only for the reformat).
    """
    s = R.Script(STORM)
    tree = {"src/lib.rs": "from . import api\nfrom . import core\n",
            "src/core/mod.rs": "from .store import Store\n",
            "src/core/store.rs": C.body("src/core/store.rs", 6),
            "src/api/routes.rs": C.body("src/api/routes.rs", 8),
            "src/queue.rs": C.body("src/queue.rs", 7),
            "tests/api.rs": C.body("tests/api.rs", 5)}
    s.write(".gitignore", "output/\n")
    s.commit("c0", message="scaffold")
    for path, text in tree.items():
        s.write(path, text)
    s.commit("c1", message="publish the module tree", author="pi-agent")

    s.move("src/core/store.rs", "src/store/mod.rs")
    s.move("src/api/routes.rs", "src/api/handlers.rs")
    s.move("src/queue.rs", "src/scheduler.rs")
    s.move("tests/api.rs", "tests/query.rs")
    s.commit("c2", message="rename the modules to what they do", author="pi-agent")

    rewritten = C.light_edit(C.body("src/api/routes.rs", 8), key="router", fraction=0.4)
    s.write("src/http/handlers.rs", rewritten)
    s.remove("src/api/handlers.rs")
    s.commit("c3", message="rename the api router and fold the cache in", author="pi-agent")

    s.write("README.md", "# fixture\n\nThe router lives in `http::handlers` now.\n")
    s.write("src/lib.rs", "from . import http\nfrom . import core\n")
    s.commit("c4", message="point the docs and the crate root at the new names", author="pi-agent")

    s.write(".github/workflows/ci.yml", "name: ci\non: [push]\njobs: {}\n")
    s.commit("c5", message="refresh the CI image", author="sam")

    s.write("src/http/handlers.rs", reformat(rewritten))
    s.commit("c6", message="reformat the handlers module", author="pi-agent")

    s.move("src/scheduler.rs", "src/scheduler/mod.rs")
    s.commit("c7", message="name the scheduler module after its job", author="pi-agent")
    return s, (
        "Four byte-identical moves, then a move that also rewrote the file, then a documentation "
        "turn, a CI turn, a reformat of a file nobody moved, and a delayed move two turns on. The "
        "three quiet windows are most of the value of this fixture: a detector that fires on a "
        "reformat is not a rename detector.")


def storm_truth() -> tuple[list[TruthWindow], dict]:
    """Truth windows read out of the blessed fixture file, mapped onto the authored history.

    The mapping is `at: <commit index>` → the window that *ends* at that commit, which is what a
    reviewer of that turn would open. `expect: fire` rows become true pairs (kind from the shape the
    blessed detail describes, evidence graded by what the two blobs hold, which for this fixture is
    hand-graded below and not read off any detector), and `expect: silent` rows become windows whose
    pair list is empty — so anything emitted there is graded as a wrong pairing.
    """
    doc = json.loads(FIXTURE_CORPUS.read_text())
    if doc.get("scenario") != STORM:
        raise ValueError("%s is not the rename_storm ground truth" % FIXTURE_CORPUS)
    # commit index -> (window id, base label, head label)
    windows = {index: ("c%d" % index, "c%d" % (index - 1), "c%d" % index)
               for index in range(1, 8)}
    # Hand-graded evidence: identical bytes are strong, a 40 % rewrite is moderate, and the delayed
    # move is strong again. Nothing here consults a detector; the grading is of the blobs.
    evidence = {("src/api/handlers.rs", "src/http/handlers.rs"): "moderate"}
    by_id: dict[str, TruthWindow] = {}
    for row in doc["findings"]:
        if row["kind"] != "rename":
            continue
        window_id, base, head = windows[row["at"]]
        window = by_id.setdefault(window_id, TruthWindow(
            scenario=STORM, id=window_id, base=base, head=head, granularity="episode",
            policy="any", review=True, pairs=[], arrivals=[],
            note="blessed expectations from %s at commit %d: %s"
                 % (FIXTURE_CORPUS.name, row["at"], row["detail"])))
        if row["expect"] == "fire":
            for src, dst in row["pairs"]:
                window.pairs.append({
                    "src": src, "dst": dst,
                    "kind": "rename",
                    "truth": "true",
                    "evidence": evidence.get((src, dst), "strong")})
        elif row["expect"] == "silent" and row["pairs"]:
            raise ValueError("silent rename row with pairs listed at commit %d" % row["at"])
    # The reformat window is the trap and it is only in the file as a `silent` row; make sure the
    # window exists even when a blessed file lists nothing for it.
    for index, (window_id, base, head) in windows.items():
        by_id.setdefault(window_id, TruthWindow(
            scenario=STORM, id=window_id, base=base, head=head, granularity="episode",
            policy="any", review=True, pairs=[], arrivals=[],
            note="no rename expected in this window"))
    # One episode window: the reformat and the delayed move are the same turn in the blessed file,
    # which is the case `s4` found invisible in both commit-pair windows and trivial here (`G23`).
    by_id["ep-late"] = TruthWindow(
        scenario=STORM, id="ep-late", base="c5", head="c7", granularity="episode", policy="any",
        review=True,
        pairs=[{"src": "src/scheduler.rs", "dst": "src/scheduler/mod.rs", "kind": "rename",
                "truth": "true", "evidence": "strong"}],
        arrivals=[],
        note="the reformat and the delayed move are one turn: a half-noise, half-move episode")
    stats = {"source": str(FIXTURE_CORPUS.relative_to(FIXTURE_CORPUS.parents[3])),
             "blessed_rows": len([r for r in doc["findings"] if r["kind"] == "rename"]),
             "windows": len(by_id)}
    return list(by_id.values()), stats


# -- the corpus as a whole -----------------------------------------------------------------------

def fixtures() -> dict[str, tuple]:
    """`{scenario: (script, note)}` — the spike's set plus the fixture `e2.4` names."""
    out = {name: make for name, make in specs.SPECS.items()}
    out[STORM] = rename_storm
    return out


def truth_windows() -> list[TruthWindow]:
    """Every hand-authored window: the spike's truth files plus the blessed storm expectations."""
    windows = load_truth(TRUTH_DIR)
    storm, _ = storm_truth()
    return windows + storm


def build(work: Path, only: list[str] | None = None) -> dict[str, dict]:
    """Write every fixture to disk with real `git`, and time it (the build is part of the cost)."""
    built: dict[str, dict] = {}
    for name, make in fixtures().items():
        if only and name not in only:
            continue
        script, note = make()
        started = time.perf_counter()
        labels = R.build(work / name, script)
        built[name] = {"path": work / name, "labels": labels, "note": note,
                       "build_s": round(time.perf_counter() - started, 3)}
    return built


def resolve(built: dict[str, dict], window: TruthWindow) -> tuple[str, str] | None:
    scenario = built.get(window.scenario)
    if scenario is None:
        return None
    try:
        return scenario["labels"][window.base], scenario["labels"][window.head]
    except KeyError:
        return None


def check_coherence(built: dict[str, dict], windows: list[TruthWindow]) -> dict:
    """Refuse a label that the built repository does not support.

    Same rule the spike enforces and for the same reason — most of the bugs this catches are errors
    in the *expectations*, and an expectation naming a path that is not in the tree is a fixture that
    stopped being measured while still printing a score.
    """
    problems: list[str] = []
    checked = 0
    for window in windows:
        pair = resolve(built, window)
        if pair is None:
            problems.append("%s/%s: labels %s/%s do not resolve"
                            % (window.scenario, window.id, window.base, window.head))
            continue
        base_sha, head_sha = pair
        base_tree = R.ls_tree(built[window.scenario]["path"], base_sha)
        head_tree = R.ls_tree(built[window.scenario]["path"], head_sha)
        for row in window.pairs:
            src, dst = row["src"], row["dst"]
            if row.get("evidence", "strong") not in EVIDENCE_TIERS:
                problems.append("%s/%s: unknown evidence tier %r"
                                % (window.scenario, window.id, row.get("evidence")))
            if src == dst:
                problems.append("%s/%s: %s named on both sides" % (window.scenario, window.id, src))
            if src not in base_tree and row["truth"] == "true" \
                    and row.get("evidence", "strong") != "none":
                problems.append("%s/%s: source %s is not in %s"
                                % (window.scenario, window.id, src, window.base))
            if dst not in head_tree:
                problems.append("%s/%s: destination %s is not in %s"
                                % (window.scenario, window.id, dst, window.head))
            checked += 2
        for path in window.arrivals:
            if path not in head_tree or path in base_tree:
                problems.append("%s/%s: arrival %s is not a clean arrival"
                                % (window.scenario, window.id, path))
            checked += 2
    return {"windows": len(windows), "paths_checked": checked, "problems": problems,
            "ok": not problems}


def check_storm_claims(built: dict[str, dict]) -> dict:
    """The authored storm must reproduce the blessed file's own claims about the built tree.

    Not a restatement of the same numbers: this reads `claims[]` from the blessed ground truth —
    `paths_changed` at the storm commit — and checks the repository built here against it. If the
    authored history drifts, the gate goes red instead of the fixture quietly scoring a different
    history. `text_contains` claims name needles inside the blessed corpus's own file **bodies**, and
    those bodies are Rust-authored there and Python-authored here: they are counted and reported as
    not applicable rather than dropped, so a reader sees which claims were checked and which were not.
    """
    doc = json.loads(FIXTURE_CORPUS.read_text())
    scenario = built.get(STORM)
    if scenario is None:
        return {"ok": False, "claims_checked": 0, "claims_not_applicable": 0,
                "problems": ["%s was not built" % STORM]}
    path = scenario["path"]
    problems: list[str] = []
    checked = 0
    not_applicable = 0
    for claim in doc.get("claims", []):
        if claim["kind"] == "paths_changed":
            labels = scenario["labels"]
            base = labels.get("c%d" % (claim["commit"] - 1))
            head = labels.get("c%d" % claim["commit"])
            _removed, added = R.changed(path, base, head)
            # The blessed claim lists the paths a *rename-aware* spine reports as changed, which for a
            # pure move is the new name and not the old one. If the authored history ever adds a real
            # deletion beside the move, this comparison is where that drift shows up.
            if set(added) != set(claim["paths"]):
                problems.append("paths_changed at commit %d: authored arrivals %s, blessed %s"
                                % (claim["commit"], sorted(added), sorted(claim["paths"])))
            checked += 1
        else:
            not_applicable += 1
    return {"ok": not problems, "claims_checked": checked,
            "claims_not_applicable": not_applicable, "problems": problems}
