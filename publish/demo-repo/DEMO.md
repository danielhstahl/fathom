# signal — the repository Fathom demos itself on

This is a real git repository with 21 commits, two human authors and one agent, and every one of them
exists to put a row in a different part of [Fathom](https://github.com/danielstahl/fathom): a rename, a
dependency bump in two ecosystems, a deleted test, a CI job, an introduced cycle, and a merge. It is the
same miniature Fathom unpacks when you run `fathom --demo` — the bytes are literally the same, they come
out of the pack `crates/fathom-cli/assets/fathom-demo.pack` — published as an ordinary clone so the tour
can be run against something you `git clone` rather than something a binary materialises for you.

```bash
cargo install fathom-cli        # or: cargo build -p fathom-cli, or the installer in the README
fathom index && fathom episodes --plain
```

## Five minutes, with the gate

```bash
fathom index --no-progress --json          # ~0.3 s for 21 commits; the JSON splits it by phase
fathom episodes --plain                    # the review rail, in a log instead of a frame
fathom why 4513a9880166 --plain            # one episode's risk score, as an addition you can check
fathom check --base 4513a9880166 --head HEAD --fail-on warning   # the gate: exit 7, and a sentence
fathom review sign ep-4a42de295c13b5229318 --note "read on the demo"   # the act, as committed markdown
fathom review status                       # still 7: one sign-off is not twenty-one
fathom export --episodes ep-4a42de295c13b5229318 --format markdown --out review.md
```

The fifth line is the interesting one. `fathom check` **fails** on this history: four new direct
dependencies arrive between the first commit and HEAD, and a gate that approved all of them would be
decoration. The findings are the product working, not the demo being broken — see
[the gate's own page](https://github.com/danielstahl/fathom/blob/main/docs/concepts/ci-gate.md).

## Why this history looks like this

| Commit shape | What it exercises |
|---|---|
| `add the crate`, `parse:`, `query:` | the plain spine: commits to episodes, churn, the rail |
| `move core.rs into its own module` | a rename — episode identity across a path change |
| `tests: fold the empty-store case…`, and the deleted test beside it | the test-surface delta, and `untested_new_logic` |
| `bump serde…`, `bump express…`, `add toml…` | dependency deltas in two ecosystems, and the gate's `dependencies` check |
| `rustfmt the parse module` | noise suppression: reformat churn discounted, and named |
| `ci: deploy the release build…` | non-code surfaces: a change an operator has to act on |
| `generated with claude: …` | attribution: an agent-authored commit, ranked as one |
| the merge at the tip | episode identity across a history rewrite |

`tools/gen_demo_repo.py` in the Fathom repository is what builds it, and
`python3 tools/five_minute_demo.py run` there is what runs this page's commands and fails if any of them
stops being true. The script is the version of this tour that cannot go stale.

## The gate that runs here

[`.github/workflows/fathom.yml`](.github/workflows/fathom.yml) calls Fathom's reusable
`fathom-check` workflow on every pull request — the same five lines any repository can copy. It reports
rather than blocks (`mode: warn`), because on this repository the findings are the exhibit.
