"""ADR-003's acceptance criteria, as tests.

Like `tests/test_storage_contract.py` for ADR-004 and `tests/test_harness_contract.py` for ADR-005, this
file *is* the executable form of the document: a clause of
[docs/adr/0003-episodes.md](../docs/adr/0003-episodes.md) with no test here is a clause that will drift.

What makes this file different from the other two is what it reads. ADR-003's rules are implemented in
**two** languages — `crates/fathom-ingest/src/segment.rs` (normative) and `src/recommender/segment.py`
(the second opinion that makes "the same rule" mean something) — and stored in **two** schema copies. So
these tests parse the ADR's own tables and diff them against all four places, rather than restating the
numbers in Python and calling that a check. A table in prose that nothing parses is a table that disagrees
with the code by the next quarter.

Run from the repository root:

    python3 -m pytest -q tests/test_episode_identity_contract.py -v

Stdlib + git only, deliberately: an identity contract that needs a build step is one that gets skipped,
and skipped is how a rewrite starts orphaning sign-offs.
"""

from __future__ import annotations

import json
import pathlib
import re

import pytest

from recommender import segment as S

ROOT = pathlib.Path(__file__).resolve().parent.parent
ADR = ROOT / "docs" / "adr" / "0003-episodes.md"
RUST_SEGMENT = ROOT / "crates" / "fathom-ingest" / "src" / "segment.rs"
RUST_STORE = ROOT / "crates" / "fathom-store" / "src" / "episode.rs"
RUST_INDEX = ROOT / "crates" / "fathom-analyze" / "src" / "episodes.rs"
PY_STORE_SCHEMA = ROOT / "src" / "recommender" / "store" / "schema.py"
PY_STORE_GIT = ROOT / "src" / "recommender" / "store" / "git.py"
CONFIG_SPEC = ROOT / "src" / "recommender" / "config" / "schema.py"
JSON_SCHEMA = ROOT / "schemas" / "fathom.schema.json"
REPO_CONFIG = ROOT / "fathom.toml"
ADR_README = ROOT / "docs" / "adr" / "README.md"
ROOT_README = ROOT / "README.md"
THESIS = ROOT / "docs" / "concepts" / "thesis.md"

#: The rule-name spellings Rust uses in code (`Rule::TimeGap`) vs the wire (`time-gap`).
RUST_VARIANT = {
    "override": "Override",
    "hook": "Hook",
    "trailer": "Trailer",
    "branch": "Branch",
    "author": "Author",
    "time-gap": "TimeGap",
    "message-shape": "MessageShape",
    "root": "Root",
}


# ---------------------------------------------------------------------------
# the ADR's tables, parsed
# ---------------------------------------------------------------------------
def _text(path: pathlib.Path) -> str:
    return path.read_text(encoding="utf-8")


def _rows(text: str, anchor: str) -> list[list[str]]:
    """The data rows of the first markdown table after `anchor`, header and rule dropped."""
    after = text.split(anchor, 1)
    assert len(after) == 2, "anchor %r not found in the ADR" % anchor
    rows: list[list[str]] = []
    header_seen = False
    for raw in after[1].splitlines():
        line = raw.strip()
        if line.startswith("//!"):        # a Rust doc-comment table, which this file also diffs
            line = line[3:].strip()
        if line.startswith("|"):
            cells = [c.strip() for c in line.strip("|").split("|")]
            if all(set(c) <= set("-: ") for c in cells):
                continue  # the header rule
            if not header_seen:
                header_seen = True   # the header row itself
                continue
            rows.append(cells)
        elif header_seen and rows:
            break
    assert rows, "no table found after %r" % anchor
    return rows


def adr_boundary_table() -> list[dict]:
    out = []
    for cells in _rows(_text(ADR), "### 4.2 The cascade"):
        out.append({"pri": int(cells[0]), "rule": cells[1].strip("`"), "class": cells[2],
                    "fires": cells[3], "conf": float(cells[4])})
    return out


