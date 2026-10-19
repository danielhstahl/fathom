# Pathology detectors (`e4.3`, `reco-2ox`)

A **detector** is a pure function that looks at one reviewable episode — and, where it needs one, at the
episodes before it — and returns **findings**: things a coding agent characteristically did that a
human should look at. It is not a linter and not a policy engine. A linter asks "is this legal?"; a
detector asks "is this the shape of someone working without a map?", which is a question about a
trajectory and not about a line.

The module is `fathom_analyze::detectors`
([`crates/fathom-analyze/src/detectors/`](../../crates/fathom-analyze/src/detectors)). Its contract, in
full:

```rust
pub type Detect = fn(&Episode, &Corpus<'_>, &Policy) -> Vec<Finding>;
```

Three inputs, one output, no capabilities. A detector opens no store, runs no `git`, reads no clock and
talks to no surface. [`collect`](../../crates/fathom-analyze/src/detectors/collect.rs) is the only part
of the module that holds a `Store` and a `Repository`, and it hands over plain data; every detector is
then a function that a fixture can call, which is what makes
[the fixture corpus](../../crates/fathom-analyze/tests/fixtures/detectors) the whole of its test story.

**The content state is an input, not a detail.** [`Episode::content`] is `full`, `partial` or `absent`:
whether the hunks were actually read. A detector that reads `absent` as "found nothing" is the failure
ADR-001 §7.7 refuses — an unknown printed as a zero — so a scan over an `absent` episode reports
`not examined` per detector, with a gap line naming what would fill it, and prints nothing else.

## The catalogue

Twelve detectors, in the order `scan` runs them. *Needs* is what the read has to carry: `rows` is the
path rows and their taxonomy, `content` is the hunk lines, `history` is the episodes before this one.

| id | asks | needs | default |
|---|---|---|---|
| `scope_creep` | did every touched path fall inside the scope somebody declared? | rows | **on** |
| `untested_new_logic` | is there new logic here that nothing in this episode runs? | content | **on** |
| `test_weakening` | did the test surface get smaller, softer, or narrower in this episode? | content | **on** |
| `debug_leftover` | did something written to look at the program survive into the commit? | content | **on** |
| `todo_debt` | how much explicitly unfinished work did this episode add? | content | **on** |
| `generated_edit` | is a file a program owns being edited by hand? | content | **on** |
| `secret_string` | does a committed string have the shape of a credential? | content | **on** |
| `lint_escape` | was a check switched off here, and which one? | content | **on** |
| `churn_loop` | has this region been edited across N episodes and put back again? | history | **off** |
| `rework_loop` | is this episode undoing, or re-landing, something that already happened? | history | **off** |
| `shotgun_edit` | does one episode reach into several places that have nothing to do with each other? | rows | **off** |
| `copy_paste_growth` | did new text arrive as a copy of other new text? | content | **off** |

The machine-readable form of that table is
[`catalogue.json`](../../crates/fathom-analyze/tests/fixtures/detectors/catalogue.json), blessed by
[`tests/detector_fixtures.rs`](../../crates/fathom-analyze/tests/detector_fixtures.rs), and
[`tests/test_pathology_contract.py`](../../tests/test_pathology_contract.py) fails if this table, that
file and the Rust `CATALOGUE` ever disagree.

### Every finding carries four things

The ticket's list, enforced in
[`finding.rs`](../../crates/fathom-analyze/src/detectors/finding.rs) rather than left to convention:

1. **a human-readable reason** — `message`, one sentence naming the paths, counts and the literal it
   saw. No adjectives without a number beside them;
2. **a jump target** — `{view, target}`, ADR-006 §4's registry ids (`diff`, `timeline`, `structure`,
   `queue`, `graph`) and `<episode>#<path>`, which is the shape the diff pane's path selector already
   speaks;
3. **a confidence** — `0..=1`, never `1.0` for an inferred claim, and each detector's price list is in
   its own file header;
4. **one-key suppression** — `suppression_key`, `detector:subject`, which is the whole of what you type
   into `detectors.suppress` to make it stop.

Two rules make the key worth typing. It contains **no episode id and no timestamp** — a key that moved
when the history was rewritten would have to be retyped after every rebase, and ADR-003 §2.7 is explicit
that a review does not move when the history does. And it is derived from the **subject**, never from
the message, because messages get reworded and a reword must not invalidate a suppression.

Severity is `high | medium | low` — ADR-004 Appendix A's `finding.severity` CHECK, the only severity
vocabulary in the product with a constraint behind it. The delta document's comment floats
`high | notable | note`; nothing writes those strings today, and a second vocabulary with no constraint
is the "fourth spelling" its own comment warns about. Evidence grades are ADR-004's
`observed | inferred | asserted | absent`. Its `evidence.kind` list (`range`, `hunk`, `test`,
`config_key`, …) has no entry for a claim about a whole file or about another episode, which two of these
detectors need, so this catalogue writes two kinds — `path` and `episode` — and the ADR amendment that
admits them is `e4.4`'s, made when findings are persisted and the CHECK constraint exists to be amended.

