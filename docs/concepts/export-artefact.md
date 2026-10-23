# The review artefact: one record, three renderings, and the same bytes twice

- **Status**: Accepted, and implemented
- **Ticket / plan key**: `reco-1tn` (`e5.7`)
- **Depends on**: [the delta model](delta-model.md) (`e2.11`) for every figure,
  [the episode narrative](episode-narrative.md) (`e4.7`) for the prose,
  the review ledger of `e4.6` (crates/fathom-analyze/src/ledger/) for the sign-offs,
  [the dependency delta](dependency-delta.md) (`e2.6`) and
  [the folder-structure delta](folder-structure.md) (`e2.3`) for the two range-wide comparisons
- **Implements**: the plan's `e5.7` acceptance criteria — markdown that renders in a GitHub PR preview with
  nested evidence links, two consecutive exports of the same range that are byte-identical, one
  self-contained printable HTML file that escapes what the repository committed, and a sign-off block that
  carries reviewer, time, scope and the reason behind every dismissal; ADR-001 §7.10 (one reality: the
  screen, the artefact and an agent read the same bytes); ADR-002 §5.3 (one façade, so the CLI, the TUI and
  an MCP tool call cannot mean three different things by "export").
- **Executes as**: `crates/fathom-analyze/src/export/record.rs` (the record, its builder and its self-check),
  `crates/fathom-analyze/src/export/markdown.rs` and `crates/fathom-analyze/src/export/html.rs` (the two
  renderings), `crates/fathom-analyze/src/export/mod.rs` (`Format` and `render`, the one door three surfaces
  walk through), and `crates/fathom-cli/src/export.rs` as `fathom export`; pinned by
  `crates/fathom-analyze/tests/export.rs` and `crates/fathom-cli/tests/export_verb.rs`, and the `--json`
  payload is validated against [`schemas/export.schema.json`](../../schemas/export.schema.json).

---

## 1. What this decides

One question, per range:

> **Can a reviewer paste this into a pull request, or attach it to an audit, and have it still be true when
> somebody reads it next week?**

Everything below follows from that. The artefact is not a report generator with its own opinions: it is a
**rendering of the delta documents** the index already holds, plus the ledger's sign-offs and the detectors'
findings, in a shape a person acts on.

Four consequences, each of which is a test:

* **One structure, three renderings.** `export::build` composes a single `Record`; markdown, HTML and JSON
  are layouts over it. The JSON artefact *is* the record (`fathom-review-record/1`), which is why the
  cross-format tests can assert that a finding in the JSON is present in the markdown and the HTML too —
  a fact hidden in one format because it is awkward to lay out is the second reality ADR-001 §7.10 exists
  to forbid.
* **No clock.** Nothing in the record reads the time. Every instant it prints is one the repository or the
  ledger already held — a commit's committer date, a sign-off's `signed:` stamp. This is what makes
  "byte-identical twice" a property rather than a coincidence: the alternative is an artefact a reviewer
  cannot diff against the copy they read last Tuesday.
* **Committed text is data.** A commit subject, a tag message, a ledger note: each is text nobody vetted,
  and the artefact puts it into markdown and HTML — two document types where text can turn into markup.
  The renderers treat that as a safety property, not a cosmetic one (§4).
* **An absent answer is printed as an absence.** A range with no dependency delta prints
  *Not computed for this range* with the ticket that owns it; the seven sections print their
  `measured / partial / unavailable` counts; every gap names an owner. Zero and "nobody looked" are
  different facts and the artefact does not blur them.

## 2. What the artefact contains

The order is the order a reviewer works in: verdict, then the episodes, then the evidence, then what to do.

