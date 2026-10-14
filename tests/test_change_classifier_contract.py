"""`reco-dsh`'s acceptance criteria, as tests.

This file is the executable form of
[docs/concepts/change-classification.md](../docs/concepts/change-classification.md), in the
same relation as `tests/test_taxonomy_contract.py` is to `paths.md`: a clause of that
document with no test here is a clause that will drift. The four acceptance criteria map to
the sections below in order — accuracy on the labelled fixture set, reported (AC 1);
confidence plus a human-readable reason reachable from the UI (AC 2); ambiguous paths landing
in `other` rather than being guessed into `source` (AC 3); and per-blob caching that is
stable across runs (AC 4).

The rest is the cross-contract work that makes the classifier a *component* of the plan
rather than a module: the path-only classifier never contradicting `e0.9`'s table, the
analyzer manifest declaring every config key the payload reads, the noise vocabulary
agreeing with the store's, and the analysis half staying IO-free (ADR-002 §4.5).

Stdlib and pytest only; every fixture runs against the **built-in** tables with no
configuration, so a failure is a defect in the classifier or the marker table, never in
somebody's `fathom.toml`.
"""

from __future__ import annotations

import json
import pathlib
import subprocess
import sys
import time

import pytest

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "src"
sys.path.insert(0, str(SRC))

from recommender.change import cache as CACHE_MOD                                # noqa: E402
from recommender.change import corpus as CORPUS                                   # noqa: E402
from recommender.change import eval as EVAL                                        # noqa: E402
from recommender.change import evidence as EV                                      # noqa: E402
from recommender.change import explain as X                                        # noqa: E402
from recommender.change import markers as M                                        # noqa: E402
from recommender.change import policy as P                                         # noqa: E402
from recommender.change.cache import BlobCache, blob_oid, classification_key       # noqa: E402
from recommender.change.engine import (CLASSIFIER_VERSION, Change, ChangeClassifier,  # noqa: E402
                                       Classification)
from recommender.change.episode import Episode, classify_episode                    # noqa: E402
from recommender.store.cache import derive_key                                     # noqa: E402
from recommender.taxonomy import Taxonomy, default_taxonomy                        # noqa: E402
from recommender.taxonomy import classes as C                                      # noqa: E402
from recommender.taxonomy import corpus as TAX_CORPUS                              # noqa: E402
from recommender.taxonomy import noise as N                                        # noqa: E402

#: The marker table's digest. A pattern change without a version bump fails here, exactly as
#: `e0.9` pins its rule table: the honest options are "revert it" or "bump `MARKERS_VERSION`
#: and accept the cache invalidation", never "change the table quietly".
PINNED_MARKERS_DIGEST = "fad5868d4963b261"

#: Rows in the corpus that must be answered `other`, and so are the AC 3 fixtures.
MIN_ACCURACY = 0.95


@pytest.fixture(scope="module")
def report() -> EVAL.Report:
    return EVAL.score()


@pytest.fixture(scope="module")
def clf() -> ChangeClassifier:
    return ChangeClassifier(default_taxonomy())


def path_only() -> ChangeClassifier:
    return ChangeClassifier(default_taxonomy(), use_content=False, enable_cache=False)


def labelled():
    return [(shape, path, cls, content) for shape, path, cls, content in CORPUS.rows()]


# ===========================================================================
# AC 1 — labelled fixture set classified with at least 95 % accuracy, reported
# ===========================================================================
def test_the_labelled_set_is_classified_at_or_above_95_percent(report):
    assert report.total >= 150, "the fixture set is too small to mean anything"
    assert report.accuracy >= MIN_ACCURACY, (
        "accuracy %.3f below the bar:\n%s" % (report.accuracy, EVAL.report_text(report)))


def test_every_class_in_the_vocabulary_is_exercised():
    empty = [cls for cls, count in CORPUS.counts().items() if not count]
    assert not empty, "no fixture rows for %s" % ", ".join(empty)


def test_every_fixture_row_is_answered_and_labelled_from_a_real_shape():
    for shape in CORPUS.shapes():
        assert CORPUS.FIXTURES[shape].get("note"), "%s has no reason to exist" % shape
        assert CORPUS.FIXTURES[shape]["files"], "%s is empty" % shape
    for _shape, path, cls, _content in labelled():
        assert cls in C.PATH_CLASSES, path
        assert not path.startswith("/"), path