def adr_relink_table() -> list[dict]:
    out = []
    for cells in _rows(_text(ADR), "### 6.2 The tiers, and the tie-breakers"):
        out.append({"tier": int(cells[0]), "method": cells[1].strip("`"), "evidence": cells[2],
                    "relation": cells[3].strip("`"), "score": float(cells[4])})
    return out


def adr_vectors() -> list[dict]:
    out = []
    for cells in _rows(_text(ADR), "## Appendix A"):
        paths = [p.strip().strip("`") for p in cells[3].split(",")]
        subject = cells[2].split("→")[0].strip().strip("`")
        out.append({"author": cells[1].strip("`"), "subject": subject, "paths": paths,
                    "fingerprint": cells[4].strip("`"), "id": cells[5].strip("`")})
    return out


def adr_vocabularies() -> dict[str, list[str]]:
    out = {}
    for cells in _rows(_text(ADR), "## Appendix B"):
        name = re.sub(r"\s*\(§[^)]*\)", "", cells[0]).strip()
        out[name] = [v.strip().strip("`") for v in cells[2].split(",")]
    return out


def adr_creation_sources() -> dict[str, float]:
    out = {}
    for cells in _rows(_text(ADR), "### 9.3 Provenance"):
        out[cells[0].strip("`")] = float(cells[1])
    return out


@pytest.fixture(scope="module")
def adr() -> str:
    return _text(ADR)


# ---------------------------------------------------------------------------
# 1. the document itself carries the ticket's clauses
# ---------------------------------------------------------------------------
def test_adr003_carries_every_clause_the_ticket_asked_for(adr):
    # The acceptance criteria, as a checklist a reviewer can actually run with `grep`.
    required = {
        "amend worked example": "### 5.1",
        "rebase worked example": "### 5.4",
        "reword worked example": "### 5.5",
        "squash worked example": "### 5.6",
        "merge worked example": "### 5.8",
        "cherry-pick worked example": "### 5.9",
        "the stated invariant": "**I1**",
        "boundary tie-breakers": "**TB1**",
        "boundary tie-breakers, last": "**TB10**",
        "relink tie-breakers": "**RL1**",
        "relink's hard limit": "**RL0 —",
        "provisional episode decision": "## 7. Provisional episodes",
        "interleaved human decision": "## 8. A human commit between agent commits",
        "sign-off from the harness owner": "### 9.4 Sign-off",
        "what was not approved": "## 14. What this ADR moved, instead of approving",
        "verification section": "## 17. Verification",
    }
    missing = {name: token for name, token in required.items() if token not in adr}
    assert not missing, "ADR-003 is missing: %s" % ", ".join(missing)


def test_the_invariant_is_stated_in_words_the_ticket_used(adr):
    # Criterion 2, verbatim enough that a future edit cannot soften it into a wish.
    assert "stable across `git rebase -i` of the same content" in adr
    assert "member SHAs" in adr and "extent" in adr, "the ticket's other half is named, not skipped"
    assert "fuzzy" in adr, "the declined half of the ticket's identity scheme is addressed"


# ---------------------------------------------------------------------------
# 2. the identity rule: prose == Rust framing == Python == published vectors
# ---------------------------------------------------------------------------
def test_the_identity_formula_in_the_prose_is_the_formula_in_the_code(adr):
    block = adr.split("```", 2)[1]
    for line in ("fingerprint            = sha256(", "fallback_fingerprint   = sha256(",
                 "shape_fingerprint      = sha256("):
        assert line in block, "§2.2 must publish all three digest formulas"
    # Rust: the namespace, the framing order, the id prefix and length.
    rust = _text(RUST_SEGMENT)
    assert 'pub const IDENTITY_SCHEMA: &str = "fathom-episode/1";' in rust
    framing = [
        "digest.update(IDENTITY_SCHEMA.as_bytes());",
        "digest.update(author_identity.as_bytes());",
        "digest.update(subject.as_bytes());",
        "digest.update(b\"fallback\");",
        "digest.update(b\"shape\");",
    ]
    earlier = framing[0]
    for want in framing[1:]:
        assert want in rust, "`identity()` no longer contains %r" % want
        assert rust.index(earlier) < rust.index(want) or want in framing[-2:], (
            "the framing order changed; the digests and Appendix A moved with it")
    assert 'format!("ep-{}", &fingerprint[..20' in rust, "the id is `ep-` + 20 hex (ADR-003 §2.3)"
    assert S.IDENTITY_SCHEMA == "fathom-episode/1"
    assert S.ID_PREFIX == "ep-" and S.ID_HEX == 20