| Section | What it answers | Where it comes from |
|---|---|---|
| Scope and verdict | which range, resolved to what, how many episodes and commits, how many are signed off | [`Scope::resolve`](../../crates/fathom-cli/src/export.rs) and the summary roll-up |
| Summary card | scale, churn, reviewable churn, highest risk, one row per episode with its decision | the episode delta documents' totals and risk view |
| What this record can and cannot answer | each of the seven sections' state across the range, with the owning plan key | the delta documents' section states |
| Episodes | `e4.7`'s narrative verbatim, then the episode's commits, card, ranking and file rows | `narrative::compose` + the delta document |
| Dependency delta | what the tree now depends on that it did not, **for the range** (not per episode) | `fathom_analyze::deps`, compared between the range's endpoints |
| Architecture delta | directories touched, renames, deletions, binaries, surfaces, noise folded away | the delta documents' file rows |
| Findings with evidence | every detector finding, worst first, each with its evidence lines and its suppress key | `fathom_analyze::risk::collect` |
| Operator-action checklist | what is still being asked of a human, open first, each with what to type | decisions, findings, ledger problems |
| Sign-offs | reviewer, kind, instant, decision, scope, dismissal reason, note, digests, and whether the content still matches | `.fathom/reviews/` through `fathom_analyze::ledger` |
| Review memory | every note the range carries: kind, status, author, instant, its words as written, how it is attached now (including after a rewrite), and its digests | `.fathom/reviews/notes.md` through `fathom_analyze::notes` (`e6.5`, `reco-552`) |
| Commits | every commit in scope, with author, instant, its episode and a web link when the remote makes one derivable | the store's commit rows |
| What this record cannot answer | every gap, once, with its owner and remedy | the delta documents' gaps + the export's own |
| Provenance and self-check | which contracts and version wrote the file, which checks ran, and what they found | `export::record::CHECKS` |

The artefact's own contract name is `fathom-review-record/1`; it names the delta, ledger and narrative
contracts it was built from, so a reader holding a stale file can tell which layer moved.

## 3. Byte-stability, and what it costs

`export::build` is a pure function of its `Input`: episodes, commits, sign-offs, findings, the dependency
delta, and the strings the CLI read out of the store and the ledger. Determinism is structural rather than
hoped for:

* episodes sort by `(first member commit, id)`, commits by `(instant, oid)`, findings by
  `(severity, kind, subject)`, sign-offs by `(instant, id)`, checklist items open-first then by kind;
* maps are `BTreeMap`s, never `HashMap` iterations;
* nothing calls `now()`, reads a locale, or probes a terminal;
* the `--out` and `--root` the operator typed are stripped from the echoed command line, because an artefact
  whose text depends on where it was saved is not the same artefact twice.

What it costs is a rule about new fields: **anything that is not derived from the input does not belong in
the record.** A generated timestamp, a duration, a path outside the repository, a "generated on" footer —
each would break the acceptance criterion, so the self-check refuses absolute paths in the fields the module
owns and the tests rebuild a record after a wall-clock second and compare bytes.

Ordering is deliberately not by wall-clock-of-the-run: the same range re-exported after a sign-off changes
is a different record and says so, which is the honest version of "stale".

## 4. Hostile input, in two document types

A commit subject is user input. The fixture corpus is one subject carrying all of it at once —
`<script>alert("x")</script>`, `<img src=x onerror=…>`, a `javascript:` link, a `|` and a backtick.

**HTML** is the artefact's own format, so it escapes everything unconditionally — text, attributes, and code
spans alike (`&lt;script&gt;` inside `<code>` is still readable). The document is one file: inline CSS only,
no `<link>`, `<img>`, `<iframe>`, `<form>`, no `url(http…)`, and a
`Content-Security-Policy` meta of `default-src 'none'; script-src 'none'; …` so that even a renderer bug
cannot reach the network or run script. It is printable: `@page` margins, `break-inside: avoid` on
episodes, rows and list items, `thead` repeated across pages, navigation hidden, and
`print-color-adjust: exact` because colour carries meaning here (the section states, open versus done) and
every colour-coded mark also says what it means in words.

**Markdown** has an inert span and a live everything-else, so the rule is different and narrower:

* every field is escaped on the way in (`inline()` for text, `code()` for a path or digest — the latter
  widens its own fence until the text cannot close it early), which kills the `|` that would split a table
  row and the link-shaped subject that would otherwise become clickable;
* the narrative is printed as `e4.7` composed it — that verbatim-ness is check three of the self-check —
  with one rendering pass, `safe_prose`, that turns committed text back into text: a `<` that opens a tag
  becomes `&lt;`, a `[` that opens a `javascript:`/`data:`/`vbscript:`/`file:` link gets escaped so the link
  never forms. Both are done **outside code spans only**, because inside a span the text is already inert and
  escaping it there would print `&lt;` where the reviewer came to read `<`.

The store is not touched by any of this: the ledger and the index keep the subject verbatim, which is its own
test. Escaping is a rendering decision, and losing what a commit said is not a security improvement.

## 5. The sign-off block

A sign-off is a claim about the past — *"I read this, at this time, against these digests"* — and the
artefact is a claim about the present. Printing the stamp without the second would be the worst kind of
green tick, so each entry prints:

* reviewer and whether they were a human or an agent acting for one;
* the instant, as the ledger holds it, in ISO-8601 UTC — never the export's own clock;
* the **scope** it covered: commits, files, `+add −del`, paths — plus the member commits it named, and how
  many it did not print, so a capped list is not read as a complete one;
* for a dismissal or a snooze, the reason code **and** the shipped vocabulary's words for it, next to the
  reviewer's own note: a dismissal without its reason is the decision that hides a problem;
* the digests it was taken against, labelled, plus the entry's own tamper digest and the ledger file it came
  from, so an audit can re-hash it with `fathom review verify`;
* whether the content still matches — `unchanged`, `changed`, plus the ledger's own explanation, and nothing
  at all when the comparison could not be made (`None` is not "unchanged").

A dismissed episode is not counted as reviewed: the north-star number is accepted sign-offs, and the summary
says `0 of 2 … · 1 undecided · 1 refused or deferred` rather than rounding a dismissal up.

## 5b. The review-memory block

The sign-off says a person accepted a change; the memory says what they wrote down while doing it — the
decision and its reason, the question nobody has answered, the follow-up somebody owes
([the review memory](review-memory.md)). Each note prints its kind, its status, its author and author kind,
the instant the file holds, its text as written, and the evidence: the episode it is about *after* the re-link
ladder, the rung that placed it, the episode it was written against when a rebase moved it, its four identity
digests, its own tamper digest and the file-and-anchor a reader can open.

An unanswered question or follow-up is also an open checklist item — `Answer the question nt-…`, with
`fathom note answer nt-… nt-… "…"` as the thing to type — so `--strict` refuses to call a range finished
while a reviewer's question is still sitting in the memory file. A note whose episode was rewritten away
stays in the artefact and says so: an orphan is a fact, and a note that vanished from the export would read
as a note that was deleted.

## 6. The self-check is a check

Every artefact prints `integrity` — the verdict and the list of checks — and `Record::verify` is run at build
time, so `integrity.problems` is a result rather than a claim. The checks are `export::record::CHECKS`: the
delta documents verify and are in the stated scope; the ordering matches what the record says it used; every
narrative is the bytes the composer produces for its own document; every finding and sign-off belongs to an
episode in scope; closed checklist items name the entry that closed them; no absolute path, generated
timestamp or control byte leaks; and the summary's counts are the counts of the rows it summarises.

The tamper tests plant each fault — a finding for an episode outside the range, a summary that disagrees with
its episodes, a sign-off for nothing, a host path in `provenance.store` — and require the check to name it.
An audit nobody has seen fail is decoration.

## 7. The verb

```
fathom export [--format md|html|json] [--out STEM] [--episodes RANGE]
              [--root R] [--stdout] [--strict] [--json]
```

* `--episodes` accepts an episode id, a revspec (`HEAD`, `HEAD~3`, an oid prefix), and the ranges `A..B`
  (exclusive of `A`) and `A..=B`. A bare handle means *the episode that handle belongs to*, which is what the
  single-commit verbs mean by the same spelling and what makes the default `HEAD` a one-episode artefact
  rather than the whole repository.
* `--out` is a stem; the format's extension is appended, and a stem that already ends in it is used as typed.
* `--stdout` puts the artefact on stdout and the report on stderr, so `fathom export --stdout > review.md`
  captures the artefact and nothing else. It conflicts with `--json`, because two owners of stdout is a
  usage error, not a composition.
* `--strict` exits `7` (`CHECK_VIOLATIONS`) while the checklist has an open item, and still writes the
  artefact — the operator has to read something to close the gate.
* `--json` prints `fathom.cli.export/1`: the scope, the record's contract and section states, the summary,
  the sign-offs, the open checklist items, the artefact's path, size and sha256. The byte-stability guarantee
  lives in the artefact; this payload describes a *run*, so it names the destination too.

Refusals write nothing: no index is `12` (`INDEX_DEGRADED`) with `fathom index` as the remedy, a rev that
names nothing is `1`, `--root` outside a repository is `3`, and a fourth format is `2` — the three formats
are the façade's list, and adding one is a §5.3 change rather than a flag.

## 8. What is deliberately not here

* **No upload, no PR host.** The artefact is a file. Posting it is your `gh` call (thesis §8 non-goal 4).
* **No second template language.** The TUI's `o` preview renders the same `Record` through these same
  functions; a preview that disagrees with the file is a bug in this module, which is what makes the
  byte-stability test worth having.
* **No new analysis.** Every number in the artefact came from the delta model or from a comparison module
  that already existed; where an answer is not computed yet, the artefact prints the plan key that owns it.