def test_misclassifications_are_printed_with_the_reason_they_got(report):
    """A red gate has to be diagnosable in the log, not merely red."""
    text = EVAL.report_text(report)
    assert "accuracy" in text and "PASS" in text if report.accuracy >= MIN_ACCURACY else "FAIL"
    for row in report.mistakes():
        assert row.path in text and row.reason in text


def test_the_fixture_set_measures_content_evidence_not_just_paths():
    """The proof that this ticket is tested: delete the content half and the score drops.

    A corpus answerable from the path alone would be `e0.9`'s corpus again, and the
    classifier could pass with `evidence.py` deleted.
    """
    rows = EVAL.score().content_dependent()
    assert len(rows) >= 12, "only %d rows need content evidence" % len(rows)
    full = EVAL.score().accuracy
    assert EVAL.score(path_only()).accuracy < full


def test_report_names_the_bar_it_is_measured_against(report):
    assert EVAL.MIN_ACCURACY == 0.95
    assert "bar 95%" in EVAL.report_text(report)


# ===========================================================================
# AC 2 — confidence plus a human-readable reason, reachable from the UI
# ===========================================================================
def test_every_decision_carries_a_confidence_and_a_sentence(report):
    for row in report.rows:
        assert 0.0 <= row.confidence <= 1.0, row.path
        assert row.band in P.BANDS, row.path
        assert len(row.reason) > 20, "%s has no readable reason" % row.path


def test_reasons_name_the_rule_or_marker_that_decided(report):
    """A reason that could have been printed by a coin flip is not a receipt."""
    for row in report.rows:
        head = row.reason
        assert ("rule" in head or "marker" in head or "bytes" in head or "parses" in head
                or "floor" in head or "content" in head), "%s: %s" % (row.path, head[:90])


def test_the_json_payload_holds_what_the_pane_needs(clf):
    decision = clf.classify_path("src/api.pb.go",
                                 "// Code generated by protoc-gen-go. DO NOT EDIT.\n"
                                 "package userpb\n")
    payload = decision.as_dict()
    for key in ("cls", "confidence", "band", "reason", "noise", "votes", "scores", "facts",
                "cache_key", "taxonomy_version", "markers_version"):
        assert key in payload, key
    assert json.dumps(payload, sort_keys=True)             # serialisable, no clock inside
    assert payload["winner"]["source"].startswith("marker:")
    assert payload["facts"]["markers"][0]["line"] == 1


def test_badge_and_detail_are_usable_in_the_panes_min_width(clf):
    """ADR-006 §6.2: the diff tree pane is grouped by taxonomy class, 28 ideal / 20 minimum."""
    for _shape, path, _cls, content in labelled():
        decision = clf.classify_path(path, content)
        assert len(decision.badge()) <= 20, decision.path
        assert len(decision.badge(width=20)) <= 20, decision.path
        assert decision.detail() == decision.reason
        assert decision.cls in decision.badge()


def test_explain_text_renders_confidence_reason_and_evidence(clf):
    decisions = [clf.classify_path("src/api.pb.go",
                                   "// Code generated by protoc-gen-go. DO NOT EDIT.\n"),
                 clf.classify_path("notes/.keep", ""),
                 clf.classify_path("Cargo.lock",
                                   "# This file is automatically @generated by Cargo.\n")]
    text = X.explain_text(decisions)
    for needle in ("CLASS", "confidence", "why", "evidence", "scores", "noise", "ui", "badge"):
        assert needle in text, needle
    table = X.table_text(decisions)
    assert table.splitlines()[0].startswith("path")
    assert len(table.splitlines()) == 1 + len(decisions)


def test_the_cli_verbs_run_and_agree_with_the_library():
    def run(*args):
        env = {"PYTHONPATH": str(SRC), "PATH": "/usr/bin:/bin"}
        return subprocess.run([sys.executable, "-m", "recommender.change", *args],
                              cwd=str(ROOT), env=env, capture_output=True, text=True)

    quiet = run("explain", "-q", "Cargo.lock", "src/a.py")
    assert quiet.returncode == 0, quiet.stderr
    assert "lockfile" in quiet.stdout
    scored = run("score", "--min", "0.95")
    assert scored.returncode == 0, scored.stdout[-500:]
    payload = json.loads(run("explain", "--json", "Cargo.lock").stdout)
    assert payload["decisions"][0]["cls"] == "lockfile"
    assert run("classes").stdout.count("\n") > 10
    assert "gen.protoc" in run("markers").stdout


