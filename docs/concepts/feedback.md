# The beta loop: where feedback goes, what happens to it, and when

Status: the loop opened with `e7.7` (**reco-rny**). The two forms are in the repository
([`.github/ISSUE_TEMPLATE/`](../../.github/ISSUE_TEMPLATE)), so is the register it writes
([`.plan/intake/`](../../.plan/intake)), and so are the gates
([`tools/check_beta_loop.py`](../../tools/check_beta_loop.py),
[`tools/beta_intake.py`](../../tools/beta_intake.py)). What is not in the repository, and cannot be, is the
part that needs other people: §8 says exactly which sentences are still unwritten and why.

This is the *channel* page. If you are holding a symptom right now, [troubleshooting](../troubleshooting.md)
is faster — it has the symptoms that are already known and the exit-code table. If you are the person who
reads what arrives, this is your runbook, and §4's seven dispositions are the only seven answers a report
can get.

The failure this page exists to prevent is the one every alpha dies of: reports arrive in four places, get
answered by whoever saw them first, and nothing is written down, so the same three complaints are
rediscovered by the next person and the release notes describe the code rather than the complaints. There
is **one channel**, **one weekly triage**, **one register**, and **a rule that a fixed report is named in
the release notes in the reporter's own words**.
---

## 1 The channel: one place, two forms

**The channel is the repository's issue tracker**, and it has exactly two forms:

| Form | For | What it makes you paste |
|---|---|---|
| [Something is wrong](../../.github/ISSUE_TEMPLATE/bug_report.yml) | a crash, a wrong number, a finding that is not true, a run that never finished | the command line and its output, what you expected and why, and `fathom doctor --json` |
| [Beta feedback — how it went](../../.github/ISSUE_TEMPLATE/feedback.yml) | what you were doing, where you lost the plot, what you wish it did (feature requests live here, not in a third form) | the same five sentences, and `fathom doctor --json` or the word `n/a` with a reason |

Both forms require the doctor object (§2). Blank issues are switched off
([`config.yml`](../../.github/ISSUE_TEMPLATE/config.yml)), because a blank issue always costs one round trip
to ask for the version, and the intake cadence cannot afford round trips it could have spent reading.

**What is deliberately *not* a channel:** telemetry or a "send diagnostics" button (ADR-001 §8 and
[privacy](privacy.md) put the network out of the product's reach — the report *is* the transport, which is
also why the report is yours to redact), a chat server, a shared inbox, a mention to a maintainer's
personal account, and replying to a release announcement. A report that arrives somewhere else is not
rude, it is invisible: it will not be triaged on Tuesday, it will not get a disposition, and it will not be
in the release notes. If you found one of those, the favour is to copy it into the tracker, not to answer it
where it landed.

