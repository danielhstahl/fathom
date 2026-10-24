# Release and distribution — cutting an alpha, and what an installer promises

Status: the skeleton was built by `e0.7` (**reco-e77**); the artefact set was extended into packaging by
`e7.4` (**reco-3q4**); the alpha was cut by `m0` (**reco-n1j**).
Config: [`dist-workspace.toml`](../../dist-workspace.toml). Generated workflow:
[`.github/workflows/release.yml`](../../.github/workflows/release.yml). Gate:
[`tools/check_release.py`](../../tools/check_release.py), asserted by
[`tests/test_release_contract.py`](../../tests/test_release_contract.py) and
[`tests/test_packaging_contract.py`](../../tests/test_packaging_contract.py). Measured evidence:
[`benchmarks/reports/dist-release-plan.json`](../../benchmarks/reports/dist-release-plan.json).
Tools that make artefacts: [`tools/gen_shell_artifacts.py`](../../tools/gen_shell_artifacts.py)
(completions, man pages), [`tools/package_release.py`](../../tools/package_release.py) (`.deb`, `.rpm`),
[`tools/rehearse_installer.py`](../../tools/rehearse_installer.py) (§4), and
[`packaging/README.md`](../../packaging/README.md) for what lives in that directory.

ADR-002 §10.1 handed the artefact set to `e0.7`, and `e0.7` built it: `cargo-dist` configured, a
generated workflow, one binary. What it could not build, and what no commit can build, is the second half
of `m0`'s third acceptance criterion — "**alpha artefacts installable on three OSes**". A commit can
decide the pipeline; only a public remote, three runners and three humans can decide the sentence. So
this document is the part that turns "the config exists" into "cutting an alpha is a repeatable, checked
act that someone who is not the author can perform in ten minutes".

`e7.4` is the half of that sentence a commit *can* decide: an installer per OS, a system package for the
Linuxes that have one, a static build for the ones that don't, completions and a man page that cannot
lie about the flags, and a written account of which steps need a secret a fork will never have (§8).

The failure this document exists to prevent is the one `e0.7` left lying around: ADR-002 §10.3 records
that `package.repository` is a **placeholder** (`github.com/danielstahl/fathom`), that `cargo-dist` bakes
the installers' download URL out of it, and that a stale value is *a broken install, not a cosmetic bug*.
That is the shape of every problem here: silent, plausible, and discovered by a stranger following a
one-liner in a README.

## 1 What an alpha release is made of

One binary (`fathom`, built from `fathom-cli` with the default feature set, which **is** the product per
ADR-002 §6.1), six targets, four installers (shell, PowerShell, msi, Homebrew), one source tarball, four
Linux system packages, one checksum file, and per-artefact `.sha256` for the things `dist` does not
checksum itself. The tables below are generated from
[`benchmarks/reports/dist-release-plan.json`](../../benchmarks/reports/dist-release-plan.json),
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
| `aarch64-unknown-linux-gnu` | `ubuntu-24.04-arm` | `fathom-cli-aarch64-unknown-linux-gnu.tar.xz` | `--artifacts=local --target=aarch64-unknown-linux-gnu` |
| `x86_64-apple-darwin` | `macos-15-intel` | `fathom-cli-x86_64-apple-darwin.tar.xz` | `--artifacts=local --target=x86_64-apple-darwin` |
| `x86_64-pc-windows-msvc` | `windows-2022` | `fathom-cli-x86_64-pc-windows-msvc.zip` | `--artifacts=local --target=x86_64-pc-windows-msvc` |
| `x86_64-unknown-linux-gnu` | `ubuntu-22.04` | `fathom-cli-x86_64-unknown-linux-gnu.tar.xz` | `--artifacts=local --target=x86_64-unknown-linux-gnu` |
| `x86_64-unknown-linux-musl` | `ubuntu-22.04` | `fathom-cli-x86_64-unknown-linux-musl.tar.xz` | `--artifacts=local --target=x86_64-unknown-linux-musl` |
<!-- END GENERATED: targets -->