# ===========================================================================
# AC 3 — ambiguous paths land in `other`, never guessed into `source`
# ===========================================================================
def test_ambiguous_paths_are_other_and_not_source():
    """Two honest claims, no basis to choose: the answer is `other`, and both are printed."""
    import json as _json

    clf = ChangeClassifier(default_taxonomy())
    body = _json.dumps({("key_%02d" % i): {"a": i} for i in range(24)}, separators=(",", ":"))
    ambiguous = clf.classify_path("misc/dump.qqq", body)
    assert ambiguous.cls == C.PathClass.OTHER, ambiguous.reason
    assert ambiguous.band == P.BAND_AMBIGUOUS
    assert "within" in ambiguous.reason, ambiguous.reason
    assert "generated" in ambiguous.reason and "config" in ambiguous.reason


def test_a_path_with_no_rule_and_no_content_evidence_is_other():
    clf = ChangeClassifier(default_taxonomy())
    for path, content in (("weird.thing", "just some prose, honestly\n"),
                          ("misc/.keep", ""),
                          ("bin/mystery", "Talked to the platform team about the bucket.\n"),
                          ("misc/blob.qqq", "\x01\x02 unlabelled bytes\n")):
        decision = clf.classify_path(path, content)
        assert decision.cls == C.PathClass.OTHER, (path, decision.cls, decision.reason)


def test_source_is_never_the_default_answer():
    """The structural form of AC 3: `source` requires an affirmative claim."""
    clf = ChangeClassifier(default_taxonomy())
    for _shape, path, _cls, content in labelled():
        decision = clf.classify_path(path, content)
        if decision.cls != C.PathClass.SOURCE:
            continue
        affirmative = [v for v in decision.votes
                       if v["class"] == C.PathClass.SOURCE
                       and v["weight"] >= P.CONFIDENCE_FLOOR]
        assert affirmative, "%s became source with no affirmative claim: %s" % (path,
                                                                                decision.reason)


def test_the_fallback_rule_can_never_produce_source():
    tax = Taxonomy.from_dict({})
    clf = ChangeClassifier(tax)
    for path in ("a.zzz", "b/c/q", "d/e/f.weird", "g"):
        assert clf.classify_path(path, "").cls == C.PathClass.OTHER, path


def test_conflicting_claims_produce_other_with_both_named():
    result = P.combine([P.Vote(cls="asset", weight=0.60, source="shape:data", kind="content"),
                        P.Vote(cls="config", weight=0.55, source="shape:config",
                               kind="content")])
    assert result.cls == C.PathClass.OTHER
    assert result.band == P.BAND_AMBIGUOUS
    assert result.confidence < P.CONFIDENCE_FLOOR
    assert len(result.scores) == 2


def test_low_confidence_is_reported_as_low_confidence(clf):
    episode = classify_episode(Episode.from_files("ep-x", {
        "src/a.py": "import os\n",
        "weird.blob": "nothing at all\n",
    }))
    low = episode.low_confidence()
    assert any(d.path == "weird.blob" for d in low)
    assert episode.unclassified()[0].path == "weird.blob"


def test_a_generated_file_under_vendor_stays_vendored(clf):
    """A convention about the tree is not refuted by a header; the claim is still printed."""
    decision = clf.classify_path(
        "vendor/legacy/x.pb.go",
        "// Code generated by protoc-gen-go. DO NOT EDIT.\npackage legacy\n")
    assert decision.cls == C.PathClass.VENDORED, decision.reason
    assert any(v["source"] == "marker:gen.protoc" for v in decision.votes), decision.votes


# ===========================================================================
# AC 4 — cached per blob, stable across runs
# ===========================================================================
def test_the_same_blob_at_the_same_path_is_a_cache_hit():
    clf = ChangeClassifier(default_taxonomy())
    change = Change(path="src/a.py", content="x = 1\n")
    first, second = clf.classify(change), clf.classify(change)
    assert not first.cached and second.cached
    assert first.as_dict() == second.as_dict()
    assert clf.stats()["counters"]["cache_hit"] == 1