## Why four of them ship off

`G3` — the false-positive guardrail, ADR-001 §5.4, set by `e0.12` — is `≤ 0.30` at the alpha cut and
`≤ 0.15` at 1.0, with `miss_rate ≤ 0.20`. A budget nobody has measured is not a budget, and the
measurement is its own ticket: **`e4.3a` (`reco-mkl.1`)** — three pinned OSS repositories,
hand-authored dispositions, per-detector attribution, blessed and re-checked in CI. So the rule that
came with `e0.12` is applied literally:

> A detector whose own false positives could push `G3` past the alpha gate ships disabled by default,
> **fixture and all**, and the ticket that re-enables it names the measurement that justifies it.

The four that ship off share one property: each of them has to decide that a *legitimate* activity is
suspicious. Three episodes touching one function may be thrash or may be a hard bug; a cross-cutting
change may be a shotgun or may be a rename; twelve copied lines may be growth or may be a table that was
always a table. The eight that ship on do not make that judgement: they read a declaration the repository
made (`scope_creep`, `generated_edit`), a directive that is simply a fact (`lint_escape`,
`test_weakening`, `debug_leftover`, `todo_debt`), a shape with a vendor's prefix on it (`secret_string`),
or an absence a reviewer can see for themselves (`untested_new_logic`).

Disabled does not mean absent. Each row is in the table, prints `disabled` in a scan's per-detector
accounting, has a fixture that runs with every detector enabled, and carries a `reenable` line naming the
measurement — which is what a reviewer asking "why didn't I hear about this?" is owed.

## A worked example, end to end

Take the `lint_escape` fixture
([`lint_escape.json`](../../crates/fathom-analyze/tests/fixtures/detectors/fixtures/lint_escape.json)).
The input is an episode whose diff contains two added lines in `src/parse/lexer.rs`:

```rust
#[allow(clippy::unwrap_used)]
fn peek(&self) -> char { self.buf[0].unwrap() }
```

The output — verbatim from
[`lint_escape.expected.json`](../../crates/fathom-analyze/tests/fixtures/detectors/fixtures/lint_escape.expected.json)
— is one finding:

```json
{
  "detector_id": "lint_escape",
  "subject": "src/parse/lexer.rs",
  "message": "src/parse/lexer.rs adds 1 check-suppressing directive(s) — a lint — and the reason is not in the diff: #[allow(clippy::unwrap_used)]",
  "severity": "high",
  "confidence": 0.9,
  "jump_view": "diff",
  "jump_target": "ep-fix-lint-escape#src/parse/lexer.rs",
  "suppression_key": "lint_escape:src/parse/lexer.rs",
  "evidence": [
    "observed hunk src/parse/lexer.rs @@ -1,4 +1,6 @@ :: #[allow(clippy::unwrap_used)]",
    "absent config_key clippy::unwrap_used :: the directive names the check it switches off; nothing in the diff says why it had to be switched off"
  ]
}
```

Read it as the reviewer would. *What*: a lint was switched off, and it was `clippy::unwrap_used` —
specific enough to act on. *Where*: `jump` sends the surface to that file in that episode's diff. *How
sure*: `0.9`, because the directive is a fact in the diff and not an inference — the `0.1` is the case
where the diff is one half of a commit that also explains itself. *What is missing*: the second evidence
row is `absent`, graded as such, because "no reason given" is a claim about something not there and says
so instead of inflating the confidence.

Now the reviewer's two moves, both one keystroke:

```toml
# fathom.toml — this finding, and its family, forever
[detectors]
suppress = ["lint_escape:src/parse/lexer.rs"]
```

```toml
# or the whole detector, in a repository that has decided allow-attributes are its business
[detectors]
suppress = ["lint_escape"]
```

Keys are matched as an exact id, an exact key, or a glob over keys with the same `globset` semantics as
every other pattern in the product — `*` stays inside one path segment, `**` crosses them.
`suppress` is the off switch; `enabled` is the on switch and can only add.

## Writing one

Four edits. No registry call, no `main`, no surface to teach: `scan`, the fixture test and the delta
document iterate the table, so a new row is picked up by everything that prints findings.

**1. The detector.** A new file, with a header comment that answers three questions *before* the code —
what it claims, what would make it wrong, and what its confidence price is. A rule that does not know
its own false positives is not ready to ship on.