The one exception, and it is a real one: **a private security report** goes to
[the adviser form](https://github.com/danielstahl/fathom/security/advisories/new), and it lands in
[the threat model](../security/threat-model.md)'s register instead of the beta ledger. Anything whose value
is "here is how to make the tool say *clean* about an unsafe repository" belongs there and not here.

## 2 What a report has to carry, and why

`fathom doctor --json` is the one artefact that turns "on my machine" into something a maintainer can
diagnose without a conversation. One JSON object: the version and compiled-in feature set, the OS, arch and
terminal, the SQLite build and its capabilities, the index state, epoch and integrity verdict, the config
layers in force, the watch/daemon lease, and every finding with its remedy. `--json` is not cosmetic: the
same object is what `tools/check_beta_loop.py` can read, so a report with it in can be *gated* rather than
described.

It contains your repository's paths and your username. That is the point of the difference between a
diagnostic and telemetry: **you read it before it leaves**. Redact freely — the fields a diagnosis needs are
`build`, `features`, `sqlite`, `index`, `config` and `findings`. Two things a report should *not* carry:
`fathom analyze --json` (it holds file bodies and commit subjects) and anything you pasted without reading.

| Field | Why it is there |
|---|---|
| the exact command line, with flags | `--format`, `--detectors`, `--theme` and `--root` change the answer; half of "it's broken" is a flag nobody mentioned |
| the exit code | `7` is a finding, `12` is a degraded index, `4` is a store that will not open — [the codes are a contract](../reference/cli.md), so the number *is* the diagnosis |
| what you expected, and where you read that | turns a complaint into a claim someone can check against a document; if the document was wrong, the defect is the document |
| `fathom doctor --json` | the machine's own description of itself, which is the only part of the report nobody has to remember |
| a reduction (`fathom --demo`, a range, three commits) | the difference between a fixed bug and a fixed *bug that stays fixed*: a reduction becomes a fixture |

The miniature is worth asking about explicitly, because it is unusual and useful: `fathom --demo` unpacks a
twenty-one-commit repository inside the binary and indexes it. If the miniature reproduces the wrong number,
the reporter has a minimal case, the maintainer has a fixture, and CI can hold the fix open forever after.

## 3 The cadence: once a week, thirty minutes, a rota

| When | What | Who |
|---|---|---|
| **Tuesday, ~30 minutes** | read everything `triage:untriaged` since the last pass; give every report a disposition, an owner, and a bead where one is owed | the triage rota (one named person per week, rotating) |
| **Tuesday, same session** | file the defects the self-dogfood pass (§6) found on this repository, and give last week's rows their disposition if they have aged out | same |
| **Each release cut** | copy this cycle's `triage:fixed` rows into `[Unreleased]` under a *From the beta* heading, in the reporter's words; then `python3 tools/gen_changelog.py audit` refuses the cut until the notes account for every commit | whoever cuts the release ([the runbook](release.md) §3) |
| **Every commit (CI)** | the templates still require the doctor object, the dispositions and the vocabulary still agree, the ledger's rows are well formed, and the changelog covers its commits | `contracts.yml`, `ci.yml` |

Three rules that make a 30-minute cadence enough:

* **First touch inside a week, always.** A report may be answered "cannot reproduce yet, here is what I
  need" — it may not be unanswered. Silence is the one failure mode a volunteer project cannot recover from,
  because it is indistinguishable from disinterest.
* **`triage:needs-repro` has a 14-day clock and an owner.** When it runs out the disposition becomes
  `triage:cannot-reproduce` *in writing*, with what was tried. A report never closes by fading.
* **Nothing is closed without a disposition.** A duplicate gets counted, a cannot-reproduce stays on the
  register, and a `by-design` names the sentence that decided it. The register in `.plan/intake/` is
  append-only, so a week's work is a row per report and a sum at the bottom.

## 4 The seven dispositions

Every report ends in exactly one of these, in the ledger and as the label on the issue. A report may carry
more than one over time — `triage:needs-repro` becoming `triage:ticketed` becoming `triage:fixed` is the
normal path — but it always has one *right now*.

| Disposition | Means | What the reporter gets | What the ledger records |
|---|---|---|---|
| `triage:ticketed` | real, accepted, and captured as a bead with an owner | the bead id, and the bead's text names the reporter | bead id, owner, area |
| `triage:fixed` | acted on in the tree | the commit/tag it lands in, and a line in the release notes (§5) | the bead, the release, and the symptom in the reporter's words |
| `triage:duplicate` | already known | the thing it duplicates — never "known, thanks" and nothing more | the id it duplicates, so the count of people hit by it survives |
| `triage:needs-repro` | something the reporter has to supply | the one question, and what would settle it | the owner and the 14-day clock |
| `triage:by-design` | deliberate, and documented | the sentence that decided it (an ADR, a non-goal, a calibration note), quoted | where the decision lives, or the document that now carries it |
| `triage:wont-fix` | real, and not happening in v1 | the non-goal that covers it, named ([the thesis](thesis.md) is where those live) | the non-goal, so the next reporter is told the same thing |
| `triage:cannot-reproduce` | not reproducible with what was supplied | what was tried, and what would have settled it | what was tried, so the second attempt is not a repeat of the first |

`triage:ticketed` is the disposition this project calls **feedback captured as beads**: the tracker is where
reports arrive, [beads](release.md) is where work lives, and nothing counts as accepted until it is a bead
with an owner. `python3 tools/beta_intake.py` is the mechanical half of that step (§7).

## 5 Publishing what was fixed, and why

A fixed report earns three sentences, not a bullet that says "fix a bug":

1. **the symptom, in the reporter's words** — "a lockfile-only change came out the riskiest episode in the
   repository" outlives any paraphrase, and it is how the next person recognises their own bug;
2. **the cause**, named as a mechanism rather than an incident — "the noise policy matched `.github/*.lock.yml`
   before the lockfile tables were consulted" says something a reviewer can act on;
3. **why it happened now**, if that is interesting — the missing test, the fixture that could not see it,
   the axis that was never measured.

Those three sentences land in `CHANGELOG.md` under `[Unreleased]` in a **From the beta** heading, and the
same text is what the release's GitHub body gets (the changelog *is* the release body —
[the release runbook](release.md) §1). The `triage:fixed` row in the ledger carries the same three
sentences, so the ledger, the changelog and the release cannot disagree, and
[`tools/gen_changelog.py`](../../tools/gen_changelog.py) `audit` makes a cut that dropped a commit fail
instead of shipping.

The weekly digest is the second half of "publish what was fixed": the bottom of each ledger file is a
summary block — how many arrived, what the dispositions were, what got fixed and why — and it is posted
wherever the release is announced. It is short on purpose. A digest nobody reads is a digest that stops
being written.

## 6 The loop pointed at this repository