def test_the_key_names_the_path_the_blob_and_both_tables():
    clf = ChangeClassifier(default_taxonomy())
    oid = blob_oid("x = 1\n")
    base = classification_key(path="src/a.py", oid=oid, taxonomy=clf.taxonomy)
    other_path = classification_key(path="src/b.py", oid=oid, taxonomy=clf.taxonomy)
    other_blob = classification_key(path="src/a.py", oid=blob_oid("y = 2\n"),
                                    taxonomy=clf.taxonomy)
    other_markers = classification_key(path="src/a.py", oid=oid, taxonomy=clf.taxonomy,
                                       markers_digest="deadbeef")
    assert len({base.raw, other_path.raw, other_blob.raw, other_markers.raw}) == 4
    assert base.raw.startswith("ck1:")
    other_table = Taxonomy.from_dict({"taxonomy": {"classes": {"source": ["src/a.py"]}}})
    other_table_key = classification_key(path="src/a.py", oid=oid, taxonomy=other_table,
                                         config={"taxonomy": {"classes":
                                                             {"source": ["src/a.py"]}}})
    assert other_table_key.raw != base.raw


def test_the_cache_entry_is_scoped_per_blob_as_adr_004_says():
    """`taxonomy.path` is `blob`-scoped but keyed by path *and* content; so is this."""
    analyzer = CACHE_MOD.CLASSIFIER_ANALYZER
    assert analyzer.scope_kind == "blob"
    assert analyzer.id == "change.classify"
    assert "taxonomy.path" in analyzer.depends_on, "the cascade must reach this analyzer"
    assert analyzer in CACHE_MOD.ANALYZER_SET, "it must resolve inside the manifest set"
    assert CACHE_MOD.CACHE_VERSION == CLASSIFIER_VERSION


def test_a_taxonomy_version_bump_invalidates_through_the_dependency_cascade():
    clf = ChangeClassifier(default_taxonomy())
    key = classification_key(path="src/a.py", oid=blob_oid("x = 1\n"), taxonomy=clf.taxonomy)
    bumped = derive_key(analyzer_id=CACHE_MOD.CLASSIFIER_ANALYZER.id,
                        input_hash=key.input_hash, input_kind="blob",
                        versions={"taxonomy.path": 99}, config={},
                        analyzers=CACHE_MOD.ANALYZER_SET)
    assert bumped.raw != key.raw
    assert bumped.dep_digest != key.dep_digest


def test_the_declared_config_keys_are_the_ones_that_move_an_answer():
    """ADR-004 §5.5: the key names everything the payload reads, and nothing it does not."""
    from recommender.taxonomy import rules as R

    declared = set(CACHE_MOD.CONFIG_KEYS)
    read = set(R.CLASS_KEYS) | {R.CLASS_MAP_KEY.replace("<class-name>", ""),
                                "noise.builtins_version"}
    assert read <= declared, declared ^ read
    tax_a = Taxonomy.from_dict({"test": {"globs": ["special/**"]}})
    tax_b = Taxonomy.from_dict({})
    key_a = classification_key(path="special/x.py", oid=blob_oid("x = 1\n"), taxonomy=tax_a,
                               config={"test": {"globs": ["special/**"]}})
    key_b = classification_key(path="special/x.py", oid=blob_oid("x = 1\n"), taxonomy=tax_b,
                               config={})
    assert key_a.raw != key_b.raw
    assert tax_a.classify("special/x.py").cls != tax_b.classify("special/x.py").cls


def test_cache_bytes_are_identical_across_two_processes():
    script = (
        "import json,sys;sys.path.insert(0,'src');"
        "from recommender.change import ChangeClassifier, Change;"
        "from recommender.change.cache import BlobCache;"
        "c=ChangeClassifier.default();"
        "rows=[(p,ct) for _s,p,_e,ct in __import__('recommender.change.corpus',fromlist=['x'])"
        ".rows()];"
        "[c.classify(Change(path=p,content=ct)) for p,ct in rows];"
        "sys.stdout.write(BlobCache.decode(c.cache.encode()).encode().decode())")
    env = {"PYTHONPATH": str(SRC), "PATH": "/usr/bin:/bin"}
    first = subprocess.run([sys.executable, "-c", script], cwd=str(ROOT), env=env,
                           capture_output=True, text=True)
    second = subprocess.run([sys.executable, "-c", script], cwd=str(ROOT), env=env,
                            capture_output=True, text=True)
    assert first.returncode == 0, first.stderr[-400:]
    assert first.stdout == second.stdout and len(first.stdout) > 1000
    payload = json.loads(first.stdout)
    assert payload["analyzer"] == "change.classify"
    assert len(payload["entries"]) == len(json.loads(second.stdout)["entries"])


