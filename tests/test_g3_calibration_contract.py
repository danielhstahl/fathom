"""Guardrail 3 calibration (`e4.9`, `reco-mkl.1`), as tests: the corpus, the labels and the claim.

`e4.3` and the beta milestone `m1` both defer to "the threshold agreed in `e0.12`", and ADR-001 §5.4 is
where `e0.12` wrote those numbers down. This suite is the executable form of the four things that make the
measurement more than a number in a document:

1. **the corpus is three real repositories pinned by commit**, and every label resolves inside the window
   it claims — a label that cannot be found is a claim about a commit that is not there;
2. **the labels are inputs and not output**: nothing in this tree writes a `truth.json`, every dismissal
   uses a code from the closed vocabulary `e4.6` owns, and every row carries the note and the evidence line
   that make it arguable;
3. **the thresholds are §5.4's numbers and not a second copy with an opinion** — the bars are parsed out of
   `docs/concepts/thesis.md` §5.4 and compared with `thresholds.json`, and the gates' declared `expected`
   are compared with what the blessed report says happened;
4. **the written answer quotes the report**, in both documents that cite it, so a re-bless that moves a
   number cannot leave the prose behind.

The harness itself is not re-run here — that is
`cargo run -p fathom-analyze --features sqlite-bundled --example g3_calibration -- --check`, and CI runs
it. What CI cannot check without a build is what this file is for, and it is cheap: no compilation, no
clone, no network.

    python3 -m pytest -q tests/test_g3_calibration_contract.py
"""

from __future__ import annotations

import json
import re
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
CALIBRATION = ROOT / "crates" / "fathom-analyze" / "calibration" / "g3"
CORPUS = CALIBRATION / "corpus"
THRESHOLDS = CALIBRATION / "thresholds.json"
CORPUS_README = CALIBRATION / "README.md"
REPORT = ROOT / "benchmarks" / "reports" / "e4-3a-g3-calibration.json"
THESIS = ROOT / "docs" / "concepts" / "thesis.md"
CONCEPT = ROOT / "docs" / "concepts" / "false-positive-calibration.md"
HARNESS = ROOT / "crates" / "fathom-analyze" / "examples" / "g3_calibration.rs"
DISPOSITION_RS = ROOT / "crates" / "fathom-analyze" / "src" / "queue" / "disposition.rs"

#: The four settling acts `e4.6`'s ledger recognises (`ledger::entry::Decision`).
SETTLING_DECISIONS = {"accepted", "flagged", "dismissed", "snoozed"}

#: Codes that are neither a false positive nor a true positive: the dismissal is about the reviewer's
#: attention or the queue's state, so it sits in `G3`'s denominator and not its numerator.
NEITHER_CODES = {"not-my-area", "already-known", "superseded", "will-revert", "other"}


@pytest.fixture(scope="module")
def thresholds() -> dict:
    return json.loads(THRESHOLDS.read_text())


@pytest.fixture(scope="module")
def report() -> dict:
    return json.loads(REPORT.read_text())


@pytest.fixture(scope="module")
def pins() -> dict[str, dict]:
    out = {}
    for directory in sorted(p for p in CORPUS.iterdir() if p.is_dir()):
        out[directory.name] = json.loads((directory / "pin.json").read_text())
    return out


@pytest.fixture(scope="module")
def truths() -> dict[str, dict]:
    out = {}
    for directory in sorted(p for p in CORPUS.iterdir() if p.is_dir()):
        out[directory.name] = json.loads((directory / "truth.json").read_text())
    return out


@pytest.fixture(scope="module")
def shipped_reason_codes() -> set[str]:
    """The closed dismiss vocabulary, read out of the Rust source that owns it."""
    text = DISPOSITION_RS.read_text()
    block = text.split("pub const REASONS", 1)[1].split("];", 1)[0]
    return set(re.findall(r'\(\s*"([a-z-]+)"\s*,', block))