def test_the_published_vectors_in_appendix_a_are_reproducible():
    """The ADR publishes digests; the Python implementation must produce exactly them.

    This is the cross-implementation half of the identity rule: the Rust side asserts the same
    vectors in `identity_matches_the_published_golden_vector`, so neither side gets to define
    "same episode" on its own.
    """
    vectors = adr_vectors()
    assert len(vectors) >= 3, "Appendix A should publish at least the three documented vectors"
    for i, v in enumerate(vectors, start=1):
        got = S.identity(v["author"], v["subject"], v["paths"])
        assert got["fingerprint"] == v["fingerprint"], (
            "vector %d: the ADR says %s, the implementation says %s — one of them is wrong and the "
            "user's store does not care which" % (i, v["fingerprint"], got["fingerprint"]))
        assert S.episode_id(got["fingerprint"]) == v["id"], "vector %d: id mismatch" % i


def test_normalisation_is_part_of_the_identity_contract():
    # §2.1's `normalize`, pinned rather than implied — a fixup must not move an id, and a reword must.
    cases = {
        "fixup! feat(store): Make Epochs  SAFE": "make epochs safe",
        "squash! chore: Tidy  up": "tidy up",
        "amend! docs: X": "x",
        "feat(store)!: Refactor the Store": "refactor the store",
        "FIX: The Crash": "the crash",
        "  plain   subject  ": "plain subject",
    }
    for raw, want in cases.items():
        assert S.normalize_subject(raw) == want, raw


def test_rebase_of_the_same_content_does_not_move_the_id():
    """Invariant I1, exercised rather than asserted in prose.

    A rebase gives every commit in the range a new oid, a new date and a new parent. None of those
    is an identity input, so the ids must be unchanged — and none of the *digests* moves either,
    which is why no relink pass is needed for the common case at all.
    """
    before = S.identity("dev@example.invalid", "feat: epoch column", ["src/a.rs"])
    after = S.identity("dev@example.invalid", "feat: epoch column", ["src/a.rs"])
    assert before == after, "identity is a pure function of the preserved facts (I1, I3)"

    # An amend that also stages a file: the strong digest moves, tier 1's digest does not.
    amended = S.identity("dev@example.invalid", "feat: epoch column", ["src/a.rs", "src/b.rs"])
    assert amended["fingerprint"] != before["fingerprint"]
    assert amended["fallback_fingerprint"] == before["fallback_fingerprint"]

    # A reword: the mirror image, which is why there is a third digest.
    reworded = S.identity("dev@example.invalid", "feat: the epoch column, safely", ["src/a.rs"])
    assert reworded["fingerprint"] != before["fingerprint"]
    assert reworded["fallback_fingerprint"] != before["fallback_fingerprint"]
    assert reworded["shape_fingerprint"] == before["shape_fingerprint"]

    # `--amend --reset-author` moves everything (§2.6, §5.3): fail-visible, not silently trusted.
    reset = S.identity("someone.else@example.invalid", "feat: epoch column", ["src/a.rs"])
    assert reset["fingerprint"] != before["fingerprint"]
    assert reset["fallback_fingerprint"] != before["fallback_fingerprint"]
    assert reset["shape_fingerprint"] != before["shape_fingerprint"]

    # Identity ignores membership: a second commit joining the episode changes nothing (I3).
    assert S.identity("dev@example.invalid", "feat: epoch column", ["src/a.rs"]) == before


