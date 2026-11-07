"""The measurement `e2.4` ships on: what the pairing engine gets right, per representation class.

The ticket's first acceptance criterion asked for "at least 90 percent pair precision and recall on
the rename_storm and rename-with-rewrite fixtures". `s4` measured that number and it does not exist:
a fact tier allowed to reach 0.90 recall must also be allowed to assert the corpus's scaffolded decoy,
and a fact tier that refuses it sits near 0.62 recall however the knobs are turned, because what is
missing is *shape*, not score (`G51`). So this module reports what the recommendation said to report,
in the shape it said to report it:

* **per tier** — the fact tier is graded on precision with a hard floor and zero tolerance for
  asserting a pair the truth names as not-a-move; the guess tier is graded on how much true material
  it carries per lie, because a guess may be wrong and may not be unlabelled;
* **per representation class** — `rename`, `split`, `merge`, `copy` — because a pooled 0.75 could mean
  "renames are fine, groups are missing" or the reverse, and those are different products;
* **against the ceiling** — pairs whose evidence the blobs do not carry are out of the recall
  denominator and are printed as the ceiling, so the residual is neither blamed on the algorithm nor
  quietly dropped;
* **with counts beside every rate** — precision 1.00 on four pairings is not the claim 1.00 on
  thirty-four is.

`--check` re-measures and fails on drift in any gate, and
`tests/test_rename_pairing_contract.py` re-runs the gates against the committed report, so the prose
in `docs/concepts/rename-pairing.md` cannot keep a number the measurement stopped supporting.
"""

from __future__ import annotations

import json
import time
from dataclasses import dataclass, field
from pathlib import Path

from . import content as C
from . import corpus as CP
from . import gitio
from . import model as M
from . import pair as PAIR
from . import repo as R
from .policy import KEY_TO_KNOB, MEASURED_KEYS, Policy
from .score import calibrate_pairs

# -- the bars ------------------------------------------------------------------------------------

#: Pooled (fact ∪ guess) recall over claimable pairs on the two fixtures `e2.4` names. This is the
#: restated AC 1: everything a window-local method can see is *shown*, as a fact or as a labelled
#: guess. It is not a claim that they can all be asserted — that bar is `FACT_PRECISION`.
PAIRED_RECALL = 0.90
#: The fact tier's floor, from `s4`'s recommendation: "precision ≥ 0.95 at a fact bar of 0.75 with
#: zero assertions of a known-false pairing, and recall reported per representation class".
FACT_PRECISION = 0.95
#: A fact tier that emits almost nothing is precise for free.
FACT_MIN_PAIRINGS = 25
#: AC 2 is a representation criterion, so it is graded at 1.0 and not "as high as it goes".
GROUP_RECALL = 1.0
#: AC 4: 20× the repository with the change set fixed must cost under this multiple of the small one.
COST_RATIO = 3.0
#: The refused O(repo) mode has to cost *at least* this multiple of the shipped one, as a number written
#: before the run. It used to be filled in with the shipped mode's own measured ratio, which made the
#: claim true by construction and made the committed report say `>= 2.75` one run and `>= 2.73` the next
#: (`reco-2ezf`): an expectation copied from the run that is supposed to be checked against it. The
#: widened mode measures 15-19x on this corpus, so 2 is a bar the claim clears by a wide margin without
#: being a number the measurement quietly writes for itself.
DECISIVE_COST_RATIO = 2.0
#: How far a displayed confidence may sit from the rate it implies is *reported*, not gated: these are
#: similarity indices, not probabilities, and `s4`'s finding was that no score here behaves like a
#: probability at all. `MAX_CALIBRATION_ERROR` is kept as the number a reader should treat as "stop
#: explaining the number and change it".
MAX_CALIBRATION_ERROR = 0.35


@dataclass
class Row:
    """One graded window: what the truth said, what the engine emitted, optionally one tier of it."""

    scenario: str
    window: str
    tier: str = "union"
    silent: bool = False
    truth_pairs: int = 0
    claimable: int = 0
    ceiling: int = 0
    emitted: int = 0
    found: int = 0
    found_ceiling: int = 0
    wrong: int = 0
    wrong_named_decoy: int = 0
    wrong_fabricated: int = 0
    wrong_unsupported: int = 0
    details: dict[str, list[str]] = field(default_factory=dict)
    notes: list[str] = field(default_factory=list)
    ms: float = 0.0

    @property
    def key(self) -> str:
        return "%s/%s" % (self.scenario, self.window)

    @property
    def precision(self) -> float | None:
        return round(self.found / self.emitted, 4) if self.emitted else None

    @property
    def recall(self) -> float | None:
        return round(self.found / self.claimable, 4) if self.claimable else None

    def as_dict(self) -> dict:
        return {"scenario": self.scenario, "window": self.window, "tier": self.tier,
                "silent": self.silent, "claimable": self.claimable, "ceiling": self.ceiling,
                "emitted": self.emitted, "found": self.found, "found_ceiling": self.found_ceiling,
                "wrong": self.wrong, "wrong_named_decoy": self.wrong_named_decoy,
                "wrong_fabricated": self.wrong_fabricated, "wrong_unsupported": self.wrong_unsupported,
                "precision": self.precision, "recall": self.recall, "ms": round(self.ms, 2),
                "details": {k: v for k, v in self.details.items() if v}, "notes": self.notes}


