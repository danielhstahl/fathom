# Release and distribution — cutting an alpha, and what an installer promises

Status: the skeleton was built by `e0.7` (**reco-e77**); the alpha was cut by `m0` (**reco-n1j**).
Config: [`dist-workspace.toml`](../../dist-workspace.toml). Generated workflow:
[`.github/workflows/release.yml`](../../.github/workflows/release.yml). Gate:
[`tools/check_release.py`](../../tools/check_release.py), asserted by
[`tests/test_release_contract.py`](../../tests/test_release_contract.py). Measured evidence:
[`benchmarks/reports/dist-release-plan.json`](../../benchmarks/reports/dist-release-plan.json).

ADR-002 §10.1 handed the artefact set to `e0.7`, and `e0.7` built it: `cargo-dist` configured, a
generated workflow, one binary. What it could not build, and what no commit can build, is the second half
of `m0`'s third acceptance criterion — "**alpha artefacts installable on three OSes**". A commit can
decide the pipeline; only a public remote, three runners and three humans can decide the sentence. So
this document is the part that turns "the config exists" into "cutting an alpha is a repeatable, checked
act that someone who is not the author can perform in ten minutes".

The failure this document exists to prevent is the one `e0.7` left lying around: ADR-002 §10.3 records
that `package.repository` is a **placeholder** (`github.com/danielstahl/fathom`), that `cargo-dist` bakes
the installers' download URL out of it, and that a stale value is *a broken install, not a cosmetic bug*.
That is the shape of every problem here: silent, plausible, and discovered by a stranger following a
one-liner in a README.

## 1 What an alpha release is made of

One binary (`fathom`, built from `fathom-cli` with the default feature set, which **is** the product per
ADR-002 §6.1), four targets, two installers, one source tarball, one checksum file. The tables below are
generated from [`benchmarks/reports/dist-release-plan.json`](../../benchmarks/reports/dist-release-plan.json),
which is what `cargo dist plan` said when it was last run against this tree — refresh it with

```bash
dist plan --output-format=json | python3 tools/check_release.py capture   # re-measure (needs `dist`)
python3 tools/check_release.py write                                     # re-render this page
```

CI runs both in check mode, so this page cannot quietly disagree with the config it describes.

### 1.1 The target matrix

<!-- BEGIN GENERATED: targets -->
| Target triple | Runner (GitHub-hosted) | Artefact it builds | Built by |
|---|---|---|---|
| `aarch64-apple-darwin` | `macos-14` | `fathom-cli-aarch64-apple-darwin.tar.xz` | `--artifacts=local --target=aarch64-apple-darwin` |
| `x86_64-apple-darwin` | `macos-15-intel` | `fathom-cli-x86_64-apple-darwin.tar.xz` | `--artifacts=local --target=x86_64-apple-darwin` |
| `x86_64-pc-windows-msvc` | `windows-2022` | `fathom-cli-x86_64-pc-windows-msvc.zip` | `--artifacts=local --target=x86_64-pc-windows-msvc` |
| `x86_64-unknown-linux-gnu` | `ubuntu-22.04` | `fathom-cli-x86_64-unknown-linux-gnu.tar.xz` | `--artifacts=local --target=x86_64-unknown-linux-gnu` |
<!-- END GENERATED: targets -->

Each triple is built **natively**, on a runner whose own machine is that triple, which is why there are
four rows and not two: the tree-sitter grammars and the bundled SQLite are C, and a cross-compile is a
different build with different bugs. The four triples are the four a stock GitHub runner compiles on its
own. `aarch64-unknown-linux-gnu`, `x86_64-unknown-linux-musl` and `aarch64-pc-windows-msvc` are
deliberately absent — they need a cross-toolchain (zigbuild, or an arm64 Linux runner) and that is
`e7.4` (**reco-3q4**), which owns cross-platform packaging proper. `deny.toml`'s `[graph] targets` does
list every triple ADR-002 cares about, so licence and advisory resolution is checked across the whole
platform set whether or not a binary is built for it.

### 1.2 The artefact set

