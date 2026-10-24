# The beta loop (`e7.7`, **reco-rny**) — decisions, and what was refused

The ticket was four acceptance criteria about a *process*: onboard five users, triage twenty issues, cut a
release a week, never forget a commit in the notes, and eat the dog food. A process written down with nothing
that fails when it is not followed is an intention, so the work here is mostly mechanisms, and the interesting
part of the record is which mechanisms were refused.

The findings themselves live in [`.plan/intake/2026-W43.md`](intake/2026-W43.md) and
[`evidence/reco-rny/fathom-on-itself.md`](evidence/reco-rny/fathom-on-itself.md); the user-facing statement of
the process is [docs/concepts/feedback.md](../docs/concepts/feedback.md); the cadence went into
[docs/concepts/release.md](../docs/concepts/release.md) §9–§10. This file is why the shape is what it is.

## 1 One channel, and the diagnostic is the channel's price

**Decided:** a single documented channel — GitHub issues, two forms — and `fathom doctor --json` required in
both, with the instruction to read it before sending. **Refused:** a Discord, a Slack, an email address, a
"talk to us directly" line, and a free-text intake field that could hold anything. The reason is the project's
no-telemetry promise: the tool learns nothing about a user unless the user sends something, so a channel without
a structured diagnostic buys a conversation, and this project's open defects are full of what those conversations
cost — a defect whose bead had to spell out how to reproduce it, in prose, twelve times over. It also buys
*private* reports, and there is no way to promise confidentiality on a Discord without a security plan.
`B03`/`B04` in `tools/check_beta_loop.py` keep the vocabulary and the pointers to the one channel;
`B02` fails any form whose doctor field is not `required: true` and does not mention redaction.

**Decided, and stated as the cost:** one channel plus a slow cadence loses the reports nobody would file. The
alternative that would fix that — collecting something automatically — is the thing the thesis forbids. The
sentence is in §2 of the channel page rather than left out.

## 2 Twenty issues is a register or it is a mood

**Decided:** `.plan/intake/<ISO-week>.md`, one row per report, disposition from a fixed seven-word vocabulary,
owner, bead-or-decision; validated by `tools/beta_intake.py ledger` and by
`tests/test_beta_loop_contract.py::test_the_weekly_register_validates`. **Refused:** counting `bd list` labels
as the register. Labels exist, and are checked for consistency with the vocabulary (`B03`), but they can only be
applied to a report that became a bead — and `triage:by-design`, `triage:deferred` and `triage:invalid` are the
majority of what a real triage pass produces in a project with no external users yet, so a label-based count
would quietly reward filing beads for everything.

**Decided:** the register is committed, with `type:` per row (`self-dogfood`, `board`, `docs`, `calibration`,
`ci`) rather than a separate file per source. The ticket allows "at least twenty"; twenty rows of a single
table is auditable in a way that twenty rows across five files is not, and the mixed provenance is the honest
shape of a first pass in a project whose only users are its authors. `B08` refuses a row whose disposition is
not one of the seven, a `triage:ticketed` row that names no bead, and a `triage:needs-info` row with no clock —
the failure mode of every triage process is the report that is neither closed nor owned.

**The honesty clause, kept in three places.** Zero of the 26 rows came from outside this repository, and the
criterion that needs five external users is `m0.1` (`reco-lnd`, a tag nobody can `brew install`) and `m0.2`
(`reco-3gz`, an announcement nobody has made). It is in the register's digest, in §4 of the channel page and in
`beta_intake.py criteria`'s `state` field. A criterion reported as met because the metric was chosen loosely
enough is the specific way a loop like this becomes theatre.

## 3 The changelog is generated, on demand, and the gate is coverage

**Decided:** `tools/gen_changelog.py` reads `git log` and *renders on demand*; the file is edited only by
`cut`. **Refused:** a committed per-release markdown fragment per PR (the towncrier shape). It needs an external
tool to be useful, it puts a second copy of "what changed" somewhere nobody reads when the question is asked of
the release, and this repository's commits are already written as prose sentences with bead refs in them — the
material is in the log, so the generator should read the log.

**Decided:** `lint` is advisory in CI and `audit` is blocking. Making the conventional-commit *shape* a
hard gate here would force `fix(area):` prefixes onto commit subjects that are already better than a
conventional header can be (this repository's subjects are sentences), and a gate that forces a worse commit
message to satisfy a formatter is a gate that will be removed within a sprint. What actually needs to be
guaranteed is not the format but that **nothing is dropped**, which is what `audit` measures — and it found five
shipped tickets with no entry (`e3.7`, `e7.1`, `e5.6`, `e7.8`, `s3`) on its first run against its own history.
The fallback ladder (trailer → header → bead title → prose subject) is what lets `audit` be strict about
coverage while `lint` stays permissive about style; `C06` prints which rung each commit took, so the permissive
half cannot rot into "the generator is guessing".

**Decided:** `.plan/changelog-coverage.json` exists and is **empty**, and `C07` makes it shrink-only. An
exemption list that can only shrink cannot become where the awkward commits go.

**Decided:** the "two consecutive releases" criterion is proven by `self-test` performing two real cuts in a
throwaway repository, *plus* refusing a cut while a commit is missing from the notes — and `criteria` prints
`proven twice in self-test; 1 release(s) cut on this tree so far` rather than `met`. This clone has no remote
and no tags; the previous-release resolver therefore falls back to the commit that edited the changelog's last
version heading, which is the thing `B01` in the CI checker also had to learn. A criterion satisfied only by a
machine that does not exist here is a claim, not a check.

## 4 The dog food is the release step, not a hero

**Decided:** `.github/workflows/self-review.yml` on `release: published`, so the review happens every week by
clock and cannot be skipped by the person it would inconvenience; it carries the same `0 or 7` exit-code
contract as `dogfood.yml`, and `check_ci_gate.py`'s new rule group checks the workflow the way it checks the
other four — including that the `fathom` flag sits behind a *path* (`./target/release/fathom check`), which the
original flag regex missed and the self-test now pins. **Decided:** the manual pass is still written down by
hand this time (`.plan/evidence/reco-rny/fathom-on-itself.md`, with the JSON the workflow will produce from now
on), because the automation and the first human pass are the same act and the human one has to exist before the
automated one has anything to be compared against.

**Refused:** a "Fathom is happy with itself" badge, and making the self-review a required status check. The pass
found two `error` findings on `HEAD` and neither was true; a gate that blocks on the tool's own false positives
teaches everyone to distrust the gate, and the false positives are now `reco-wk4` and `reco-u1i` with the
evidence attached.

## 5 What this does not close, and what the next pass should look at

* `m0.1` and `m0.2` still gate the only criterion that matters for a beta; this ticket makes the loop able to
  receive reports, not the alpha able to be installed. `reco-lnd`, `reco-3gz`.
* The label definitions in `.github/labels.yml` are data checked against the vocabulary, but nothing applies
  them to the forge yet — no third-party label-sync action was added, because that is a supply-chain decision
  with its own review, not a side effect of a docs ticket.
* `fathom review status` is the sign-off oracle; `episodes --state` is a mirror that reconciles on the next
  index. Stated in the evidence, and worth a line in the docs if a second person trips on it.
* The self-review's numbers are the baseline now: 13 uncovered ratchet rows, a 0.1 % `diff.per_file` hit rate,
  3 of 109 episodes reviewed, architecture 92/100 with the boundaries dimension unwritten. If next week's pass
  is not measurably more boring, the loop is reading and not closing.