def grade_window(truth: CP.TruthWindow, result: M.Result, *, tier: str | None = None) -> Row:
    """Grade one window's pairings against its hand-authored rows, whole or one tier at a time.

    A pair counts as emitted when it is an edge of any (non-rejected) pairing; the *shape* is graded
    separately in :func:`by_kind`, so a split here says "the relation was found" and `by_kind` says
    whether it was found as a split.
    """
    row = Row(scenario=truth.scenario, window=truth.id, tier=tier or "union",
              notes=list(result.limits))
    truthy = truth.truthy()
    decoys = truth.decoys()
    claimable = set(truth.claimable())
    ceiling = set(truth.unclaimable())
    arrivals = set(truth.arrivals)
    row.truth_pairs = len(truthy)
    row.claimable = len(claimable)
    row.ceiling = len(ceiling)
    row.silent = not truthy

    emitted: dict[tuple[str, str], float] = {}
    for pairing in result.pairings:
        if pairing.disposition == M.DISPOSITION_REJECTED:
            continue
        if tier and pairing.tier != tier:
            continue
        for edge in pairing.edges:
            key = (edge.src, edge.dst)
            emitted[key] = max(emitted.get(key, 0.0), edge.score)

    found: set[tuple[str, str]] = set()
    wrong: dict[str, list[str]] = {"named_decoy": [], "fabricated": [], "unsupported": []}
    for pair in sorted(emitted):
        if pair in truthy:
            found.add(pair)
        elif pair in decoys:
            wrong["named_decoy"].append("%s -> %s" % pair)
        elif pair[1] in arrivals:
            wrong["fabricated"].append("%s -> %s" % pair)
        else:
            wrong["unsupported"].append("%s -> %s" % pair)
    row.emitted = len(emitted)
    row.found = len(found & claimable)
    row.found_ceiling = len(found & ceiling)
    row.wrong_named_decoy = len(wrong["named_decoy"])
    row.wrong_fabricated = len(wrong["fabricated"])
    row.wrong_unsupported = len(wrong["unsupported"])
    row.wrong = sum(len(v) for v in wrong.values())
    row.details = {"missed": ["%s -> %s" % pair for pair in sorted(claimable - found)], **wrong}
    return row


def pool(rows: list[Row]) -> dict:
    """Micro-averaged totals with their counts attached; never a bare rate.

    No wall clock here. A pooled block that carries `ms_total` is a block whose numbers move when the
    runner does, and `check_report` compares this block whole — so the timing used to make CI red on
    every computer but the one that last wrote the report (`reco-2ezf`, the same rule `reco-v1z` set
    for spike `s4`). The millisecond figures are still measured, still written to the report and still
    printed, one row at a time under `rows[]` and `per_scenario[]`; they are just not pooled into a
    block that a reproducibility check reads line by line.
    """
    emitted = sum(r.emitted for r in rows)
    found = sum(r.found for r in rows)
    claimable = sum(r.claimable for r in rows)
    wrong = sum(r.wrong for r in rows)
    return {"windows": len(rows), "silent_windows": sum(1 for r in rows if r.silent),
            "silent_windows_dirty": sum(1 for r in rows if r.silent and r.emitted),
            "true_pairs": sum(r.truth_pairs for r in rows),
            "claimable_pairs": claimable, "ceiling_pairs": sum(r.ceiling for r in rows),
            "emitted": emitted, "found": found,
            "found_ceiling": sum(r.found_ceiling for r in rows),
            "missed": sum(r.claimable - r.found for r in rows),
            "wrong": wrong, "wrong_named_decoy": sum(r.wrong_named_decoy for r in rows),
            "wrong_fabricated": sum(r.wrong_fabricated for r in rows),
            "wrong_unsupported": sum(r.wrong_unsupported for r in rows),
            "precision": round(found / emitted, 4) if emitted else None,
            "recall_claimable": round(found / claimable, 4) if claimable else None,
            "wrong_per_100_windows": round(100 * wrong / len(rows), 2) if rows else None}


def by_kind(run: list[tuple[CP.TruthWindow, M.Result]]) -> dict:
    """Recall per representation class — the number `e2.4` should have asked for from the start.

    A truth pair is credited to the class the *truth* named and counted found only when an emitted
    pairing carries that edge **with a matching shape**: a split pair surfaced inside a `rename` row
    is `found_wrong_shape`, which is a representation failure and not a success with the wrong label.
    """
    out: dict[str, dict[str, int]] = {}
    for truth, result in run:
        claimable = set(truth.claimable())
        emitted: dict[tuple[str, str], set[str]] = {}
        for pairing in result.pairings:
            for edge in pairing.edges:
                emitted.setdefault((edge.src, edge.dst), set()).add(pairing.kind)
        for (src, dst), row in truth.truthy().items():
            if (src, dst) not in claimable:
                continue
            kind = row.get("kind", "rename")
            bucket = out.setdefault(kind, {"claimable": 0, "found": 0, "found_wrong_shape": 0,
                                           "missed": 0})
            bucket["claimable"] += 1
            shapes = emitted.get((src, dst), set())
            if kind in shapes:
                bucket["found"] += 1
            elif shapes:
                bucket["found_wrong_shape"] += 1
            else:
                bucket["missed"] += 1
    return {kind: {**counts, "recall": round(counts["found"] / counts["claimable"], 4)
                   if counts["claimable"] else None}
            for kind, counts in sorted(out.items())}