# ---------------------------------------------------------------------------------------------
# 1. the corpus: three real repositories, pinned by commit
# ---------------------------------------------------------------------------------------------


def test_exactly_three_pinned_repositories(pins):
    assert len(pins) == 3, f"{CORPUS.relative_to(ROOT)} holds three repositories, found {list(pins)}"


def test_each_pin_is_a_pinned_commit_and_window(pins):
    for name, pin in pins.items():
        assert re.fullmatch(r"[0-9a-f]{40}", pin["head"]), f"{name}: head is not a full oid"
        # Not a style preference: the miss-rate gates are rates over the mature reworked episodes this
        # window contains, and at 60 episodes per pin the denominator was 7 — a 0.20 gate cannot tell
        # 0.20 from 0.60 at n = 7. Shrinking a window re-breaks that gate (corpus README §6).
        assert pin["window"] >= 180, f"{name}: a window under 180 episodes cannot gate a miss rate"
        assert pin["url"].startswith("https://"), f"{name}: url must be cloneable"
        assert pin["repository"] == name, f"{name}: pin name and directory disagree"
        # The harness has no clock; `pinned_now` is it, so it has to be a real instant in the shape the
        # harness parses (`iso_to_millis` panics on anything else).
        assert re.fullmatch(
            r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z", pin["pinned_now"]
        ), f"{name}: pinned_now is not `YYYY-MM-DDTHH:MM:SSZ`"
        for field in ("why", "pinned_by"):
            assert len(pin[field]) > 40, f"{name}: `{field}` must say something a reader can argue with"


def test_pins_are_the_reporters_the_harness_measured(pins, report):
    measured = {r["repository"] for r in report["repositories"]}
    assert measured == set(pins), "the report and the corpus disagree about which repositories exist"
    for row in report["repositories"]:
        assert row["head"] == pins[row["repository"]]["head"], f"{row['repository']}: re-pinned but not re-measured"
        assert row["window"] == pins[row["repository"]]["window"]
        assert row["episodes"] > 0 and row["scored"] == row["episodes"], (
            f"{row['repository']}: an episode without a score is an episode the flag set cannot see"
        )


def test_each_window_delivered_the_episodes_it_claims(pins, report):
    """A shallow clone does not fail; it measures fewer episodes than the pin claims and says nothing.

    The fetcher's depth is derived from the window for exactly this reason (`window * 4 + 20`, because the
    harness walks four commits per episode), and a widened pin whose clone ran out would quietly shrink the
    denominator every miss-rate gate is measured on.
    """
    for row in report["repositories"]:
        window = pins[row["repository"]]["window"]
        assert row["episodes"] == window, (
            f"{row['repository']}: the pin claims a {window}-episode window and the harness measured "
            f"{row['episodes']} — re-run `python3 tools/fetch_g3_corpus.py --force`"
        )


def test_the_fetcher_is_a_fetcher_and_not_a_generator():
    """`tools/fetch_g3_corpus.py` fetches what a pin names, checks the oid came back, and stops.

    It fetches a pinned oid rather than cloning a branch tip, because a branch tip is a moving thing and a
    calibration corpus that moves underneath the labels is measuring a different corpus.
    """
    text = (ROOT / "tools" / "fetch_g3_corpus.py").read_text()
    assert "--depth" in text and "rev-parse" in text, "the fetcher must fetch a pinned oid, not a branch"
    assert "the remote gave" in text, "the fetcher must refuse a remote that returns something else"
    for forbidden in ("write_text", "shutil.copy", "open(", "\"commit\""):
        assert forbidden not in text, f"the fetcher must never write {forbidden!r} — it only fetches"