The beta loop is not only for strangers. Every week, the same pass runs **Fathom against its own repository**
— `fathom index`, then `check`, `architecture`, `ratchet`, `why`, `export` and `review sign` over the
episodes this repository just made — and what the tool says about its own author is filed as ordinary
beta rows with ordinary dispositions, in the same ledger, from the same vocabulary. §10 of
[the release runbook](release.md) is the per-release half; the record of the first pass is
[`.plan/evidence/reco-rny/fathom-on-itself.md`](../../.plan/evidence/reco-rny/fathom-on-itself.md).

Two things come out of that pass that no external user can give you. One is the defect the product makes for
itself: a cycle between a module and its own submodule, reported at `error` severity on the repository's own
newest commit, in a repository whose author is the tool's author — which is the only place a false positive
can be dealt with in the same hour it is found. The other is the credibility claim: *"this is what Fathom
says about Fathom"* is the most honest marketing this project has, and it is also a regression test, because
a release that changed what the tool says about itself now has to explain the difference.

## 7 The mechanism, as code

| Command | What it does | Proves it can fail |
|---|---|---|
| `python3 tools/beta_intake.py draft --report <file>` | reads a filed report (the form's markdown, plus the doctor object), checks the required parts are there, and prints the `bd create` line that captures it as a bead — labels, priority, the reporter's handle, the symptom sentence first | `--self-test` feeds it six broken reports and requires all six to be refused |
| `python3 tools/beta_intake.py ledger --week 2026-W43` | validates the week's register: every row has a disposition from the vocabulary, an owner where one is owed, and a bead where the disposition claims one; then prints the counts against the beta's criteria | same `--self-test`, which corrupts a row four ways |
| `python3 tools/check_beta_loop.py` | the channel and the documents: both templates exist and require the doctor object, blank issues are off, the labels the templates use exist in `.github/labels.yml`, the dispositions in the code, the labels and this page are the same seven words, this page is the documented channel and is reachable from `docs/index.md`, and the four beta criteria are printed as data with a verdict | `--self-test` mutates eight copies of its own inputs |
| `python3 tools/gen_changelog.py audit` / `self-test` | the changelog half: every commit since the last release is accounted for in the notes, and two consecutive release cuts are performed in a throwaway repository | both built into the commands |

CI runs all four (`contracts.yml`), and the pytest mirror is
[`tests/test_beta_loop_contract.py`](../../tests/test_beta_loop_contract.py).

## 8 What this page cannot decide from inside a repository

Stated as the four acceptance criteria the ticket carries, with what is and is not true today:

* **"At least five external users onboarded with their feedback captured as beads" — not met.** The
  mechanism is here: two forms, a required diagnostic, a command that turns a report into a bead. The users
  are not, because the alpha is not installable yet — the tag has not been pushed to a public remote
  (`m0.1`, **reco-lnd**), so there is nothing to onboard anybody *to* — and because announcing is a person
  with a keyboard, not a commit (`m0.2`, **reco-3gz**). Counting them is the adoption ledger in
  [the release runbook](release.md) §5, which is deliberately opt-in: no telemetry means quiet users are
  invisible, and under-counting is the correct price of that decision.
* **"At least twenty beta issues triaged with explicit dispositions" — met on the first pass**, against a
  population that is real rather than invented: this repository's own open defects, the findings Fathom made
  about itself, the calibration gates that are red, and the contract tests that were red when the beta gate
  was last assessed. The register and its sum are in [`.plan/intake/`](../../.plan/intake). None of these
  came from outside the project, which is exactly the difference between this row and the one above it.
* **"Fathom runs against its own repository in CI and the artefact is published per release" — met in CI,
  not yet per release.** `.github/workflows/ci.yml` has run `fathom check` over this repository since
  `e6.6`, and [`.github/workflows/self-review.yml`](../../.github/workflows/self-review.yml) is the per-release
  half: it indexes this repository at the tag, exports the review artefact and the doctor object, and
  attaches both to the release. It runs on `release: published`, which needs a release to publish, which
  needs the same tag push as the criterion above.
* **"Release cadence and changelog process documented and followed for two consecutive releases" — the
  process is documented ([§9 of the runbook](release.md)) and generated
  ([`tools/gen_changelog.py`](../../tools/gen_changelog.py)), and two consecutive cuts are performed by
  `self-test`; on this tree one release has actually been cut.** Cutting `alpha.2` for real needs the version
  bump, a re-measured `dist` plan and a tag push, and the last of those is the same blocker again. What is
  *not* possible any more is cutting a release whose notes dropped a commit: that is the gate now.

---

**Next**: [the release runbook](release.md) for the cadence and the cut ·
[the CI gate](ci-gate.md) for the gate that runs on this repository ·
[troubleshooting](../troubleshooting.md) for the answers side of the same machine ·
[privacy](privacy.md) for why the report is the transport.
