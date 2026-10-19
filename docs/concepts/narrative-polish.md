# Does an LLM narrative polish beat the deterministic template? (spike `s5`)

- **Status**: Decided — **ship the template, ship no LLM path**. The seam stays unwired.
- **Ticket / plan key**: `reco-6p8` (`s5`), deciding `reco-2rx` (`e4.7`)
- **Decision issue**: `reco-nv0`, linked from `reco-2rx` as a dependency
- **Implements**: the question ADR-001 §8 non-goal 6 deferred and ADR-002 §4.5 pre-funded. ADR-002
  gives `fathom-analyze` a `net-llm` feature for "optional narrative polish (`e4.7`, spike `s5`)" —
  a seam drawn before anybody measured what it buys. The thesis (§non-goal 6, row 6) named the one
  result that would have justified it:

  > Only if `s5` shows a measured, non-trivial win on trust and usefulness *with* hallucination
  > counts at zero against the evidence set.
- **Executes as**: `src/recommender/narrative/` (`packet.py`, `template.py`, `audit.py`, `panel.py`,
  `corpus.py`, `spike.py`), the recorded arms, prompts, blinded corpus and gzipped source deltas under
  [`crates/fathom-analyze/spike/s5/`](../../crates/fathom-analyze/spike/s5), the numbers in
  [`benchmarks/reports/s5-narrative-polish.json`](../../benchmarks/reports/s5-narrative-polish.json),
  and the checks in `tests/test_narrative_spike_contract.py`.

---

## 1. The question, in the shape a measurement can hold

`e4.7` ships a deterministic, template-composed narrative per episode, every claim citing an evidence
id. It also floats an *optional* LLM polish behind `privacy.llm`. This spike decides whether that
optional half earns its complexity, or whether the honest answer is that the template's weakness is a
bug in the template.

Three arms compose a narrative for the same 20 real episodes out of the same evidence set:

| arm | what it is | who wrote it |
|---|---|---|
| **A — template** | `template.py`: sections of cited figures, connectives and nothing else | code |
| **B — polish** | a rewrite of A given the same citation index ([prompt](../../crates/fathom-analyze/spike/s5/prompt/polish.md)) | the model |
| **C — why-line** | one sentence of *why*, from the commit subjects and nothing else ([prompt](../../crates/fathom-analyze/spike/s5/prompt/why.md)) | the model |

Arm C exists because the real question is not "LLM or no LLM" but **how much rope earns its keep**.
Giving a model the whole narrative and giving it one sentence are different products with different
failure surfaces, and a spike that measures only the widest seam cannot say whether a narrower one
would have been fine.

## 2. The corpus, and why nobody got to pick it

The only real history this repository can reach is its own: every commit here was authored by the
agent loop the product reviews, indexed by the shipped segmenter at
`8d7868828d8149fa2e31d824326dd822517d2a31` into 51 episodes. Selection is arithmetic, recorded in
`selection.json`, and re-derivable without running anything: indexable episodes (a delta document
that parsed, ≥1 changed file, `totals.reconciles` true — none were excluded) in `lineage_seq` order,
at positions `round(i·(n−1)/19)` for `i` in 0..19. No sampling, so no re-drawing until the numbers
looked good; no "first 20", which would have been one week of history called a corpus.

The evidence set is the `fathom-delta/1` document `fathom analyze` prints, with an address on every
fact (`packet.py`): each figure, each changed file, each gap, the risk vector, the attribution and
the boundary rule. A narrative is then a list of *(sentence, cited item ids)* pairs and "supported"
stops being an opinion. The index is deliberately generous — every changed file is listed, not a
top-N — because a cap would turn a truthful sentence about the ninth file into a hallucination, and
a false positive in this experiment biases towards "do not ship", which is the wrong direction to
bias a decision.

## 3. What was measured

**Hallucination count** (`audit.py`). A claim is one sentence, minus its citations. It is supported
when every number it asserts is in the evidence set, every path and code token occurs in it, any
counted noun (files, commits) matches a figure the packet can actually produce, and any motive
(*because*, *to <verb>*) is carried by something it cites. Two tiers, always reported together:

* **strict** — a number must appear verbatim in the packet;
* **tolerant** — also allowed the stated derivations: a file's own `+`/`-`, their sum and difference,
  shares of churn, counts by surface / change kind / directory, the test share. The arithmetic a
  careful reader could do with the packet open, and nothing more.

Tolerant is the headline, because the strict tier charges an arm `96.6 % reviewable` when the packet
says `2 999 of 3 103`, and an audit that cries wolf on arithmetic gets switched off. Two defect
classes are counted separately and are **not** hallucinations: `uncited` (checkable, no citation) and
`inference` (evaluative — "probably mechanical" — which the evidence neither states nor contradicts).
An arm can be hallucination-free and still be unship-able for either. None of the three arms produced
a single `uncited` or `inference` finding either; the prompts forbade both, and the arms obeyed.

**Blind panel** (`panel.py`). Labels come from a hash of `(seed, episode, arm)`, so a filename leaks
nothing; each scorecard carries the digest of the blinded corpus it scored; `blinding.json` — the
key — was written *after* all three scorecards existed, and `validate` refuses a panel whose
scorecards post-date the key. Reviewer flags are checked, not believed: `cross_check` re-finds each
flagged claim inside the mechanical findings and reports the **over-flag rate**.

**Determinism.** Arm A rendered twice in-process and once in a fresh interpreter, digests compared.
Arm B re-sampled on 6 episodes and compared bytes, word counts and the set of figures asserted.