def test_the_harness_reads_truth_and_never_writes_it():
    """The rule spikes `s1` and `s2` left behind, enforced over the whole tree.

    Any line in any source file that names `truth.json` is checked: it may read the file, and it may not
    carry a write primitive. A harness that authors its own truth measures the harness, so this is not
    hygiene — it is the measurement's validity.
    """
    writers = ("write_text", "File::create", "fs::write", "File::options", ".create(true)", "> truth")
    offenders = []
    for path in sorted(list(ROOT.rglob("*.rs")) + list(ROOT.rglob("*.py"))):
        if ".pytest_cache" in path.parts or path == Path(__file__):
            continue
        for line_no, line in enumerate(path.read_text(errors="ignore").splitlines(), 1):
            if "truth.json" in line and any(w in line for w in writers):
                offenders.append(f"{path.relative_to(ROOT)}:{line_no}")
    assert offenders == [], f"these lines write a truth file that only a reviewer may write: {offenders}"
    harness = HARNESS.read_text()
    assert "read_to_string" in harness and "truth.json" in harness


# ---------------------------------------------------------------------------------------------
# 2. the labels: hand-authored, closed-vocabulary, argued
# ---------------------------------------------------------------------------------------------


def test_vocabulary_was_found_in_the_source_that_owns_it(shipped_reason_codes):
    assert {"false-positive", "low-stakes", "intentional"} <= shipped_reason_codes
    assert len(shipped_reason_codes) == 8, shipped_reason_codes


def test_every_truth_file_names_who_wrote_it_and_what_a_row_means(truths):
    for name, truth in truths.items():
        assert truth["schema"] == "fathom.g3-labels/1", f"{name}: unexpected schema"
        authoring = truth["authoring"]
        for field in ("annotated_by", "protocol", "annotated_on", "meaning"):
            assert authoring[field].strip(), f"{name}: authoring.{field} is empty"
        assert "README.md" in authoring["protocol"], f"{name}: the protocol must be a written procedure"
        # The honest-labelling rule (corpus README §5): whoever wrote them, the file says what kind of
        # actor it was, because `e4.6` forbids an agent from being the accountable human.
        assert "(agent)" in authoring["annotated_by"] or "(human)" in authoring["annotated_by"], (
            f"{name}: annotated_by must declare whether a human or an agent authored the labels"
        )


def test_labels_are_unique_argued_and_use_the_closed_vocabulary(truths, shipped_reason_codes, thresholds):
    for name, truth in truths.items():
        seen = set()
        for label in truth["labels"]:
            commit = label["commit"]
            assert re.fullmatch(r"[0-9a-f]{40}", commit), f"{name}: {commit} is not a full oid"
            assert commit not in seen, f"{name}: {commit} is labelled twice"
            seen.add(commit)
            assert label["decision"] in SETTLING_DECISIONS, f"{name}/{commit}: not a settling act"
            assert label["note"].strip() and label["evidence"].strip(), (
                f"{name}/{commit}: a verdict with no note or no evidence cannot measure "
                "whether the flag was worth raising"
            )
            assert len(label["note"]) > 40, f"{name}/{commit}: a one-clause note is not a judgement"
            assert label["actor"].strip(), f"{name}/{commit}: no actor"
            reason = label["reason"]
            if label["decision"] == "dismissed":
                assert reason in shipped_reason_codes, f"{name}/{commit}: {reason!r} is not in the shipped list"
                assert reason in set(thresholds["fp_reasons"]) | NEITHER_CODES, (
                    f"{name}/{commit}: {reason!r} is neither one of §5.4's false-positive codes "
                    "nor one of the neither-codes this harness documents"
                )
            else:
                assert reason == "", f"{name}/{commit}: only a dismissal carries a reason code"
            assert label["blame"] == sorted(set(label["blame"])) or not label["blame"], (
                f"{name}/{commit}: blame must be sorted and free of duplicates"
            )