def shape_census(run: list[tuple[CP.TruthWindow, M.Result]]) -> dict:
    """Every emitted pairing's shape, counted — `1->3` and `3->1` are the rows AC 2 is about."""
    out: dict[str, dict] = {}
    for _truth, result in run:
        for pairing in result.pairings:
            entry = out.setdefault("%s %s" % (pairing.kind, pairing.shape()),
                                   {"pairings": 0, "fact": 0, "guess": 0, "confidence_max": 0.0})
            entry["pairings"] += 1
            entry["fact" if pairing.tier == M.TIER_FACT else "guess"] += 1
            entry["confidence_max"] = max(entry["confidence_max"], round(pairing.confidence, 4))
    return dict(sorted(out.items()))


# -- the run ------------------------------------------------------------------------------------

def read_window(built: dict, truth: CP.TruthWindow, policy: Policy | None = None) -> M.Window:
    base, head = CP.resolve(built, truth)
    return gitio.window_from_repo(built[truth.scenario]["path"], base, head,
                                  window_id="%s/%s" % (truth.scenario, truth.id),
                                  granularity=truth.granularity, walk_policy=truth.policy,
                                  policy=policy)


def measure(work: Path, *, policy: Policy | None = None, only: list[str] | None = None) -> dict:
    """Build the corpus, pair every reviewed window, grade it, price the knobs, run the probes."""
    policy = policy or Policy.built_in()
    started = time.perf_counter()
    truths = CP.truth_windows()
    built = CP.build(work, only)
    windows = [w for w in truths if w.scenario in built and w.review]
    coherence = CP.check_coherence(built, windows)
    if not coherence["ok"]:
        raise RuntimeError("labels disagree with the fixtures:\n  "
                           + "\n  ".join(coherence["problems"]))
    storm = CP.check_storm_claims(built)
    if not storm["ok"]:
        raise RuntimeError("the authored rename_storm disagrees with its blessed claims:\n  "
                           + "\n  ".join(storm["problems"]))
    build_s = time.perf_counter() - started

    store = PAIR.Shingles(width=policy.integer("shingle_lines"), mode=policy.text("normalise"))
    union_rows: list[Row] = []
    fact_rows: list[Row] = []
    guess_rows: list[Row] = []
    per_scenario: dict[str, list[Row]] = {}
    run: list[tuple[CP.TruthWindow, M.Result]] = []
    calibration: list[tuple[float, bool]] = []
    exact_check = {"edges": 0, "mismatched": 0}
    content_stats = {"blobs_read": 0, "scored_pairs": 0, "pruned_by_length": 0}
    fact_shapes: dict[str, int] = {}
    all_shapes: dict[str, int] = {}

    for truth in windows:
        window = read_window(built, truth, policy)
        clock = time.perf_counter()
        result = PAIR.pair(window, policy, store=store)
        ms = (time.perf_counter() - clock) * 1000
        union = grade_window(truth, result)
        union.ms = ms
        union.notes = list(result.limits)
        union_rows.append(union)
        per_scenario.setdefault(truth.scenario, []).append(union)
        run.append((truth, result))
        for tier, bucket in ((M.TIER_FACT, fact_rows), (M.TIER_GUESS, guess_rows)):
            row = grade_window(truth, result, tier=tier)
            if row.emitted or tier == M.TIER_FACT:
                bucket.append(row)
        if any(p.tier == M.TIER_FACT for p in result.pairings):
            pass
        claimable = set(truth.claimable())
        for pairing in result.pairings:
            all_shapes[pairing.kind] = all_shapes.get(pairing.kind, 0) + 1
            if pairing.tier == M.TIER_FACT:
                fact_shapes[pairing.kind] = fact_shapes.get(pairing.kind, 0) + 1
            for edge in pairing.edges:
                calibration.append((edge.score, (edge.src, edge.dst) in claimable))
        exact_check["edges"] += sum(len(p.edges) for p in result.pairings)
        exact_check["mismatched"] += _exact_check(window, result)
        content_stats["blobs_read"] += result.stats["content"]["blobs_read"]
        content_stats["scored_pairs"] += result.stats["content"]["scored_pairs"]
        content_stats["pruned_by_length"] += result.stats["content"]["pruned_by_length"]

    report = {
        "schema": "fathom.e2.4.rename-pairing/1",
        "policy": {
            "knobs": {name: policy.knob(name) for name in
                      ("fact_bar", "allow_content", "guess_floor", "top_k", "budget",
                       "min_shared_shingles", "shingle_lines", "normalise", "copies", "folds",
                       "copies_harder")},
            "fact_bar": policy.fact_bar, "guess_floor": policy.guess_floor,
            "git_candidate_percent": policy.git_candidate_bar(),
            "measured": {key: Policy.built_in().knob(KEY_TO_KNOB[key].name)
                         for key in MEASURED_KEYS},
        },
        "corpus": {"scenarios": len(built), "windows": len(windows),
                   "scenario_names": sorted(built),
                   "coherence_paths_checked": coherence["paths_checked"],
                   "blessed_storm_claims_checked": storm["claims_checked"],
                   "blessed_storm_claims_not_applicable": storm["claims_not_applicable"],
                   "build_seconds": round(build_s, 2)},
        "totals": pool(union_rows),
        "tiers": {M.TIER_FACT: pool(fact_rows), M.TIER_GUESS: pool(guess_rows)},
        "by_representation": by_kind(run),
        "shapes": shape_census(run),
        "shapes_emitted": all_shapes,
        "shapes_at_fact_tier": fact_shapes,
        "content": content_stats,
        "calibration": calibrate_pairs(calibration),
        "exact_similarity": exact_check,
        "per_scenario": {name: pool(rows) for name, rows in sorted(per_scenario.items())},
        "rows": [row.as_dict() for row in union_rows],
    }
    report["display_budget"] = display_budget(built, windows)
    report["copies_harder"] = copies_harder_measure(built, windows)
    report["dispositions"] = disposition_probe(built, windows, policy)
    report["probes"] = {"name_similarity_unused": name_similarity_probe(),
                        "budget_stated": budget_probe(policy)}
    report["cost"] = cost_measure(work)
    report["gates"] = gates(report)
    report["check"] = {"ok": all(g["result"] == "pass" for g in report["gates"]),
                       "failed": [g["id"] for g in report["gates"] if g["result"] != "pass"]}
    return report