**Cost and latency.** Arm A's render wall-clock, measured here, per episode, excluding the store read
(the same framing `e2.10`'s card budget uses). Arms B and C: **modelled** — see §6.

## 4. Results

### 4.1 Hallucinations, coverage and volume

| arm | claims | hallucinated (tolerant) | hallucinated (strict) | uncited | inference | evidence coverage | words/episode |
|---|---|---|---|---|---|---|---|
| A — template | 265 | **0** | 0 | 0 | 0 | 0.61 | 563 |
| B — polish | 188 | **0** | 0 | 0 | 0 | 0.32 | 187 |
| C — why-line | 20 | **0** | 0 | 0 | 0 | 0.02 | 19 |

Zero on all three arms — and §6 explains why that is an *upper bound on a disciplined arm*, not a
rate a stranger would hit. What the polish arm did instead is in the second-to-last column: it cites
0.32 of the evidence set where the template cites 0.61. **It bought its readability by dropping
figures.** That is a measured cost, not a vibe, and it is the finding that makes the usefulness delta
below less flattering than it looks.

### 4.2 The blind panel (3 rubric passes × 20 episodes × 3 arms, mean of 20 per cell)

| arm | accuracy | usefulness | trustworthiness |
|---|---|---|---|
| A — template | 4.667 | 2.233 | **4.667** |
| B — polish | 4.667 | **4.400** | 4.000 |
| C — why-line | 4.667 | 2.667 | 4.667 |

Inter-rater agreement: 53.1 % exact agreement, mean absolute difference 0.29–0.63 depending on the
pair. The usefulness jump is real and large (2.23 → 4.40). The trustworthiness delta goes the *wrong
way* (-0.667): a reviewer who reads the polished prose trusts the artefact less, and the likeliest
cause is the coverage loss above — prose that reads complete while carrying half the numbers is
exactly the failure a reviewer smells without being able to point at.

The ticket asked for three human reviewers; what it got was three rubric passes of one model
(§6). That is the weakest part of this measurement and §7 is written so it survives anyway.

**The panel's own flags.** Reviewers hand-flagged 12 claims as suspect; the mechanical audit
confirmed **0** of them (`over_flag_rate` 1.00). They split into two useful piles. Seven were
*interpretations* a reviewer disliked but the evidence supports ("breadth at its cap: 6 surfaces over
111 files" — true, `breadth (12/12)`). Five were complaints about the **delta document**, not the
narrative: the `reviewable churn 3 711 of 3 711 (spine-1/noisy-path, keeps 25%)` figure prints the
"keeps 25%" clause even when nothing was discounted, and the `cannot see whether…` blind-spot list
reads as a list of accusations. Both are worth fixing in `e4.7`'s inputs and neither has anything to
do with an LLM — a follow-up ticket, not a policy.

### 4.3 Cost, latency and determinism per episode

Arm A's milliseconds are measured. Arms B's dollars and seconds are **modelled** from recorded token
counts at a reference price band ($3/Mtok in, $15/Mtok out, 55 tok/s, 0.4 s to first token); no API
key or provider endpoint was reachable from the spike sandbox, so nothing in those columns is an
invoice or a wall-clock. Coverage is the share of the evidence index each arm cited.

| episode | what it was | A ms | A words | B words | B tok in/out | B $ modelled | B s modelled | A cov | B cov |
|---|---|---|---|---|---|---|---|---|---|
| `5d5c29` | define ZZ Epic One and put t | 2.923 | 483 | 179 | 1394/283 | 0.008427 | 5.55 | 0.90 | 0.54 |
| `12c385` | ADR-002: Rust workspace layout and cra | 2.933 | 505 | 178 | 1531/284 | 0.008853 | 5.56 | 0.82 | 0.43 |
| `9eae7d` | path taxonomy and noise poli | 3.209 | 558 | 187 | 1984/299 | 0.010437 | 5.84 | 0.60 | 0.35 |
| `b73a5a` | repository discovery and git | 3.196 | 542 | 180 | 1955/282 | 0.010095 | 5.53 | 0.62 | 0.32 |
| `83ce1e` | incremental indexing with a  | 3.41 | 565 | 189 | 2439/294 | 0.011727 | 5.75 | 0.46 | 0.25 |
| `f4ae22` | dependency manifest and lock | 5.288 | 540 | 169 | 3691/291 | 0.015438 | 5.69 | 0.30 | 0.16 |
| `1fcc80` | episode segmentation — bound | 3.536 | 587 | 197 | 2006/307 | 0.010623 | 5.98 | 0.68 | 0.37 |
| `a0d6c1` | the TUI shell — tickless loo | 3.581 | 583 | 204 | 2219/313 | 0.011352 | 6.09 | 0.55 | 0.30 |
| `2fc505` | background materialisation — | 3.228 | 538 | 175 | 1871/272 | 0.009693 | 5.35 | 0.68 | 0.32 |
| `5b0963` | spike s3 — what a graph-per- | 3.28 | 555 | 174 | 1800/286 | 0.00969 | 5.6 | 0.76 | 0.39 |
| `3e7c88` | test surface delta and cover | 3.433 | 594 | 168 | 2046/269 | 0.010173 | 5.29 | 0.62 | 0.31 |
| `8881ed` | session state persistence an | 3.17 | 531 | 170 | 1882/263 | 0.009591 | 5.18 | 0.64 | 0.31 |
| `dc66dc` | the TUI gets a data path — t | 3.494 | 550 | 194 | 1880/293 | 0.010035 | 5.73 | 0.68 | 0.34 |
| `c88985` | the queue ranks — the shippe | 3.214 | 525 | 180 | 2174/276 | 0.010662 | 5.42 | 0.52 | 0.29 |
| `44973a` | hooks v1 ingestion — the rea | 3.207 | 549 | 176 | 1932/268 | 0.009816 | 5.27 | 0.64 | 0.34 |
| `bf3b97` | rename and copy pairing — a  | 3.379 | 583 | 187 | 2103/294 | 0.010719 | 5.75 | 0.58 | 0.28 |
| `0a4595` | a turn_id is <session_id>: | 4.101 | 698 | 231 | 2291/351 | 0.012138 | 6.78 | 0.74 | 0.40 |
| `50e8c8` | one versioned delta model, a | 3.425 | 577 | 189 | 2514/290 | 0.011892 | 5.67 | 0.46 | 0.24 |
| `b55068` | dependency risk — who asked  | 3.032 | 526 | 181 | 1866/283 | 0.009843 | 5.55 | 0.62 | 0.32 |
| `03fcc0` | the first five minutes — a r | 4.087 | 679 | 225 | 3416/336 | 0.015288 | 6.51 | 0.35 | 0.20 |
| **mean** | | **3.37** | 563 | 187 | — | **0.0108** | **5.70** | 0.61 | 0.32 |

At 1 000 episodes an index-wide re-narration would cost ~$11 modelled and ~95 minutes of decode,
against ~3.4 s of template CPU. Neither number is the reason for the decision, and both are
placeholders until a provider is actually priced (§6).

**Determinism.** Arm A: 20/20 byte-identical in-process and identical across a fresh interpreter —
no clock, no locale, no terminal probe, so a golden test can bless it and `e5.7` can ship byte-stable
exports. Arm B: **0 of 6 resamples byte-identical**, word counts 181–201 against 87–111 on the same
prompt, and 0–3 figures asserted in one sample and absent from the other. One sample is not a
determinism guarantee — but the drift it shows is in *which facts got said*, which is the drift that
breaks a blessed artefact.

## 5. What arm A is bad at, since that is what the polish arm was bought to fix

Arm A reads like a spec sheet, and the panel said so with a 2.23. Concretely, three things are wrong
with it and **all three are template bugs**:

1. **No ordering by importance.** `**What** — lines +9 356 -71 [f1]; files 64 file(s) [f2]; churn …`
   prints figures in document order, so the reader has to find the one that matters.
2. **It repeats itself.** The `attention` section relays the risk figures and then the `Risk` section
   prints them again, because the delta document relays them again.
3. **It quotes the delta document's house style at the reader.** `path-rule discount only — not
   answered here, it is e2.2's` is an internal sentence wearing an external hat, and it is the reason
   five reviewer flags landed on the delta document rather than the narrative.

None of that needs a language model. It needs the template to rank before it prints, to stop
duplicating a section it has already relayed, and to phrase a gap as what the reader cannot do. Arm B
proves the fix works (§4.2's usefulness column) *and* what the fix costs (§4.1's coverage column,
§4.2's trust column) — which is precisely the trade a template must not be asked to make, because a
template can drop the verbosity and keep every figure.

## 6. Threats to validity, stated before they are discovered

This is a spike and the honest part of a spike is the list of reasons not to believe it.

1. **The author and the auditor are the same model.** The same session wrote `audit.py` and wrote the
   arms, so the arms were written by a model that knew, implicitly, what the audit would catch. The
   zero hallucination count is therefore an **upper bound on a disciplined arm** — it is *not* a rate
   some other model at some other temperature would hit, and it is certainly not what happens when the
   prompt is "write a helpful summary". Any re-run must include an arm prompted for helpfulness rather
   than citation, or the number is meaningless.
2. **The panel is not human.** Three rubric passes of one base model, one shared reading of the
   corpus, so inter-rater agreement measures rubric spread rather than reviewer spread. A person who
   has to sign a merge and a model scoring a rubric fail on different things.
3. **Cost and latency are modelled, not measured.** No API key or provider endpoint was reachable from
   the sandbox. Token counts were estimated at 4 characters per token from recorded characters and
   priced at a placeholder band. `REFERENCE` in `spike.py` is the single place to change when someone
   with a bill re-runs this.
4. **One repository, one author.** Every episode is Fathom's own history, annotated by the project
   author, in a repo whose commit discipline (ticket id, em-dash, purpose in the subject) is better
   than most and is exactly the condition that makes arm C look good: its escape hatch — "the commit
   subjects do not say what this episode was for" — was never needed on this corpus.
5. **Blinding hid labels, not style.** Nobody can mistake a 500-word figure table for a 180-word
   sentence, so the panel scored recognisable artefacts with unknown provenance. That removes label
   anchoring; it does not remove style inference.
6. **The audit is a proxy and it was tuned.** It went red on 13 of 20 arm-A narratives on its first
   run — every one of those was a bug in the audit (digit grouping like `3 664`, spelled compounds
   like "forty-five" split into 40 and 5, subset counts like "test side 14 files" charged against the
   episode total). The controls that fixed those — the derivation set, the disqualifier list, the
   quoted-span exemption — are leniency, and each leniency is a hallucination class the audit will now
   miss. The control that matters is §4.1's first row: **the control arm is clean at both tiers**, so
   the audit is not simply crying wolf; the corollary is that it can only find what its rules describe.
7. **Arm A's zero is structural, not behavioural.** A template cannot name a file the delta document
   did not name and cannot invent a motive because it does not write motives. That is the whole
   argument, and it is worth saying plainly: the template is not *less likely* to hallucinate, it has
   no channel to hallucinate through.

## 7. Recommendation

**Ship arm A. Ship nothing of the LLM path.** Not "gate it behind further work" as a polite way of
saying "later": the `net-llm` narrative seam gets no consumer, no `privacy.llm` surface and no code
path, and reopening it requires clearing every gate below. Recorded as `reco-nv0`.

1. Hallucinated claims at zero on ≥100 episodes across ≥3 repositories, **including an arm prompted
   for helpfulness instead of citation** — the omission of that arm is gate 1's whole point.
2. A measured win on **trustworthiness** as well as usefulness, from three **human** reviewers. Use is
   cheap to buy with prose; trust is the bar ADR-001 actually sets, and here it went backwards.
3. Cost and latency measured against a real provider — bill and wall-clock — and a fallback path that
   is **byte-identical** to the no-LLM path when the call fails, times out or is offline.
4. `e5.7`'s byte-stable exports survive a polished narrative across reruns, or the guarantee is
   dropped for polished episodes **on purpose, in the open**, not lost quietly because a model rewrote
   a sentence differently on Tuesday.
5. Citation preservation **enforced by the renderer, not instructed to the model**: a polish that
   drops or invents a citation must fail the render. `e4.7`'s AC says "every claim carries a
   resolvable evidence reference (validated in tests)"; a test that only checks arm A proves nothing
   about the arm that needs it.

And do the thing the spike actually found: **fix the template.** Rank figures by what they cost a
reader rather than printing document order, stop repeating the attention block, and phrase gaps as
what the reader cannot do. Arm B's 4.4 usefulness is reachable with none of its coverage loss, and it
is the version that keeps determinism, the byte-stable export, the offline guarantee and the audit's
one clean zero.

## 8. Reproducing this

The 20 selected delta documents ship with the spike, gzipped, under `spike/s5/deltas/`, so the
selection rule, the packets and the whole report re-derive in CI with no binary, no network and no
clock. To rebuild the packets from a live index instead:

```bash
# 1. the evidence set: one `fathom analyze --json` per selected episode
for ep in $(python3 -c "import json;print(' '.join(e['episode'] for e in json.load(open('crates/fathom-analyze/spike/s5/selection.json'))['episodes']))"); do
  ./target/debug/fathom analyze "$ep" --json > /tmp/deltas/"$ep".json
done
# 2. packets + arm A (arms B and C are recorded artefacts and are not regenerated)
python3 -m recommender.narrative collect --deltas /tmp/deltas --spike crates/fathom-analyze/spike/s5
# 3. the report, and the check CI runs
python3 -m recommender.narrative score --spike crates/fathom-analyze/spike/s5 \
        --report benchmarks/reports/s5-narrative-polish.json
python3 -m recommender.narrative score --spike crates/fathom-analyze/spike/s5 \
        --report benchmarks/reports/s5-narrative-polish.json --check
```

`--check` re-derives every recorded figure and names the ones that moved, ignoring the columns a
re-run legitimately changes (measured milliseconds, modelled cost). The claim audit also runs
standalone, which is how every finding in it got hand-adjudicated:

```bash
python3 -m recommender.narrative audit --packet <packet.json> --text <narrative.md>
```
