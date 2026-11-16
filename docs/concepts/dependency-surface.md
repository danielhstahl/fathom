# The dependency surface: what a revision declares, what it resolved, and what this build could not read

This page moved. Its prose is now in
[Dependencies: what a revision declares, what an episode changed, and whether to be worried](dependencies.md),
which `reco-48qd` merged from `dependency-surface.md`, `dependency-delta.md` and `dependency-risk.md` under
[the documentation merge map](../reference/doc-map.md)'s M6 — three pages about one crate, now one page in
the order the questions are asked. Nothing was dropped, and this page is a signpost: it restates no claim
and holds no table.

Where this page's sections went — all of them inside *the surface*:

- the title, and why a parse failure is a value and not an error →
  [The surface](dependencies.md#the-surface-what-a-revision-declares-what-it-resolved-and-what-this-build-could-not-read)
- §1 → [What the surface decides, and what it refuses to](dependencies.md#what-the-surface-decides-and-what-it-refuses-to)
- §2 → [The shape of the surface](dependencies.md#the-shape-of-the-surface)
- §3 → [Twenty-five formats, eight ecosystems](dependencies.md#twenty-five-formats-eight-ecosystems)
- §4 → [Workspaces and monorepos](dependencies.md#workspaces-and-monorepos)
- §5 → [When a file cannot be read](dependencies.md#when-a-file-cannot-be-read)
- §6 → [Diffs](dependencies.md#diffs), the front door to *the delta*
- §7 → [Features](dependencies.md#features) · §8 → [The budget](dependencies.md#the-budget)
- §9 → [What is deliberately not parsed](dependencies.md#what-is-deliberately-not-parsed)
- §10 → [Where the surface's tests are](dependencies.md#where-the-surfaces-tests-are)