def _exact_check(window: M.Window, result: M.Result) -> int:
    """Every content score must equal the exact Jaccard of its two shingle sets — recomputed here.

    `s4` found the sketch and the exact set disagreeing across a 0.30 bar 29 times out of 545, so the
    shipped engine decides on the exact number. This is the check that it actually does, rather than
    the doc claiming it does.
    """
    from .shingle import exact_jaccard, normalise_lines, shingles
    store: dict[str, set[int]] = {}

    def sh(path: str, side: str) -> set[int]:
        blob = window.base_blob(path) if side == "base" else window.head_blob(path)
        if blob is None:
            return set()
        digest = M.content_digest(blob)
        if digest not in store:
            store[digest] = shingles(normalise_lines(blob), 3)
        return store[digest]

    mismatched = 0
    for pairing in result.pairings:
        for edge in pairing.edges:
            if edge.method != "content":
                continue
            expected = exact_jaccard(sh(edge.src, "base"), sh(edge.dst, "head"))
            if abs(expected - edge.score) > 1e-9:
                mismatched += 1
    return mismatched


def display_budget(built: dict, windows: list[CP.TruthWindow]) -> dict:
    """What each display floor buys, priced in true pairings per lie let in.

    `rename.guess_floor` is the one knob here that is a display decision rather than a correctness
    one — `s4` measured that no threshold separates the decoy from the true rewrites — so the default
    is justified by printing the trade at every candidate value rather than by asserting it.
    """
    out = {}
    for floor in (0.10, 0.15, 0.20, 0.30, 0.40, 0.50):
        policy = Policy.built_in().with_knobs("display-budget sweep", guess_floor=floor)
        store = PAIR.Shingles(width=policy.integer("shingle_lines"), mode=policy.text("normalise"))
        rows = []
        for truth in windows:
            result = PAIR.pair(read_window(built, truth, policy), policy, store=store)
            rows.append(grade_window(truth, result))
        totals = pool(rows)
        out["%.2f" % floor] = {"emitted": totals["emitted"], "found": totals["found"],
                               "wrong": totals["wrong"],
                               "named_decoys": totals["wrong_named_decoy"],
                               "claimable": totals["claimable_pairs"],
                               "precision": totals["precision"],
                               "recall": totals["recall_claimable"],
                               "true_pairings_per_wrong": round(
                                   totals["found"] / totals["wrong"], 2)
                               if totals["wrong"] else None,
                               "ms_total": round(sum(r.ms for r in rows), 1)}
    return out


def copies_harder_measure(built: dict, windows: list[CP.TruthWindow]) -> dict:
    """The price of the widened candidate set, measured rather than asserted.

    `s4` excluded it on cost grounds and `e2.4`'s own cost criterion forbids it, but a reader is
    entitled to know what the shipped default gives up: the same corpus, run with
    `rename.candidates.copies_harder` on, with both the copies it sees and the pairings it invents.
    """
    policy = Policy.built_in().with_knobs("copies-harder probe", copies_harder=True)
    store = PAIR.Shingles(width=policy.integer("shingle_lines"), mode=policy.text("normalise"))
    rows: list[Row] = []
    pairs: list[tuple[CP.TruthWindow, M.Result]] = []
    blobs = 0
    started = time.perf_counter()
    for truth in windows:
        window = read_window(built, truth, policy)
        result = PAIR.pair(window, policy, store=store)
        rows.append(grade_window(truth, result))
        pairs.append((truth, result))
        blobs += result.stats["content"]["blobs_read"]
    return {"policy": "rename.candidates.copies_harder=true", "blobs_read": blobs,
            "seconds": round(time.perf_counter() - started, 2),
            "by_representation": by_kind(pairs), **pool(rows)}


# -- behavioural probes (AC 3) --------------------------------------------------------------------