Six triples, and each one is built by a machine that *is* that triple — natively on `ubuntu-24.04-arm`
for arm64 Linux, which is why `dist.github-custom-runners` states its host: without it `dist` would
assume the runner was the x86_64 image it knows and reach for `cargo-zigbuild` to cross-compile a target
it could compile directly. The reason to prefer native builds here is that the six tree-sitter grammars
and the bundled SQLite are C, and a native C toolchain is not a cross wrapper's best day.

`x86_64-unknown-linux-musl` is the **static** build, and it is the answer to "the container image
somebody pinned in 2019": no glibc version assumption, because there is no glibc. It needs `musl-tools`
on the runner (declared under `[dist.dependencies.apt]`, scoped to that one target) for the same C.

`aarch64-unknown-linux-musl` and `aarch64-pc-windows-msvc` are the two still missing, both for the same
reason: an arm64 Windows runner does not exist, and a second cross-target whose only evidence is a green
tick is a claim this document would rather not make. Adding either is a `targets` line plus, if needed, a
runner. `deny.toml`'s `[graph] targets` lists every triple ADR-002 cares about, so licence and advisory
resolution is checked across the whole platform set whether or not a binary is built for it.

### 1.2 The artefact set

<!-- BEGIN GENERATED: artefacts -->
| Artefact | Made | What it is |
|---|---|---|
| `source.tar.gz` | once per release | the source tree a distro or an auditor builds from, with its own `.sha256` |
| `fathom_0.1.0~alpha.1_amd64.deb` | once per release |  |
| `fathom_0.1.0~alpha.1_arm64.deb` | once per release |  |
| `fathom-0.1.0~alpha.1-1.x86_64.rpm` | once per release |  |
| `fathom-0.1.0~alpha.1-1.aarch64.rpm` | once per release |  |
| `fathom-cli-installer.sh` | once per release | the `curl … \| sh` script for Linux and macOS; picks the target from the machine |
| `fathom-cli-installer.ps1` | once per release | the `irm … \| iex` script for Windows, so a Windows user is not told to use bash |
| `fathom.rb` | once per release |  |
| `sha256.sum` | once per release | one digest per artefact, so a download can be checked without trusting the transport |
| `fathom-cli-x86_64-pc-windows-msvc.msi` | once per release |  |
| `fathom-cli-aarch64-apple-darwin.tar.xz` | per target | `fathom` plus `CHANGELOG.md`, `README.md`, `completions`, `man`, with `fathom-cli-aarch64-apple-darwin.tar.xz.sha256` beside it |
| `fathom-cli-aarch64-unknown-linux-gnu.tar.xz` | per target | `fathom` plus `CHANGELOG.md`, `README.md`, `completions`, `man`, with `fathom-cli-aarch64-unknown-linux-gnu.tar.xz.sha256` beside it |
| `fathom-cli-x86_64-apple-darwin.tar.xz` | per target | `fathom` plus `CHANGELOG.md`, `README.md`, `completions`, `man`, with `fathom-cli-x86_64-apple-darwin.tar.xz.sha256` beside it |
| `fathom-cli-x86_64-pc-windows-msvc.zip` | per target | `fathom` plus `CHANGELOG.md`, `README.md`, `completions`, `man`, with `fathom-cli-x86_64-pc-windows-msvc.zip.sha256` beside it |
| `fathom-cli-x86_64-unknown-linux-gnu.tar.xz` | per target | `fathom` plus `CHANGELOG.md`, `README.md`, `completions`, `man`, with `fathom-cli-x86_64-unknown-linux-gnu.tar.xz.sha256` beside it |
| `fathom-cli-x86_64-unknown-linux-musl.tar.xz` | per target | `fathom` plus `CHANGELOG.md`, `README.md`, `completions`, `man`, with `fathom-cli-x86_64-unknown-linux-musl.tar.xz.sha256` beside it |
<!-- END GENERATED: artefacts -->