def test_the_cache_is_bounded_and_round_trips():
    cache = BlobCache(limit=8)
    for i in range(20):
        cache.put("ck1:%d" % i, {"cls": "source", "confidence": 0.7})
    assert len(cache.entries) <= 8
    restored = BlobCache.decode(cache.encode())
    assert restored.entries == cache.entries
    with pytest.raises(ValueError):
        BlobCache.decode(json.dumps({"analyzer": "other"}))


def test_classification_survives_a_round_trip_through_json(clf):
    decision = clf.classify_path("src/api.pb.go",
                                 "// Code generated by protoc-gen-go. DO NOT EDIT.\n")
    again = Classification.from_dict(json.loads(json.dumps(decision.as_dict())))
    assert again.cls == decision.cls and again.confidence == decision.confidence
    assert again.reason == decision.reason and again.cached is True


# ===========================================================================
# the ladder: who outranks whom, asserted rather than hoped
# ===========================================================================
def test_the_precedence_ladder_is_strictly_ordered():
    assert P.W_ASSERTION > P.W_PATH_CONVENTION > P.W_BINARY > M.DECISIVE
    assert P.DECISIVE == M.DECISIVE, "the marker band and the policy band must be one number"
    assert P.W_BUILTIN_BASE + P.W_SPECIFICITY_MAX < P.DECISIVE
    assert P.W_EXTENSION < P.W_BUILTIN_BASE
    assert P.W_BUILD_MEMBER < P.W_BUILTIN_BASE
    assert P.CONFIDENCE_FLOOR > P.AMBIGUITY_GAP


def test_an_assertion_in_fathom_toml_outranks_content_evidence():
    """The promise `paths.md` §9 made: content sniffing must stay overrulable by a human."""
    tax = Taxonomy.from_dict({"taxonomy": {"classes": {"source": ["src/protos/**"]}}})
    clf = ChangeClassifier(tax)
    decision = clf.classify_path("src/protos/api.py",
                                 "// Code generated by protoc-gen-go. DO NOT EDIT.\n")
    assert decision.cls == C.PathClass.SOURCE, decision.reason
    assert decision.band == P.BAND_CONFIRMED
    assert "configuration" in decision.reason or "asserted" in decision.reason


def test_a_generated_banner_outranks_the_builtin_path_rule():
    clf = ChangeClassifier(default_taxonomy())
    decision = clf.classify_path("src/client.py",
                                 "# This file was automatically generated. Do not edit.\n"
                                 "class Client: pass\n")
    assert decision.cls == C.PathClass.GENERATED
    assert decision.overrode_path and decision.path_class == C.PathClass.SOURCE
    assert decision.noise == N.Noise.GENERATED, "noise follows the class claim that won"


def test_noise_follows_the_class_when_content_moves_it():
    clf = ChangeClassifier(default_taxonomy())
    decision = clf.classify_path("src/api.py",
                                 "// Code generated by protoc-gen-go. DO NOT EDIT.\n")
    assert decision.noise == "generated" and decision.noise_score > 0
    assert decision.noise_rule == "class:generated"


def test_a_config_noise_demotion_travels_with_the_path_answer():
    tax = Taxonomy.from_dict({"noise": {"globs": ["src/vendorish/**"]}})
    clf = ChangeClassifier(tax)
    decision = clf.classify_path("src/vendorish/a.py", "class A: pass\n")
    assert decision.noise == N.Noise.UNKNOWN and decision.cls == C.PathClass.SOURCE
    assert decision.noise in N.NOISE_LEVELS


def test_no_answer_is_ever_outside_the_store_vocabulary(clf, report):
    for _shape, path, _cls, content in labelled():
        decision = clf.classify_path(path, content)
        assert decision.cls in C.PATH_CLASSES
        assert decision.noise in N.NOISE_LEVELS, (path, decision.noise)