```rust
//! `my_detector.rs` — one sentence on the pathology (`e4.3`).
//!
//! **What it claims.** The exact, checkable statement, with the knob named.
//! **What would make it wrong.** The legitimate shapes it cannot tell apart, and which of them it
//! refuses outright.
//! **Confidence.** The price list, and why the ceiling is where it is.

use crate::detectors::finding::{Evidence, EvidenceKind, Finding, Severity};
use crate::detectors::model::{Corpus, Episode};
use crate::detectors::policy::Policy;

pub fn detect(episode: &Episode, corpus: &Corpus<'_>, policy: &Policy) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in &episode.files {
        if file.noisy() || file.is_test() {
            continue; // the refusals are as much the detector as the matches are
        }
        findings.push(
            Finding::build("my_detector", 1, &episode.id, file.path.clone(), "the one-sentence claim")
                .severity(Severity::Medium)
                .confidence(0.6)
                .evidence(Evidence::observed(
                    EvidenceKind::Hunk,
                    format!("{} {}", file.path, file.hunks[0].header),
                    crate::detectors::quote(&file.added(), policy.evidence_lines),
                ))
                .jump_path(&file.path)
                .finish(), // `finish()` derives the suppression key and refuses to drop evidence
        );
    }
    findings
}
```

**2. The row.** One `Descriptor` in `CATALOGUE`
([`mod.rs`](../../crates/fathom-analyze/src/detectors/mod.rs)) — `id`, `version`, `title`, `asks`,
`default_on`, `needs`, `reenable` (empty unless it ships off), `detect`. Bump `version` when a rule
change would move an existing episode's verdict: that number is what `finding.detector_version` stores,
so a stored finding can be told from the one this build would produce.

**3. The fixture.**
[`make_fixtures.py`](../../crates/fathom-analyze/tests/fixtures/detectors/make_fixtures.py) writes
`fixtures/my_detector.json` — one episode (plus `prior`, if the detector needs history), small enough to
read on one screen, hand-authored and never a repository cloned off the internet.

**4. Bless, then read what it said.**

```console
$ python3 crates/fathom-analyze/tests/fixtures/detectors/make_fixtures.py
$ FATHOM_BLESS=1 cargo test -p fathom-analyze --test detector_fixtures
$ cargo test -p fathom-analyze --test detector_fixtures
```

The `*.expected.json` files are blessed by the test, never hand-written: a hand-written expectation is a
second opinion nobody ran. **Read the message it wrote before you commit it.** That sentence is the
product now, and the clean corpus next to it is the half that keeps the detector honest — every detector
must stay silent on all three clean episodes, including the ones that ship disabled.

Then add the row to the table above; `tests/test_pathology_contract.py` will tell you if you forgot.

## Knobs

Every number a detector reads is a declared `detectors.*` key, printed with every report so a verdict can
be reproduced, and refused by name when a value would make a detector unable to report anything:

| key | default | what it holds |
|---|---|---|
| `detectors.enabled` | *(see `e2.7`'s key)* | membership only — it cannot remove a default; an id this build does not know is a notice naming a valid one |
| `detectors.suppress` | *(empty)* | the off switch: detector ids, exact keys, or globs over keys |
| `detectors.max_findings` | `12` | per-detector row cap; what it hides is counted, not dropped |
| `detectors.untested.min_added_lines` | `20` | how big a new public surface has to be to owe a test |
| `detectors.todo.min_new` | `1` | new markers before `todo_debt` speaks (`0` is a real threshold) |
| `detectors.churn.min_episodes` | `3` | episodes on one region before the loop is a loop |
| `detectors.rework.min_overlap` | `0.60` | line-set overlap that counts as a revert |
| `detectors.shotgun.min_clusters` | `4` | directory clusters before the spread is a shotgun |
| `detectors.copy.min_block_lines` | `12` | consecutive new lines that make a copied block |

Bad values are a notice naming the key and the default that stood, never a failed index run: a typo in a
repository's `fathom.toml` costs that repository a knob, not its review.

**The seam, said out loud.** `detectors.enabled` predates this catalogue — it shipped in `e2.7` listing the
test-surface finding ids (`test_deletion` among them), which are Python findings with knobs and not
catalogue rows. The two lists are one key today and they are not reconciled here, because the reconciliation
belongs to the ticket that persists findings and has to read both. Until then: an id this build's catalogue
does not know is a notice naming a valid id, `enabled` can only add membership, and
`detectors.suppress` is the only switch that turns one of these eight off.

## Where the findings go

Into the delta document, through [`Supplied::findings`]
([`insight/delta.rs`](../../crates/fathom-analyze/src/insight/delta.rs)), which is the only door the TUI,
`fathom analyze`, the export and an MCP result read answers through (ADR-002 §5.3). A detector finding
converts with [`Finding::to_insight`], which *points* rather than quotes: the delta payload keeps its
ADR-001 §6.3 rule that file contents stay out of the document, so the pointer is a path-and-range or a
JSON pointer into the document, and the quoted lines stay in the finding. Persisting findings to
`e1.10`'s `finding` table — with their `detector_id`, `detector_version`, `suppression_key` and jump —
is `e4.4`'s and `e4.6`'s work; the vocabulary this catalogue writes is already the one that table's CHECK
allows, which is the part `e4.3` could get right on its own.

The four things a finding is made of are also the four things a suppression key has to survive: a reword
must not invalidate a key, a rebase must not move one, and a renamed file may. That is the whole reason
`suppression_key` is derived from the subject and nothing else.
