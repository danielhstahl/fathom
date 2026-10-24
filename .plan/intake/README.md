# The beta intake register (`e7.7`, **reco-rny**)

One file per ISO week, one row per report, one disposition per row. The channel and the cadence are
[docs/concepts/feedback.md](../../docs/concepts/feedback.md); this directory is the *record* that page
describes, and the thing the acceptance criterion ("at least twenty beta issues triaged with explicit
dispositions") is actually a sentence about.

**The shape.** A markdown table with these columns, because `python3 tools/beta_intake.py ledger` reads it:

| Column | Rule |
|---|---|
| `id` | `W<week>-<n>`, unique within the week |
| `source` | `external`, `self-dogfood`, `board`, `ci` or `beta-gate` — the distinction is the whole honesty of the file |
| `reporter` | a handle for `external` rows (the adoption ledger counts handles); `—` otherwise |
| `area` | what it touches, in the words the form's dropdown uses |
| `what it says` | the symptom, in the reporter's words — not a paraphrase, and not a ticket title |
| `disposition` | exactly one of the seven in `.github/labels.yml` / feedback.md §4 |
| `owner` | a named person or rota; `triage:needs-repro` without one is a rule violation |
| `bead / next` | the bead, the duplicate, the release, or the document that decided it — required by `triage:ticketed`, `triage:fixed` and `triage:duplicate` |

**Beads versus the tracker.** The *tracker* labels (`triage:*`) are the disposition vocabulary, because that
is what a visitor to the issue tracker can see and what the weekly pass moves. The *bead* a disposition
creates carries its own area labels (`defect`, `gate`, `cache`, …) in the board's existing style. Two label
systems for two systems, and the ledger row is the join between them.

**Append, do not tidy.** A row is never edited into nonexistence. When a report's disposition changes, a new
row goes in citing the old one (`was W43-07`), because "we changed our minds on 2026-11-03" is information
and a rewritten table is how a project ends up with a register nobody believes.

**Commands.**

```bash
python3 tools/beta_intake.py ledger                    # validate every week, print the counts
python3 tools/beta_intake.py ledger --week 2026-W43 --json
python3 tools/beta_intake.py draft --report /tmp/report.md   # a filed report → the `bd create` that captures it
```