# ===========================================================================
# the marker table
# ===========================================================================
def test_marker_table_is_clean_and_versioned():
    ids = [marker.id for marker in M.markers()]
    assert len(ids) == len(set(ids)), "duplicate marker id"
    for marker in M.markers():
        assert marker.cls in C.PATH_CLASSES, marker.id
        assert marker.note, "%s has no note a reviewer could read" % marker.id
        assert marker.literals or marker.regex, marker.id
        assert 0 < marker.weight <= 1
    assert M.by_id("gen.protoc") is not M.by_id("nope")


def test_marker_digest_is_pinned_and_stable():
    """A marker-table change without a version bump shows up here first.

    The same bargain as `e0.9`'s table digest: revert it, or bump `MARKERS_VERSION` and
    accept the cache invalidation the change buys. What is not available is changing the
    table quietly, because a cached classification quotes this digest in its key.
    """
    assert M.digest() == PINNED_MARKERS_DIGEST, M.digest()
    assert M.digest() == M.digest()
    assert len(M.digest()) == 16
    assert M.MARKERS_VERSION == 1


def test_markers_only_read_the_head_of_the_file():
    banner = "// Code generated by protoc-gen-go. DO NOT EDIT.\n"
    padding = "\n".join("// filler line %d" % i for i in range(80))
    assert M.scan(padding + banner) == []
    assert M.scan(banner) and M.scan(banner)[0].id == "gen.protoc"
    assert len(M.head_of("x" * 100_000)) <= M.HEAD_BYTES


@pytest.mark.parametrize("text,expected", [
    ("v 0.1 0.2 0.3\nvn 0 1 0\nf 1/1/1 2/2/2\n", "asset.mesh"),
    ("-- +goose Up\nALTER TABLE t ADD COLUMN c TEXT;\n", "mig.goose"),
    ("-- +migrate Up\nSELECT 1;\n", "mig.bun"),
    ("Revision ID: abc\ndown_revision = None\n", "mig.alembic"),
    ('{"lockfileVersion": 3}', "lock.npm"),
    ("# yarn lockfile v1\n\nreact@18.3.1:\n", "lock.yarn"),
    ("<!-- Generated by sphinx. Do not edit this file manually. -->\n", "gen.by_hand"),
    ("// GENERATED CODE - DO NOT MODIFY BY HAND\n", "gen.banner"),
    ("<auto-generated>\n// This code was generated by a tool.\n", "gen.dotnet"),
    ("[Fact]\npublic void Runs() {}\n", "test.csharp"),
    ("#!/usr/bin/env bash\nset -euo pipefail\n", "src.shebang"),
    ('syntax = "proto3";\npackage a;\n', "schema.protobuf"),
])
def test_the_markers_that_carries_a_class_are_fired_by_that_content(text, expected):
    hits = M.scan(text)
    assert hits, "no marker matched %r" % text[:40]
    assert expected in [hit.id for hit in hits], [hit.id for hit in hits]


def test_a_marker_hit_is_quoted_line_numbered_and_weighted():
    hit = M.scan("// Code generated by protoc-gen-go. DO NOT EDIT.\nx\n")[0]
    assert hit.line == 1 and hit.cls == C.PathClass.GENERATED and hit.weight >= M.DECISIVE
    assert "protoc" in hit.quote()
    assert set(hit.as_dict()) == {"marker", "class", "weight", "matched", "line", "note"}


# ===========================================================================
# the evidence layer
# ===========================================================================
def test_bytes_facts_are_tolerant_never_thrown():
    facts = EV.inspect_bytes("weird.json", b'{"broken": ')
    assert facts.is_text and facts.notes, "a malformed document must be a note, not a raise"
    binary = EV.inspect_bytes("a.bin", b"\x00\x01\x02binary")
    assert not binary.is_text and "NUL" in binary.reason_text
    empty = EV.inspect_bytes("empty.py", "")
    assert empty.available and empty.markers == []
    missing = EV.inspect_bytes("gone.py", None)
    assert not missing.available
    assert EV.votes_from(missing, path="gone.py") == EV.path_hints("gone.py")
    assert not EV.content_votes(missing)


