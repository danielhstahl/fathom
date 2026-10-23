# Privacy: no network by default, and nothing leaves unless you send it

- **Status**: Accepted, implemented, and mechanically checked (the gates are in §5, not in this sentence).
- **Ticket / plan key**: `e6.8` (**reco-h7p**).
- **Decided by**: [ADR-001 §7.5 and §8.5](../adr/0001-product-thesis.md) (the promise),
  [ADR-002 §6](../adr/0002-workspace.md) (the feature model that makes it checkable),
  [the thesis glossary](thesis.md) (*network consent*).
- **Executable half**: `crates/fathom-core/src/privacy.rs` (the consent table),
  `crates/fathom-cli/src/features.rs` + [`diagnose.rs`](../../crates/fathom-cli/src/diagnose.rs)
  (`fathom doctor --privacy`), `tools/check_privacy.py` + `tools/privacy_netprobe.c` (the proof),
  `tools/gen_sbom.py` (what ships), `.github/workflows/privacy.yml` (the gate).

This tool reads your whole repository history. That is not a detail to disclose in a footnote, so the
posture is stated as two claims and each has a check attached to it:

1. **No network by default.** A default build makes no outbound call, and no listener. Not "does not,
   usually": the code to reach a socket is not in the binary.
2. **Local-only metrics.** Everything Fathom records lives in `.fathom/` inside your repository, where
   you can read it, export it and delete it. There is no endpoint, no install identifier, no background
   writer, and no configuration key that would send anything anywhere.

Both are worth less as a promise than as something you can verify in ten seconds, so §4 and §5 are the
verification and this page is only the map to it.

---

## 1. Gate one: the code is not in the binary

Every feature that can open a socket is named `net-*`, is **off** in the default feature set, and is
declared in one table —
[`fathom_core::privacy::NET_FEATURES`](../../crates/fathom-core/src/privacy.rs) — which the binary, the
documentation generator and the tests all read.

| Feature | What it can reach | Consent key | Status |
|---|---|---|---|
| `net-advisories` | OSV advisory lookups for dependency risk | `privacy.advisories` | shipped, off by default |
| `net-llm` | an outbound model call for narrative polish | `privacy.llm` | reserved and **unwired** — spike `s5` measured polish and refused it ([details](narrative-polish.md)) |
| `net-http-transport` | an MCP transport that *listens* on a socket instead of speaking stdio | `privacy.mcp_http_transport` | the stdio server ships without it; the gate is live and asking for `http` refuses, because no listener is compiled in (ADR-002 §10.3) |

Two things fall out of making it a compile-time decision:

* `cargo tree -p fathom-cli --no-default-features --features minimal -e normal` contains no HTTP client,
  no TLS stack and no git transport. CI asserts that list of names, and
  `python3 tools/check_privacy.py check` re-computes the same closure from the manifests so the claim
  holds on a machine that has never run `cargo`.
* **stdio MCP is not a network feature.** The MCP server's default transport is stdio, which opens no
  socket; the capability gate lives in the request handler ([ADR-005 §4.3](../adr/0005-harness-contract.md)),
  where choosing a different transport cannot walk around it. A listener would be a privacy decision, so
  `net-http-transport` wears the same consent gate as an outbound call.

`tokio` is the one async dependency a default build may carry (the MCP server needs an executor), and
only with a feature list that cannot open a socket — `ALLOWED_TOKIO_FEATURES` in
[`tools/workspace_model.py`](../../tools/workspace_model.py). Feature unification is exactly the
accident this prevents: one opt-in turning `tokio/net` on for the whole workspace would otherwise be
invisible.

## 2. Gate two: consent, which a repository cannot give you

A `net-*` feature that *is* compiled in still does nothing until the matching key is true:

```toml
# ~/.config/fathom/config.toml — your file, not the repository's
[privacy]
advisories = true          # only now does the advisory lookup run
```

Every `privacy.*` key is **user-scoped**, and that is the load-bearing word. A cloned `fathom.toml`
setting `privacy.advisories = true` is read, discarded, and named on the terminal:

```
ignored: this repository's fathom.toml sets privacy.advisories — that is read and discarded;
         set it in your own config if you meant it
```

The reason is concrete: a repository-level consent key would make `git clone` a privacy event, and a
one-line `git clone && fathom index` would then be a chain from somebody else's commit to an outbound
request from your machine. `fathom index` on a clone must be able to do nothing but read local objects.