def test_the_twin_rule_disambiguates_without_merging(adr):
    """TB10 / §5.9: same digest, two live episodes, two ids, two reviews."""
    d = S.identity("dev@example.invalid", "fix: the crash", ["src/a.rs"])
    first, second, third = (S.episode_id(d["fingerprint"], n) for n in (1, 2, 3))
    assert first == "ep-82b50d5356e8f59a3473"
    assert second == first + "-2" and third == first + "-3"
    assert "**TB10**" in adr and "cherry-pick" in adr


# ---------------------------------------------------------------------------
# 3. the boundary cascade: ADR table == Python table == Rust cascade == config
# ---------------------------------------------------------------------------
def test_the_boundary_table_is_the_cascade_is_the_config_vocabulary(adr):
    table = adr_boundary_table()
    assert [r["rule"] for r in table] == [r[1] for r in S.BOUNDARY_RULES], (
        "ADR-003 §4.2 and `recommender.segment.BOUNDARY_RULES` disagree about the order")
    assert [(r["conf"], r["class"]) for r in table] == [(r[3], r[2]) for r in S.BOUNDARY_RULES], (
        "the confidences or classes disagree")

    rust = _text(RUST_SEGMENT)
    for row in table:
        variant = RUST_VARIANT[row["rule"]]
        # Every rule's price must appear in the Rust cascade exactly as published.
        assert re.search(r"Rule::%s,\s*%s\)" % (variant, re.escape(str(row["conf"]))), rust), (
            "ADR-003 says `%s` prices at %s; `decide()` does not say that"
            % (row["rule"], row["conf"]))
    # And in the module doc's table, which is what a reader of the code reads first (§14.1).
    doc_table = _rows(rust, "# The boundary rules, in priority order")
    assert [r[1].strip("`") for r in doc_table] == [r["rule"] for r in table], (
        "`segment.rs`'s module doc lists a different cascade from ADR-003 §4.2 — ADR-003 §14.1 "
        "decided the cascade wins, so fix the doc")
    assert [(r[1].strip("`"), r[2], float(r[4])) for r in doc_table] == [
        (r["rule"], r["class"], r["conf"]) for r in table]


def test_the_boundary_order_is_not_configurable_but_is_one_vocabulary(adr):
    """§4.5: the key restates the order, refuses to reorder it, and uses the stored spellings."""
    spec = _text(CONFIG_SPEC)
    key = spec.split('path="episode.boundaries"', 1)[1].split("_k(", 1)[0]
    enum = re.search(r"enum=\(([^)]*)\)", key)
    assert enum, "episode.boundaries lost its enum"
    allowed = [v.strip().strip('",') for v in enum.group(1).split(",") if v.strip()]
    rules = [r[1] for r in S.BOUNDARY_RULES]
    assert set(allowed) <= set(rules), (
        "the config enum spells a boundary rule %s, which `boundary_rule` does not store"
        % sorted(set(allowed) - set(rules)))
    assert list(S.BOUNDARY_ORDER) == ["hook", "trailer", "author"], (
        "the honoured prefix changed; `fathom-cli`'s notice and §4.5 both name it")

    jschema = json.loads(_text(JSON_SCHEMA))
    episode = jschema["properties"]["episode"]["properties"]
    assert episode["boundaries"]["items"]["enum"] == allowed, (
        "schemas/fathom.schema.json disagrees with the key spec; regenerate it")
    assert episode["boundaries"]["default"] == list(S.BOUNDARY_ORDER)

    # The Rust side recognises the key and refuses to obey a reordering, naming the ADR.
    cli = _text(ROOT / "crates" / "fathom-cli" / "src" / "segment.rs")
    assert "ADR-003" in cli and "not applied" in cli, (
        "the notice that refuses a reordered cascade no longer names where the decision lives")