def test_magic_and_extension_agree_on_which_kind_of_non_text_a_file_is():
    """`refine_not_text`: bytes say "not text", the table says *which* non-text class."""
    clf = ChangeClassifier(default_taxonomy())
    assert clf.classify_path("assets/logo.unknown", b"\x89PNG\r\n\x1a\n" + b"\x00" * 8).cls \
        == C.PathClass.ASSET
    assert clf.classify_path("packs/blob.unknown", b"\x7fELF\x02\x01" + b"\x00" * 8).cls \
        == C.PathClass.BINARY
    votes = P.refine_not_text([P.Vote(cls=C.PathClass.BINARY, weight=0.94, source="bytes:nul",
                                      note="NUL")], C.PathClass.ASSET)
    assert votes[0].cls == C.PathClass.ASSET and votes[0].weight < 0.94


def test_minification_and_source_maps_read_as_generated():
    body = "var a={};function b(c){return c}" * 40 + "//# sourceMappingURL=x.js.map\n"
    decision = ChangeClassifier(default_taxonomy()).classify_path("assets/analytics.xjs", body)
    assert decision.cls == C.PathClass.GENERATED, decision.reason
    assert decision.facts["minified"] and decision.facts["source_map"]


def test_build_config_membership_is_evidence_about_the_repository():
    files = {"CMakeLists.txt": "add_executable(fathom src/main.c src/engine/render.c)\n",
             "src/main.c": "#include <stdio.h>\nint main(){return 0;}\n",
             "src/engine/render.c": "int render(void){return 1;}\n",
             "src/orphan.weird": "nothing names me\n"}
    index = EV.BuildIndex.from_files(files)
    assert index.references("src/engine/render.c") == ("CMakeLists.txt",)
    assert index.references("src/orphan.weird") == ()
    clf = ChangeClassifier(default_taxonomy(), build_index=index)
    rescued = Change(path="src/engine/render.c", content=files["src/engine/render.c"])
    assert clf.classify(rescued).cls == C.PathClass.SOURCE
    # a file only the build config vouches for is a weak claim, not a confident one
    via_build = clf.classify(Change(path="src/orphan.weird", content="nothing names me\n"))
    assert via_build.cls == C.PathClass.OTHER


def test_extension_hints_corroborate_but_never_overrule_the_table():
    clf = ChangeClassifier(default_taxonomy())
    decision = clf.classify_path("dist/bundle.js", "var a=1;\n")
    assert decision.cls == C.PathClass.GENERATED, "`.js` must not out-shout `dist/`"
    assert any(v["source"] == "ext:.js" for v in decision.votes)


# ===========================================================================
# episodes
# ===========================================================================
def test_an_episode_aggregates_without_losing_a_path():
    clf = ChangeClassifier(default_taxonomy())
    episode = Episode.from_files("ep-1", {
        "src/a.py": "import os\n",
        "src/b.py": "import sys\n",
        "tests/test_a.py": "def test_a():\n    assert 1\n",
        "Cargo.lock": "# This file is automatically @generated by Cargo.\nversion = 4\n",
        "db/migrations/0001.sql": "-- +goose Up\nCREATE TABLE t (a TEXT);\n",
    })
    result = classify_episode(episode, clf)
    assert len(result.decisions) == 5
    counts = result.class_counts()
    assert sum(counts.values()) == 5
    assert counts[C.PathClass.SOURCE] == 2 and counts[C.PathClass.MIGRATION] == 1
    assert 0 < result.test_surface_ratio() < 1
    assert 0 < result.noise_share() < 1
    assert not result.is_noise_only()
    assert "5 paths" in result.summary_line() and "ep-1" in result.summary_line()


def test_a_noise_only_episode_says_so_for_e2_2():
    episode = Episode.from_files("ep-noise", {
        "Cargo.lock": "# This file is automatically @generated by Cargo.\nversion = 4\n",
        "dist/app.min.js": "var a=1;" * 60,
    })
    result = classify_episode(episode, ChangeClassifier(default_taxonomy()))
    assert result.is_noise_only() and result.noise_share() > 0.7


def test_episode_output_is_deterministic_bytes():
    files = {"src/a.py": "import os\n", "weird.zzz": "hmm\n",
             "Cargo.lock": "# This file is automatically @generated by Cargo.\n"}
    first = json.dumps(classify_episode(Episode.from_files("ep", files)).as_dict(),
                       sort_keys=True)
    second = json.dumps(classify_episode(Episode.from_files("ep", dict(reversed(
        list(files.items()))))).as_dict(), sort_keys=True)
    assert first == second