def test_fp_reasons_are_the_documented_collapse(truths, thresholds, shipped_reason_codes):
    """§5.4's four codes collapse onto three shipped ones, and the file says so instead of hiding it."""
    fp = thresholds["fp_reasons"]
    assert fp == ["intentional", "false-positive", "low-stakes"]
    assert set(fp) <= shipped_reason_codes
    assert "not-applicable" not in fp
    assert "not-applicable" in thresholds["labelling_note"], (
        "the mapping that drops §5.4's `not-applicable` has to be written down somewhere a reader "
        "will look, not left as an omission"
    )
    # Every dismissal in the corpus is either one of §5.4's FP codes or one of the neither-codes.
    reasons = {l["reason"] for t in truths.values() for l in t["labels"] if l["decision"] == "dismissed"}
    assert reasons <= set(fp) | NEITHER_CODES


def test_blame_names_score_or_a_real_detector(truths, report):
    catalogue = {d["id"] for row in report["repositories"] for d in row["detectors"]}
    assert len(catalogue) == 12, f"the report's detector table is the whole catalogue: {sorted(catalogue)}"
    for name, truth in truths.items():
        for label in truth["labels"]:
            for token in label["blame"]:
                assert token == "score" or token in catalogue, (
                    f"{name}/{label['commit'][:8]}: blames {token!r}, which is neither `score` "
                    "nor a detector in the catalogue"
                )
            if label["blame"] and label["decision"] != "dismissed":
                pytest.fail(f"{name}/{label['commit'][:8]}: only a false alarm gets a blame")


def test_every_label_resolved_inside_its_window(report):
    """An orphaned label is a pin that moved; a refused code is a vocabulary that drifted."""
    for row in report["repositories"]:
        assert row["orphaned_labels"] == [], f"{row['repository']}: labels no longer resolve"
        assert row["refused_labels"] == [], f"{row['repository']}: labels outside the shipped vocabulary"


def test_every_flagged_episode_got_a_label(report):
    """`labelled(e)` is `G3`'s denominator; a flag nobody looked at is a hole in the guardrail.

    A label whose episode the cut stopped flagging is the opposite shape, and `reco-6co` is what taught the
    two apart: changing what gets flagged stranded three dispositions on bot bumps that fell out of the
    cut. They stay in `truth.json` — a reading of a diff is not retracted because the ranking moved — and
    the report prints them in `labels_unflagged` instead of folding them into `labelled`, because a rate
    may only be computed over what the cut flagged. Before that, the harness refused a `labelled_total`
    that disagreed with the cut at all, which conflated a hole with a surplus.
    """
    for row in report["repositories"]:
        for cut in ("at_tau_alpha", "at_tau_decile", "at_corpus_cut"):
            assert row[cut]["unlabelled_flagged"] == [], f"{row['repository']} {cut}: unlabelled flags"
        assert row["labelled_total"] == len(row["labels"])
        assert row["at_corpus_cut"]["labelled"] + len(row["labels_unflagged"]) == row[
            "labelled_total"
        ], f"{row['repository']}: labels are neither on a flagged episode nor declared in `labels_unflagged`"
        assert len(row["labels_unflagged"]) == sum(
            1 for label in row["labels"] if label["episode"] not in
            {receipt["episode"] for receipt in row["receipts"]}
        ), f"{row['repository']}: `labels_unflagged` and the receipts disagree"


# ---------------------------------------------------------------------------------------------
# 3. the thresholds: §5.4's numbers, gates that declare themselves
# ---------------------------------------------------------------------------------------------