When a compiled-in feature is asked to run without consent, it is an **exit code, not a silent no**:
exit `8` (`NETWORK_CONSENT_REQUIRED`), with the key in the message — a refusal that does not name the
switch that would answer it is a mystery, not an error.

The canonical table is generated into [reference/features.md](../reference/features.md); this page keeps
the prose, and `tools/check_docs_site.py` fails if a feature appears in one and not the other.

## 3. What Fathom writes about you, and where

`fathom doctor --privacy` prints it for the repository you are standing in, and
`fathom doctor --json` prints the same as data (`privacy_posture`). As of this writing:

| | |
|---|---|
| Outbound features compiled in | **none** in a default build |
| Consent | user-scoped; `~/.config/fathom/config.toml` (XDG) or `~/.fathom/config.toml` (legacy) |
| Usage metrics | **none leave this machine** |
| Local records | everything under `.fathom/`: the index and analysis cache (`paths.store`, `paths.objects`), the review ledger (`paths.reviews`), the export directory (`paths.export`), the harness event stream (`paths.events`), the advisory cache (`deps.advisories.cache`) and the ratchet baseline |

Nothing in that list is read by anything that can send it. Nothing is uploaded, polled or phoned home;
there is no endpoint compiled in to phone home *to*, which is a stronger statement than "we do not call
it", and it is what §5 checks.

Read and export them yourself, with the same binary:

```bash
fathom stats                          # scale, index state, cache hit rate
fathom doctor --json                  # the whole report, paths made relative or `~`
fathom export --out review.md         # the artefact you meant to keep
rm -rf .fathom                        # the undo, which is complete
```

The one place a number about Fathom's use leaves a machine is the **adoption ledger** in
[release.md §5](release.md): an alpha user runs `fathom doctor --json`, *reads it* (it contains their
paths), and sends it to a human who types a row into a table. That is opt-in in the only sense the word
means, and it under-counts on purpose. A number obtained by watching people is not a number this product
should have ([ADR-001 §8](../adr/0001-product-thesis.md)).

## 4. What `fathom doctor --privacy` says

```console
$ fathom doctor --privacy          # the privacy section; the rest of the report is elided
privacy: what this build can reach, and what it will not do unasked
FEATURE              OWNER            COMPILED  CONSENT  STATE    CONSENT KEY                  SET IN   OWNER TICKET
net-advisories       fathom-analyze   no        no       off      privacy.advisories           default  e2.7
net-llm              fathom-analyze   no        no       off      privacy.llm                  default  e4.7
net-http-transport   fathom-mcp       no        no       off      privacy.mcp_http_transport   default  e6.4
outbound features compiled in: none — no HTTP client and no listener in this binary (ADR-002 R6)
consent is user-scoped: set `privacy.<key> = true` in user=~/.config/fathom/config.toml (absent), legacy=~/.fathom/config.toml (absent) — a repository's own fathom.toml cannot grant it, because a clone must not consent for you
local records: .fathom (1 file(s), 421888 B on disk) — read them with `fathom stats` or `fathom doctor --json`, write them out with `fathom export`, delete them by removing the directory
usage metrics: nothing leaves this machine — no endpoint, no identifier, no background writer: everything Fathom writes stays under `.fathom/` in this repository, where you can read it, export it and delete it
```

(The last three lines are long on purpose: they are one sentence each, and a report a user can read in a
terminal is a report that does not hide its clauses behind a line break someone else chose.)

`COMPILED` is gate one, `CONSENT` is gate two, and `STATE` is the verdict the two add up to: `off` (the
code is absent), `waiting` (compiled in, consent withheld — the key would turn it on) or `ON`. `SET IN`
names the layer that answered, which is how you tell "you never said anything" (`default`) from "you said
no yourself" (`user`).

## 5. How this is verified, mechanically

`python3 tools/check_privacy.py` carries the checks. `check` needs no toolchain and no network;
`offline` needs the built binary.