<!-- BEGIN GENERATED: artefacts -->
| Artefact | Made | What it is |
|---|---|---|
| `source.tar.gz` | once per release | the source tree a distro or an auditor builds from, with its own `.sha256` |
| `fathom-cli-installer.sh` | once per release | the `curl … \| sh` script for Linux and macOS; picks the target from the machine |
| `fathom-cli-installer.ps1` | once per release | the `irm … \| iex` script for Windows, so a Windows user is not told to use bash |
| `sha256.sum` | once per release | one digest per artefact, so a download can be checked without trusting the transport |
| `fathom-cli-aarch64-apple-darwin.tar.xz` | per target | `fathom` plus `CHANGELOG.md`, `README.md`, with `fathom-cli-aarch64-apple-darwin.tar.xz.sha256` beside it |
| `fathom-cli-x86_64-apple-darwin.tar.xz` | per target | `fathom` plus `CHANGELOG.md`, `README.md`, with `fathom-cli-x86_64-apple-darwin.tar.xz.sha256` beside it |
| `fathom-cli-x86_64-pc-windows-msvc.zip` | per target | `fathom` plus `CHANGELOG.md`, `README.md`, with `fathom-cli-x86_64-pc-windows-msvc.zip.sha256` beside it |
| `fathom-cli-x86_64-unknown-linux-gnu.tar.xz` | per target | `fathom` plus `CHANGELOG.md`, `README.md`, with `fathom-cli-x86_64-unknown-linux-gnu.tar.xz.sha256` beside it |
<!-- END GENERATED: artefacts -->

`e0.7`'s acceptance criterion said "downloadable without auth", and the honest reading of that is worth
writing down because it has been misunderstood twice: GitHub **workflow artefacts** (what a PR build
uploads) require a signed-in human, and no `cargo-dist` setting changes that. The auth-free artefact is
the GitHub **release** a tag produces — public assets, plus the two installers. A per-PR link that needs
no login needs an object store this project does not have; that half of the sentence belongs to `e7.4`.

Two more files ride on every release, generated from the lockfile and attached by
[`.github/workflows/sbom.yml`](../../.github/workflows/sbom.yml) (`e6.8`, **reco-h7p**):
`fathom-<version>.cdx.json`, a CycloneDX 1.5 SBOM of what a default build links, and
`fathom-<version>-licenses.md`, the same inventory read as licence obligations against `deny.toml`. Both
are committed under `sbom/` and re-generated on every PR, so the asset on the release page is the same
bytes a reviewer diffed in the pull request; `python3 tools/gen_sbom.py check` is what keeps them from
going stale, and [privacy.md §6](privacy.md) explains what they do and do not answer.

## 2 The version, and what a tag means

One number, in one place: `[workspace.package] version` in
[`Cargo.toml`](../../Cargo.toml). Every crate inherits it (`R05` fails any crate that restates it), and
the seven path-dependency requirements must move with it — a path dep requiring `0.1.0` does **not**
resolve against a `0.1.0-alpha.1` crate, which is how a half-done bump becomes someone else's broken
build. The alpha is `0.1.0-alpha.1`.

A **prerelease** version, not `0.1.0`, for two reasons that are both about other people:

* `cargo-dist` marks a release with a prerelease-style tag as a **prerelease** on GitHub, so the alpha
  cannot become "latest". `install.sh` resolves "latest" for anyone who does not name a version, and an
  alpha becoming a stranger's default install is the failure mode `R12` checks mechanically.
* an alpha that collects feedback is **re-cut** — `alpha.2`, `alpha.3` — so the series has to exist
  before it is needed rather than invented during an incident.

The store does not care what the version is. ADR-004's cache keys are built from the derived
*analyzer* versions (`Versions::default`, one integer per analyzer, plus the `e1.15` feature digest), not
from this string, so tagging an alpha invalidates nobody's index.

`fathom --version` prints the version and the resolved feature set, which is the minimum a bug report
needs; `fathom doctor --json` prints it again with the OS, arch, terminal, SQLite capabilities and the
whole index state. Both are one line and one JSON object respectively, because `e0.7` owns that format
(ADR-002 §10.1) and scripts parse it.

## 3 The runbook: cutting the alpha

Everything below is four commands and a checklist. The first two can run on a laptop with no network;
the rest need the remote, which is the thing this repository does not have yet.

1. **Pre-flight.** `python3 tools/check_release.py --tag v0.1.0-alpha.1`. This is `R01`–`R12` plus the
   tag check `R13`: the tag must name the version the package carries (cargo-dist refuses one that does
   not, *at publish time*, which is the worst possible moment), and `CHANGELOG.md` must have a dated
   section for it, because `cargo-dist` builds the release body out of that file — a version with no
   section announces nothing.
2. **Replace the placeholder repository URL**, if the public remote is not
   `github.com/danielstahl/fathom`. `package.repository` in `crates/fathom-cli/Cargo.toml` is the only
   place it is written; `dist generate` rewrites the workflow, and `R09` fails the README until the
   documented install URL matches what the installer will bake. ADR-002 §10.3 is the reason this is a
   rule and not a comment.
3. **Regenerate and commit.** `dist generate` (never hand-edit `release.yml`; CI runs
   `dist generate --check`), `dist plan --output-format=json | python3 tools/check_release.py capture`,
   `python3 tools/check_release.py write`, `cargo fmt`, commit.