`e0.7`'s acceptance criterion said "downloadable without auth", and the honest reading of that is worth
writing down because it has been misunderstood twice: GitHub **workflow artefacts** (what a PR build
uploads) require a signed-in human, and no `cargo-dist` setting changes that. The auth-free artefact is
the GitHub **release** a tag produces — public assets, plus the installers and the formula. A per-PR link
that needs no login needs an object store this project does not have, and that half of the sentence is
still true: `pr-run-mode = "upload"` gives a reviewer a build of every target on every PR, behind a login.

The four `.deb` and `.rpm` rows are `[[dist.extra-artifacts]]`: `cargo-dist` has no deb or rpm target, so
`packaging/linux-packages.sh` builds them (§1.3). They carry their own `.sha256` beside them, in the same
`<digest>  <name>` form `dist` uses, because `dist` checksums the artefacts it knows about and these are
the ones it does not — a package with nothing to check it against is the one row on the release page where
you would have to take the transport's word for it.

Two more files ride on every release, generated from the lockfile and attached by
[`.github/workflows/sbom.yml`](../../.github/workflows/sbom.yml) (`e6.8`, **reco-h7p**):
`fathom-<version>.cdx.json`, a CycloneDX 1.5 SBOM of what a default build links, and
`fathom-<version>-licenses.md`, the same inventory read as licence obligations against `deny.toml`. Both
are committed under `sbom/` and re-generated on every PR, so the asset on the release page is the same
bytes a reviewer diffed in the pull request; `python3 tools/gen_sbom.py check` is what keeps them from
going stale, and [privacy.md §6](privacy.md) explains what they do and do not answer. (`cargo-dist` can
generate a CycloneDX SBOM itself with `cargo-cyclonedx = true`; it is deliberately off, because two
SBOMs per release is two answers to one question and the reviewed one is the one already wired.)

### 1.3 What is inside an archive, and what a package is

Every archive carries four things beside the binary: `README.md` and `CHANGELOG.md` (`dist`'s
`auto-includes`), and two directories — `completions/{bash,zsh,fish}/…` and `man/man1/fathom*.1` — named
by `include` in `dist-workspace.toml`. Both are generated from the clap definitions, by the binary itself:

```bash
cargo build -p fathom-cli && python3 tools/gen_shell_artifacts.py write   # regenerate packaging/
python3 tools/gen_shell_artifacts.py check                                # what CI runs
fathom completions zsh                                                    # the same command, yours
```

`fathom completions` renders the same `clap::Command` `main` parses with, so the committed file *is* the
parser, printed: a flag cannot appear in `--help` and be missing from the completions or the man page,
which is the criterion `e7.4` was given and the reason there is no hand-written completion script in this
repository to rot. The installers install binaries only, so the two directories arrive beside the binary
as files rather than littering the install directory — `man fathom-check` works after a `.deb` install,
and after an untar with `MANPATH` pointed at the archive.

The system packages are **repacked from those archives**, not recompiled. `dist` runs
`packaging/linux-packages.sh` in its global phase, by which point every per-target archive has been
downloaded back into `target/distrib/`; `tools/package_release.py` then relocates the archive's own bytes
into each format's FHS layout:

| In the archive | In the `.deb` / `.rpm` |
|---|---|
| `fathom` | `/usr/bin/fathom` |
| `completions/bash/fathom` | `/usr/share/bash-completion/completions/fathom` |
| `completions/zsh/_fathom` | `/usr/share/zsh/vendor-completions/_fathom` (deb), `…/site-functions/_fathom` (rpm) |
| `completions/fish/fathom.fish` | `/usr/share/fish/vendor_completions.d/fathom.fish` |
| `man/man1/*.1` | `/usr/share/man/man1/*.1.gz` |
| `README.md`, `CHANGELOG.md` | `/usr/share/doc/fathom/`, beside a `copyright` naming the licence |

Three consequences worth knowing before you change any of it: a `.deb` therefore contains *the same bytes
the tarball contains* (not a second compile that happened to run on the release machine); the packager
refuses to run if the archive has no completions in it, which is the failure mode of a half-applied
`include`; and the `.rpm` is written by `rpmbuild` — the reference implementation, installed by the
script because the runner image lacks it — so it is guaranteed to be an RPM but not byte-reproducible,
while the `.deb` is written here with zero timestamps and is.

