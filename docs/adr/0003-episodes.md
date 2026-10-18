# ADR-003: the Episode — identity, segmentation, boundary tie-breakers and stability across rewrites

- **Status**: Accepted
- **Date**: 2026-10-16
- **Ticket / plan key**: `reco-ubw` / `e0.3`
- **Implements**: [ADR-001](0001-product-thesis.md) — *Episode* (§4.1), the whole of §4.3 (*Commit*,
  *Episode identity*, *Content fingerprint*, *Lineage*, *Provisional episode*, *Rewrite*,
  *Session / turn*), and the principles §7.4 (deterministic by default), §7.6 (degrade, never block),
  §7.7 (confidence is part of the answer), §7.8 (**stability across rewrites is a feature, not an
  implementation detail**), §7.9 (explainable before clever).
- **Constrains**: `reco-nts` (`e1.3` segmentation — already built, and §14 lists the six places this
  ADR moved rather than the code), `reco-rlf` (`e4.6` review ledger: it must reuse the digest tiers and
  the relink vocabulary in §6, not invent its own), `reco-w7q` (`e4.1` attribution: it owns alias
  resolution, and §2.6 tells it what it costs to change), `reco-552` (`e6.5` shared review memory),
  `reco-c54` (`e4.2` intent capture), `reco-wsj` (`e4.5` attention queue), `reco-4ey` (`e2.10` episode
  card: the boundary display rule §4.7), `reco-tt4` (`e2.11` `EpisodeDelta`), `reco-p3i` (`e6.4`
  `explain_episode`), `reco-1tn` (`e5.7` export), `reco-6hp` (`e1.11` session state), `reco-949`
  (`e1.9` watch mode), `reco-ld8` (`e1.10` `fathom show`, which already prints `boundary <rule> <conf>`),
  `reco-f28` (`e1.4`, the provisional half of §7), `reco-q6r` (spike `s2`, the only ticket allowed to
  move the constants in §4.4 — **closed**; §18 is its report, and it moved none of them).
- **Depends on**: ADR-001 (accepted); [ADR-004 §4.3](0004-storage.md) for the columns this writes;
  [ADR-005](0005-harness-contract.md) §2.1 and §3.3 for what a `turn_id` is; spike `s2` **has reported**
  (§18) and still deliberately gates nothing here (§13).
- **Signed off by**: the owner of the harness hook contract — `reco-3pu` (`e0.5`, ADR-005), on the
  clauses in §9. The acceptance criterion asked for that signature; §9.4 records it and where it lives.
- **Executed by**: `crates/fathom-ingest/src/segment.rs` (the rules and the digests),
  `crates/fathom-store/src/episode.rs` (the tables, the relink pass, the retire/prune rule),
  `crates/fathom-analyze/src/episodes.rs` (the wiring),
  `crates/fathom-analyze/tests/episodes.rs` (real `git` histories: amend, reword, squash, disappearance,
  the ≥90 % corpus), `crates/fathom-ingest/tests/…`, `src/recommender/segment.py` (the second
  implementation of the digests and the closed vocabularies), `src/recommender/store/git.py`, and
  `tests/test_episode_identity_contract.py` (this document as tests).

> **The two rules everything else follows from.**
> 1. An episode is addressed by **what a rewrite preserves** — who authored it, what its subject claims,
>    which paths it touches — and never by what a rewrite changes: the oid, the tree, the date, the
>    parent, the membership.
> 2. When identity cannot be *proved*, the reviewer is shown the episode again. A lost review is visible
>    work; a wrong relink is invisible false assurance. Every tie-breaker in this document is
>    subordinate to those two.