| Rule | What fails it |
|---|---|
| `V01` | the default build's resolved closure links a network-capable crate, or enables a `net-*` feature |
| `V02` | the consent table in Rust, in `tools/workspace_model.py` and in `schemas/fathom.schema.json` are not one table |
| `V03` | a `privacy.*` key is not user-scoped, or does not default to `false` |
| `V04` | an egress-capable call (`std::net`, an HTTP client) appears in default-shipped Rust code with no `net-*` gate in the file |
| `V05` | a Python analysis module imports a network stack, anywhere in the file — the only IO surfaces are the gated CLI edges that perform a consented fetch |
| `V06` | this page goes missing, stops being linked from the README or the docs front door, or falls silent about a consent key |
| `V07` | the SBOM and licence report of §6 are not generated, or nothing publishes them per release |

`offline` is the acceptance criterion, and it is a measurement rather than a reading of the source: it
unpacks the miniature repository that ships inside the binary, refuses egress, runs ten commands
(`--version`, `--help`, `stats`, `check`, `search`, `analyze`, `show`, `doctor`, `doctor --json` and a
**cold `index`**), and requires the trapped run to be byte-identical to the run with the network
available. Only the stamps the run carries are excused — the clock and the durations a
command prints — and the report counts how many lines that was, so an excuse is never invisible.

The trap is picked at runtime and **verified before it is trusted** (`check_privacy.py selftest`): a
probe program that *should* be blocked is run first, and if it is not blocked the tool refuses to report
a pass. In order of preference:

1. a network namespace (`unshare -rn`), which leaves only a loopback interface, and refuses everything else;
2. an `LD_PRELOAD` tripwire (`tools/privacy_netprobe.c`, compiled on the spot) that logs and refuses
   `connect`, `bind`, `sendto` and `getaddrinfo` for anything that is not loopback — this is the one that
   can also **count** attempts, which is why CI prefers it on a machine where user namespaces are
   unavailable;
3. `sandbox-exec` on macOS, with a seatbelt policy that denies `network*`.

CI runs it in [`.github/workflows/privacy.yml`](../../.github/workflows/privacy.yml), on the default
binary, and uploads the JSON report as a build artefact. ADR-002 §8.3 had recorded the connection-blocking
assertion as "a requirement with a named owner rather than as something measured" because the development
container has no namespace tooling; that sentence is closed here.

## 6. What a release publishes

`python3 tools/gen_sbom.py write` generates, into `sbom/`, and `check` fails CI on drift:

* **`fathom-<version>.cdx.json`** — CycloneDX 1.5: the application, its own crates, and every third-party
  crate the *default* build links, each with its pinned version, its `pkg:cargo/` URL, the SHA-256 from
  `Cargo.lock` and its declared licence. Deterministic by construction (`serialNumber` is a UUIDv5 over
  the lockfile digest, and there is no timestamp), so the committed artefact is diffable and a check can
  be meaningful.
* **`fathom-<version>-licenses.md`** — the same inventory read as obligations: a verdict per component
  (`allowed` / `violation` / `unknown`, computed against `deny.toml`'s allow-list, which stays the single
  policy source), the expressions counted, and two rows that have to say "none": no crate on
  `deny.toml`'s ban list, and no network-capable crate.

Both are attached to the GitHub release by `sbom.yml`. This is not a substitute for `cargo deny check
licenses` in CI — deny decides whether a PR may merge; the report says what a release contains — and
neither replaces the licence policy itself.

## 7. What this does not claim

* **A `net-*` build you asked for does use the network.** Consent plus the compile flag turns a real
  feature on; it talks to one endpoint fixed in code, and there is no key that redirects it, so "which
  host did that query" has one answer.
* **Your own environment can still reach out.** A proxy in your environment variables, or an agent that
  has its own network access, is not Fathom. The claim is about what this binary does unasked, and
  `fathom doctor` prints the `FATHOM_*` variable *names* it sees (never values) so a surprising
  environment is at least visible in a bug report.
* **The harness event stream is yours.** `.fathom/events.jsonl` is written by your agent's hooks
  ([ADR-005 §2](../adr/0005-harness-contract.md)); Fathom reads it and never sends it anywhere, but the
  agent that writes it is a program of your choosing with its own policy.
* **Signed artefacts and per-PR artefact hosting** are `e7.4` (**reco-3q4**); `sha256.sum` is integrity,
  not authenticity ([release.md](release.md)).
* **A security review** is `e7.8` (**reco-41a**), which reads this page as its starting position rather
  than as its conclusion.

---

**Next**: [reference/features.md](../reference/features.md) for every build feature and its consent key ·
[config.md](config.md) for the keys named above ·
[release.md §5](release.md) for counting users without watching them.
