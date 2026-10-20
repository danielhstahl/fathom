# Episode: the thing you review

- **Status**: the definition this product is built on (`e0.3`, **reco-ubw**); identity and segmentation
  shipped in `e1.3` (**reco-982**).
- **Decided by**: [ADR-003](../adr/0003-episodes.md) (identity, extent, the boundary cascade, re-link
  tiers), with the measured accuracy of the boundaries in
  [episode segmentation](episode-segmentation.md).
- **In code**: `crates/fathom-ingest/src/segment.rs`, `crates/fathom-core` (identity), and the glossary
  entry in [the thesis](thesis.md) that every later ticket cites.
- **Read this if**: you have just run the [quickstart](../quickstart.md) and want the noun the whole tool
  turns on.

An **episode** is the change produced by **one agent turn**: the ordered set of commits that turn
produced, plus optionally its uncommitted working-tree state. It is the atomic reviewable unit — every
review state, finding, risk score and metric in Fathom is keyed to an episode, not to a commit and not to
a pull request.

That is the answer the quickstart promises you will be able to give after five minutes. The rest of this
page is the four things people get wrong about it.

## It is not a commit, and not a PR

| | one episode is |
|---|---|
| a **commit** | one episode may have several: an agent that amended twice and merged a branch produced three commits and one thing to review |
| a **branch** | an episode has no branch of its own; it lives on a ref, in a *lineage*, and the lineage is context rather than identity |
| a **pull request** | a PR is a delivery vehicle with a review *attached*; an episode is the review, and exists whether or not anything is merged |
| a **session / turn** | a session contains many turns, and Fathom takes the turn as the *hint* about where an episode begins — it is not the unit, because turns split, resume and get compacted |
| a **diff** | a diff is text between two blobs; an episode is a unit of *work*, with an extent — which commits it covers and what it is compared against |

The reason the unit has to be the turn and not the commit is that this is how the work was actually
attempted: the question a reviewer is asked is "did what the agent meant to do happen, and is it safe",
and that question does not have a commit-sized answer.

## Identity: why it survives a rebase

Every episode carries a **content digest** built from the facts a rewrite is supposed to preserve — the
files it touched and their final content, the author, the subject and message shape — and deliberately
*not* from the SHAs of its commits. So when you amend, squash or rebase:

* the episode's **id may move** (it is derived from content, and content changed);
* the **review does not move with it**, because a review is keyed to identity plus a content fingerprint,
  and Fathom runs a ladder of re-link tests (ADR-003 §6, four tiers, each with a published price) to
  re-attach it. A re-link that cannot be made is **fail-visible**: the ledger says `orphan` and the work
  re-appears in the queue rather than being quietly considered reviewed.

This is the single design decision that makes the tool usable on real agent history, because agent history
is rewritten constantly. It is also why a [sign-off](review-ledger.md) is stored with four digests rather
than one: each digest answers exactly one question, and "was this the same thing" is not one of them.

**Consequences you can observe.** `fathom show <episode>` accepts an episode id, a revision
(`HEAD~3`, `main`, `v1.2^{}`) or an oid prefix, and prints the digests it derived the identity from. The
queue's selection follows the episode through all four re-link tiers, so a cursor does not jump when the
history moves under it.

## How boundaries are found, and what a wrong one hides

The boundary cascade is an ordered set of tests, cheapest and most certain first: harness evidence (a hook
event said "this turn, these commits"), a subject-claim rung, a shared time+author window, a declared-scope
claim, a trailer. `episode.time_gap_minutes` (default 30) is the clock rung, and it is deliberately not
first: the clock is the rung that gets it wrong most often.

The measured result — and the reason the config is what it is — is in
[episode segmentation](episode-segmentation.md): the shipped cascade is precise (0.938) and short of
recall (0.747) on 204 hand-annotated turns, and splitting every commit into its own episode scores better
on naive F1 while **hiding 23 of 74 pathologies**. A boundary that is too coarse merges two intentions; one
that is too fine dissolves the pattern a detector was going to notice. Both are wrong, and the tool prefers
the first, because a merged episode is still reviewable and a dissolved one is not.

`fathom segment explain <episode>` would be the verb for "which rung won", and it is not in this build;
the cascade's own knobs are `episode.*` keys ([config](config.md)), and the cascade itself is asserted in
`tests/test_episode_identity_contract.py` and `tests/test_segmentation_spike_contract.py`, which is where a
change to a rung has to be argued.

## Author kind is a label you can overrule

Every episode is labelled `agent`, `human` or `unknown`, with a confidence and a list of evidence — trailers,
hook events, committer-versus-author shape, and a learned correction. The label is an *estimate*, and
`fathom attribution` is the correction path:

```bash
fathom attribution list                        # what the classifier currently believes
fathom attribution correct <commit> --kind agent --note "pair session, human typed"
fathom attribution forget <identity>
```

A correction is keyed to an **identity** (an address), not to an episode id, because the identity is what
you meant and episode ids move when a lineage is rewritten. Two consequences: attribution is not a
truth you look up, it is a stance the tool holds and shows its evidence for
([attribution, `e4.1`](risk-scoring.md)); and an uncredited commit is scored as *more* risky than an
agent's, not less — nobody-credited is the worst answer to "who wrote this", and the risk model treats it
that way ([risk score](risk-score.md)).

## Provisional episodes: the ones that are not history yet

Uncommitted work can be an episode too (`kind = 'provisional'`), drawn with a distinct marker and **never**
mixed into a committed count in any headline figure — not the queue badge, not the ambient line, not a
metric. That last rule is a schema property, not a UI convention: the windows that metrics are computed
over join committed rows only, so a provisional episode cannot leak into a number by accident.

## Where an episode shows up

| Surface | Verb or key | What it answers about the episode |
|---|---|---|
| Timeline (the rail) | `t` | what happened, in order, and which of it you have not read |
| Queue | `q` | which episode to look at next, and why that one ([risk score](risk-score.md)) |
| `fathom show` | headless | identity, authorship, message, trailers, file rows with their taxonomy decision |
| `fathom analyze` | headless | the [delta](delta.md) — the claims, with citations |
| `fathom narrative` | headless | the prose version of the same claims |
| `fathom why` | headless | the [risk score](risk-score.md) as an arithmetic you can check |
| `fathom review sign` | headless | "a human accepted this", durably ([the ledger](review-ledger.md)) |

---

**Next**: [delta](delta.md) for what Fathom claims about an episode ·
[episode segmentation](episode-segmentation.md) for the boundary cascade in full ·
[ADR-003](../adr/0003-episodes.md) for the identity model it all comes from.