def test_the_gap_default_is_one_number_in_every_place_that_prints_it():
    """§4.4: 30 minutes, and ADR-003 §14.3 refuses to let a second default exist."""
    want = S.DEFAULT_GAP_MINUTES
    rust = _text(RUST_SEGMENT)
    assert re.search(r"gap_millis:\s*%d\s*\*\s*60\s*\*\s*1000" % want, rust), (
        "`Rules::default()`'s gap is no longer %d minutes; the published default and §4.4 moved" % want)
    spec = _text(CONFIG_SPEC)
    key = spec.split('path="episode.time_gap_minutes"', 1)[1].split("_k(", 1)[0]
    assert re.search(r"default=%d" % want, key), "the config spec's default moved"
    jschema = json.loads(_text(JSON_SCHEMA))
    assert jschema["properties"]["episode"]["properties"]["time_gap_minutes"]["default"] == want
    assert re.search(r"^time_gap_minutes = %d$" % want, _text(REPO_CONFIG), re.M), (
        "the repository's own fathom.toml sets a different gap than the default it documents")


def test_type_change_and_continuation_rules_are_the_ones_documented():
    """TB7 and TB8, whose exact shape is the difference between one turn and three."""
    rust = _text(RUST_SEGMENT)
    assert "rules.gap_millis / 9" in rust, "TB7's clock guard moved (gap ÷ 9)"
    assert "shares_path(previous, commit)" in rust, "TB7's shared-path condition moved"
    assert "normalized.starts_with(marker.as_str())" in rust, (
        "TB8: single-word markers must be a prefix test")
    assert "normalized.contains(marker.as_str())" in rust, "TB8: phrases may appear anywhere"
    doc = _text(RUST_SEGMENT)
    for marker in ("wip", "fixup!", "address review"):
        assert marker in doc, "the marker list no longer contains %r" % marker


# ---------------------------------------------------------------------------
# 4. relink: tiers, tie-breakers, and the schema that must permit them
# ---------------------------------------------------------------------------
def test_the_relink_tiers_are_the_ones_the_store_runs(adr):
    table = adr_relink_table()
    assert [r["method"] for r in table] == S.relink_order(), (
        "ADR-003 §6.2 and `recommender.segment.RELINK_TIERS` disagree")
    store = _text(RUST_STORE)
    for row in table:
        digest = _tier_digest(row["method"])
        candidates = ['("%s", "%s", "%s", %s)' % (digest, row["relation"], row["method"], row["score"]),
                      '"%s", "%s", %s, now' % (row["relation"], row["method"], row["score"])]
        assert any(c in store for c in candidates), (
            "tier %d (`%s`) is published as %s at %s, which the store does not do"
            % (row["tier"], row["method"], row["relation"], row["score"]))
    assert '"squash", "member-set", 0.6' in store, "tier 3's write moved"
    # RL2/RL5: one claim per live episode, and every carry written down.
    assert "claimed.insert(new_id.clone())" in store, "RL2 (one relink per live episode) moved"
    assert "INSERT INTO episode_rewrite" in store, "RL5 (every relink is recorded) moved"
    # RL0/RL5: dead→live only, never onto an episode that already has a verdict.
    assert "o.live = 0" in store and "n.live = 1" in store, "RL0 (dead→live only) moved"
    assert "FROM review_state q WHERE q.episode_id = n.id" in store, (
        "RL0's second half (never overwrite a decision) moved")
    # RL3: the tie-break inside a tier is closest-in-time.
    assert "ORDER BY n.id, o.last_utc DESC, o.created_utc DESC" in store, "RL3 moved"


def _tier_digest(method: str) -> str:
    return {"fingerprint": "fallback_fingerprint", "shape": "shape_fingerprint"}.get(method, method)


def _create_block(ddl: str, table: str) -> str:
    """One `CREATE TABLE <table> … ;` block, so a CHECK is matched against its own table."""
    m = re.search(r"CREATE TABLE IF NOT EXISTS %s \(.*?\n\)" % table, ddl, re.S)
    assert m, "no CREATE TABLE %s in the DDL" % table
    return m.group(0)