## 2 The version, and what a tag means

One number, in one place: `[workspace.package] version` in
[`Cargo.toml`](../../Cargo.toml). Every crate inherits it (`R05` fails any crate that restates it), and
the seven path-dependency requirements must move with it — a path dep requiring `0.1.0` does **not**
resolve against a `0.1.0-alpha.1` crate, which is how a half-done bump becomes someone else's broken
build. The alpha is `0.1.0-alpha.1`.

Since `e7.4` the version reaches further than the binary, so a bump is four regenerations rather than
seven manifest lines: every man page header says `fathom <version>`, every `.deb`/`.rpm` file name carries
`0.1.0~alpha.1` (a `-` is illegal in an upstream version, and `~` is how both package managers spell
"sorts before"), and `dist-workspace.toml`'s `extra-artifacts` list names those files. `R15` and
`python3 tools/gen_shell_artifacts.py check` are what make forgetting one of them a red build rather than
a misnamed artefact; §3 step 3 is the order to do them in.

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

Everything below is a handful of commands and a checklist. The first three can run on a laptop with no
network; the rest need the remote, which is the thing this repository does not have yet.

1. **Pre-flight.** `python3 tools/check_release.py --tag v0.1.0-alpha.1`. This is `R01`–`R17` plus the
   tag check `R13`: the tag must name the version the package carries (cargo-dist refuses one that does
   not, *at publish time*, which is the worst possible moment), and `CHANGELOG.md` must have a dated
   section for it, because `cargo-dist` builds the release body out of that file — a version with no
   section announces nothing.
2. **Replace the placeholder repository URL**, if the public remote is not
   `github.com/danielstahl/fathom`. `package.repository` in `crates/fathom-cli/Cargo.toml` is the only
   place it is written; `dist generate` rewrites the workflow, and `R09` fails the README until the
   documented install URL matches what the installer will bake. ADR-002 §10.3 is the reason this is a
   rule and not a comment.
3. **Regenerate and commit.** Four generators, in this order, then `cargo fmt`:

   ```bash
   dist generate                                # release.yml from dist-workspace.toml (never hand-edited)
   dist plan --output-format=json | python3 tools/check_release.py capture
   cargo build -p fathom-cli                    # the default-feature build the artefacts come from
   python3 tools/gen_shell_artifacts.py write   # packaging/completions and packaging/man
   python3 tools/gen_docs.py write --page cli   # the CLI reference, from the same binary
   python3 tools/check_release.py write         # this page, from the captured plan
   ```

   Step 3 is where a version bump shows up in the man pages and step 1 is where it shows up in the
   `.deb` names, so the order is not arbitrary — and `dist generate` is only safe because `dist` owns
   `release.yml` and CI runs `dist generate --check`.
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
   the ledger rather than this paragraph. `python3 tools/rehearse_installer.py` (§4) does the half of
   this that is mechanical — right artefact fetched, checksum matched, version matched — on whatever
   machine runs it, and prints the row in the shape the ledger wants.
6. **At a *minor* release, move the demo with it.** The demo repository is versioned on the minor line, not
   the patch line, because a patch that changes nothing the tour shows must not re-record a demo — a
   timestamp on a recording that did not change is worse than no timestamp:

   ```bash
   python3 tools/publish_demo_repo.py build --out /tmp/signal   # assembles and tags demo-v<major>.<minor>
   python3 tools/record_demo.py record                          # re-make the film at the new tag
   python3 tools/demo_acceptance.py check                       # the clone-plus-one-command half
   git add docs/demo/five-minute.cast docs/demo/five-minute.cast.json publish/demo-repo/PATHOLOGIES.md
   ```

   `tools/record_demo.py check` is what makes the step real: the cast's header carries the tag it was
   recorded at, and once the crate's minor line moves the committed film names the old one, so a release
   that skipped the re-recording fails CI instead of quietly shipping a stale film. [The demo page](../demo/five-minute.md) is the tour itself; note that `demo-v*` is a tag on the *demo
   repository*, which is assembled from this one rather than pushed inside it.

