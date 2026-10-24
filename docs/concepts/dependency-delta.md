# The dependency delta

The delta half of `fathom_analyze::deps` answers the question an episode of dependency change actually poses:
**what is installed now that was not installed before, and what made it so.** It diffs two *resolved
graphs*, not two lockfile texts, and every row it produces carries the evidence for its own claim.

It is a consumer of [the dependency surface](dependency-surface.md). The surface is an event-invariant
projection — what this revision declares and resolves; the delta is what the episode did to it. The
previous half (`e2.5`) built the before-and-after `Delta` of dependency files; this half reads those two
revisions as graphs and compares them.

This document answers *what moved and what proves it*. It deliberately does not answer the two questions a
reviewer of the same table asks next — *who asked for this dependency*, and *should I be worried* — which
are [`e2.7`'s](dependency-risk.md): provenance, advisories and licence policy read this delta's rows and
own those answers.

## Model

```rust,ignore
let episode = deps::dependency_episode_repository(&repo, Some(old_tree), Some(new_tree), &Default::default())?;
println!("{}", deps::render_table(&episode, &Default::default()));
```

- **`DependencyState`** — one revision as a resolved graph plus the declarations that requested it.
  Built from either side of the episode: `State::of(&Surface)` for a cold scan, `State::of_delta(&Delta)`
  for the warm path, where only the changed files are in hand.
- **`DependencyDelta`** — the change between two states: one `DepChange` per package per
  thing that happened to it, with counts by attribution, graph-size movement per ecosystem, source and
  licence set deltas, peer conflicts, a pattern label, a sentence, and notes.
- **`FileDiff` / `Hunk`** — the line-level diff of each changed dependency file, and `locate_hunk`, which
  finds, for one claim, the hunk that proves it.
- **`render_table`** — the dependency table for an episode, with its drill-down.

## Direct and transitive are properties of the change

A package is direct when *this repository asks for it*: a manifest declares it, or the lockfile says so
(Gemfile.lock's `DEPENDENCIES`, NuGet's `"type": "Direct"`, npm's importer block). It is transitive when
another resolved package asks for it. Attribution is recorded per change with the evidence it came from —
`lockfile` statement, manifest declaration, resolved edge — because the strength differs and the reader is
entitled to know which they got.

Three rules that exist because the naive version is wrong:

- **A manifest can contradict its own lockfile.** The lockfile is what resolved; the manifest is intent.
  Where both speak, the lockfile wins and the contradiction is a note.
- **A `// indirect` requirement is not a direct dependency.** Go writes indirect requires into `go.mod`,
  which is the manifest; the comment is the only thing distinguishing them, so it is read.
- **Where the graph is not recorded, nothing is called transitive.** `go.sum` is a hash list with no edges,
  and a Gradle lockfile resolves one configuration per line. A package there that no manifest declares is
  `unknown`, with a note naming the file and the reason — an unprovable claim is worse than an absent one.

## Why the graph, and not the lockfile diff

A lockfile diff tells you every line that moved. That is the wrong unit:

- **A mass transitive refresh moves a hundred lines and means one thing** — the lockfile was refreshed.
  A `dependency_bump_storm` is a *pattern*, decided by whether the moves have a direct cause, not by how
  many lines changed. The test that matters: one new direct dependency plus twelve refreshed transitive
  packages the new dependency does not explain is reported as *both*, because they are two facts and
  collapsing them is how a reviewer misses the second one.
- **A version can change without a line changing.** Re-resolving a range may pick a different version while
  the version *string* stays put; the graph comparison also catches integrity, source and licence changes
  for a package whose version never moved.
- **Attribution needs edges.** "Who asked for this" is a question about the graph. The answer is a path —
  `foo → bar → baz` — and it is stored on the row.
- **Two lockfiles can express one graph.** Format churn — lockfileVersion 2 to 3, a reordered dependency
  list — is a large text diff and a zero graph delta.

## Provenance, licences, conflicts

Version changes are one family: **upgrade**, **downgrade**, and **version changed** for the pair that cannot
be ordered (compare with the version grammar's own limits — an unorderable pair is stated, never guessed).
**Source switches** are separate: a package that stopped coming from the registry and started coming from
git is a supply-chain event, not a version change, and the source *identity* deliberately ignores git refs
and commit hashes — moving a commit is a ref move, while naming a new URL, registry or path is a **new
source**, reported on its own line. Integrity changes for an unchanged version are their own row: same
version, different bytes.

Licences are compared as sets, because the interesting fact is *what the repository now ships* — the sum of
the licences newly seen and vanished, with the package that brought each one.

A **peer conflict** is claimed only where the graph contradicts a declared range: something requires
`react >=18` and the resolution contains `react 17.0.1`. An unsatisfiable range that cannot be parsed is
never a conflict; that is the version module's rule, and the delta inherits it.

## The warm path, and the honesty it forces

The episode path has both revisions' changed files and nothing else, which is enough to diff graphs and not
always enough to attribute them. When the changed set holds a lockfile but not the manifest that declares
one of its packages, that package is `unknown` and a note says so. A table that says "3 unattributed, see
notes" is worth more than one that guesses, because the guess is what a reviewer would have to re-check.

## Reading the table

```text
dependency delta — Net +2 packages from 1 new direct dependency (foo 2.1.0); plus a mass refresh of 12 transitive packages nobody asked for; licence set +1.
pattern         mixed (bump storm)
graph           js 13 → 15 (+2)
direct          +1 upgraded 0 downgraded 0 -0
  direct        + foo 2.1.0                js
transitive      +1 upgraded 12 downgraded 0 -0 (refresh of 12)
  explained     1 explained by a direct change, 12 without one
  …
drill-down

  transitive · added bar 1.0.0 · js · package-lock.json
    attribution: transitive — another resolved package asks for it
    asked for by: foo
    @@ -31,76 +32,91 @@
    +    "node_modules/foo": {
    +      "version": "2.1.0",
```

Every printed row has a hunk or says why it has none. The excerpt is cut round the line that proves the
row, not the top of the hunk: in a mass refresh the two are usually a hundred lines apart.

## Where it reaches the product

Three reads, one measurement each, and each one says which of them it is:

| read | entry point | what the pair of trees is |
|---|---|---|
| a cold scan of a revision | `State::of` on a `Surface` | one revision, no delta |
| an episode, from the index | `Supplied::for_episode_with_repo` (`e2.11`'s seam) | the episode's first member's parent and its last member |
| a range someone typed | `fathom check --only dependencies --base A --head B` | the two refs, as written |

The episode read is the one that closed Epic 2's exit criterion — *every episode answers what it now depends
on, with every number traceable* — and it carries three rules that the CLI cannot invent for itself:

- **An empty answer is a measurement.** An episode that touched no manifest produces a measured section
  whose graph was not read, not a `0 → 0` line implying a graph was compared. The section says the file pair
  held nothing dependency-shaped, which is a fact about the episode; `Unavailable` stays reserved for a read
  with no object database, which is a fact about the reader.
- **Every figure carries the command that reproduces it.** The direct count, the graph size, the sources and
  the pattern all point at `fathom check --only dependencies --base <from> --head <to>`, which is the same
  analysis on the same two commits. A reviewer who does not believe a number runs the number.
- **The dependency answer is not added to an episode's churn.** The delta document reports dependency
  movement as its own section; the churn in `what changed` counts it as ordinary file churn with the
  ecosystem named, because a lockfile's lines are churn and pretending otherwise moves a number the reviewer
  is holding.

The roll-up on `fathom episodes` deliberately does *not* carry this answer: it would open one object pair per
row to print one line each, on a hot path with a stated budget (ADR-004 §11).

## Limits

- The graph is only as good as the parser that read it. Lockfiles that record no edges cannot support
  transitive claims, and say so.
- Version comparison is an ordering over what a human could read. Unknown grammars, wildcards and
  non-numeric versions produce "cannot order", not a guess.
- Resolved-graph size counts resolved entries per ecosystem, so a lockfile that lists one package many
  times over-counts, and a manifest-only member under-counts. It is exact only when the file says it is.
- The peer conflict check is the declared range against the resolved version — real resolution backtracking,
  which npm and pnpm do at install time, is out of scope.