def test_thresholds_are_the_numbers_in_adr001_section_5_4(thresholds):
    """The thresholds come from one place, and that place is §5.4 — so §5.4 is parsed, not trusted."""
    text = THESIS.read_text()
    section = text.split("### 5.4", 1)[1].split("### 5.5", 1)[0]
    gates = re.search(
        r"`G3 ≤ ([0-9.]+)` at alpha, `≤ ([0-9.]+)` at 1\.0; `miss_rate ≤ ([0-9.]+)`", section
    )
    assert gates, "§5.4's provisional-gates sentence moved; the copy in thresholds.json is uncheckable now"
    assert float(gates.group(1)) == pytest.approx(thresholds["g3_alpha"])
    assert float(gates.group(2)) == pytest.approx(thresholds["g3_1_0"])
    assert float(gates.group(3)) == pytest.approx(thresholds["miss_rate_max"])
    tau = re.search(r"default risk score ≥ (\d+)", section)
    assert tau and int(tau.group(1)) == thresholds["tau_alpha"], (
        "§5.4's published τ and thresholds.json's are one number; change the document, not this file"
    )


def test_every_gate_declares_what_it_expects(thresholds):
    ids = [gate["id"] for gate in thresholds["gates"]]
    assert len(ids) == len(set(ids)), "two gates with one id is one gate that never reports"
    for gate in thresholds["gates"]:
        assert gate["expected"] in {"pass", "fail", "not-measurable"}, gate["id"]
        assert gate["metric"], gate["id"]
        assert gate["claim"].strip(), gate["id"]
        assert len(gate.get("why", "")) > 30, f"{gate['id']}: a gate nobody wrote a reason for"
        assert gate.get("min") is not None or gate.get("max") is not None, (
            f"{gate['id']}: a gate with no bound is a caption, not a gate"
        )


def test_no_gate_came_out_other_than_declared(report, thresholds):
    declared = {g["id"]: g["expected"] for g in thresholds["gates"]}
    blessed = {g["id"]: g for g in report["gates"]}
    assert set(blessed) == set(declared), "the report and thresholds.json disagree about which gates exist"
    surprises = [
        f"{gid}: declared {declared[gid]}, measured {blessed[gid]['status']}"
        for gid in declared
        if blessed[gid]["status"] != declared[gid]
    ]
    assert surprises == [], "\n".join(surprises)


def test_a_gate_that_passes_on_an_empty_denominator_is_a_lie(report):
    """The floor rule, asserted against the blessed artefact rather than against a comment."""
    for gate in report["gates"]:
        if gate["status"] == "pass":
            assert "under the floor" not in gate["detail"]
        if gate["status"] == "not-measurable":
            assert "floor" in gate["detail"] or "no measurement" in gate["detail"] or "no denominator" in gate["detail"]


def test_the_alpha_gate_is_measurable_at_the_published_tau_and_fails(report):
    """The published τ is 40, it flags a population, and the gate comes out over the bar.

    `e4.9`'s 60-episode pins gave three flagged episodes at τ 70, under the floor of five, so the gate had
    to refuse to read them (`not-measurable`); the widened corpus read 0.333 on six flags, and six flags is
    still not a rate over what a reviewer is shown. `reco-6co` moved τ to the corpus's own top-decile
    floor, which is the lowest threshold whose whole flag population the hand-authored labels can answer
    for. That last clause is what this test guards: if the shipped default ever sinks below the floor, or
    the corpus shrinks so the gate's denominator falls under five, the gate has to be re-declared — not
    left to print a pass on an anecdote or a rate over unlabelled flags.
    """
    alpha = report["aggregate"]["cut_tau_alpha"]
    tau = report["formulas"]["tau_alpha"]
    assert alpha["tau"] == tau == 40
    assert alpha["labelled"] >= 5, (
        "the published default no longer flags enough episodes to measure `G3` there: the corpus shrank, "
        "and `g3-alpha-at-published-tau` has to be re-declared `not-measurable` rather than left to guess"
    )
    sweep = {row["tau"]: row for row in report["aggregate"]["tau_sweep"]["rows"]}
    assert sweep[tau]["unlabelled"] == 0, (
        f"τ {tau} flags {sweep[tau]['unlabelled']} episode(s) nobody labelled: the published default is "
        "below the floor the corpus can answer for"
    )
    below = sweep.get(tau - 5)
    assert below is None or below["unlabelled"] > 0, (
        f"τ {tau - 5} has no unlabelled flags either: the published default is no longer the lowest "
        "threshold the labels can answer for, and §5.4's sentence about why it is 40 has to be re-read"
    )
    declared = {g["id"]: g for g in report["gates"]}["g3-alpha-at-published-tau"]
    assert declared["expected"] == declared["status"] == "fail", (
        f"measured {declared['detail']}: over the 0.30 bar on {alpha['labelled']} labelled flags"
    )