### 3.1 What the pipeline guarantees, and what it does not

Guaranteed, and gated: the workflow and its config agree (`dist generate --check` in CI, `R07`
offline); the target set still covers three operating systems (`R02`) and the two extra triples
(`R14`); every installer still exists, both script installers (`R03`) and the msi and the formula
(`R14`); the msi can still be built, because its `upgrade-guid` survived (`R14`); the system packages
are still named for the version being released (`R15`); the completions and man pages exist, are not
empty, and are still `include`d (`R16`) — and are byte-identical to what the binary renders, checked by
`tools/gen_shell_artifacts.py` in its own CI job; every PR builds every target and uploads it (`R04`,
`pr-run-mode = "upload"`); one crate is releasable and it is the binary (`R06`); the documented install
URL is the baked one (`R09`); the captured plan still describes the config (`R10`); a prerelease cannot
publish itself as latest (`R12`); and the document still says which steps need a secret (§8, `R17`).

Not guaranteed here, and honest about it: that the C toolchains on the six runners produce a
*working* binary (that is what step 5 is for — `dist build` on a Linux box proves the release profile
compiles and installs, and proves nothing about `x86_64-pc-windows-msvc`); that the msi, the formula and
the `.rpm` are *right* rather than merely built (only WiX, Homebrew and `rpm -qp` can say, and CI runs
the last of those three); that a signed-in human is not required for workflow artefacts (§1.2); that an
unsigned macOS binary will not Open without a right-click (§8); and that a tag published from a dirty tree
is a bad idea that nothing here prevents.

## 4 Testing an install without publishing anything

The rehearsal is the whole criterion minus the network, on a Linux box, in front of a mirror of the
release page that is a local directory:

```bash
python3 tools/rehearse_installer.py                    # builds the local artefacts if need be
python3 tools/rehearse_installer.py --skip-build       # re-use what is already in target/distrib
python3 tools/rehearse_installer.py --self-test        # negative controls: the checks can fail
python3 tools/rehearse_installer.py --json             # the ledger row, mechanically
```

