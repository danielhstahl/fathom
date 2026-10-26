---

## Reviewing this repository with Fathom

This history is a fixture: every commit exists to put a row somewhere in
[Fathom](https://github.com/danielhstahl/fathom). Five of them exist to plant a pathology on purpose — a
churn loop, an unplanned dependency, a new cycle, a deleted test and a commit that is whitespace and
nothing else.

- [`DEMO.md`](DEMO.md) is the five-minute tour: five acts, one command each, and the one command whose
  artefact carries every finding this repository documents. It does not name the answers.
- [`PATHOLOGIES.md`](PATHOLOGIES.md) is the answer key, and says plainly which of the five Fathom names
  today and which three it cannot name yet — with the owner and the remedy for each gap.
- [`.github/workflows/fathom.yml`](.github/workflows/fathom.yml) is the gate running here: five lines that
  call Fathom's reusable workflow, the same five any repository can copy.

Both markdown files are generated from the catalogue that a machine runs against a real binary in
[Fathom's CI](https://github.com/danielhstahl/fathom/blob/main/docs/demo/five-minute.md), so a claim here
that stops being true fails a build rather than disappointing a visitor.