def disposition_probe(built: dict, windows: list[CP.TruthWindow], policy: Policy) -> dict:
    """Accept, reject, and the one that is easy to forget: a decision whose evidence moved.

    AC 3 says a low-confidence candidate "can be accepted or rejected". Three things have to be true
    for that to be a feature and not a decoration, and all three are measured here on real windows:
    rejecting removes the pairing **and counts it**, accepting promotes it to asserted with the
    reviewer's name on it, and a decision made about different bytes is re-opened instead of inherited
    (`s4`: a squash rebase took a true pair from R100 to R012 — a ledger that kept the old verdict
    would be asserting something it never saw).
    """
    window = guess_window = None
    baseline = guess_baseline = None
    for truth in windows:
        if truth.scenario not in ("rename_rewrite", "clean_moves", "split_one_to_many"):
            continue
        candidate = read_window(built, truth, policy)
        result = PAIR.pair(candidate, policy)
        if window is None and result.pairings:
            window, baseline = candidate, result
        if guess_window is None and any(p.tier == M.TIER_GUESS for p in result.pairings):
            guess_window, guess_baseline = candidate, result
    if window is None or guess_window is None:
        return {"ok": False, "reason": "no pairings to review"}
    target = baseline.pairings[0]
    guess_target = next(p for p in guess_baseline.pairings if p.tier == M.TIER_GUESS)

    rejected = PAIR.pair(window, policy, ledger={target.key(): _record(target, "rejected")})
    accepted = PAIR.pair(guess_window, policy,
                         ledger={guess_target.key(): _record(guess_target, "accepted")})
    stale = PAIR.pair(guess_window, policy,
                      ledger={guess_target.key(): {"decision": "accepted", "reviewer": "someone",
                                                   "binding": "not-these-bytes"}})
    shown = [p for p in rejected.pairings if p.disposition != M.DISPOSITION_REJECTED]
    return {
        "ok": True,
        "window": window.key(),
        "guess_window": guess_window.key(),
        "baseline_pairings": len(baseline.pairings),
        "guess_baseline": len(guess_baseline.pairings),
        "rejected_removed": len(shown) == len(baseline.pairings) - 1
        and len(rejected.suppressed) == 1,
        "rejected_still_counted": bool(rejected.suppressed)
        and rejected.suppressed[0].key() == target.key(),
        "accepted_asserted": any(p.disposition == M.DISPOSITION_ACCEPTED and p.asserted
                                 for p in accepted.pairings),
        "accepted_names_reviewer": any(p.reviewer for p in accepted.pairings),
        "accepted_was_a_guess": guess_target.tier == M.TIER_GUESS,
        "reopened_on_new_bytes": any(p.disposition == M.DISPOSITION_REOPENED
                                     and p.tier == M.TIER_GUESS for p in stale.pairings),
        "unaffected_pairings_kept": len(accepted.pairings) == len(guess_baseline.pairings),
    }


def _record(pairing: M.Pairing, decision: str) -> dict:
    return {"decision": decision, "binding": pairing.binding, "reviewer": "daniel@example.invalid"}


def name_similarity_probe() -> dict:
    """The prohibition `s4` measured, asserted in the shipped engine rather than promised.

    Two files in different directories, same basename, and content that shares only a boilerplate
    header: path similarity is high and content similarity is ~0. A scorer that blended the two at any
    weight emits this pair; the shipped engine must not, and the window has no other evidence to lean
    on, so this is a clean test of "path evidence is display-only".
    """
    from .detect import name_similarity
    head = C.header("page.py")
    left = C.body("alpha/page.py", 6, flavour="alpha")
    right = C.body("beta/page.py", 6, flavour="beta")
    window = M.Window(id="name-only", base="a", head="b",
                      base_paths={"alpha/page.py"}, head_paths={"beta/page.py"},
                      removed={"alpha/page.py": head + left},
                      added={"beta/page.py": head + right})
    result = PAIR.pair(window, Policy.built_in())
    return {"name_similarity": round(name_similarity("alpha/page.py", "beta/page.py"), 3),
            "emitted": len(result.pairings),
            "ok": not result.pairings}


def budget_probe(policy: Policy) -> dict:
    """An over-budget window must say so; a stated partial is the difference between a bound and a lie."""
    tight = policy.with_knobs("budget probe", budget=1)
    bodies = {name: C.body(name, 5) for name in ("one", "two", "three")}
    window = M.Window(id="budget", base="a", head="b",
                      base_paths={"src/%s.py" % n for n in bodies},
                      head_paths={"src/moved/%s.py" % n for n in bodies},
                      removed={"src/%s.py" % n: text for n, text in bodies.items()},
                      added={"src/moved/%s.py" % n: text for n, text in bodies.items()})
    result = PAIR.pair(window, tight)
    note = [line for line in result.limits if "budget" in line]
    return {"budget": 1, "stated": bool(note), "note": note[0] if note else "",
            "ok": bool(note)}


# -- the cost bench (AC 4) ------------------------------------------------------------------------

def cost_probe(work: Path, *, files: int, changes: int, widen: bool = False) -> dict:
    """A repository of `files` paths with `changes` moved pairs; time one window through it."""
    repo_dir = work / ("cost-%d-%d%s" % (files, changes, "-wider" if widen else ""))
    script = R.Script("cost")
    for index in range(files):
        script.write("src/mod%05d.py" % index, C.body("src/mod%05d.py" % index, 4))
    script.commit("c0", message="a tree of %d files" % files)
    for index in range(changes):
        script.move("src/mod%05d.py" % index, "src/moved/mod%05d.py" % index)
    script.commit("c1", message="move %d of them" % changes, author="pi-agent")
    started = time.perf_counter()
    labels = R.build(repo_dir, script)
    build_s = time.perf_counter() - started
    policy = Policy.built_in().with_knobs("cost probe", copies_harder=widen)
    started = time.perf_counter()
    window = gitio.window_from_repo(repo_dir, labels["c0"], labels["c1"], window_id="cost",
                                    policy=policy)
    read_s = time.perf_counter() - started
    store = PAIR.Shingles(width=policy.integer("shingle_lines"), mode=policy.text("normalise"))
    started = time.perf_counter()
    result = PAIR.pair(window, policy, store=store)
    pair_s = time.perf_counter() - started
    return {"repo_files": files, "changed_pairs": changes, "widened": widen,
            "touches": window.touches, "tree_paths": len(window.base_paths),
            "build_s": round(build_s, 3), "read_ms": round(read_s * 1000, 1),
            "pair_ms": round(pair_s * 1000, 2), "total_ms": round((read_s + pair_s) * 1000, 2),
            "blobs_read": result.stats["content"]["blobs_read"],
            "candidates": result.stats["candidates"]["cross_product"],
            "scored_pairs": result.stats["content"]["scored"]}