It runs `dist build --artifacts=global` (plus `cargo build --release` for this machine's triple), copies
the installers and the local archives into a temporary mirror, runs
`curl --fail --silent --show-error --proto '=https' --url <mirror>/install.sh --output - | sh` against it
— the README's one-liner, with only the host changed — into a prefix that cannot touch `$HOME`, and then
insists on five things: the installer fetched **the archive for this machine's triple**, the digest it
verified is the digest the release computed, the installed binary is on `PATH`, `--version` prints the
version the manifest says, and `--help` lists the verbs the docs say exist. `--self-test` corrupts the
local mirror four ways (missing artefact, wrong checksum, wrong version in the binary, half-written
archive) and requires the rehearsal to catch all four — a rehearsal that cannot fail is not a test. It
prints the result in §5's row shape, so the mechanical half of a ledger row is not hand-typed.

`dist`'s own name-override variables (`FATHOM_CLI_*`) are what make this possible, and they are the
reason to be suspicious of them: a release that renamed a binary would silently rehearse something else.
The table below is generated from the captured plan and is what the rehearsal depends on.

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

The third variable is not decoration. `install_local` appends `case ":$PATH:" in … esac` to
`.bash_profile` and `.profile` *even when the directory is already on `PATH`*, so the naive form of this
test leaves two stray lines in your dotfiles every time you run it. Any install test that does not set it
is a test that shreds the machine it runs on.

The reason this rehearsal exists as a script and not as prose is the third bullet above: an installer is
`env(1)` interpolation, so the variables are the whole mechanism, and the mechanism is worth looking at
once — it is `ARG0`, `awk` and `tail` finding a marker line (`#{0} …`), with no external parser.

The `.deb` rehearses the same way, one step earlier: `python3 tools/package_release.py --self-test`
fails loudly on five corruptions of its own inputs, and `dpkg-deb --install --force-all` in a container
followed by `fathom --version` is the equivalent of the `curl` line for the packager. CI runs the
self-tests and `dpkg-deb --info --contents`; it does not run `apt install`, and does not pretend to.

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

- **Which humans ran the installer on which machine.** That is §5, and `m0` cuts the alpha.
- **The actual alpha tag.** Step 4 above is a person with push access, not a CI job.
- **Cross-platform packaging proper.** `e7.4` (**reco-3q4**) added the msi, the `.deb`/`.rpm`, the musl
  target, the Homebrew tap and the shell artefacts (§1.1–§1.3); `aarch64-pc-windows-msvc` remains out
  (§1.1), and a public, login-free *per-PR* download is still true of the release and false of the
  workflow artefact.
- **Where releases are announced**, and whether the changelog is edited into the release body by hand or
  left as the generated draft.
- **Signing and notarisation**, and which maintainer-only secret stands in front of each of them. `e7.4`
  added the msi and the tap, which makes the question live rather than hypothetical, so §8 is the answer:
  named, who holds it, what a fork sees, and what is wired today.

## 8 The secrets, and what a fork gets instead

This is the ticket's fourth criterion — *which release steps need maintainer-only secrets* — stated as a
list rather than as a hope. It is generated from `SECRETS` in `tools/check_release.py`, and `R17` fails if
a row disappears, because "the signing story is a paragraph" is how a project ends up with a release
process only one person can run.

<!-- BEGIN GENERATED: secrets -->
| Secret | What it unlocks | Who holds it | What a fork sees | State today |
|---|---|---|---|---|
| `GITHUB_TOKEN` | create the release and upload every artefact to it | Actions provides it | works: it is the fork's own token on its own releases page | wired (`permissions: contents: write` in `release.yml`) |
| `HOMEBREW_TAP_TOKEN` | push the generated formula to the tap repository | maintainer-only: a `repo`-scoped token for the tap | no publish. The `.rb` is still an artefact on the release; `brew install` from a tap simply is not this release's install path | not wired: `publish-jobs = ["homebrew"]` is commented out until the tap repository exists, because a publish job whose secret is missing fails the release rather than skipping |
| `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY` | codesign the macOS binaries with a Developer ID | maintainer-only: an Apple Developer ID certificate, exported and base64'd | nothing to lose: the artefacts are unsigned today, so a fork is no worse off than upstream | not wired, and not one line: `cargo-dist` 0.33 has no macOS signing hook, so this needs a build wrapper (`github-build-setup`, or `package.build-command`) that runs `codesign` before `dist` archives the binary |
| `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | notarise the macOS artefacts, so Gatekeeper does not call the installer an unidentified developer | maintainer-only: an Apple Developer account, an app-specific password, the team id | nothing, as above. The user workaround for an unsigned binary is right-click -> Open, which is what every install of this release asks for today | not wired: `xcrun notarytool` runs after the artefacts exist and then `staple`s, so it belongs in the same wrapper as codesigning rather than in a `dist` setting |
| `SSLDOTCOM_USERNAME`, `SSLDOTCOM_PASSWORD`, `SSLDOTCOM_CREDENTIAL_ID`, `SSLDOTCOM_TOTP_SECRET` | sign the Windows `.exe` and `.msi` | maintainer-only: an SSL.com code-signing certificate (or an Azure Artifact Signing certificate profile, which is `azure-windows-sign` instead) | nothing: unsigned today | not wired. `ssldotcom-windows-sign` / `azure-windows-sign` are `dist` settings, so this half is a config line once a certificate exists; until then SmartScreen may warn about an unsigned msi, which is the honest current behaviour and not a bug to hide |
| *none* | build every artefact and publish a release of your own | nobody: no build step needs a secret | the row that matters for a fork: the tarballs, msi, formula, `.deb` and `.rpm` all build with no credentials, and `publish = false` means there is no crates.io token to be missing | by design (ADR-002 §10.1: no published crates in v1) |
<!-- END GENERATED: secrets -->

Three sentences of prose for what the table cannot say.

**Nothing in the build needs a secret.** Every artefact — six archives, the msi, the formula, the `.deb`s
and the `.rpm`s, both scripts — is produced by `dist build` on a runner with no credentials beyond the
automatic `GITHUB_TOKEN`. That is the property that makes the release path testable at all (§4), and it is
why signing is deliberately last: an unsigned artefact set is an honest artefact set, and a fork is not
second-class.

**Signing is not a `cargo-dist` setting for macOS, and pretending otherwise is how projects end up with a
half-wired signing step.** `dist` ships Windows signing as a setting (`ssldotcom-windows-sign`,
`azure-windows-sign`) and has no macOS equivalent in 0.33: codesigning has to run *between* the build and
the archiving, and notarisation has to run *after* the artefact exists and then `staple` the ticket back
onto it. The place for that is `github-build-setup` (job steps injected around the build) or
`package.build-command`, and the reason it is not in this tree is that a wrapper around an unsigned
release cannot be tested without the certificate — an untestable step in the release path is worse than an
absent one, and this document says that in the same voice it says everything else.

**The Homebrew publish job is commented out rather than enabled.** `tap` is set, `formula = "fathom"`, the
`.rb` is built and lands in `target/distrib/` — and `publish-jobs = ["homebrew"]` sits in the config as a
comment, because a publish job with a missing `HOMEBREW_TAP_TOKEN` does not skip, it fails, and a failed
release job after the tag exists is the worst possible moment to discover the tap repository does not.
Two lines change when the tap exists: set the secret, uncomment the job. Until then the formula is an
artefact a person can `brew install ./fathom.rb` with, and §1.2 says so instead of implying it.

**What a fork should conclude.** Clone, change the repository URL (§3 step 2), and every artefact in §1.2
builds and installs, unsigned, from your own releases page. Nothing in this project's release path is
secret; the only thing a fork cannot do is be trusted by strangers, which is a certificate, not a config.

## 9 The cadence, and the changelog process that goes with it

Status: opened by `e7.7` (**reco-rny**). Everything in this section is either a command or a rule a
command enforces; the sentence *"we release when we feel like it"* is the thing being replaced, because a
cadence only the author can follow is not a cadence, it is a mood.

**The cadence.** Four clocks, and one person a week who is not necessarily the author:

| Clock | What is cut | What has to be true |
|---|---|---|
| **weekly, Tuesday** | `0.1.0-alpha.N+1` from `master` | the release job is green; `[Unreleased]` accounts for every commit since the last tag (`tools/gen_changelog.py audit` refuses anything else); the two graphs and the demo checks are green |
| **on a merged defect a user hit** | a patch, any day | the defect is a row in [the intake register](feedback.md) with a disposition, and the release notes carry its three sentences (§5 of that page) |
| **at an epic's close** | a minor | the epic's exit criteria are a runnable gate (`tools/epic6_acceptance.py` is the pattern), and the demo is re-recorded — §3 step 6 |
| **at 1.0** | the minor that stops the `alpha` suffix | the calibration bars in [false-positive calibration](false-positive-calibration.md) §5 and the two external pipelines this project counts |

Two rules that keep a weekly cut cheap: **nothing waits for a release**, because the cut is Tuesday whether
or not anything interesting landed, and a release that has to be *worth it* is a release that gets postponed;
and **a cut is never edited after it is published** — a bad alpha is followed by another alpha, because
silently rewriting a release body is how a project loses the ability to say what it shipped when.

**The changelog is generated, and never regenerated.** `CHANGELOG.md` stays hand-written prose with arguments
in it — that is what makes it worth reading, and no commit subject contains the reasoning. What is generated is
the *checking* of it, by [`tools/gen_changelog.py`](../../tools/gen_changelog.py):

```bash
python3 tools/gen_changelog.py draft            # what the commits since the last release would say
python3 tools/gen_changelog.py audit            # CI: does [Unreleased] account for every one of them?
python3 tools/gen_changelog.py lint             # which commits are conventional-shaped, which took the ladder
python3 tools/gen_changelog.py cut --version 0.1.0-alpha.2   # rename the heading, date it, open a new one
python3 tools/gen_changelog.py self-test        # two consecutive cuts, in a throwaway repository
```

**Conventional commits, and the house style they have to share a file with.** The header grammar is the usual
one — `feat(scope)!:` / `fix:` / `perf:` / `refactor:` / `docs:` / `deprecated:` / `removed:` / `security:`,
with `build`/`ci`/`test`/`chore`/`style` kept out of the release notes, `BREAKING CHANGE:` or `!` promoting an
entry into **Breaking changes**. Two additions, both here because of how this repository actually commits:

* `Changelog: <one sentence>` and `Changelog-Section: Fixed` trailers, for a commit whose subject is an essay
  (most of them) and should contribute one accurate bullet rather than a truncated one;
* a **fallback ladder** for the commits that use no header at all: classify from the trailers, else from the
  bead refs, else from keywords into Fixed or Changed — and `lint` says out loud, per commit, which rung it
  took. A guess the tool admits is a guess a reviewer can overrule; a guess it hides is the same kind of defect
  the risk vector refuses to have.

**The gate is coverage, not style.** `audit` reads the commits since the previous release — from the tag if the
clone has one, which this clone does not (it has no remote to push a tag to), else from the commit that added
the previous `## [version]` heading — and fails on any commit the release notes do not account for, by bead
ref, plan key, short sha, or the `Changelog:` text. `C03` is that rule. Known omissions may be exempted in
`.plan/changelog-coverage.json` with a reason and an owner, and `C07` makes that list **shrink-only**: an
exemption that has become covered fails until it is deleted. Cutting a release (`cut`) runs the audit first and
refuses, so the failure this section exists to prevent — a shipped ticket nobody announced — is now a red build.

**Two consecutive releases, and the honest state of that criterion.** The beta's fourth criterion is that this
process is *followed*, twice. What is true today: the process is documented here, the tool enforces it, the
audit runs in CI on every commit, and `self-test` performs two consecutive cuts — dated headings, generated
bodies, a refusal when a commit is missing, a refusal when the version and `Cargo.toml` disagree — in a
throwaway repository, through the same code path `cut` uses on this tree. On this tree **one release has been
cut** (`0.1.0-alpha.1`, §6), because a second needs the version bump, a `dist plan` re-capture by a machine
that has `dist` installed, and a tag pushed to a public remote that does not exist yet. `python3
tools/check_beta_loop.py --criteria` prints that state as data rather than as a claim, and it will say
`2` the day someone runs §3 for the second time.

## 10 What a release publishes about itself

The third beta criterion is that **Fathom runs against its own repository in CI and the artefact is published
per release**, and the two halves had different owners before `e7.7`.

* **In CI, since `e6.6`**: the `check-gate` job in `.github/workflows/ci.yml` builds the headless binary,
  indexes this repository with a shallow clone, runs `fathom check` twice (the gate, then the SARIF), times it
  against the 60-second budget and uploads the SARIF. [The dogfood workflow](../../.github/workflows/dogfood.yml)
  runs the same gate over three repositories this project does not own.
* **Per release, since `e7.7`**: [`.github/workflows/self-review.yml`](../../.github/workflows/self-review.yml)
  runs on `release: published` and on `workflow_dispatch`, and attaches three files to the release it is
  published from: the review artefact for the release range (`fathom export`, markdown, with its sign-off
  block and its self-check verdict), `fathom doctor --json` for the runner's own build of the released
  version, and a small `self-review.json` of the numbers a stranger would otherwise have to re-derive —
  episodes indexed, findings by check and severity, the architecture score, the ledger's coverage, and every
  non-zero exit code with its name. If no release exists yet (a manual dispatch against a branch), the same
  three files land as workflow artefacts instead, and the job says which happened rather than failing quietly.

Why it is worth a job at all: the artefact is the product's own claim about itself at the exact bytes a user
installed, and a release that changed what the tool says about its own repository has to explain the difference
in the release's own notes. The record of the first manual pass, with its commands and its eleven filed
defects, is [`.plan/evidence/reco-rny/fathom-on-itself.md`](../../.plan/evidence/reco-rny/fathom-on-itself.md);
the weekly act that keeps it current is §3 and §6 of [the feedback channel](feedback.md).