def test_every_decisions_scores_are_sorted_and_bounded(clf):
    for _shape, path, _cls, content in labelled()[:60]:
        decision = clf.classify_path(path, content)
        scores = [score for _cls, score in decision.scores]
        assert scores == sorted(scores, reverse=True), path
        assert all(0 <= score <= 1 for score in scores), path


# ===========================================================================
# cross-contract: this classifier against the rest of the plan
# ===========================================================================
def test_the_classifier_never_disagrees_with_the_path_table_when_content_is_off():
    """Switch the content half off and the answer must be exactly `e0.9`'s.

    If it were not, the classifier would be a second opinion about what `vendor/` means —
    the duplication `paths.md` §1.1 exists to prevent.
    """
    tax = default_taxonomy()
    clf = path_only()
    for _shape, path, _cls, _content in labelled():
        assert clf.classify_path(path).cls == tax.classify(path).cls, path
    for _shape, path, _cls, _noise in TAX_CORPUS.rows():
        assert clf.classify_path(path).cls == tax.classify(path).cls, path


def test_the_analysis_half_of_the_package_is_io_free():
    """ADR-002 §4.5, mechanically: only `cli.py` may read files, env or subprocesses.

    Checked on the syntax tree rather than with a text grep, because the honest reason a
    module is pure is written in its docstring, and a grep punishes the documentation.
    """
    import ast

    forbidden_imports = {"os", "subprocess", "pathlib", "time", "datetime", "socket"}
    pure = ["markers.py", "evidence.py", "policy.py", "engine.py", "episode.py", "cache.py",
            "corpus.py", "eval.py", "explain.py"]
    for name in pure:
        path = SRC / "recommender" / "change" / name
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        for node in ast.walk(tree):
            if isinstance(node, ast.Import):
                for alias in node.names:
                    root = alias.name.split(".")[0]
                    assert root not in forbidden_imports, "%s imports %s" % (name, alias.name)
            elif isinstance(node, ast.ImportFrom):
                root = (node.module or "").split(".")[0]
                assert root not in forbidden_imports, "%s imports from %s" % (name, node.module)
            elif isinstance(node, ast.Call):
                func = node.func
                text = getattr(func, "id", "") or getattr(func, "attr", "")
                assert text != "open", "%s opens a file" % name
                assert text != "system", "%s runs a program" % name
        assert "os.environ" not in path.read_text(encoding="utf-8").split(
            '"""', 2)[-1], "%s reads the environment" % name
    cli_text = (SRC / "recommender" / "change" / "cli.py").read_text(encoding="utf-8")
    assert "Path(" in cli_text, "the CLI is the half that reads files"


def test_no_module_below_the_classifier_invents_its_own_taxonomy():
    """`e2.1` consumes the table; it does not get a second vocabulary of classes."""
    for name in ("engine.py", "evidence.py", "policy.py", "episode.py"):
        text = (SRC / "recommender" / "change" / name).read_text(encoding="utf-8")
        assert 'cls="source"' not in text and "PathClass.SOURCE, [" not in text
    assert set(P.CLASS_PRIORITY) == set(C.PATH_CLASSES)


def test_the_classifier_is_fast_enough_to_run_per_changed_file(clf):
    """The same budget argument `e0.9` makes: analysis is per file, inside an index run."""
    rows = [(path, content) for _s, path, _c, content in labelled()]
    cold = ChangeClassifier(default_taxonomy(), enable_cache=False)
    t0 = time.perf_counter()
    for path, content in rows:
        cold.classify_path(path, content)
    per_file = (time.perf_counter() - t0) / len(rows)
    assert per_file < 0.001, "one file took %.3f ms" % (per_file * 1000)


def test_repo_config_is_not_read_by_the_classifier_itself():
    """Only the CLI resolves configuration; the engine takes it as a value."""
    clf = ChangeClassifier(Taxonomy.from_dict({}))
    assert clf.config_values == {}
    tax = Taxonomy.from_dict({"taxonomy": {"classes": {"test": ["src/special/**"]}}})
    other = ChangeClassifier(tax)
    assert other.classify_path("src/special/a.py", "x = 1\n").cls == C.PathClass.TEST
    assert clf.classify_path("src/special/a.py", "x = 1\n").cls == C.PathClass.SOURCE