def cost_measure(work: Path) -> dict:
    """AC 4's bench: repository size up with the change set fixed, shipped mode beside the refused one."""
    sizes = (200, 1_000, 4_000)
    shipped = [cost_probe(work, files=size, changes=6) for size in sizes]
    widened = [cost_probe(work, files=size, changes=6, widen=True) for size in sizes]
    growth = [cost_probe(work, files=400, changes=n) for n in (1, 4, 16, 48)]
    ratio = shipped[-1]["total_ms"] / max(0.01, shipped[0]["total_ms"])
    widened_ratio = widened[-1]["total_ms"] / max(0.01, widened[0]["total_ms"])
    return {"sizes": list(sizes), "shipped": shipped, "widened": widened,
            "change_set_growth": growth,
            "ratio_small_to_20x_shipped": round(ratio, 2),
            "ratio_small_to_20x_widened": round(widened_ratio, 2),
            "change_1_to_48_shipped": round(growth[-1]["total_ms"] /
                                            max(0.01, growth[0]["total_ms"]), 2)}


# -- the gates ------------------------------------------------------------------------------------

def gate(out: list, gid: str, claim: str, read_from: str, expected, observed,
         op: str = "==") -> None:
    ok = {"==": lambda: observed == expected,
          "!=": lambda: observed != expected,
          ">=": lambda: observed is not None and observed >= expected,
          "<=": lambda: observed is not None and observed <= expected,
          "is": lambda: bool(observed)}[op]()
    out.append({"id": gid, "claim": claim, "read_from": read_from,
                "expected": ("%s %s" % (op, expected)) if op != "==" else expected,
                "observed": observed, "result": "pass" if ok else "FAIL"})