4. **Tag and push.** `git tag v0.1.0-alpha.1 && git push --tags`. `dispatch-releases = true` means a
   push does not release anything by itself: the workflow runs on a `workflow_dispatch` carrying the tag,
   so the release is an explicit act.
5. **Verify on the three operating systems** — the criterion, and the part that cannot be delegated to a
   machine. For each of Linux, macOS (both an Intel and an Apple-silicon machine if both are available)
   and Windows: follow the README's one-liner **exactly as written, on a machine that has never seen this
   repository**, then run `fathom --version` (right version, feature list present), `fathom doctor
   --json` (index path, SQLite capabilities), and one real review: `fathom index` on a repository with
   agent commits in it, then `fathom tui`. Record the result — OS, arch, version, date, what broke — in
   the ledger in §5, because "installable on three OSes" is a claim about machines, and the evidence is
   the ledger rather than this paragraph.

### 3.1 What the pipeline guarantees, and what it does not

Guaranteed, and gated: the workflow and its config agree (`dist generate --check` in CI, `R07`
offline); the target set still covers three operating systems (`R02`); both installers still exist
(`R03`); every PR builds every target and uploads it (`R04`, `pr-run-mode = "upload"`); one crate is
releasable and it is the binary (`R06`); the documented install URL is the baked one (`R09`); the
captured plan still describes the config (`R10`); a prerelease cannot publish itself as latest (`R12`).

Not guaranteed here, and honest about it: that the C toolchains on the four runners produce a
*working* binary (that is what step 5 is for — `dist build` on a Linux box proves the release profile
compiles and installs, and proves nothing about `x86_64-pc-windows-msvc`); that a signed-in human is not
required for workflow artefacts (§1.2); that a tag published from a dirty tree is a bad idea that nothing
here prevents.

## 4 Testing an install without publishing anything

The installers take their download URL from the environment, so the whole install path can be exercised
against a local mirror before anything is public. This was run here, and it is the closest thing to proof
the pipeline can get without a remote:

```bash
dist build --artifacts=local            # the archive — only if *this* machine's triple is in the list
dist build --artifacts=global           # the installers, the source tarball, sha256.sum
mkdir /tmp/mirror && cp target/distrib/* /tmp/mirror/
(cd /tmp/mirror && python3 -m http.server 8099 --bind 127.0.0.1 &)
FATHOM_CLI_DOWNLOAD_URL=http://127.0.0.1:8099 \
  FATHOM_CLI_INSTALL_DIR=/tmp/fathom-installed FATHOM_CLI_NO_MODIFY_PATH=1 \
  sh /tmp/mirror/fathom-cli-installer.sh
/tmp/fathom-installed/bin/fathom --version
```

Two honest caveats. First, the first line does nothing on a machine whose triple is not in the target
list: `dist` prints "building artifacts:" and stops, because there is no artefact to build for a host it
does not ship. That is not a bug and it is worth knowing before you try this on an Apple-silicon or
`aarch64` Linux box — the rehearsal then needs a scratch copy of the tree with the host triple added to
`dist-workspace.toml`, which is exactly how the run below was performed. Second, `dist` refuses to
cross-compile (in its own words: "cross-compiling to macOS is a road paved with sadness"), so what this
exercises is the **mechanics** — release profile compiles, archive layout, checksum, installer, `PATH`,
the installed binary runs and reports its feature set — on whichever machine runs it, never the artefact
another OS actually receives. That is what §3 step 5 is for. The checksum is what makes a download
trustworthy in production; verify it with `sha256sum -c` rather than assuming the transport.