**Contents.** [0 Why](#0-why-this-document-exists-and-what-it-decides) ·
[1 What an episode is](#1-what-an-episode-is-precisely) ·
[2 Identity](#2-identity) · [3 Extent](#3-extent-members-the-parent-boundary-and-the-diff-an-episode-shows) ·
[4 Boundaries](#4-boundaries-the-cascade-and-its-tie-breakers) ·
[5 Worked examples](#5-worked-examples) · [6 Relink](#6-re-linking-how-a-decision-survives-a-rewrite-that-moves-the-id) ·
[7 Provisional](#7-provisional-episodes) · [8 Interleaved humans](#8-a-human-commit-between-agent-commits) ·
[9 Harness evidence](#9-harness-evidence-provenance-and-the-sign-off) ·
[10 Lineage](#10-lineage-and-ordering) · [11 Metrics](#11-what-the-metrics-read-from-here) ·
[12 Not decided](#12-what-this-adr-deliberately-does-not-decide) ·
[13 Deferred](#13-deferred-with-named-reasons) · [14 What moved](#14-what-this-adr-moved-instead-of-approving) ·
[15 Alternatives](#15-alternatives-considered) · [16 Consequences](#16-consequences) ·
[17 Verification](#17-verification--the-commands-that-are-the-acceptance-criteria) ·
[18 Spike s2](#18-what-spike-s2-measured-and-what-it-decided)
[17 Verification](#17-verification--the-commands-that-are-the-acceptance-criteria) ·
[Appendix A](#appendix-a--the-published-identity-vectors) · [Appendix B](#appendix-b--the-closed-vocabularies)

---

## 0. Why this document exists, and what it decides

The episode is the atomic reviewable unit, so *every* foreign key in the product points at one:
deltas, findings, risk scores, review state, the ledger, the queue, the export, the MCP tools, and three
of the five metric denominators. Nineteen tickets want to write one of those keys. An id chosen after
they are written is an id that cannot be changed, and the operation that breaks it — `git rebase -i`
before a push — is something every user of this product does weekly. ADR-001 §7.8 is unambiguous about
the class of failure: *if a history rewrite orphans review state, that is a defect with a severity, not a
limitation to document.*

The ticket set this ADR out to "content-addressed: ordered member commit SHAs plus parent boundary, plus
a fuzzy content fingerprint for re-linking". §2 keeps half of that and rejects half, and says why: the
member SHA list is the thing a rewrite destroys by definition, so it is the episode's **extent**, never
its **identity**; and "fuzzy" re-linking is declined for v1 (§13.1) in favour of three exact digests and
one shared-commit test, because a fuzzy match silently attaches a human sign-off to the wrong change.

| # | Decision | Where | Checked by |
|---|---|---|---|
| 1 | **Identity is a digest of author + normalised subject + path set**, with two weaker digests beside it; member SHAs are the extent, not the identity | §2 | `identity_matches_the_published_golden_vector`, `test_the_identity_formula_in_the_prose_is_the_formula_in_the_code` |
| 2 | **A closed eight-rule boundary cascade**, first opinion wins, no blending, one recorded reason per boundary | §4 | `test_the_boundary_table_is_the_cascade_is_the_config_vocabulary`, `segmentation_scores_over_ninety_percent_on_an_agent_shaped_corpus` |
| 3 | **Harness turn evidence outranks every git heuristic; a human's forced boundary outranks the harness** | §4.3, §9 | `a_turn_named_by_the_harness_beats_every_heuristic`, `a_forced_split_makes_a_boundary_and_the_episode_says_it_was_overridden` |
| 4 | Rewrites: amend / rebase / reword / squash / merge / cherry-pick each have a named, tested answer | §5 | the eight git-driven tests in `crates/fathom-analyze/tests/episodes.rs` |
| 5 | **Relink is three exact tiers, dead→live only, one claim per live episode, always written down** | §6 | `a_reword_moves_the_id_and_the_fallback_digest_carries_the_decision`, `…the_shape_digest…`, `a_squash_relinks_on_a_shared_commit…` |
| 6 | A partial tree **is** a provisional episode, and nothing durable may be keyed to it | §7 | `test_provisional_rules_are_stated_and_the_writer_is_accounted_for` |
| 7 | A human commit between agent commits is **its own episode**, never absorbed | §8 | `a_human_commit_between_agent_commits_is_its_own_episode` |
| 8 | **The tie-breakers are numbered (TB1–TB10, RL1–RL7)**, so a reviewer can cite one and a test can assert it | §4.6, §6.2 | `tests/test_episode_identity_contract.py` |
| 9 | The closed vocabularies (`boundary_rule`, `episode_rewrite.relation`/`method`, `created_source`) are fixed here, so `e4.6` cannot invent a fourth spelling | App. B | `test_the_closed_vocabularies_are_the_ones_the_schemas_allow` |

### 0.1 Constraints inherited, not chosen

| Constraint | Source | Consequence here |
|---|---|---|
| An episode is one agent turn, not one commit and not one session | ADR-001 §4.1 | The boundary cascade (§4) is a turn detector, not a batcher |
| Determinism: identical input, identical bytes | ADR-001 §7.4 | Segmentation reads no clock, no locale, no wall-clock order (§4.1); the walk order is total |
| No crate below `fathom-cli` reads a config file | ADR-002 §4.5 | Every tunable in §4.4 arrives as a `Rules` **value**; `segment.rs` never opens a file |
| The store is append-only; derived rows are re-derived, never migrated | ADR-004 §9.1, §10 | Episodes are rebuilt every index run and the ones nobody decided about are pruned (§6.5) |
| Sign-off must survive a rewrite | ADR-001 §7.8 | Identity (§2) and relink (§6) exist for this alone |
| Harness output is untrusted input; a missing event degrades, never blocks | ADR-005 §1.3, §3.6 | Absent turn evidence is *absence of evidence*, not a boundary (§9.2, TB6) |
| A heuristic boundary is labelled as a guess, in the UI, the JSON and the export | ADR-001 §7.7 | `boundary_rule` + `boundary_confidence` are stored, printed by `fathom show` and by the card (§4.7) |

### 0.2 New vocabulary, entered through the glossary

This ADR uses four nouns that ADR-001 did not have. Per ADR-001's own rule ("a term not listed here is
not project vocabulary yet — add it here first"), **episode extent**, **digest tier**, **episode twin**
and **fail-visible** are added to [the glossary](../concepts/thesis.md) §4.3 in this same change, and
nowhere else; `tools/check_docs.py` fails the build if any of them is defined twice.

---

## 1. What an episode is, precisely

ADR-001 §4.1 owns the definition — *the change produced by one agent turn, materialised as the ordered
set of commits that turn produced*. What this ADR adds is the edges of it:

| Clause | Statement |
|---|---|
| **E1** | An episode is a non-empty, contiguous run of commits on one lineage. One or more; never zero. |
| **E2** | A turn that produced no commit produces **no episode**. There is nothing to review, and an empty card in the queue spends the attention the queue exists to save. The turn is still in the event log (ADR-005), which is where "the agent said it was done and changed nothing" is visible. |
| **E3** | A commit belongs to exactly one **live** episode per segmentation run. Membership is disjoint *within* a run; the same commit may appear in the history of two episodes across a rewrite (the dead one keeps its row, §6.5). |
| **E4** | An episode is **derived**, never authoritative: every index run re-derives the whole set (§10.2). Deleting the episode rows and re-deriving them must reproduce the same ids byte-for-byte. |
| **E5** | An episode is not a branch, a PR, a session, a commit, or a diff. It is the unit a human can be asked "did you look at this?", which is a question about a turn's *intent*, not about a git object. |
| **E6** | Everything reviewable hangs off the id: review state, findings, risk, delta, ledger entries, export rows. Nothing may key a durable claim to a commit oid — `episode_member` is a join table for display and diffing, not an address. |

---

## 2. Identity

### 2.1 The inputs, and what is deliberately left out

Identity is computed from **the first member commit of the episode** — the commit that opened it — and
from these five facts about it. The exclusions are the load-bearing part of the table: each one is out
because a rewrite that must not move the id would otherwise move it.

| Input | In the strong digest | In the `fallback` | In the `shape` | Why in, or why out |
|---|---|---|---|---|
| Author identity: `author_email` lower-cased and trimmed, else `author_name` | ✓ | ✓ | ✓ | Preserved by every rewrite except `--reset-author` (§2.6) |
| Normalised subject (first line: lower-cased, `fixup!`/`squash!`/`amend!` and one leading conventional type stripped to a fixed point, whitespace collapsed) | ✓ | ✓ | — | The claim the change makes. Survives amend, rebase and a `fixup!` squashed into its target |
| Sorted, de-duplicated path set touched by that commit (`git_commit_file.new_path`, i.e. post-change paths) | ✓ | — | ✓ | The shape of the change. Sorted because git reports paths in tree order and a rewrite reorders nothing that matters |
| Member commit oids | ✗ | ✗ | ✗ | **The thing a rewrite exists to change.** A digest that includes them is a digest that moves on rebase — the bug this section exists to kill |
| Parent oids | ✗ | ✗ | ✗ | The extent and the diff endpoint (§3), not the identity |
| Tree oid | ✗ | ✗ | ✗ | Unchanged by a rebase, but changed by the amend that fixes the typo in the comment — exactly the case that must *not* move the id |
| Author/committer dates | ✗ | ✗ | ✗ | `rebase` re-stamps them by default |
| Committer identity | ✗ | ✗ | ✗ | Changes for reasons that mean nothing about the change (CI bots, `-c user.…` overrides) |
| Message body and trailers | ✗ | ✗ | ✗ | Trailers carry turn evidence; an amend that re-stamps a trailer is not a new change |
| Hunk text, `n_add`, `n_del` | ✗ | ✗ | ✗ | A squash combines hunks; making identity depend on membership makes identity move on `git add` |
| `session_id` / `turn_id` | ✗ | ✗ | ✗ | Evidence about *boundaries* (§9), not about content |
| Which ref or branch it is on | ✗ | ✗ | ✗ | The same change moved between branches is the same change (§5.9) |

### 2.2 The three digests

```
normalize(s) = lowercase · strip fixup!/squash!/amend! and one leading conventional type,
               repeatedly to a fixed point · collapse whitespace
paths(C)     = sorted, de-duplicated set of post-change paths of C's first member
author(C)    = lower-cased author email, or lower-cased author name when there is none

fingerprint            = sha256("fathom-episode/1" 0 author 0 normalize(subject) 0 path 0 …)
fallback_fingerprint   = sha256("fathom-episode/1" "fallback" 0 author 0 normalize(subject))
shape_fingerprint      = sha256("fathom-episode/1" "shape"    0 author 0 path 0 …)
```

`0` is one NUL byte: framing, not concatenation, so no subject can be written that collides with a path
list. All three are digests of **facts a rewrite preserves**, and each trades one fact away, which is the
whole reason there are three:

| Digest | Over | Survives | Breaks on | Used by |
|---|---|---|---|---|
| `fingerprint` | author, subject, paths | `--amend`, `--no-edit` (even with new content), a plain `rebase -i`, `rebase --onto`, a force-push, a `fixup!` squashed into its target | a reword; a squash that adds files; a split; `--reset-author` | it **is** the id (§2.3) |
| `fallback_fingerprint` | author, subject | an amend or squash that moved files, a rebase that also moved files | a reword | relink tier 1 (§6.2) |
| `shape_fingerprint` | author, paths | a reword, including one performed mid-rebase | a move/rename of the touched files, a split | relink tier 2 (§6.2) |

Three digests, three failure shapes, and a fourth test that is not a digest at all (a shared member
commit) for the case where the change was both reworded *and* moved. §6.3 is honest about the residual
case those still miss.

### 2.3 The id itself

```
episode_id = "ep-" + hex(fingerprint)[0..20]
```

Twenty hex characters (80 bits) because the id is printed in an 80-column rail next a summary, and
because the collisions that actually happen are *twins*, not birthday attacks: two byte-identical
changes — the same fix cherry-picked to two branches, the same "bump version" commit made twice —
produce the same digest. **Rule TB10**: when two live episodes in one run share a digest, the first in
walk order keeps the bare id and each later **twin** gets `-2`, `-3`, … Twins are separate episodes with
separate review state (§5.9), and the suffix is allocated by the walk order (§4.1), which is total and
deterministic, so the allocation is reproducible.

**Appendix A** publishes the exact digests for three worked fact-sets. Two independent implementations
must reproduce them byte-for-byte: a rule that exists once is a description, not a rule.

### 2.4 The namespace, and when it moves

`IDENTITY_SCHEMA = "fathom-episode/1"` is the first bytes of every digest. It moves — `fathom-episode/2`
— if and only if a change would otherwise silently re-address existing episodes: the input set (§2.1),
the normalisation (`normalize`), the framing, or the author-resolution rule (§2.6). The bump is the
mechanism ADR-004 §6's invalidation row *episode segmentation rule change* refers to: ids move, so
episode-scoped results are re-derived, and old ids stop resolving. **Bumping it is not a migration**:
the store is class R (ADR-004 §10), so "re-derive" is the whole recovery plan, and `e4.6`'s ledger is the
one place where an old id may still need to resolve — which is why §6.4's rewrite ledger exists.

### 2.5 What identity is *not*, and where the ticket's other half lives

The ticket's "ordered member commit SHAs plus parent boundary" is not the identity. It is the episode's
**extent**, and it is stored, because three things genuinely need it:

| Needs the extent | Needs it for |
|---|---|
| the diff an episode shows | `parent(first member)` → `last member` (§3.2) |
| the rework detector (§5.3's R1/R2) | which commits' content counts as *this episode's* |
| relink tier 3 | "the digest moved entirely, but a commit is still here" |

Identity answers *is this the same thing?*; extent answers *which commits, and what is the diff against?*
Collapsing the two is what makes a rebase orphan a review: the extent changes on every rebase, the
identity must not.

### 2.6 Authorship is part of identity, on purpose

`author(…)` is the lower-cased email, so `git commit --amend --reset-author` **does** move the id, and a
person who sets `user.email` differently tomorrow gets different ids tomorrow. That is chosen, not
inherited: authorship is one of the three facts the digest claims, and pretending it is not would make
the digest's meaning depend on a case-by-case exception nobody can remember. Consequences, accepted:

- `e4.1` (`reco-w7q`) owns author *aliasing* (one human, three addresses). When it decides two addresses
  are one author, that decision changes identity and therefore **must bump `IDENTITY_SCHEMA`** and relink
  whatever it re-addresses. This ADR tells it the price now, so it does not discover it in `e4.6`.
- Attribution confidence (§ `author_kind`, §8) is a **separate** question with a separate column and a
  separate confidence, and it is *not* part of identity. `e4.1` may change attribution without moving a
  single id.

### 2.7 The invariants

These are the clauses other tickets may quote. Each is checked; the check is named.

| # | Invariant | Check |
|---|---|---|
| **I1** | **Episode identity is stable across `git rebase -i` of the same content.** Picking, reordering, re-dating and re-parenting commits — while each keeps its author, subject and path set — changes no episode id. | `a_rebase_preserves_identity_and_the_weaker_digests_cover_what_it_cannot`; `test_rebase_of_the_same_content_does_not_move_the_id` |
| **I2** | **A decision never disappears with the history that carried it.** If the id survives, the decision is already there; if the id moved, a relink tier carries it; if no tier fires, the episode comes back **unread**, never silently "already reviewed". | `a_reviewed_episode_that_disappears_is_kept_and_marked_dead`, §6.3 |
| **I3** | Identity depends only on §2.1's inputs. It is a pure function of a commit's own facts: adding a commit to an episode does not move its id, and neither does rewriting a *later* member. | `identity_matches_the_published_golden_vector` |
| **I4** | No two live episodes share an id (TB10). | `a_cherry_pick_is_a_second_episode_that_shares_the_digest_not_the_id` |
| **I5** | Two runs over the same repository state produce byte-identical episodes: same ids, same member order, same summaries, same confidence. | `writing_the_same_episodes_twice_changes_nothing_and_claims_nothing` |
| **I6** | Every boundary names the rule that made it and prices it (§4.2). No boundary is unexplained. | `one_agent_turn_is_one_episode_and_the_boundaries_say_why` |
| **I7** | A live episode is one that some current commit belongs to. No ghost episode that only rewritten-away commits belonged to stays in the queue. | `squashing_leaves_no_ghost_episode_beside_the_one_it_replaced`, `episode_inputs_follow_the_refs_and_drop_what_a_rebase_left_behind` |

---

## 3. Extent: members, the parent boundary, and the diff an episode shows

**E1** already said the members are a contiguous run on one lineage. The rest of the geometry:

| Clause | Statement |
|---|---|
| **X1** | Members are stored in **walk order** in `episode_member.ordinal`, oldest-first along the lineage. `first_oid` / `last_oid` are the ends; `n_commits`, `n_files`, `n_add`, `n_del`, `n_binary` are sums over members and are **derived**, rewritten every run (ADR-004 §4.3's "not authoritative" columns). |
| **X2** | The **start boundary** of an episode is the pair `(first_oid, parent(first_oid))`. Storing the pair, and not just the first oid, is what makes "which episode owns this commit?" answerable after a rewrite and what makes a split auditable. |
| **X3** | The **diff an episode shows** is `parent(first_oid) … last_oid`, following the **first parent** at a merge. For a normal episode that is "what this turn produced"; for a merge episode (§5.8) it is precisely "what the merge brought in", which is the only question a reviewer asks of a merge. |
| **X4** | An episode's *tree endpoint* is `tree(last_oid)`, used for tree-scoped analysis (ADR-004 §6's `warm.reindex` rows). It is an endpoint, not an identity (§2.1). |
| **X5** | The **parent boundary never appears in the id.** A rebase changes it for every commit in the range; I1 forbids the id from moving. |
| **X6** | Membership is replaced, not merged, on every write: a commit that moved into another episode must stop being pointed at the old one, and membership of a *dead* episode is left alone so its diff stays renderable (§6.5). |

---

## 4. Boundaries: the cascade, and its tie-breakers

### 4.1 The walk, which is where all the determinism lives

Segmentation is a single ordered pass over the commits the current refs can reach.

- **W1** The order is a real topological order over stored parents (parents before children), with
  `(author date, oid)` as the tie-break. **The wall clock is never read**, and a clock that lies — a
  rebased commit dated before its own child, a laptop in the wrong timezone — is normalised by the
  topological order rather than trusted. (`episode_inputs_are_topologically_ordered_when_the_clock_lied`.)
- **W2** Only ref-reachable commits are walked. The store is append-only, so rewritten-away commits are
  still in `git_commit` as an audit trail; segmenting them would show the reviewer their change twice —
  the one they are reviewing and the ghost its rewrite replaced. (I7;
  `episode_inputs_follow_the_refs_and_drop_what_a_rebase_left_behind`.)
- **W3** The whole lineage is re-segmented on every run, warm or cold. A boundary is a property of a
  *neighbourhood* — the commit before a new one decides whether the new one starts an episode — and an
  incremental-only rule would miss a force-push that rewrote a neighbourhood without adding rows. It
  costs milliseconds over rows already in SQLite and it is idempotent, because of I1.
- **W4** `fathom.toml` never reaches this code as a file read: the tunables in §4.4 are handed in as a
  `Rules` value by `fathom-cli` (ADR-002 §4.5). A crate that reads config to decide how to analyse has
  cache keys that lie.

### 4.2 The cascade

Eight rules, tried in this order. **First opinion wins**, and the rule that won is recorded on the
episode with the confidence in the last column.

| Pri | Rule | Class | Fires when | Conf |
|---|---|---|---|---|
| 1 | `override` | human | `fathom.toml` forces a split before this commit, or forbids one | 1.0 |
| 2 | `hook` | evidence | this commit and the previous one both name a harness turn, and the turn ids differ | 1.0 |
| 3 | `trailer` | evidence | an `Agent-Turn:` trailer (or the reserved `Fathom-Episode:`) names a turn different from the previous commit's | 0.9 |
| 4 | `branch` | graph | this commit is a merge, or the previous one was | 0.8 |
| 5 | `author` | heuristic | the author identity changed | 0.75 |
| 6 | `time-gap` | heuristic | the gap since the previous commit exceeds `episode.time_gap_minutes` | 0.6 |
| 7 | `message-shape` | heuristic | the subject is a **continuation** → keep; or a changed conventional type elsewhere in the tree after a clock move → split | 0.4 |
| 8 | `root` | heuristic | nothing above fired and there is no previous commit | 0.5 |

Class matters: `human` is a boundary a person drew, `evidence` is a statement someone *made* (in an
event, a trailer, or a config file), `graph` is a fact about the commit graph, and `heuristic` is a guess
Fathom is making on the reviewer's behalf. **Evidence outranks heuristic** is the shape of the cascade;
the ordering *within* each class is argued below, because "why is a merge above an author change" is a
question a reviewer is entitled to ask.

**Amended by `e6.1` (`reco-afh`), per §18.2 D5.** Rung 3's *key* moved from `Fathom-Episode:` to
`Agent-Turn:`, because that is the spelling the corpus, `src/recommender/store/git.py` and §9.1's `hook`
transport already use, and `e6.1` is the writer: `fathom_ingest::hooks::trailer_block` emits
`Agent-Session:` and a session-qualified `Agent-Turn:`; the keys themselves are defined once in
`fathom_ingest::segment` (`TRAILER_TURN_KEY` / `TRAILER_TURN`) and re-exported by `fathom_ingest::hooks`, so
writer and rung 3 cannot drift — which is exactly what D5 was afraid of.
`Fathom-Episode:` stays **reserved**: it is the day Fathom itself declares an episode start, it is still
read, and nothing writes it. The `boundary_rule` vocabulary in Appendix B did not move, which is the whole
point of D5 — `trailer` names a class of evidence, not a key.

> **Read with §18.** Spike `s2` measured this cascade against 204 hand-annotated turns. The table above is
> still exactly what `crates/fathom-ingest/src/segment.rs` does — §18 changed no code — but §18.2 decides
> one precedence change to it (a `subject-claim` rung above `time-gap`), scheduled as `e1.13`
> (`reco-quk.2`) by the `e0.12` sequencing checkpoint and still not landed, and records that rung 3 reads a
> trailer nothing writes yet: §18.2 D5, decided by `e0.12`, names `e6.1` (`reco-afh`) as the writer of the
> trailer that exists rather than leaving the rung documented as live.

### 4.3 Why this order, edge by edge

| Edge | Argument |
|---|---|
| `override` > `hook` | A forced boundary is a human acting deliberately, with a name and a config line behind it; the harness is automated evidence. The one is configuration and the other is a measurement, and when the operator says "cut here", the cut is the answer. The episode also sets `overridden = 1`, so a forced edge is never mistaken for an observed one. |
| `hook` > `trailer` | Same class, differing in directness: a turn event is the harness speaking, a trailer is a note left in git by whoever wrote the commit message and may be stale, copied, or a lie. |
| `hook`, `trailer` > `branch` | A named turn is intent; a merge is a topology. A turn that ends in a merge is one reviewable unit, not two (§5.8), and no heuristic should outvote a named boundary. |
| `branch` > `author` | Two authors can collaborate inside a merge's incoming branch, but the merge itself is still a distinct reviewable decision. Also: without this edge, a merge by the same author would be absorbed into the surrounding turn and the reviewer would never see the merge. |
| `author` > `time-gap` | A different author is a different turn *always*, and the rule cuts both ways (§8). The clock is a proxy for "a new decision"; the author *is* a new decision. |
| `time-gap` > `message-shape` | Two hours apart is a new turn whatever the subjects say; a subject line is a weak signal dressed as a strong one. |
| `message-shape` last | An agent routinely does `feat:` → `fix:` → `test:` inside one turn, so a conventional-type change is the weakest splitting signal in the set, and it is deliberately handicapped (TB7). Continuation markers (`wip`, `fixup!`, `address review`, `typo`, `fmt`, …) keep, because the commonest segmentation error is cutting one turn's follow-up commits into a second episode. |
| `root` | A root commit is not a boundary; it is the absence of one. It gets its own rule so that "we had to start somewhere" is distinguishable from "a rule fired", at the mid-point 0.5: no evidence either way. |

### 4.4 The constants, and who may move them

| Constant | Default | Overridden by | Notes |
|---|---|---|---|
| gap | **30 minutes** (`episode.time_gap_minutes`) | `fathom.toml`, user layer, `--set` | The published default in `schemas/fathom.schema.json`, `docs/concepts/config.md`, and `fathom.toml`, and now the compiled-in Rust default too (§14.3). A contract test asserts all four agree. |
| type-change guard | gap ÷ 9 (**~3.3 min**) | derived, not settable | TB7's clock condition. Derived rather than a second knob, so one number is what a reviewer reasons about. |
| continuation markers | `wip`, `fixup!`, `squash!`, `amend`, `address review`, `review feedback`, `typo`, `formatting`, `fmt`, `lint`, `follow-up`, `followup`, `missed`, `oops` | `Rules` value | Single-word markers are **prefix** tests, phrases are **substring** tests (TB8). |
| agent markers | `bot`, `agent`, `codex`, `copilot`, `claude`, `llm`, `assistant`, `cursor`, `devin`, `aider`, `openhands`, `swe-agent`, `autofix` | `Rules` value | Used for `author_kind` (§8), which is *not* identity (§2.6). |
| forced edges | none | `episode.force_split`, `episode.force_merge` | Two spellings only: `oid:<prefix>` and `subject:<substring>`; a value that is neither is named on the next run, never dropped silently. |

**`s2` (`reco-q6r`) is the only ticket allowed to move the numbers**, and it moves them with measurements
rather than with a feeling. **It has reported (§18.2), and it moved none of them.** The gap's measured
pooled optimum is 10 minutes rather than 30, which buys +0.022 boundary F1 and *costs* hidden pathologies,
and once §18's `subject-claim` rule lands the knob is flat within ±0.02 across 5–60 minutes — so 30 minutes
stays the published default, deliberately, with the measurement written down instead of the guess quietly
left standing. The rest of §4.4's constants were not re-tuned and remain guesses with printed prices.

### 4.5 `episode.boundaries` is a restatement, not a control

The cascade order **is not configurable in v1**. The config key exists, is typed `replace` (merging a
priority order produces an order nobody chose), and its only honoured value is the canonical prefix
`["hook", "trailer", "author"]`. Any other value is *recognised and refused on the run that ignored it* —
a notice naming the value and pointing here, never a quiet difference in the result. The enum is the
rule names in Appendix B, so the config vocabulary and the stored `boundary_rule` vocabulary are one
vocabulary: `time`/`merge` in the schema and `time-gap`/`branch` in the store was a drift, and §14.5
fixed it.

Why not honour a reordering, since the plumbing exists? Because the ordering encodes an argument (§4.3)
about which evidence is trustworthy, and a user-level reorder would produce episodes whose boundaries can
no longer be explained by a document. `force_split` / `force_merge` are the supported way to disagree:
they are specific, named, per-boundary, and recorded on the episode as `overridden`.

### 4.6 Tie-breakers (the acceptance criterion's "written down and testable")

**Boundary tie-breakers.**

| # | Rule |
|---|---|
| **TB1** | Exactly one rule is recorded per boundary — the first with an opinion. There is no blending, no weighted sum, no voting between rules: a score that mixes a 1.0 event with a 0.4 guess produces a number that cannot be falsified, and ADR-001 §7.9 is explicit about which side that argument loses. |
| **TB2** | Order is strict and total: `override > hook > trailer > branch > author > time-gap > message-shape`, with `root` as the no-previous-commit case. Two rules firing at once is the *normal* case (a new author after a two-hour gap fires two), and the cascade is what makes "why is this a new episode?" a one-word answer. |
| **TB3** | A rule that would split and a rule that would keep never negotiate: the higher rule's verdict stands, and the lower one is not recorded. The `overridden` flag is the only place a second opinion is kept. |
| **TB4** | Two commits naming **different** turn ids split, unconditionally (1.0): no heuristic may un-cut a boundary the harness drew, including the clock. |
| **TB5** | Two commits naming the **same** turn id never split, *even across a large gap* — an agent that paged a human and came back 40 minutes later is one turn. `hook` returns "keep" and stops the cascade rather than falling through to `time-gap`. |
| **TB6** | Turn evidence on **one** side only is not evidence of a boundary: absence of evidence is not evidence of a boundary (ADR-001 §7.7 — `unknown` is a value, not a vote). The cascade falls through to the next rule. |
| **TB7** | A changed conventional type splits only when three conditions hold **jointly**: the type changed, **no path is shared** with the previous commit, and more than gap÷9 of clock has passed. Two turns' worth of intent must be visible in more than the subject line, because `feat:`→`fix:`→`test:` inside one turn is the common case this rule would otherwise shred. |
| **TB8** | A single-word continuation marker must be a **prefix** ("lint the new module" keeps, "add a lint rule" does not); a phrase marker may appear anywhere, because a phrase is specific enough to mean the same thing wherever it sits. |
| **TB9** | Within a rule, ties are broken by walk order (W1), never by insertion order, filesystem order, or the clock. Same store ⇒ same episodes: I5. |
| **TB10** | Live-id collisions go to walk order: first keeps the bare id, later twins get `-2`, `-3` (§2.3). A suffix is a *disambiguator within a run*, never part of a durable reference: §13.4 explains why the store re-resolves rather than reusing a stale suffix. |

**Relink tie-breakers** are numbered RL1–RL7 in §6.2, because "which dead episode does this decision
belong to now?" has the same shape of problem and deserves its own numbered answers.

### 4.7 Confidence is a price, not a probability

`boundary_confidence` is an **ordinal on a 0–1 scale that prices how much a boundary should be trusted**,
not a calibrated probability, and it is never multiplied into a risk score as though it were. Its three
jobs: rank guesswork against evidence in the queue, drive the display rule below, and make a wrong guess
traceable to a class of rule. Calibration belongs to `e4.4`; **the display rule** (which `e2.10` and
`fathom show` both honour — the CLI prints the pair today) is: any boundary **below 0.6** prints its rule
name beside the episode. The store already keeps the counter this rule needs —
`v_unknowns`'s `boundary_low_confidence` row counts `boundary_confidence < 0.6`, in the same view that
makes "unknown is a first-class value" (ADR-001 §7.7) a queryable fact — so the threshold is one number in
two places, and `time-gap` (0.6) sits exactly on the line: it is the last honest heuristic rather than a
guess, and printing it is `e2.10`'s editorial choice rather than this ADR's requirement.

---

## 5. Worked examples

Each example is a rewrite a reviewer actually performs, what the store does, what the reviewer sees, and
the test that pins it. `id` means the derived episode id; `§6 tier N` means the relink tier that carried
the decision across.

### 5.1 `git commit --amend --no-edit`, content changed

```
before   A ── B(5f2a)                      ep-9461… {B}   state: accepted
after    A ── B′(c810)  same subject,
                         same author,
                         same paths       ep-9461… {B′}  state: accepted
```

New oid, new tree, new date. Identity **unchanged**, because §2.1's three facts are untouched. The store
upserts the same primary key, so the review row was never detached and **no relink is needed or
performed** — the common case must be shown to be handled by doing nothing, not by cleverness. The member
list points at the commit that exists now (`X6`); `episode_rewrite` stays empty.
Test: `amending_a_commit_leaves_the_episode_and_the_review_exactly_where_they_were`.

### 5.2 `--amend` that also adds a file

Paths grow, so the strong digest moves and the id moves with it; the subject did not, so **tier 1**
(`fallback`, author+subject, score 0.9) carries the decision to the new id and writes
`(old, new, relation=rebase, method=fingerprint, score=0.9)`. The old row is kept, marked dead, because
somebody made a decision about it (§6.5).
Tests: `a_rebase_preserves_identity_and_the_weaker_digests_cover_what_it_cannot` (the digests),
`a_reword_moves_the_id_and_the_fallback_digest_carries_the_decision` (the carry — the store test is
*named* for a reword, but what it pins is tier 1 firing, which is the same join an amend that grew its
file list uses).

### 5.3 `git commit --amend --reset-author`

Authorship is one of the three facts (§2.6), so all three digests move and **no tier fires**: the change
comes back unread. Accepted as a consequence of taking authorship seriously, and fail-visible (§6.3)
rather than silently trusted. `e4.1`'s aliasing is the remedy when the two identities are one person.

### 5.4 `git rebase -i` with picks only (and its cousins `rebase --onto`, `git push --force`)

Every commit in the range gets a new oid and a new date and a new parent; nothing else changes. By I1 no
id moves, so **every** review state in the range survives with no relink, no ledger churn and no re-review.
This is the acceptance criterion's stated invariant, and it is the case the digest design was built for:
the member SHAs, the parent boundary and the tree all moved, and the identity did not care.
Tests: I1's two checks, plus `a_rewrite_that_keeps_the_digest_keeps_the_review_state_by_doing_nothing`.

### 5.5 `rebase -i` with `reword`

```
before   A ── B(ep-aaaa) ── C(ep-bbbb)          B accepted
after    A ── B′(ep-zzzz) ── C(ep-bbbb)         reworded only
```

B's strong digest moved (subject) and its fallback digest moved with it, but the path set is unchanged,
so **tier 2** (`shape`, author+paths, 0.7) carries the accept onto `ep-zzzz` and records
`relation=rebase, method=shape`. C is untouched — a rewrite is not an excuse to re-queue an unchanged
neighbour. The rail shows one episode per turn, not six, and the dead row keeps its decision so the ledger
can be read from either end.
Test: `a_reword_in_a_rebase_moves_the_id_and_the_review_state_follows_it`.

### 5.6 `fixup!` + `rebase -i --autosquash`, and squash under a new subject

`normalize_subject` strips `fixup!`/`squash!`/`amend!` **to a fixed point**, so the fixup commit and its
target normalise to the same claim: while the fixup exists it groups with its target (TB8 keeps them in
one episode), and squashing it in does not change the surviving subject. What changes is the *path set*
of the surviving commit if the fixup touched other files — which is 5.2 again, carried by tier 1.

Squashing two turns into one commit under a **brand-new** subject is a different event: no digest of
either survivor matches, so the carry rides on **tier 3** (a shared member commit) with
`method=member-set, score=0.6`. One episode remains, no ghost of the pair survives (I7), and the decision
is on the episode that exists now.
Tests: `a_fixup_subject_normalises_to_its_target`, `squashing_leaves_no_ghost_episode_beside_the_one_it_replaced`,
`a_squash_relinks_on_a_shared_commit_and_a_prune_removes_what_nobody_reviewed`.

### 5.7 The PR squash-merge

The GitHub-style "Squash and merge" is 5.6 with a new branch under it: the PR's commits vanish from the
target branch and one commit remains whose subject is usually the PR title. **The decision is carried by
tier 1 when the title matches the first commit's subject** (the usual case, because the host pre-fills it)
and by tier 3 when the human retyped it. What is *not* done is treating the PR as the episode: the PR is
a host object with a number, and the episodes are the turns inside it — which is what lets the rail say
"this squash-merge contains the three turns you already reviewed" instead of presenting a fourth
duplicate of their content.

### 5.8 A merge commit, and a merge train

```
    B1 ── B2 ─┐
  A ────────── M ── D          episodes: A, B1 B2 (one turn), M, D
```

- **A merge commit is its own episode** (`branch`, 0.8). Reviewing "the merge of branch X" and reviewing
  the commits on branch X are two different questions, and collapsing them loses the merge entirely.
- **The commit after a merge starts a new episode.** A post-merge fixup is a different unit than the
  merge it fixes.
- **A train of merges is N episodes**, one per merge, including a bot's auto-merge train. Deliberate: each
  merge brought a different set of commits, has a different author, and is the place a pathology entered.
  The cost — a rail full of merges in a merge-heavy repo — is answered by the rail's author and
  path filters plus `dismiss`, not by folding merges together and hiding which one did the damage
  (§13.3 defers coalescing until there is a measurement of how often it hurts).
- The merged branch's own commits are segmented independently and are **not** re-absorbed into the merge's
  member list; a commit still belongs to exactly one live episode (E3).
- The diff a merge episode shows is first-parent-relative (X3): what the merge brought in.
Test: `a_merge_is_its_own_episode`.

### 5.9 Cherry-pick: the twin

A cherry-pick preserves **author** by default, and normally preserves subject (with `-x`, the
`(cherry picked from commit …)` note goes in the *body*, which is not an identity input) and path set.
So the pick and its source have the same digest — and per TB10 they are two episodes with ids `ep-x` and
`ep-x-2`.

| Clause | Statement |
|---|---|
| **C1** | A cherry-pick is a **separate episode** from its source. Same bytes, two places, two reviews: a sign-off on `main` is not a sign-off on the release branch, and silently sharing one would be the worst kind of lie this product could tell. |
| **C2** | Relink is **never** live→live (§6.1 RL4), so a decision on the source never propagates to the twin. |
| **C3** | `episode_rewrite.relation='cherry'` is reserved for the writer that will link twins — `e4.6`'s ledger, which can read the `-x` trailer. No writer claims it today (§6.4). |
| **C4** | A backport whose subject is deliberately different (`[6.2] fix the crash`) is a different episode with a different id, and *nothing carries across*. Correct: different code in a different branch is a different review, and this is the case §13.1's fuzzy matching was declined for — a guessed link would say "you already reviewed this". |
| **C5** | Rebase-merge and squash-merge PR flows are §5.4 and §5.7; they are not twins. |
Test: `a_cherry_pick_is_a_second_episode_that_shares_the_digest_not_the_id`.

### 5.10 Force-push that drops work, and the split

Dropping commits (`reset --hard`, a force-push that loses a turn) retires the episodes whose commits are
gone: **live = 0**, then pruned if nobody decided about them, kept and visible if somebody did —
"you dismissed this; its commits are gone" is a displayable fact, not a vanishing row (§6.5).

Splitting one commit into two produces two live episodes whose digests differ from the original's, and
one of them shares a member with it, so tier 3 attaches the decision to that one and the other arrives
**unread**. That asymmetry is the fail-visible rule (§6.3): one of the halves needs a second look, and
the product says so rather than assuming half a review is enough.
Tests: `a_reviewed_episode_that_disappears_is_kept_and_marked_dead`, `a_squash_relinks_on_a_shared_commit…`.

---

## 6. Re-linking: how a decision survives a rewrite that moves the id

Identity handles the rewrites that preserve §2.1's facts. Re-linking handles the ones that do not. Both
are needed: an ADR that ships only identity has a nice invariant and a hole in it the size of `reword`.

### 6.1 The pass, and its one hard limit

Runs inside the same transaction as the episode write, in four steps: **write** (upsert every live
episode, stamping `seen_utc`), **retire** (anything this run did not stamp is `live = 0`), **relink**
(carry review state from dead episodes to live ones), **prune** (§6.5). The order is not cosmetic:
relink is written in terms of live and dead episodes, so retiring has to happen first or there is nothing
to relink from.

> **RL0 — relink is dead→live, and only ever into an episode that has no decision of its own.** Never
> live→live, and never over an existing review row. A reviewer's act outranks the machine's guess, always.

### 6.2 The tiers, and the tie-breakers

| Tier | `method` | Evidence | `relation` | Score | Typical cause |
|---|---|---|---|---|---|
| 1 | `fingerprint` | shared `fallback_fingerprint` — author + normalised subject | `rebase` | 0.9 | an amend that added files; a squash that kept the subject |
| 2 | `shape` | shared `shape_fingerprint` — author + path set | `rebase` | 0.7 | a reword, including one in the middle of a rebase |
| 3 | `member-set` | at least one commit in common | `squash` | 0.6 | a split; a rewrite that reworded **and** moved files |

| # | Tie-breaker |
|---|---|
| **RL1** | Tiers are tried in that order, and a stronger tier's answer is never revised by a weaker one. |
| **RL2** | A live episode is claimed by **at most one** relink: the first tier that finds a candidate, and the first candidate within that tier. |
| **RL3** | Within a tier the candidate is the **closest in time** (`ORDER BY dead.last_utc DESC, dead.created_utc DESC`) — the only ordering a rewrite leaves us with, and the one that matches how a rewrite actually works: the dead row just below the live one is almost always its predecessor. |
| **RL4** | A dead episode may be carried to **more than one** live episode. That is not an accident to guard against: it *is* the split and the twin, where one decision genuinely now concerns two changes. Each landing is written separately. |
| **RL5** | Every relink writes `episode_rewrite(old_id, new_id, relation, method, score, at_utc)`. "Did my sign-off survive the rebase?" is a query, not a hope — ADR-004 §4.3's comment says the stability clause is only testable if the rewrites are recorded, and this is where they get recorded. |
| **RL6** | The carried row keeps the original `state`, `actor` and `note`, and takes the **new** episode's fingerprint. An actor and a note are claims about a human act; a fingerprint is a claim about content, and the content changed. |
| **RL7** | A relink never invents a decision. `unread` is copied like any other state and stays `unread`: what travels is the *fact that a human was assigned or looked*, not a verdict nobody gave. |

### 6.3 The case nothing catches, and the rule that governs it

Reword **and** move files **and** rewrite the oids (a rebase that edits the message and the file list at
once): all three digests move, no member survives, and no tier fires. The honest position:

- The live episode arrives with **no review row**, i.e. `unread`, and re-enters the queue.
- The dead episode keeps its row and its decision, flagged `live = 0`, and the two are simply not joined.
- Nothing claims the change was reviewed. **Fail-visible**: a reviewer re-reviews something they had
  already signed off — annoying, recoverable, and visible — rather than having a decision silently
  attached to a change they never saw, which is unrecoverable and invisible.

That asymmetry is the whole of the tie-break: this product's currency is trust, and the only way to spend
it is to assert something that is not true. §13.1 declines fuzzy matching for the same reason, and names
what would have to be true to revisit it.

### 6.4 Which writers exist today (no more than that)

`relation ∈ (amend, rebase, squash, split, cherry, merge, other)` and
`method ∈ (exact, fingerprint, shape, member-set, manual)` are the **closed** vocabularies (Appendix B),
and the CHECK constraints in both schema copies enforce them. Today:

| Value | Written? | By |
|---|---|---|
| `relation=rebase`, `method=fingerprint` / `shape` | **yes** | tiers 1–2 |
| `relation=squash`, `method=member-set` | **yes** | tier 3 |
| `relation=amend` / `split` / `merge` | not yet — reserved so the column never has to change | `e4.6`, or a `fathom episode link` verb (`e1.10`) |
| `relation=cherry` | reserved (§5.9 C3) | `e4.6`, from the `-x` trailer |
| `method=exact`, `method=manual` | not yet — `manual` is the operator's answer to §6.3, and `exact` is a same-id relink | `e4.6` / `e1.10` |

The vocabulary is fixed **now** precisely because `reco-rlf` (`e4.6`) is the next ticket down the chain and
needs to store a link method in a committed, signed file. If it invented `ledger-fingerprint` while this
file says `fingerprint`, the two would have to be reconciled in a migration of something that must never
need one.

### 6.5 Live, dead, and pruned

| State | Who | Why |
|---|---|---|
| `live = 1` | every episode a current commit belongs to | the rail and the queue read these |
| `live = 0`, kept | dead **and** carries a review row or is a relink endpoint | "you accepted this; its commits are gone" must stay readable, and the ledger must not point at nothing |
| pruned | dead, no decision, no inbound/outbound rewrite | derived rows are disposable (ADR-004 §10, class R) |

---

## 7. Provisional episodes

ADR-001 §4.3 defines a **provisional episode** as an episode built from uncommitted work and says nothing
may be permanently keyed to a provisional identity. Here is what that means operationally.

| Clause | Statement |
|---|---|
| **P1** | A partial tree **is** a provisional episode: a dirty or staged working tree with non-noise changes and no commit covering them is a real, visible unit of work-in-progress, not a committed episode that happens to be incomplete. |
| **P2** | It is stored with `kind='provisional'`, always drawn with that marker (a distinct rail glyph and the word), and it is **never** mixed into the committed episode count in any headline figure — not the queue badge, not the ambient line, not a metric. The schema already agrees: `v_lineage_window`, the window the rework guardrail is computed over, joins `b.kind = 'committed'` only, so a provisional episode cannot enter a metric even by accident. |
| **P3** | Its identity is a digest over `HEAD` + the sorted `(path, change-kind, blob-digest)` set + the working author identity, written with the same `ep-` framing. It is **supposed to be unstable to content**: editing the tree produces a different provisional episode, so the reviewer is never looking at a stale view of work they have changed. |
| **P4** | **Nothing durable may reference it**: no review row, no ledger entry, no finding of record, no risk score of record, no metric numerator or denominator. Sign-off is *refused* on a provisional episode — a sign-off must be about content that can be reproduced, and an uncommitted tree cannot. The dispositions it may hold are none, full stop (App. B; the vocabulary is `PROVISIONAL_FORBIDDEN_STATES` in `src/recommender/segment.py`). |
| **P5** | When the commit lands, the provisional episode disappears and the committed episode is derived from the commit with §2's rule. **No re-linking is attempted between them**, because P4 left nothing worth carrying. |
| **P6** | The work is not thrown away, though: the analysis of a dirty path is cached under exactly the key the committed path will use, so the commit reuses it (ADR-004 §6.4 R6, budget `warm.reindex.provisional.ms`). This is the half that exists today. |
| **P7** | A provisional episode may be *dismissed from view* (hidden) but never dispositioned. Hiding is a view, and ADR-001 §7.2 requires it to say so and offer the key that undoes it. |
| **P8** | **What exists today**: the cache-level provisional pass (`e1.4`, ADR-004 §6.4 R6, `fathom index`'s `worktree` records) and the `kind` column with its `'provisional'` value. **What does not**: the row writer — `write_episodes` currently stamps `'committed'` unconditionally. There is no surface yet that renders a work-in-progress episode, and writing rows no pane can draw is how a half-feature ships. The writer lands with the rail's in-progress row and ambient's "latest" line (`e1.6` + `e5.1`), which are the tickets that would have to show P2's marker anyway. |

---

## 8. A human commit between agent commits

The ticket asks what happens when a person commits between two agent commits. Answer: **the human's commit
is its own episode, and it cuts the agent's work in two** — the `author` rule fires twice, once going in
and once coming out.

| Clause | Statement |
|---|---|
| **H1** | A change of author identity is a boundary, unconditionally (`author`, 0.75). Two authors means at least two intents, and the product's unit is intent. |
| **H2** | The agent's commits either side become **two** episodes. This is correct by the definition (two turns separated by a human acting), and it is also the safe direction: an episode that silently swallowed a human's edit would be a reviewable unit containing content nobody attributed. |
| **H3** | The cost is real and named: a human typo-fix *inside* one agent turn splits it, and a turn split in two must be reviewed twice. The remedies, in order of trust: turn evidence (`hook`/`trailer`, which outranks `author` — TB2), then `episode.force_merge` for the repo that habitually hand-edits mid-turn. What is *not* a remedy is time: `message-shape` cannot save it either, because `author` fires first and a continuation subject does not un-cut an author boundary. |
| **H4** | `author_kind` (`agent` / `human` / `unknown`) is a **separate, separately-confided column**: trailer evidence (`Co-authored-by`, `Assisted-by`, `Generated-with` naming an agent) prices at 0.95, an agent-sounding author string at 0.85, no identity at all at 0.0 (`unknown`, printed as `?`, never bucketed), and "a real identity with no agent evidence" at 0.5 — which is an honest way of saying *probably human, don't bet on it*. It is not part of identity (§2.6). |
| **H5** | **The boundary between the two tickets**: ADR-003 owns the *use of author identity in segmentation* (the `author` rule and the H1/H2 consequence). `e4.1` (`reco-w7q`) owns attribution proper — the marker table, alias resolution, per-file co-authorship — and may refine `author_kind` freely, provided it does not change `author_identity()` without a §2.4 namespace bump. |

---

## 9. Harness evidence, provenance, and the sign-off

### 9.1 What a turn is, and where the id comes from

ADR-005 owns the wire and the canonical event; this ADR consumes exactly two derived facts and specifies
nothing about the transport:

- `turn_id` — ADR-005 §3.3: `"<session_id>:<turn_index>"`, allocated **by the event stream at ingest**,
  never by a writer's own counter. A turn spans `turn_start` → `turn_end`, and a turn that never ends is
  closed by the next `turn_start` (`closed_by: "next-turn"`), so a crash cannot leave an episode with an
  unresolvable turn.
- `session_id` — carried, and used only for display and for the in-session guardrail (ADR-001 §5.2).

Evidence can reach `CommitFacts.turn_id` by two transports, and they are deliberately the **same rule**
(`hook`): an event stream mapping edits to commits, or an `Agent-Turn:` / `Agent-Session:` trailer written
by the harness at commit time. The rule names the class of evidence, not the cable it arrived on.

Both transports are implemented as of `e6.1` (`reco-afh`): `fathom_ingest::hooks` reads the ADR-005 stream,
allocates `turn_index` per session in `seq` order (never from a writer's counter), maps commits onto turns
([`hooks::attribute`], whose digest is what "replay reproduced the linkage" means), and writes the
session-qualified id into `CommitFacts::turn_id` for rung 2; `hooks::trailer_block` is the writer of the
trailer spelling. **The qualification is not cosmetic** — §18.2 **D4** is the defect an unqualified `3`
causes, and it is closed on both sides now: the writer emits the qualified id, and
`fathom_ingest::segment::qualify_turn` qualifies whatever a transport handed over before rung 2 or rung 3 compares it,
so a trailer written by an older harness, or by one that counts turns per session and stops there, cannot
re-open the defect. `session_id` and `turn_id` are both printed by `fathom show --json`, beside
`turn_index`, which is derived from the id rather than stored beside it.

### 9.2 Degradation, which is the part that has to be true

| Situation | Behaviour |
|---|---|
| No events file, no trailers | The cascade runs from `branch` down; every episode still exists with a named heuristic boundary. Segmentation never blocks on harness input (ADR-001 §7.6). |
| Events exist for some commits only | TB6: one-sided turn evidence is not a boundary. The unmapped commit falls through to the heuristics. |
| A `turn_id` nobody can map to a commit (no edits, crashed turn) | No episode (E2). The turn stays in the event log. |
| Events that name turns *not* in git (an aborted turn) | Ignored by segmentation; visible through `harness status` (ADR-005 §11.1), not here. |
| A trailer that contradicts an event | The event wins: `hook` outranks `trailer` (TB2). Where both exist, the trailer is what a `git log` reader sees and the event is what segmentation used, and `fathom show` prints the one segmentation used. |

### 9.3 Provenance: `created_source` and its price

A separate axis from the boundary rule, because "how did Fathom learn this episode existed?" is not the
same question as "what drew its left edge":

| `created_source` | Confidence | Meaning | Written today? |
|---|---|---|---|
| `hook` | 1.0 | an ADR-005 event opened the episode | not yet — `e6.1`'s `fathom_ingest::hooks` now produces the turns and the commit↔turn linkage, but nothing stamps `hook` in the store until the index pass consumes the reader |
| `commit` | 0.7 | a trailer on the commit named the turn/episode | the Python reference writer uses it; the Rust index pass does not |
| `index` | 0.3 | found by walking git with no harness evidence at all | **yes** — every episode today |

ADR-004 §4.3 published that price list; the Rust writer was stamping `index` at 0.7, and §14.2 corrects it
to 0.3 rather than pretending the ADR said something else. Note the two axes are orthogonal: an episode
with `boundary_rule='hook'` can have `created_source='commit'` (a trailer-named turn), and a `hook`-opened
episode with `created_source='index'` is a contradiction the display should surface, not hide.

### 9.4 Sign-off

The acceptance criterion asks that this decision be signed off by whoever owns the harness hook contract:
**`reco-3pu` (`e0.5`, [ADR-005](0005-harness-contract.md))**. Those are the clauses that owner had to agree
to, checked against ADR-005 as accepted:

| Clause | Checked against | Result |
|---|---|---|
| §9.1 what a `turn_id` is, and that the event stream allocates it | ADR-005 §3.3 | **as ADR-005 has it** — `"<session_id>:<turn_index>"`, `turn_start` → `turn_end`, a turn that never ends closed by the next `turn_start` with `closed_by: "next-turn"` |
| §9.2 absence of events is absence of evidence, never an error | ADR-005 §2.1, ADR-001 §7.6 | accepted; and ADR-005 §5.5's *both* rule (trailer **and** event) is why `trailer` and `hook` are separate rung-3 and rung-2 rules rather than one |
| §4.2 `hook` above every git heuristic, `override` above `hook` | ADR-005 §2.1, §8's named deferrals | accepted, and deliberately **not** pre-empting the measurement: whether hooks v1 actually carries enough semantics for the `hook` rule to fire at all is `s2`'s (`reco-q6r`) question, and ADR-005 §8/§9 is the list of what is deferred rather than pretended |
| §4.5 the boundary order is not configurable | ADR-005 §12's amendment clause | accepted; the order is an ADR-level decision, so changing it means amending this file, not editing a config file |
| §9.3 `created_source` prices | ADR-004 §4.3 | accepted as ADR-004 published them (and §14.2 moves the code that disagreed) |

**What this ADR asks back of the hook owner — one thing, recorded so it cannot be forgotten:** if `turn_id`
ever stops being parseable as `<session_id>:<turn_index>`, TB7 (fallback attribution) stops firing
*silently*, and every episode in a repository without trailer evidence loses its author-based boundary.
That change must come to this ADR rather than being made inside `fathom-harness`.

**Where the signature lives**: ADR-005 declares no sign-off mechanism of its own, so this records the
review where the plan can see it — a comment on `reco-ubw` (the durable text) and a comment on `reco-3pu`
(so the hook owner's ticket carries the one request above). Both restate §9 and ask the owner to reopen
this ADR rather than patch it if any clause is wrong. A sign-off that exists only in a chat log is a
sign-off nobody can audit after the context window closes, which is the thing this whole document is
against.

---

## 10. Lineage and ordering

| Clause | Statement |
|---|---|
| **L1** | A **lineage** is the ordered sequence of episodes an episode is measured within (ADR-001 §4.3). v1 has **one** lineage per repository per run: the head ref (`ref_tip` kind `head`, else the first branch, else the literal `HEAD`), and every episode is stamped `lineage_ref` + `lineage_seq`. |
| **L2** | `lineage_seq` — the walk ordinal (W1) — is the **canonical successor relation**. `created_utc` is the display field and the metric's tie-break. Where the two disagree because a clock lied, the topological order wins; §5.3's "next N episodes" is read that way. |
| **L3** | Episodes are re-derived from scratch each run (W3), so `lineage_seq` is stable for a stable repository state and shifts wholesale when the walk order changes — which is fine, because successors are a *window*, not an identity. |
| **L4** | **Per-ref lineage is deferred** (§13.2). Today a merged branch's commits interleave into the one lineage by date, so "the next 5 episodes" after a merge includes commits that were actually written before it on their own branch. This is named, not hidden, and `s2`'s corpus is the place that would find out whether it matters. |
| **L5** | Metrics never read a raw ref: `lineage_ref` is recorded so that when L4 lifts, the historical numbers can be recomputed rather than re-guessed. |

## 11. What the metrics read from here

Nothing in ADR-001 §5 is redefined here; this is the supply contract, stated once so §5's formulas have an
owner for each term.

| Metric term | What this ADR provides |
|---|---|
| *episode* (every denominator) | §1 (E1–E6), §2 (identity), §4 (boundaries). Provisional episodes are excluded (P4). |
| `created(e)` | `created_utc` = first member's author date, ordered by `lineage_seq` (L2) |
| "the next *N* episodes on its lineage" | `lineage_seq` successors within `lineage_ref` (L1–L4) |
| `added(e)`, "a commit whose diff reverts a member commit of e" (R1/R2) | the extent (§3): `episode_member` + the `parent(first) … last` endpoint |
| R3's "evidence cites e's episode identity" | §2's id, and only §2's id |
| *in-session* (G1) | `session_id` (§9.1), with `created_source` telling the reader how much to trust it |
| *maturity* (7 days vs *N* successors) | `lineage_seq` again, and `created_utc` for the clock half |
| *rework-denominated ratios* | never computed over a provisional episode (P4) or a dead episode (§6.5) |

---

## 12. What this ADR deliberately does not decide

| Not decided | Owner |
|---|---|
| The boundary *quality* of the rule set — precision/recall against 200 annotated real turns | **answered**; spike `s2` (`reco-q6r`) reported in §18. (The 200 real turns the plan asked for did not exist: 32 did, and the corpus built instead is 204 annotated turns in three labelled strata) |
| How attribution actually works (markers, aliases, per-file co-authorship) | `e4.1` (`reco-w7q`); §8 H5 fixes the seam |
| The ledger's file format, signatures, staleness rule, portability | `e4.6` (`reco-rlf`), which reuses §6's tiers and Appendix B's vocabularies |
| How findings, risk scores and deltas key themselves (they key on the id; the rest is theirs) | `e2.11`, `e4.4` |
| Whether an episode can be re-cut by a human *after* the fact, and what happens to its findings | the queue's UX, `e4.5`/`e1.6`; `force_split` is the v1 answer and is deliberately blunt |
| The rendering of the boundary label, its colour, its glyph | ADR-006, `e2.10`; §4.7 fixes only *when* it must appear |
| Multi-repo and monorepo lineage, subtree lineage, cross-repo twins | nobody yet; §10 L4 is the single-repo v1 |
| Whether the harness *should* be able to declare episode boundaries directly (it can, via turn events) and what a lying harness buys | ADR-005 §5's trust model already answers "untrusted"; this ADR adds no new authority to event content |

## 13. Deferred, with named reasons

| # | Deferred | Why, and what would end the deferral |
|---|---|---|
| **13.1** | **Fuzzy / similarity-based re-linking** (the ticket's "fuzzy content fingerprint") | Every relink attaches a *human sign-off*, and a similarity threshold tuned without data is a guess that says "you already reviewed this". The four mechanisms in §2/§6 are exact and each is falsifiable; §6.3 is the named loss. Revisit when `s2` can report the false-relink rate and the missed-relink rate against annotated ground truth, and when `e4.6`'s signature makes a wrong link auditable. A `manual` relink (`method='manual'`, already in the vocabulary) is the interim escape hatch. **Update from §18**: `s2` reported *boundary* accuracy, not relink rates — it measured where turns start, which is a different quantity, so this deferral stands exactly where it was. **Update from `e0.12`**: the measurement this row is waiting on is now a ticket — spike `s6` (`reco-f5a`) — because a deferral that names a quantity but no owner is a deferral nobody can close. |
| **13.2** | Per-ref / first-parent lineage | One lineage is a known distortion of "the next N episodes" (L4). Needs the ref graph kept through the walk and the metrics re-specified per lineage; `s2` measures whether the distortion moves G2 at all. **Update from §18**: it did not — the corpus has 8 branch names across 264 commits, which cannot test a per-ref question. This stays open, unnamed-no-longer. |
| **13.3** | Merge-train coalescing | Ratified as N episodes (§5.8). Coalescing a bot's train hides which merge admitted the pathology; revisit with a repo where the rail is unusable because of it. |
| **13.4** | Twin-suffix stability across runs | A twin's `-2` is allocated per run (TB10). If one twin disappears the survivor can renumber, and the store resolves it through tier 3 (the shared member) rather than by remembering the suffix. A permanent disambiguator would have to be content-stable *and* unique, which the digest already is or is not. |
| **13.5** | The provisional episode row writer | §7 P8: the surface that would display it does not exist yet; the cache-level half does. |
| **13.6** | Author alias resolution | `e4.1`, at the price of a namespace bump (§2.6). |
| **13.7** | Tuning every constant in §4.4 | **Closed by `s2`, §18.2** — with the result that the gap constant does *not* move, and the reason it does not is measured rather than assumed. The other constants in §4.4 (the TB7 guard, the marker lists, the forced-edge spellings) were not re-tuned and remain guesses with printed prices. |

## 14. What this ADR moved, instead of approving

An ADR written after some of its own code is a risk: the temptation is to ratify whatever shipped. Six
places this document did **not** do that, and the code moved. Each is small; all of them are the kind of
quiet disagreement that makes documentation decorative.

| # | What disagreed | Resolution |
|---|---|---|
| **14.1** | `segment.rs`'s module doc table listed `hook` first and `override` third, while `decide()` checks forced boundaries first | The **cascade wins** and the module doc now states the §4.2 order (`override` first, TB2). Prose and code in the same file disagreed; the tie-breaker they disagreed about is now decided here. |
| **14.2** | ADR-004 §4.3 published `created_source` prices 1.0/0.7/0.3; the Rust writer stamped `index` at 0.7 | Ratified as ADR-004 published (§9.3); the writer now stamps 0.3. An episode derived by walking git with no harness evidence is a 0.3 claim. |
| **14.3** | The Rust default gap was 45 minutes; the published config default — schema, docs, `fathom.toml` — was 30 | 30 minutes, the published number, and now one number (§4.4) with a test that the four copies of it agree. |
| **14.4** | The Python reference segmenter addressed episodes by **member oids** (`sha256(oids)`) and fingerprinted by **tree oid** — the exact scheme §2 rejects, and a live contradiction of ADR-004 §4.3's own comment ("Identity is the ADR-003 content-addressed id") | It now implements §2 with the same framing, prefix and length as Rust, and both are checked against Appendix A. Two implementations of an identity rule is the only way to show it is a rule. |
| **14.5** | The config enum for `episode.boundaries` spelled two rules `time` and `merge`; the store stores `time-gap` and `branch` | One vocabulary, Appendix B, and the config enum now uses the stored spellings (§4.5). |
| **14.6** | The published `episode_rewrite.method` / `link_method` CHECK in the reference schema lacked `shape`, which the Rust relink writes | Appendix B is the vocabulary; both schema copies enforce it. |

## 15. Alternatives considered

- **Identity = ordered member SHAs (the ticket's first proposal).** Rejected: a rebase changes them all,
  which is I1's exact failure; and it makes identity depend on membership, so `git add` one more commit
  and the episode is a different episode. Kept as the **extent**.
- **Identity = tree oid, or a digest of the resulting tree.** Rejected: it survives a rebase (trees are
  parent-independent) but breaks on the amend that fixes a typo — the single most common rewrite — and on
  every squash, so the whole relink pass would be load-bearing instead of exceptional.
- **Content hash of the diff hunks.** Rejected: renames, mode changes and re-wrapping change the bytes of a
  diff without changing the change, and an id that moves under `rustfmt` is an id that will be blamed for
  the reviewer's churn.
- **Per-episode sequence numbers or oids assigned at ingest.** Rejected: an arbitrary surrogate cannot be
  recomputed after a rewrite, so every relink would need a durable mapping store — which is a database of
  human decisions kept in the one thing this product promises is disposable (ADR-004 §10).
- **Fuzzy similarity relink.** Declined for v1, with the condition that would revisit it (§13.1).
- **A weighted score across boundary rules.** Rejected (TB1): a sum of a 1.0 event and a 0.4 guess is a
  number no one can falsify, and the recorded reason is the feature.
- **Configurable boundary order.** Rejected (§4.5): the order is an argument about evidence, and the
  supported way to disagree is a named, per-boundary `force_split`.
- **Fold a human's interleaved commit into the surrounding agent turn.** Rejected (H1/H2): it puts
  unattributed content inside a "reviewed" unit.
- **Merge episode = the whole incoming branch.** Rejected (§5.8): it duplicates the branch's own episodes
  and loses which merge brought what.
- **A provisional episode that can be signed off.** Rejected (P4): a sign-off that cannot be reproduced is
  an audit trail about a working tree.

## 16. Consequences

**What this makes safe.** Nineteen tickets can now store `episode.id` as a foreign key, and the answer to
"what happens when the user rebases?" is a document with named cases rather than folklore. `e4.6` has a
relink vocabulary and a staleness rule to build on instead of inventing one. `e4.1` knows where attribution
starts and how much an alias costs. `e6.1` knows what a `turn_id` must be to be usable, and that nothing
may depend on it arriving.

**What it costs.** Segmentation re-runs in full each index (W3) — O(commits) over rows already in SQLite,
which is cheap today and is the first thing to cache if a repository ever makes it hot. Three extra digest
columns per episode, and one rewrite-ledger table that grows only when history is rewritten. Displaying
guesswork (H3's split turns, §4.7's printed rule names) makes the queue noisier than an unexplained
partition would — the price of §7.7 being paid on purpose. And the fail-visible rule (§6.3) means a
user who rewords **and** moves files in one rebase will see an episode again they thought they had
signed off: the honest cost of never lying about a review.

**What it makes impossible to do quietly.** Keying review state to a commit oid (E6); attaching a decision
across a rewrite without writing it down (RL5); treating an uncommitted tree as reviewed (P4); letting a
config file reorder the boundary cascade without saying so (§4.5); shipping a fifth relink tier or a ninth
boundary rule without amending this file and both schema copies (Appendix B).

## 17. Verification — the commands that *are* the acceptance criteria

```bash
python3 tools/check_docs.py                       # glossary/plan-reference/link checks, incl. §0.2's new nouns
python3 tools/gen_adr_tables.py check             # ADR-004's generated blocks (incl. §14.6's CHECKs)
PYTHONPATH=src python3 -m pytest -q tests/test_episode_identity_contract.py -v   # this document, as tests
PYTHONPATH=src python3 -m pytest -q               # the whole contract suite
cargo test -p fathom-ingest segment::tests        # the digests, the cascade, the corpus scorer
cargo test -p fathom-store  --test episodes       # relink tiers, live/dead/prune, reachability, ordering
cargo test -p fathom-analyze --test episodes      # real git: amend, reword, squash, disappearance, ≥90 % corpus
```

### 17.1 Acceptance criteria → evidence

| Criterion | Evidence |
|---|---|
| `docs/adr/0003-episodes.md` with worked examples for amend, rebase, squash, merge and cherry-pick | this file: §5.1–§5.2 (amend), §5.4–§5.5 (rebase, reword), §5.6–§5.7 (squash, squash-merge), §5.8 (merge, merge train), §5.9 (cherry-pick, backport), §5.10 (drop, split) |
| Stated invariant: identity stable across `git rebase -i` of the same content | **I1** (§2.7), with §5.4; `amending_a_commit_leaves_the_episode_and_the_review_exactly_where_they_were`, `a_rebase_preserves_identity_and_the_weaker_digests_cover_what_it_cannot`, `test_rebase_of_the_same_content_does_not_move_the_id` |
| Tie-breaker rules for overlapping/ambiguous boundaries, written down and testable | **TB1–TB10** (§4.6) and **RL0–RL7** (§6.2); the Rust tests named per case in §5, and `tests/test_episode_identity_contract.py`, which asserts the published tables equal the code's tables |
| Signed off by whoever owns the harness hook contract (`e0.5`) | §9.4, recorded on `reco-ubw` |
| …and the extra criterion this ADR adds to itself | every table in §4, §6 and Appendix A/B is mirrored in `src/recommender/segment.py` and diffed against the Rust source by the contract test, so prose cannot drift from either implementation without CI |

---

## 18. What spike `s2` measured, and what it decided

**Closed by `reco-q6r`** (spike `s2`). Corpus: `crates/fathom-fixtures/corpus/turns/` — **204 hand-annotated
turns over 264 commits, carrying 74 annotated pathologies**, in three strata labelled by what they can
 certify (`real`: this repository's own git history, all 34 commits of it, cut into turns by the beads
 board's own claim/close windows; `fixture`: 13 of `e0.10`'s scenarios, whose episode and finding
 annotations were already hand-authored; `authored`: 8 histories written to cover shapes the 34 real
 commits are too short to contain, each naming the real distribution its shape came from). Harness:
 `crates/fathom-analyze/examples/s2_spike.rs`; report:
 `benchmarks/reports/s2-segmentation-accuracy.json`; claims as data:
 `crates/fathom-analyze/spike/s2/thresholds.json`; prose and tables:
 [`docs/concepts/episode-segmentation.md`](../concepts/episode-segmentation.md). The harness asserts its
 `shipped` strategy reproduces `segment::segment()` exactly on every history before it scores anything.

### 18.1 The measurements

<!-- BEGIN GENERATED: s2_strategies -->
| strategy | rungs (left → right = highest → lowest priority) | P | R | **F1** | real F1 | pair F1 | turns split | turns absorbed | pathologies hidden |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `all-one` | *(no rule fires: never split)* | 1.000 | 0.000 | 0.000 | 0.000 | 0.076 | 0/204 | 204/204 | 4/74 |
| `all-split` | always-split | 0.752 | 1.000 | 0.859 | 0.969 | 0.000 | 47/204 | 0/204 | 23/74 |
| `A-time-author` | author › time-gap | 0.963 | 0.577 | 0.722 | 0.877 | 0.233 | 4/204 | 108/204 | 8/74 |
| `B-hook-commit` | hook(commit) | 1.000 | 0.203 | 0.338 | 0.000 | 0.095 | 0/204 | 178/204 | 4/74 |
| `B-hook-harness` | hook(harness) | 1.000 | 0.747 | 0.855 | 0.873 | 0.656 | 0/204 | 76/204 | 4/74 |
| `B-hook-then-git` | hook(commit) › branch › author › time-gap | 0.971 | 0.742 | 0.841 | 0.877 | 0.376 | 4/204 | 66/204 | 8/74 |
| `C-trailer-shape` | trailer(Fathom-Episode) › message-shape | 0.643 | 0.050 | 0.092 | 0.063 | 0.084 | 5/204 | 195/204 | 4/74 |
| `C-agent-trailer-shape` | trailer(Agent-Turn) › message-shape | 0.927 | 0.352 | 0.510 | 0.063 | 0.111 | 5/204 | 142/204 | 5/74 |
| **`D-shipped`** | hook(commit,session-qualified) › trailer(Fathom-Episode) › branch › author › time-gap › message-shape | 0.940 | 0.775 | **0.849** | 0.897 | 0.445 | 9/204 | 57/204 | 8/74 |
| `E-token-in-shape` | hook(commit) › trailer(Fathom-Episode) › branch › author › time-gap › ticket-token › message-shape | 0.947 | 0.879 | 0.912 | 0.915 | 0.688 | 9/204 | 36/204 | 8/74 |
| `E-token-above-gap` | hook(commit) › trailer(Fathom-Episode) › branch › author › ticket-token › time-gap › message-shape | 0.964 | 0.879 | 0.919 | 0.931 | 0.714 | 6/204 | 36/204 | 5/74 |
| `G-no-tb7` | hook(commit) › trailer(Fathom-Episode) › branch › author › ticket-token › time-gap › message-shape(keep-only) | 0.994 | 0.874 | 0.930 | 0.912 | 0.609 | 1/204 | 36/204 | 5/74 |
| **`F-hook-qualified`** | hook(commit,session-qualified) › trailer(Fathom-Episode) › branch › author › ticket-token › time-gap › message-shape | 0.965 | 0.907 | **0.935** | 0.931 | 0.749 | 6/204 | 27/204 | 5/74 |

`D-shipped` is ADR-003 §4.2 as built (`e1.3`); `F-hook-qualified` is the recommendation. Pooled is the whole corpus; `real F1` is Fathom's own git history alone; `pair F1` is co-membership, the measure `e1.3` gates the corpus at. Harness-identity check against `fathom_ingest::segment::segment()`: **True** on all 22 histories (0 mismatched).
<!-- END GENERATED: s2_strategies -->

Six readings of that table, in the order that surprised us:

1. **Precision is not the problem; recall is.** The cascade draws boundaries that are right 93.8 % of the
   time and misses a quarter of the ones a reviewer would draw. Every error is a turn that never got its
   own card — the direction that *removes* work from the queue, which is the direction a reviewer never
   sees.
2. **The trivial partition beats the cascade on the headline metric and is still wrong.** "Every commit is
   its own episode" scores 0.859 pooled / 0.969 real boundary F1 — better than what ships — because 77 % of
   real turns are one commit long, and it hides 23 of the corpus's 74 pathologies doing it. This is why
   the table carries `pathologies hidden` beside F1, and why no future tuning ticket gets to cite the
   former without the latter.
3. **`time-gap` is the wrong instrument, not merely the wrong number.** On Fathom's own loop the median gap
   between closing a ticket and claiming the next is **0.9 minutes**, while one turn's own commits can be
   33.6 minutes apart. No value of the constant separates those two populations: the pooled optimum is 10
   minutes, and below it the two measures turn against each other (§18.1's table and
   `docs/concepts/episode-segmentation.md` §5) — at 1 minute the cascade becomes the split-everything
   baseline, gaining real-stratum F1 and doubling hidden pathologies.
4. **`author` is nearly inert on agent history**: 3 of 31 real boundaries. One agent, one author identity,
   all day. It is kept because it is cheap and because §8's interleaved-human case needs it, not because it
   pulls weight in the common case.
5. **Rung 2 has no evidence and rung 3 has no writers.** Not one of the 34 real commits carries a turn
   trailer or an event (`created_source` is `index` for the whole real corpus, as §9.3 concedes), so the
   `hook` rule as wired fires on nothing today; and the `Fathom-Episode:` trailer that rung 3 reads at
   confidence 0.9 is written by nothing in this repository or the fixture corpus — it sees 8 of 182 true
   boundaries, all of them in the one history this spike authored to carry it. Spell the trailer the way
   harnesses actually write it (`Agent-Turn:`, which §9.1 already names as a `hook` transport) and the ladder
   is 0.092 → **0.338** for the spelling with TB6 intact (the 0.338 row is the `hook` rule alone, message-shape
   dropped) → **0.510** once a trailer on one side of an adjacency is read as a boundary, which is sound only
   while the harness declares every agent commit — a policy question (§18.2 D5), not a free win.
6. **The corpus's strongest git-only signal is one this ADR never named**: the ticket token in the subject
   line (`reco-q6r: …`, `[JIRA-123] …`, `#412 …`), which a loop writes on every commit of a turn and changes
   between turns. Respecting TB6 — a token on one side only is no opinion — it fires on **zero** of the
   corpus's 60 within-turn adjacencies and is worth more pooled F1 than everything above the clock
   combined. ADR-001 §4.3 counts a commit message as a claim about work, which is what makes this a
   statement the author made rather than a pattern Fathom guessed; that is also the honest limit of its
   confidence (§18.2 D1), and the whole of it is worth +0.088 pooled F1 over the shipped cascade.

### 18.2 The decisions

| # | Decision | Evidence | Status |
|---|---|---|---|
| **D1** | Add a **`subject-claim` rung above `time-gap`**: two neighbours both carrying a leading ticket-shaped token — equal → keep, different → split; a token on one side only is no opinion (TB6). Fold it into `message-shape`'s stored rule name; no `boundary_rule` vocabulary value is minted, so neither schema copy nor `fathom.toml` moves. Confidence 0.7, class `heuristic`, ordered `author > subject-claim > time-gap` and `subject-claim > message-shape`, extending §4.3's "evidence outranks heuristic" shape by one rung of *authored* evidence ranked below every declared one | 0.832 → 0.919 pooled boundary F1, 0.897 → 0.931 on the real stratum, pathologies hidden 8 → 5, annotated turns absorbed 66 → 36, false boundaries 0 | **Decided here; not yet in code.** Scheduled by `e0.12` as `e1.13` (`reco-quk.2`), which carries this row's numbers as its acceptance criteria; §4.2 stays the shipped truth until it lands |
| **D2** | **`episode.time_gap_minutes` stays 30.** No constant in §4.4 moves | pooled optimum 10 min = +0.022 F1 with no change in hidden pathologies; 1 min buys no further F1 and doubles them (8 → 14); with D1 the knob is flat within ±0.02 across 1–240 min. A published constant that has stopped mattering is not worth a migration | **Decided and effective**: §4.4, §13.7 closed |
| **D3** | **`hook` keeps rung 2, and is documented as necessary-but-not-sufficient**: the git cascade stays beneath it as the fallback, and the plan's highest-value segmentation work is the harness event ingest (`e6.1`), not further heuristic tuning | turn evidence precision 1.000 in every stratum; hook-only recall 0.000 on real history as shipped, 0.747 with harness evidence; hook-only swallows 76 annotated turns through TB6 | **Decided as a reading of §4.2**; the ingest itself is `reco-afh` (`e6.1`) |
| **D4** | **`turn_id` must be session-qualified where the transport carries it** (`<session_id>:<turn_index>`, exactly §9.1/ADR-005 §3.3). The shipped trailer transport builds the id without the session, so two turns in different sessions with the same index read as one turn and rung 2 declines to cut between them | +0.015 pooled F1 and +0.031 on the fixture stratum when qualified; the collision is a real pair in the corpus (`s1:3` beside `s2:3`) | **Landed** (`e1.14`, `reco-quk.3`; writer `e6.1`/`reco-afh`). The join is in `segment::qualify_turn`, not in `fathom-analyze/src/episodes.rs` as this row first guessed, because the trailer transport reaches `CommitFacts::turn_id` *through* that crate and a fix one level up would have left rung 3 reading the bare index. `D-shipped` in the harness moved with it, which is what §18.1's numbers now measure |
| **D5** | **Rung 3 is either pointed at the trailer that exists or marked reserved.** A documented rule at confidence 0.9 with no writer, no fixture, and 8 of 182 boundaries is decoration, and §4.2 reading as eight live rules when two are inert is the kind of drift §14 exists to prevent | 0.092 → 0.510 pooled F1 between the `Fathom-Episode:` and `Agent-Turn:` spellings | **Decided by `e0.12`.** The `trailer` value in Appendix B stays: it names a *class of evidence* — a trailer that declares an episode start — not a trailer key, so minting a ninth value would have been the drift. What moves is the key it reads, and the writer is `e6.1` (`reco-afh`), which owns the transport and already emits `Agent-Turn:` (the spelling §9.1 calls a `hook` transport, and the one `src/recommender/store/git.py` writes today). Until that ingest ships, rung 3 is **reserved, not live**: a fourth trailer spelling (`Fathom-Episode:`) is reserved for the day Fathom itself declares a start, and adding any spelling is an amendment to this section, not a schema edit |
| **D6** | **TB7 (`message-shape`'s split direction) is kept but its guard is now a named defect**: §5.2's own worked example — `feat:` then `fix: review feedback` three minutes later then `test:` four minutes after that — is **cut by the shipped cascade today**, and `segment::tests::a_run_of_agent_commits_inside_the_gap_is_one_episode` fails on it at HEAD. gap ÷ 9 (**3.33 min**) is a shorter interval than the "review came back" pattern TB7 exists to tolerate. §4.3's claim that a conventional-type change is the weakest splitting signal in the set is measured, not argued | message-shape finds 1 true boundary in 182 and false-fires 5 — the only rule in the cascade that false-fires; deleting its split direction is +0.010 pooled and −0.019 real, lifts precision to 0.994 and five of the recommendation's six split turns, and costs 0.105 of pair F1 | **Keep TB7, fix its guard**: re-derive the clock condition, owned by `reco-dot` as `e0.12` scheduled it; the failing §5.2 test is its acceptance criterion, quarantined `#[ignore]`d (not deleted) until the guard and the re-bless of this section's report land together — a rule change that silently re-scores §18.3's declared-failing gates is a measurement turning into an opinion |
| **D7** | **Two failure modes have no segmentation fix and are carried as named costs** rather than as TODOs: the *homeless turn* (no bead, no trailer, no ticket token, small gap — 2 of the 5 pathologies the recommendation still hides on real history) and the §8 H2 interleave cut, which the corpus must record as two turns because E1 makes episodes contiguous | `docs/concepts/episode-segmentation.md` §4 lists them by commit | **Decided**: recorded in §8 H3/§16 with numbers attached |

Nothing in this section changed shipped behaviour. That is deliberate: the report is a measurement of the
cascade §4.2 documents, and a spike that also rewrote the cascade would be scoring its own edit. D1 and D4
are specified to the line so the implementation ticket can land them without re-deciding anything; when it
does, `D-shipped` in the harness becomes the recommended strategy and the gap between them is the change's
regression test.

### 18.3 The gates

22 claims in `crates/fathom-analyze/spike/s2/thresholds.json`, each declaring what it expects before the
run; a gate that comes out the other way — **including a failing claim that quietly starts passing** — fails
`tests/test_segmentation_spike_contract.py`. The five that fail are the findings; they are the prices §4.4
and `e1.3`'s ≥ 0.90 acceptance criterion published and this measurement refuses to let stand unlabelled.

<!-- BEGIN GENERATED: s2_gates -->
| gate | claim | measured | verdict |
| --- | --- | --- | --- |
| `shipped-pooled-boundary-f1` | the cascade ADR-003 §4.2 ships reaches 0.90 boundary F1 on the whole corpus, which is the bar `e1.3`'s acceptance criterion published | 0.849 | ❌ as claimed |
| `shipped-pooled-pair-f1` | the same cascade also clears 0.90 on the pair co-membership measure `crates/fathom-analyze/tests/episodes.rs` gates the corpus at | 0.445 | ❌ as claimed |
| `shipped-fixture-pair-f1` | …and the reason `e1.3`'s own test still passes: on the fixture stratum — the corpus that test measures — the cascade is at 0.86 pair F1, and it is the real and adversarial strata that move the pooled number | 0.981 | ✅ |
| `shipped-beats-all-split-pooled` | the shipped cascade is not worse than the trivial 'every commit is its own episode' partition on the pooled corpus | -0.009 | ❌ as claimed |
| `shipped-beats-all-split-real` | …and not worse than it on the real stratum either, which is the stratum nobody authored to be friendly | -0.072 | ❌ as claimed |
| `all-split-hides-pathologies` | the trivial partition that beats the cascade on F1 hides at least 20 of the corpus's annotated pathologies, which is the proof that boundary F1 alone cannot choose a rule set | 23 | ✅ |
| `hook-evidence-never-lies` | turn evidence, where it exists, is never wrong: hook-over-harness-evidence precision is 0.99 or better pooled | 1.000 | ✅ |
| `hook-alone-is-not-a-strategy` | the hook rule alone reaches 0.95 pooled boundary F1 — ADR-003 §4.2's confidence 1.0 invites the reading that it is nearly the whole answer, and the corpus refuses it: with no evidence on either side of a boundary TB6 keeps the commits together, and 76 annotated turns get swallowed | 0.855 | ❌ as claimed |
| `hook-evidence-absent-on-real-history` | as shipped, the hook rule has essentially no evidence on real agent-driven history: with commit-carried evidence only, its recall on the real stratum is at most 0.05. This is a photograph of the world before `e6.1`, and the ticket that lands event ingest is expected to retire it rather than leave it quietly true | 0.000 | ✅ |
| `trailer-rule-with-trailer` | ADR-003 rung 3 (`Fathom-Episode:` trailer, confidence 0.9) does work when somebody writes the trailer it reads: on the history authored to carry it, trailers-plus-message-shape reaches 0.90 boundary F1 | 1.000 | ✅ |
| `trailer-rule-has-no-writers` | …and everywhere else that rule is dead: on the pooled corpus the trailer-plus-message-shape strategy finds at most 10 % of the true boundaries, because the trailer it reads is written by nothing in this repository or the fixture corpus | 0.050 | ✅ |
| `gap-optimum-for-time-and-author` | for a time-and-author strategy the best gap on this corpus is at or below 20 minutes, so the published 30 is not the optimum the ADR guessed it was | 10 min (peak 0.786) | ✅ |
| `gap-optimum-for-shipped` | the shipped cascade's pooled optimum gap is at or below 20 minutes too (measured 10), so `episode.time_gap_minutes` is not sitting near its optimum by luck — it is not near an optimum at all, and §4.4's 30 is a value whose measured leverage is smaller than its documentation weight | 10 min (peak 0.871) | ✅ |
| `recommendation-is-gap-insensitive` | the recommended rule set reaches 0.90 pooled boundary F1 at the published 30-minute gap, which is what licenses leaving the constant alone: once a per-commit claim carries the boundary, the clock stops being the instrument | 0.919 | ✅ |
| `recommendation-beats-shipped-pooled` | the recommended rule set (subject-claim above the clock, session-qualified turn ids) beats the shipped cascade by at least 0.05 pooled boundary F1 — a change to a documented cascade has to pay for the churn | 0.085 | ✅ |
| `recommendation-beats-shipped-real` | …and it beats it on the real stratum by at least 0.02, because a recommendation that only works on authored histories is a recommendation about the corpus | 0.035 | ✅ |
| `recommendation-hides-no-more` | the recommended rule set hides no more annotated pathologies than the shipped cascade does pooled | -3 | ✅ |
| `shipped-real-hide-bound` | the shipped cascade hides at most three annotated pathologies on the real stratum, which is where a hidden pathology is a real one | 3 | ✅ |
| `recommendation-real-hide-bound` | the recommended rule set hides at most two on the real stratum, and both are of the kind no git signal can see | 2 | ✅ |
| `subject-claim-makes-no-false-splits` | the subject-claim signal fires on at most 2 of the 60 within-turn adjacencies in the corpus: a leading tracker token is evidence rather than a guess, and it is the only added signal that costs no false boundaries | 0 | ✅ |
| `message-shape-is-weak-as-advertised` | ADR-003 §4.3's claim that a conventional-type change is the weakest splitting signal in the set is confirmed rather than assumed: message-shape finds at most 5 true boundaries in the whole corpus | 1 | ✅ |
| `recommendation-absorbs-fewer-turns` | the recommended rule set swallows fewer annotated turns than the shipped cascade, which is the failure direction that removes work from the queue | -30 | ✅ |

5 of 22 gates fail, each declared `expected: fail` before the run: `shipped-pooled-boundary-f1`, `shipped-pooled-pair-f1`, `shipped-beats-all-split-pooled`, `shipped-beats-all-split-real`, `hook-alone-is-not-a-strategy`. Every gate came out as claimed (`gates_all_as_expected: true`).
<!-- END GENERATED: s2_gates -->

### 18.4 What this spike did not measure

| Not measured | Consequence |
|---|---|
| False-relink and missed-relink rates (§13.1's stated condition) | Boundaries and relinks are different quantities; §13.1's deferral of fuzzy re-linking stands unchanged, unmeasured |
| Per-ref / first-parent lineage (§13.2, L4) | The corpus has 8 branch names across 264 commits; it cannot test a per-ref question. Still open |
| Provisional-episode thresholds (§7 P6) | Provisional episodes do not exist yet; nothing to measure |
| Anything about a *working* harness | The corpus records the world before `e6.1` (`reco-afh`): the `hook` rule's evidence is modelled as a counterfactual (`B-hook-harness`), clearly labelled, and every real-stratum number is trailer-less |
| Multi-repo, monorepo, subtree lineage | Out of the corpus, out of the ADR |

---

## Appendix A — the published identity vectors

Both implementations (§2, `crates/fathom-ingest/src/segment.rs` and `src/recommender/segment.py`) must
produce these bytes for these inputs. Vectors 1 and 2 pin the framing and the normalisation; vector 3 is
the twin case from §5.9. Framing is NUL-separated as printed in §2.2.

| # | author | subject (raw) | paths | `fingerprint` | `episode_id` |
|---|---|---|---|---|---|
| 1 | `dev@example.invalid` | `add the epoch column` | `src/a.rs`, `src/b.rs` | `9461ceafd7f5d52996f6d47a058857ed874a520a56616fca89f416cc7694bda7` | `ep-9461ceafd7f5d52996f6` |
| 2 | `dev@example.invalid` | `fixup! feat(store): Make Epochs  SAFE` → `make epochs safe` | `src/store.rs` | `6a045309070f68ff6f8c5ae912da2ff12d2eef6ae99bbf841810653a0bbb3f69` | `ep-6a045309070f68ff6f8c` |
| 3 | `dev@example.invalid` | `fix: the crash` → `the crash` | `src/a.rs` | `82b50d5356e8f59a34739bf1595246812c300a3a6aab3e91f3aee43b4c9d3523` | `ep-82b50d5356e8f59a3473` |

Digests for vector 1, which the relink examples quote: `fallback_fingerprint`
`603dca60ea5b444ea8133b587c9804bc5d83d2dc6fd848173a90c1ddc36d6124`, `shape_fingerprint`
`c3115f2777576a3da22bc546ca75a58c9fd3679405ed02f6cd6d6b5990a54b02`.

Vector 3 is what a cherry-pick of the same commit to two branches produces twice: the same digest, hence
ids `ep-82b50d5356e8f59a3473` and `ep-82b50d5356e8f59a3473-2` (§5.9, TB10).

## Appendix B — the closed vocabularies

Changing any of these means amending this file **and** both schema copies **and** the enum in
`fathom.toml`'s schema; the contract test fails otherwise. `src/recommender/segment.py` re-exports them
as constants so the tests can diff them against the Rust source and the SQL.

| Vocabulary | Column | Values |
|---|---|---|
| Boundary rules (§4.2) | `episode.boundary_rule` | `override`, `hook`, `trailer`, `branch`, `author`, `time-gap`, `message-shape`, `root` |
| Configurable subset (§4.5) | `episode.boundaries` | `hook`, `trailer`, `author`, `branch`, `time-gap`, `message-shape` (order not honoured in v1) |
| Creation sources (§9.3) | `episode.created_source` | `hook` (1.0), `commit` (0.7), `index` (0.3) |
| Rewrite relations (§6.4) | `episode_rewrite.relation` | `amend`, `rebase`, `squash`, `split`, `cherry`, `merge`, `other` |
| Rewrite methods (§6.4) | `episode_rewrite.method`, ledger `link_method` | `exact`, `fingerprint`, `shape`, `member-set`, `manual` |
| Episode kinds (§7) | `episode.kind` | `committed`, `provisional` |
| Author kinds (§8 H4) | `episode.author_kind` | `agent`, `human`, `unknown` |
| Review states | `review_state.state` | `unread`, `read`, `accepted`, `flagged`, `dismissed`, `snoozed` — none of which a provisional episode may hold (P4) |