def test_the_miss_rate_denominator_is_big_enough_to_read_the_rate(report):
    """The gate this corpus was widened for, asserted against the artefact.

    Seven mature reworked episodes could not discriminate a 0.20 gate; the ticket that widened the pins did
    it by growing the corpus, with the bars byte-identical. This is the guard that the corpus stays big
    enough for the two miss-rate gates to mean something.
    """
    miss = report["aggregate"]["miss"]
    assert miss["mature_rework"] >= 20, f"only {miss['mature_rework']} mature reworked episodes: G3's " "companion gate is an anecdote again"
    gate = {g["id"]: g for g in report["gates"]}["miss-rate-denominator-is-big-enough"]
    assert gate["expected"] == gate["status"] == "pass"


# ---------------------------------------------------------------------------------------------
# 4. the written answer quotes the report
# ---------------------------------------------------------------------------------------------


def test_the_concept_document_quotes_the_blessed_numbers(report):
    doc = CONCEPT.read_text()
    corpus = report["aggregate"]["cut_corpus"]
    miss = report["aggregate"]["miss"]
    for needle in (
        f"{corpus['g3']:.3f}",
        f"{miss['miss_rate']:.3f}",
        f"{miss['miss_rate_at_decile']:.3f}",
        str(corpus["labelled"]),
        str(corpus["false_positives"]),
        str(miss["mature_rework"]),
        str(report["aggregate"]["coverage"]["blamed_on_score"]),
    ):
        assert needle in doc, f"docs/concepts/false-positive-calibration.md no longer quotes {needle}"


def test_the_concept_document_names_the_command_and_the_report(report):
    for path in (CONCEPT, CORPUS_README):
        text = path.read_text()
        assert "g3_calibration" in text, path
        assert "--check" in text, path
    assert "e4.9" in CONCEPT.read_text()
    assert "g3_calibration" in THESIS.read_text(), (
        "§5.4 is where `e4.3` and `m1`'s deferred criteria land; it has to name the command, not just the number"
    )


def test_per_repository_detector_rows_sum_to_the_aggregate(report):
    """A repository table and an aggregate table that disagree is two measurements, not one.

    The corpus cut is the population both are computed over, so the per-repository counts must add up
    exactly — including the false positives, which is where a blame rule with a leak would show first.
    """
    aggregate = report["aggregate"]["detectors_corpus"]
    for detector_id, row in aggregate.items():
        rows = [
            next(d for d in repo["detectors"] if d["id"] == detector_id)
            for repo in report["repositories"]
        ]
        for key in ("spoke", "answerable_for", "false_positives"):
            assert sum(r[key] for r in rows) == row[key], (
                f"{detector_id}/{key}: the repositories say "
                f"{sum(r[key] for r in rows)}, the aggregate says {row[key]}"
            )