<!-- BEGIN GENERATED: installer_env -->
| Variable | Effect |
|---|---|
| `FATHOM_CLI_DISABLE_UPDATE` | skip the self-update path entirely |
| `FATHOM_CLI_DOWNLOAD_URL` | point the installer at a mirror instead of a public release (used in this repository's own end-to-end installer test) |
| `FATHOM_CLI_INSTALLER_GHE_BASE_URL` | GitHub Enterprise base URL, second spelling |
| `FATHOM_CLI_INSTALLER_GITHUB_BASE_URL` | GitHub Enterprise instead of github.com |
| `FATHOM_CLI_GITHUB_TOKEN` | a token for a private release, so an alpha can be gated without an object store |
| `FATHOM_CLI_INSTALL_DIR` | install somewhere other than `~/.local/bin` |
| `FATHOM_CLI_NO_MODIFY_PATH` | leave `PATH` (and the shell rc files) alone |
| `FATHOM_CLI_PRINT_QUIET` | less output |
| `FATHOM_CLI_PRINT_VERBOSE` | more output when the install is going wrong |
| `FATHOM_CLI_UNMANAGED_INSTALL` | the unmanaged-install directory |
<!-- END GENERATED: installer_env -->

## 5 Adoption: counting people without watching them

`m0`'s fourth criterion is "at least three people outside the core team using it weekly", and the obvious
way to measure it is the one way this project has already refused: ADR-001 §8 rules out telemetry, and
`e6.8` (**reco-h7p**) owns the privacy posture that makes "local counters, never transmitted" the design
rather than an apology. So the count is **opt-in and manual**, and it is worth being exact about what that
costs: it under-counts (quiet users are invisible), it is effort to maintain, and it is the only honest
option available. Under-counting is the right trade; a number obtained by watching people is not a
number this product should have.

What an alpha user sends, when they want to: the output of `fathom doctor --json`, which is one JSON
object containing the version, the feature set, the OS and arch, the terminal, the SQLite build and the
index state — and which contains their repository's paths, so **they read it before sending it**, which
is the whole difference between telemetry and a bug report. Nothing is uploaded, polled or phoned home;
there is no endpoint and no identifier.

The ledger is this table. A row is one person in one ISO week, added by whoever received the report;
`G1`'s engagement question (ADR-001 §5.2, target ≥ 0.50 at alpha) is *not* measured here and stays with
`e4.3`, whose corpus it belongs to — this table counts humans, not reviews.

| Week (ISO) | Handle (as they gave it) | OS / arch | `fathom --version` | What broke |
|---|---|---|---|---|
| — | no rows yet: nothing is published, so nothing is in use | — | — | — |

Three distinct handles with a row in the same week is the criterion. Two things follow from the way the
table is built, and both are deliberate: a row whose "what broke" is empty is less useful than one that
names a defect, so the ask is always "what broke", and an empty table is the truth rather than an
embellishment — the gate stays open until rows exist, which is why `m0` records this as **not started**
rather than letting the pipeline's existence stand in for a user.

## 6 The alpha gate, as data

The same four criteria `bd show reco-n1j` carries. The table is generated from `GATES` in the checker, so
this page and the tool quote one another about the verdict — and the verdict is code, which means it
travels in a diff rather than being re-measured into existence:

<!-- BEGIN GENERATED: gates -->
| Gate | What proves it | State |
|---|---|---|
| Epic 0 and Epic 1 closed, no open P1 defects | `bd list --status open -l defect` is empty; `reco-w74` and `reco-quk` are closed | met by code |
| Spike recommendations reflected in the plan (`e0.12`) | `.plan/reconciliation-e0.12.md`, and `tools/gen_segment_tables.py check` in CI | met by code |
| Alpha artefacts installable on three OSes | the pipeline is gated and measured (`tools/check_release.py`, `benchmarks/reports/dist-release-plan.json`); the tag, the runners and the three installs still have to happen — `m0.1`, and runbook §3 | ready, blocked on a public remote |
| Three people outside the core team using it weekly | nothing in-repo, and nothing can be until the alpha is installable — `m0.2`, and the ledger in runbook §5 | not started, blocked on `m0.1` |
<!-- END GENERATED: gates -->

`tools/check_release.py --tag v0.1.0-alpha.1` prints the pre-flight verdict; `bd` is the authority on
whether the milestone is closed, and the close reason on that ticket is the record of what was and was
not evidenced here. The two criteria this page cannot close are tickets now, because a milestone that
stays open holding work nobody owns is how work stops happening:

* **`m0.1` (`reco-lnd`)** — push the tag, and verify the install on three operating systems (§3 step 5).
  Blocked on a public remote: nothing here can publish to an address that does not exist yet.
* **`m0.2` (`reco-3gz`)** — announce the alpha and keep the ledger in §5 filled. Depends on `m0.1`.

## 7 What this document deliberately does not decide

* **Cross-platform packaging** — `aarch64-unknown-linux-gnu`, a static `musl` build, an `msi`, code
  signing on macOS and Windows, a Homebrew tap, per-PR artefacts on a store that needs no auth. `e7.4`
  (**reco-3q4**), which is where "installable" stops being a four-runner matrix and becomes packaging.
* **A published crate index.** `publish = false` everywhere (ADR-001 §10.1's reasoning: a published crate
  is an API promise on a moving domain model); `fathom-cli` opts *into* `dist` precisely because
  `publish = false` would otherwise hide the binary from a tool whose job is publishing it.
* **The changelog's wording.** [`CHANGELOG.md`](../../CHANGELOG.md) is the release body; the rule here is
  only that the version being tagged has a dated section, because an empty body is an announcement that
  says nothing.
* **Signed builds.** No artefact here is signed, and `sha256.sum` is integrity, not authenticity. That
  gap is named in `e7.4` rather than papered over by the word "secure".