def test_the_closed_vocabularies_are_the_ones_the_schemas_allow(adr):
    vocab = adr_vocabularies()
    rust_ddl, py_ddl = _text(RUST_STORE), _text(PY_STORE_SCHEMA)

    def check(table: str, column: str, want: list[str]) -> None:
        for name, ddl in (("rust migration", rust_ddl), ("reference schema", py_ddl)):
            block = _create_block(ddl, table)
            m = re.search(r"%s\s+TEXT NOT NULL CHECK \(%s IN \(([^)]*)\)\)" % (column, column), block)
            assert m, "%s has no CHECK on %s in the %s's %s" % (column, column, name, table)
            allowed = [v.strip().strip("'") for v in m.group(1).split(",")]
            assert set(want) <= set(allowed), (
                "%s forbids %s, which ADR-003 App. B publishes (%s: %s)"
                % (column, sorted(set(want) - set(allowed)), name, allowed))

    relations = next(v for k, v in vocab.items() if "relation" in k.lower())
    methods = next(v for k, v in vocab.items() if "method" in k.lower())
    sources = next(v for k, v in vocab.items() if "creation" in k.lower())
    kinds = next(v for k, v in vocab.items() if "kind" in k.lower() and "author" not in k.lower())
    author_kinds = next(v for k, v in vocab.items() if "author" in k.lower())
    check("episode", "kind", kinds)
    check("episode", "author_kind", author_kinds)
    check("episode", "created_source", [s.split()[0].strip("`") for s in sources])
    check("episode_rewrite", "relation", relations)
    check("episode_rewrite", "method", methods)
    assert tuple(relations) == S.REWRITE_RELATIONS and tuple(methods) == S.REWRITE_METHODS
    # `shape` is written by tier 2; a published CHECK that forbids a value the writer writes is a
    # contract that fails at INSERT time in someone's release (ADR-003 §14.6).
    assert "shape" in methods and '"shape"' in rust_ddl
    # The ledger's own link column must speak the same vocabulary, so e4.6 cannot invent a fifth.
    assert "shape" in re.search(r"link_method IN \(([^)]*)\)", py_ddl).group(1)
    # The boundary-rule vocabulary is the cascade's, and it is written in both schema comments.
    rules = next(v for k, v in vocab.items() if "Boundary rules" in k)
    assert rules == [r[1] for r in S.BOUNDARY_RULES]
    for name, ddl in (("rust", rust_ddl), ("python", py_ddl)):
        assert "|".join(rules) in ddl, "App. B's boundary vocabulary is not in the %s schema comment" % name


def test_creation_provenance_is_priced_the_way_the_writer_prices_it(adr):
    """§9.3: `index` is a 0.3 claim, because walking git earned 0.3."""
    sources = adr_creation_sources()
    assert sources == dict(S.CREATION_SOURCES), "§9.3's table and the published prices disagree"
    assert sources["index"] == 0.3, "ADR-004 §4.3 publishes 0.3 for `index`; do not re-inflate it"
    assert 'created_source: "index".to_owned()' in _text(RUST_INDEX)
    assert re.search(r"created_confidence:\s*0\.3", _text(RUST_INDEX)), (
        "ADR-003 §14.2 corrected the index writer to 0.3; it has drifted back")
    assert "1.0 hook, 0.7 commit, 0.3 index" in _text(RUST_STORE), "the DDL comment moved with it"


# ---------------------------------------------------------------------------
# 5. the parts of §5 and §7 that are statements about the code
# ---------------------------------------------------------------------------
def test_provisional_rules_are_stated_and_the_writer_is_accounted_for(adr):
    """§7 P8 is an honest gap; if someone writes the writer, they must update §7."""
    for clause in ("**P1**", "**P2**", "**P3**", "**P4**", "**P5**", "**P6**", "**P7**", "**P8**"):
        assert clause in adr, "provisional clause %s went missing" % clause
    assert "kind IN ('committed','provisional')" in _text(RUST_STORE)
    # The column admits 'provisional'; nothing writes it yet (§7 P8). If a writer lands, the ADR is
    # amended in the same change — a gap that quietly closes is a gap nobody re-reads.
    writers = [p for p in (ROOT / "crates").rglob("*.rs")
               if "'provisional'" in _text(p) and "CHECK" not in _text(p).split("'provisional'")[0][-200:]]
    assert not writers, [str(p.relative_to(ROOT)) for p in writers]
    assert S.PROVISIONAL_FORBIDDEN_STATES == ("accepted", "flagged", "dismissed")