def test_per_detector_verdicts_in_the_document_match_the_report(report):
    """The per-detector table is parsed, not spell-checked.

    Every row of the written answer's table is matched against `aggregate.detectors_corpus`: spoke-on
    count, answerable-for count, the rate, and whether the detector ships enabled. A prose paragraph about
    a detector is allowed to be eloquent; a table row is a claim with four numbers in it.
    """
    doc = CONCEPT.read_text()
    detectors = report["aggregate"]["detectors_corpus"]
    assert len(detectors) == 12, (
        "the report must carry a row for every catalogue detector, fired or not: "
        f"{len(detectors)}"
    )
    checked = 0
    for line in doc.splitlines():
        if not line.startswith("| `"):
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) < 6 or cells[1] not in {"enabled", "**disabled**", "mixed"}:
            continue
        ids = re.findall(r"`([a-z_]+)`", cells[0])
        for detector_id in ids:
            row = detectors.get(detector_id)
            assert row is not None, f"{detector_id}: in the document's table, not in the report"
            if cells[1] != "mixed":
                assert cells[1] == (
                    "**disabled**" if not row["ships_enabled"] else "enabled"
                ), f"{detector_id}: the document says it ships {cells[1]}, the report says otherwise"
            if len(ids) == 1:
                assert (row["spoke"], row["answerable_for"]) == (int(cells[2]), int(cells[3])), (
                    f"{detector_id}: the document says it spoke on {cells[2]} and was answerable for "
                    f"{cells[3]}; the report says {row['spoke']} and {row['answerable_for']}"
                )
                printed = "—" if row["g3"] is None else f"{row['g3']:.3f}"
                assert cells[4] == printed, (
                    f"{detector_id}: the document prints G3 {cells[4]}, the report prints {printed}"
                )
            else:
                # A grouped row (the never-fired five) may not claim a rate for any of them.
                assert row["spoke"] == 0 and row["g3"] is None, (
                    f"{detector_id}: grouped into a 'raised nothing' row but the report has numbers for it"
                )
        checked += len(ids)
    assert checked == len(detectors), (
        f"the table covers {checked} of {len(detectors)} detectors; an unlisted detector is an unmade claim"
    )
    # Never-measured detectors must be called out as no evidence, not as clean.
    never = report["aggregate"]["defaults"]["never_fired"]
    assert never, "the corpus raised every detector? re-read before believing that"
    for row in never:
        assert row.split()[0] in doc, f"{row} is in the report and not in the document"


def test_the_gate_table_in_the_document_matches_the_report(report):
    """§2's table is parsed: one row per gate, and the status column is the report's status."""
    doc = CONCEPT.read_text()
    blessed = {g["id"]: g["status"] for g in report["gates"]}
    seen = []
    for line in doc.splitlines():
        if not line.startswith("| `"):
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        gate_id = cells[0].strip("`")
        if gate_id not in blessed:
            continue
        seen.append(gate_id)
        status = blessed[gate_id]
        assert any(
            re.search(rf"(?<![\w-]){re.escape(status)}(?![\w-])", cell.replace("*", ""))
            for cell in cells
        ), f"{gate_id}: the document reports {cells}, the report says {status}"
    assert sorted(seen) == sorted(blessed), (
        "every gate belongs in the written answer's table: "
        f"{sorted(set(blessed) ^ set(seen))}"
    )


def test_the_corpus_readme_describes_the_pins_it_ships(pins):
    readme = CORPUS_README.read_text()
    assert "truth.json" in readme and "pin.json" in readme
    # The caveat that an agent wrote these labels, stated in the corpus rather than only in a ticket.
    assert "agent" in readme and "not certify" in readme, (
        "the corpus README must say what agent-authored labels can and cannot certify"
    )
    for name, pin in pins.items():
        assert f"`{name}`" in readme, f"{name}: a pin nobody describes"
        assert pin["head"][:12] in readme, f"{name}: the README's commit does not match the pin"
        assert f"| {pin['window']} |" in readme, f"{name}: the README's window does not match the pin"


def test_the_milestone_finding_is_written_down(report):
    """`m1`'s beta criterion is `G3 ≤ 0.30` on three pinned repositories; the shortfall is stated, not implied."""
    doc = CONCEPT.read_text()
    assert "m1" in doc and "not met" in doc
    failing = {g["id"] for g in report["gates"] if g["status"] == "fail"}
    assert {"g3-alpha-at-corpus-cut", "miss-rate-at-published-tau"} <= failing