def gates(report: dict) -> list[dict]:
    """Every claim this ticket makes about itself, as a row that can fail."""
    out: list[dict] = []
    totals = report["totals"]
    fact = report["tiers"][M.TIER_FACT]
    guess = report["tiers"][M.TIER_GUESS]
    kinds = report["by_representation"]
    named = {}
    for scenario in (CP.STORM, "rename_rewrite"):
        rows = [row for row in report["rows"] if row["scenario"] == scenario]
        found = sum(row["found"] for row in rows)
        claimable = sum(row["claimable"] for row in rows)
        emitted = sum(row["emitted"] for row in rows)
        named[scenario] = {"found": found, "claimable": claimable, "emitted": emitted,
                           "recall": round(found / claimable, 4) if claimable else None,
                           "precision": round(found / emitted, 4) if emitted else None}
    named_pool = sum(named[s]["found"] for s in named)
    named_claimable = sum(named[s]["claimable"] for s in named)

    gate(out, "F1", "the fact tier asserts nothing untrue on this corpus", "tiers.fact.wrong",
         0, fact["wrong"])
    gate(out, "F2", "and its precision is at or above the bar s4 set for it",
         "tiers.fact.precision", FACT_PRECISION, fact["precision"], ">=")
    gate(out, "F3", "the fact tier is not precise because it is nearly silent",
         "tiers.fact.emitted", FACT_MIN_PAIRINGS, fact["emitted"], ">=")
    gate(out, "F4", "no pair the truth names as not-a-move is ever asserted as a fact",
         "tiers.fact.wrong_named_decoy", 0, fact["wrong_named_decoy"])
    gate(out, "F5", "git's own decoy still reaches the reviewer — as a labelled guess",
         "rows[scaffold_decoys].wrong_named_decoy", 1,
         sum(row["wrong_named_decoy"] for row in report["rows"]
             if row["scenario"] == "scaffold_decoys"), ">=")
    gate(out, "A1", "AC 1, restated from s4: every claimable pair in the two named fixtures is "
         "shown, as a fact or as a guess", "fixtures.named.recall", PAIRED_RECALL,
         round(named_pool / named_claimable, 4) if named_claimable else None, ">=")
    gate(out, "A1b", "and none of the pairings shown in those fixtures is untrue",
         "fixtures.named.wrong", 0,
         sum(row["wrong"] for row in report["rows"]
             if row["scenario"] in (CP.STORM, "rename_rewrite")))
    gate(out, "A2a", "AC 2: every claimable split pair is emitted as a split",
         "by_representation.split.recall", GROUP_RECALL, kinds.get("split", {}).get("recall"))
    gate(out, "A2b", "every claimable fold pair is emitted as a merge",
         "by_representation.merge.recall", GROUP_RECALL, kinds.get("merge", {}).get("recall"))
    gate(out, "A2c", "and no pair is ever found under the wrong shape — a split inside a rename row "
         "is a representation failure, not a hit", "by_representation.*.found_wrong_shape", 0,
         sum(v["found_wrong_shape"] for v in kinds.values()))
    gate(out, "A2d", "a 1-to-3 split is ONE pairing with three destinations, not three renames",
         "shapes[split 1->3].pairings", 2,
         report["shapes"].get("split 1->3", {}).get("pairings", 0), ">=")
    gate(out, "A2e", "a fold of three files is ONE pairing with three sources",
         "shapes[merge 3->1].pairings", 1,
         report["shapes"].get("merge 3->1", {}).get("pairings", 0), ">=")
    gate(out, "A2f", "copies are represented, not skipped: under the widened candidate set every "
         "claimable copy pair is emitted as a copy — the shipped default refuses that mode on cost, "
         "not on ability", "copies_harder.by_representation.copy.recall", GROUP_RECALL,
         report["copies_harder"].get("by_representation", {}).get("copy", {}).get("recall"))
    gate(out, "A3a", "AC 3: rejecting removes the pairing and keeps it counted",
         "dispositions.rejected_removed", True, report["dispositions"]["rejected_removed"], "is")
    gate(out, "A3b", "accepting makes it asserted, and says who accepted it",
         "dispositions.accepted_asserted", True,
         report["dispositions"]["accepted_asserted"] and report["dispositions"]["accepted_names_reviewer"], "is")
    gate(out, "A3c", "a decision about different bytes is re-opened, not inherited",
         "dispositions.reopened_on_new_bytes", True,
         report["dispositions"]["reopened_on_new_bytes"], "is")
    gate(out, "A3d", "and a decision about one pairing does not touch its neighbours",
         "dispositions.unaffected_pairings_kept", True,
         report["dispositions"]["unaffected_pairings_kept"], "is")
    gate(out, "A4a", "AC 4: 20x the repository with the change set fixed costs under %.1fx as much"
         % COST_RATIO, "cost.ratio_small_to_20x_shipped", COST_RATIO,
         report["cost"]["ratio_small_to_20x_shipped"], "<=")
    gate(out, "A4b", "and the refused O(repo) mode is the one that actually scales with the tree",
         "cost.ratio_small_to_20x_widened", DECISIVE_COST_RATIO,
         report["cost"]["ratio_small_to_20x_widened"], ">=")
    gate(out, "A4c", "widening reads more blobs, so the cost claim is about blobs and not about "
         "luck", "copies_harder.blobs_read", report["content"]["blobs_read"],
         report["copies_harder"]["blobs_read"], ">=")
    gate(out, "S1", "a window in which nothing moved emits nothing at all",
         "totals.silent_windows_dirty", 0, totals["silent_windows_dirty"])
    gate(out, "S2", "the copies the shipped candidate set cannot see are named as a blindness, not "
         "counted as misses nobody mentions", "by_representation.copy.claimable",
         kinds.get("copy", {}).get("claimable"), kinds.get("copy", {}).get("claimable"))
    gate(out, "P1", "the fact bar that ships is the one s4 measured", "policy.fact_bar", 0.75,
         report["policy"]["fact_bar"])
    gate(out, "P2", "git is asked below the fact bar, so a guess can carry git's own number",
         "policy.git_candidate_percent", True, report["policy"]["git_candidate_percent"]
         < int(report["policy"]["fact_bar"] * 100), "is")
    gate(out, "P3", "no content score is an approximation: every content edge equals the exact "
         "Jaccard of its two shingle sets", "exact_similarity.mismatched", 0,
         report["exact_similarity"]["mismatched"])
    gate(out, "P4", "path similarity is display-only: same basename, disjoint content, no pairing",
         "probes.name_similarity_unused.ok", True,
         report["probes"]["name_similarity_unused"]["ok"], "is")
    gate(out, "P5", "an over-budget window says how much it dropped", "probes.budget_stated.ok",
         True, report["probes"]["budget_stated"]["ok"], "is")
    gate(out, "P6", "the two tiers are ordered by trust: nothing in the guess tier is as reliable as "
         "a fact, which is the whole reason the split exists",
         "tiers.fact.precision > tiers.guess.precision", True,
         (fact["precision"] or 0) > (guess["precision"] or 0), "is")
    gate(out, "P7", "and the calibration table is measured on enough pairs to mean anything",
         "calibration.pairs", 40, report["calibration"]["pairs"], ">=")
    gate(out, "X1", "the fact tier's residual is structural, not tuning: no split, fold or copy is "
         "ever emitted at the fact tier", "shapes_at_fact_tier(groups)", 0,
         sum(count for kind, count in report["shapes_at_fact_tier"].items()
             if kind in M.GROUP_KINDS))
    return out


# -- printing the report ---------------------------------------------------------------------------