def test_the_reference_segmenter_addresses_episodes_the_way_the_adr_says():
    """§14.4: `store/git.py` used to key episodes on member oids and trees. Both are banned inputs."""
    src = _text(PY_STORE_GIT)
    assert "from recommender.segment import episode_id, identity" in src, (
        "the reference segmenter no longer uses ADR-003 §2's digests")
    body = re.search(r"def _segment.*?return out", src, re.S).group(0)
    assert 'hashlib.sha256(_frame(oids))' not in body, (
        "member-oid identity is back in `_segment`, which §2.1 forbids")
    assert '"tree"' not in body, "tree-oid fingerprinting is back in `_segment`, which §2.1 forbids"
    assert "twins" in body, "the reference implementation lost TB10's twin handling"
    # The prices the reference writer stamps are ADR-003's prices, not fixtures' guesses.
    assert '"trailer", 0.9, "agent", 0.95' in body, (
        "§4.2 prices the trailer rule at 0.9 and §8 H4 prices trailer agent-evidence at 0.95")
    assert '"commit", 0.7' in body, "§9.3 prices `created_source='commit'` at 0.7"


def test_the_new_vocabulary_entered_through_the_glossary():
    """§0.2, and ADR-001's own rule: new nouns arrive in the glossary, in the same change."""
    thesis = _text(THESIS)
    for noun in ("Episode extent", "Digest tier", "Episode twin", "Fail-visible"):
        assert "**%s** —" % noun in thesis, "%s is used by ADR-003 but not defined in the glossary" % noun
    # And the entries ADR-003 *changed* say what ADR-003 decided, not what the ticket proposed.
    identity_entry = thesis.split("**Episode identity** —", 1)[1].split("\n\n**", 1)[0]
    assert "content digest" in identity_entry and "extent" in identity_entry, (
        "the glossary still describes identity as member SHAs; ADR-003 §2 amended it")
    fingerprint_entry = thesis.split("**Content fingerprint** —", 1)[1].split("\n\n**", 1)[0]
    assert "three" in fingerprint_entry, "the glossary still promises one fuzzy fingerprint"


def test_the_documents_call_adr003_accepted(adr):
    readme = _text(ADR_README)
    row = next(l for l in readme.splitlines() if l.startswith("| 003 |"))
    assert "Accepted" in row and "0003-episodes.md" in row, "docs/adr/README.md still lists 003 as open"
    assert "Open — blocked" not in readme
    root = _text(ROOT_README)
    assert "ADR-003](docs/adr/0003-episodes.md)" in root, "the top-level README does not link ADR-003"
    assert "ADR-003, 006 open" not in root


def test_the_tests_named_in_the_adr_exist():
    """§5 and §17 cite tests by name; a citation to a test that does not exist is a false citation."""
    names = set(re.findall(r"`([a-z0-9_]{8,})`", _text(ADR)))
    corpus = _text(RUST_SEGMENT) + _text(RUST_STORE) + _text(RUST_INDEX)
    corpus += "".join(_text(p) for p in (ROOT / "crates/fathom-store/tests/episodes.rs").parent.glob("*.rs"))
    corpus += "".join(_text(p) for p in (ROOT / "crates/fathom-analyze/tests").glob("*.rs"))
    corpus += _text(pathlib.Path(__file__))
    missing = sorted(n for n in names
                     if _looks_like_a_test_name(n) and ("fn %s" % n) not in corpus
                     and ("def %s" % n) not in corpus)
    assert not missing, "the ADR cites tests that exist nowhere: %s" % missing


def _looks_like_a_test_name(name: str) -> bool:
    if name in {"rust", "committed", "provisional", "fathom", "reco", "oid", "sha", "src", "grep"}:
        return False
    return any(name.startswith(p) for p in
               ("a_", "an_", "the_", "one_", "two_", "identity_", "segmentation_", "writing_",
                "scoring_", "amending_", "squashing_", "episode_inputs_", "test_"))