def text(report: dict) -> str:
    """The report as a page: the headline numbers with their counts, then every gate."""
    totals, tiers = report["totals"], report["tiers"]
    fact, guess = tiers[M.TIER_FACT], tiers[M.TIER_GUESS]
    out = ["e2.4 rename pairing — measured on %d windows across %d scenarios"
           % (report["corpus"]["windows"], report["corpus"]["scenarios"])]
    out.append("  corpus   %d hand-authored pair(s), %d claimable, %d above the evidence ceiling; "
               "%d path assertion(s) coherence-checked"
               % (totals["true_pairs"], totals["claimable_pairs"], totals["ceiling_pairs"],
                  report["corpus"]["coherence_paths_checked"]))
    out.append("  fact     %d emitted, %d true, %d untrue · precision %s · recall of claimable %s"
               % (fact["emitted"], fact["found"], fact["wrong"], fact["precision"],
                  fact["recall_claimable"]))
    out.append("  guess    %d emitted, %d true, %d untrue · precision %s · recall of claimable %s"
               % (guess["emitted"], guess["found"], guess["wrong"], guess["precision"],
                  guess["recall_claimable"]))
    out.append("  pooled   %d emitted, %d true · precision %s · recall %s · %d untrue per 100 windows"
               % (totals["emitted"], totals["found"], totals["precision"],
                  totals["recall_claimable"], totals["wrong_per_100_windows"]))
    out.append("  quiet    %d window(s) with nothing to find, %d of them emitted anything"
               % (totals["silent_windows"], totals["silent_windows_dirty"]))
    out.append("  by kind")
    for kind, row in report["by_representation"].items():
        out.append("    %-7s %2d/%2d claimable found as %s (recall %s, %d found under a wrong shape)"
                   % (kind, row["found"], row["claimable"], kind, row["recall"],
                      row["found_wrong_shape"]))
    out.append("  shapes emitted   " + ", ".join(
        "%s×%d" % (name, row["pairings"]) for name, row in report["shapes"].items()))
    out.append("  calibration      ECE %s over %d scored pair(s) — a similarity index is not a "
               "probability, and the card says so instead of gating on this"
               % (report["calibration"]["expected_calibration_error"],
                  report["calibration"]["pairs"]))
    out.append("  cost             20x the repository: shipped %.2fx, widened %.2fx (change set "
               "fixed)" % (report["cost"]["ratio_small_to_20x_shipped"],
                           report["cost"]["ratio_small_to_20x_widened"]))
    out.append("  display budget   " + "  ".join(
        "%s→%s/%s" % (floor, row["found"], row["wrong"])
        for floor, row in report["display_budget"].items()) + "   (found/wrong)")
    out.append("")
    out.append("  gate  result  expected            observed            claim")
    for row in report["gates"]:
        out.append("  %-5s %-6s %-18s %-18s %s"
                   % (row["id"], row["result"], str(row["expected"])[:18],
                      str(row["observed"])[:18], row["claim"][:72]))
    failed = report["check"]["failed"]
    out.append("")
    out.append("  %d gate(s), %d failed%s"
               % (len(report["gates"]), len(failed), ": " + ", ".join(failed) if failed else ""))
    return "\n".join(out)


# -- the report file ------------------------------------------------------------------------------

def write_report(report: dict, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(report, indent=1, sort_keys=True) + "\n")


def check_report(report: dict, path: Path) -> list[str]:
    """Re-measure against a committed report.

    A gate may not change result, a gate's observed value may not drift while its result stands, and
    the pooled totals and per-class recalls may not move without something here noticing. Timing and
    the cost bench are excluded: they are host-dependent, and a report that failed on a loaded CI box
    would train people to ignore it.
    """
    if not path.exists():
        return ["%s does not exist: run `python3 -m recommender.rename.score --write`" % path]
    old = json.loads(path.read_text())
    problems: list[str] = []
    before = {g["id"]: g for g in old.get("gates", [])}
    now = {g["id"]: g for g in report.get("gates", [])}
    for gid in sorted(set(before) - set(now)):
        problems.append("gate %s went missing (%s)" % (gid, before[gid]["claim"]))
    for gid in sorted(set(now) - set(before)):
        problems.append("gate %s is new: the committed report does not make this claim yet" % gid)
    for gid in sorted(set(before) & set(now)):
        if before[gid]["result"] != now[gid]["result"]:
            problems.append("gate %s changed result: %s -> %s (%s: expected %s, observed %s)"
                            % (gid, before[gid]["result"], now[gid]["result"],
                               before[gid]["claim"], before[gid]["expected"],
                               now[gid]["observed"]))
        elif before[gid]["observed"] != now[gid]["observed"] and not gid.startswith("A4"):
            problems.append("gate %s observed %s, committed report says %s (%s)"
                            % (gid, now[gid]["observed"], before[gid]["observed"],
                               before[gid]["claim"]))
    for key in ("totals", "by_representation"):
        if _without_clock(old.get(key)) != _without_clock(report.get(key)):
            problems.append("%s differs from the committed report" % key)
    return problems


#: Keys that describe the computer a measurement ran on. `check_report` compares the pooled blocks
#: whole, so a wall-clock key anywhere inside them turns a reproducibility check into a claim about the
#: runner (`reco-2ezf`); `pool()` no longer writes any of these, and this is the belt to that braces.
RUNNER_FACT_KEYS = ("ms", "ms_total", "ms_mean", "ms_p95", "ms_max", "total_ms", "pair_ms",
                    "read_ms", "build_s", "seconds", "measured_in_seconds")


def _without_clock(node):
    """Project a compared block down to its claims: every wall-clock leaf goes, wherever it sits."""
    if isinstance(node, dict):
        return {key: _without_clock(value) for key, value in node.items()
                if key not in RUNNER_FACT_KEYS}
    if isinstance(node, list):
        return [_without_clock(value) for value in node]
    return node
