# Threat model: what a clone is allowed to try

- **Status**: Reviewed — every row below is **fixed**, **accepted with a written rationale**, or
  **not applicable (asserted)**. Nothing is "to be looked at".
- **Ticket / plan key**: `reco-41a` (`e7.8`)
- **Implements**: [ADR-001 §4.6](../adr/0001-product-thesis.md) (a clone's config is untrusted config, and a
  clone does not get to consent, name a program, or choose where bytes go), [ADR-002 §4.5](../adr/0002-workspace.md)
  (the IO boundary that makes "no code from the repository" a checkable claim),
  [ADR-004 §7.7, §11](../adr/0004-storage.md) (size caps and the budget table that publishes them), and
  [ADR-005 §6.3](../adr/0005-harness-contract.md) (the hook event stream as untrusted input).
- **Executes as**: [`crates/fathom-cli/src/sandbox.rs`](../../crates/fathom-cli/src/sandbox.rs) (write
  containment), [`crates/fathom-analyze/tests/ledger_hardening.rs`](../../crates/fathom-analyze/tests/ledger_hardening.rs),
  [`crates/fathom-cli/tests/hostile_repo.rs`](../../crates/fathom-cli/tests/hostile_repo.rs),
  [`src/recommender/config/globs.py`](../../src/recommender/config/globs.py) (the ReDoS-proof matcher),
  [`src/recommender/deprisk/cli.py`](../../src/recommender/deprisk/cli.py) (the one network call),
  [`fuzz/`](../../fuzz/README.md) (the fuzz targets) and
  [`.github/workflows/fuzz.yml`](../../.github/workflows/fuzz.yml).
- **Owner of the document**: `analyze-agents` holds the register; a new finding is a PR to this file.

---

## 1. Scope

Fathom is a program a person runs against **a repository they cloned**, with **that person's own rights**.
That single sentence sets the whole model:

* **The attacker controls the repository.** Every byte of it: file names, file contents, lockfiles and
  manifests, commit messages and trailers, author identities, `fathom.toml`, `.gitattributes`, and — because
  `git` stores them as ordinary content and `core.symlinks` is the default on Linux and macOS — **symlinks**.
* **The attacker does not control the process.** There is no sandbox here and this document will not pretend
  there is one: a Fathom that runs as your user can do what you can do. What the model constrains is the
  program's *own* behaviour — which paths it writes, which programs it runs, which URLs it opens, how much
  memory and time a single input can cost.
* **The network is off by default** ([ADR-001 §4.6](../adr/0001-product-thesis.md), `reco-h7p`), so the
  SSRF surface is one consent-gated call rather than a library's worth of transitive fetches.
* **A refusal is a correct outcome.** Every control below is paired with the test that a refusal is
  *legible* — a published exit code, the path that was refused, where it really resolved, and a remedy. A
  control that turns a usable repository into an unusable one is an outage, not a security improvement, so
  each finding also asserts the ordinary case still works.

Out of scope, stated so nobody has to rediscover it: supply-chain compromise of the build (audited separately
by `cargo deny` and the SBOM job), a malicious *user* config (that is the user), side channels (timing,
filesystem layout), and anything a process with the user's rights could do anyway.

## 2. The register

Status vocabulary: **fixed** (closed by `e7.8`), **fixed earlier** (control predates this review, re-verified
here against its test), **accepted** (real, not closed, with the rationale written down), **n/a** (the surface
does not exist, and a test keeps it from appearing quietly).

| ID | Finding | Status | Owner | Evidence |
|---|---|---|---|---|
| F-1 | A clone's symlink writes the store or ledger outside the repository | fixed | `store-index` / `analyze-agents` | [`sandbox.rs`](../../crates/fathom-cli/src/sandbox.rs), [`hostile_repo.rs`](../../crates/fathom-cli/tests/hostile_repo.rs) |
| F-2 | SSRF through the advisory endpoint's prefix-matched allow-list | fixed | `analyze-change` | `_validate_endpoint`, `tests/test_deprisk_contract.py` |
| F-3 | Repository config names a program this process runs | fixed earlier | `tui-shell` | [`handoff.rs`](../../crates/fathom-cli/src/handoff.rs) + its `a_repository_cannot_name_an_editor_for_the_reviewer` |
| F-4 | Catastrophic backtracking in the config glob matcher | fixed | `core` (config), `analyze-change` (taxonomy) | [`globs.py`](../../src/recommender/config/globs.py), `tests/test_config_contract.py` |
| F-5 | Stack exhaustion in the hand-written YAML and XML readers | fixed | `analyze-change` | `MAX_DEPTH` in [`deps/yaml.rs`](../../crates/fathom-analyze/src/deps/yaml.rs), [`deps/xml.rs`](../../crates/fathom-analyze/src/deps/xml.rs) |
| F-6 | Huge repository content as memory/DoS | fixed earlier | `ingest-git` | `Limits` in [`record.rs`](../../crates/fathom-ingest/src/record.rs), `NotExamined::Oversized` |
| F-7 | A "ledger" file that is a device, a directory, or enormous | fixed | `analyze-agents` | `MAX_LEDGER_BYTES` and `refusal` in [`ledger/format.rs`](../../crates/fathom-analyze/src/ledger/format.rs) |
| F-8 | A clone's git hooks get executed | fixed (test) | `ingest-git` | `a_clone_owns_no_git_hook_that_fathom_runs` |
| F-9 | Hook-event stream (`.fathom/events.jsonl`) as hostile input | fixed | `ingest-git` | [`hardening.rs`](../../crates/fathom-ingest/src/hardening.rs), [`hooks_hardening.rs`](../../crates/fathom-ingest/tests/hooks_hardening.rs) |
| F-10 | Secrets and home-directory paths in reportable artefacts | fixed earlier | `core` | [`sanitize.rs`](../../crates/fathom-cli/src/sanitize.rs) |
| F-11 | A commit subject or path interpreted as a command | fixed (test) | `core` | `a_commit_message_carrying_a_command_is_still_just_text` |
| F-12 | Repository-supplied paths escaping the root in config | fixed earlier | `core` | `escapes_root`, `_check_paths_contained` in [`layer.py`](../../src/recommender/config/layer.py) |
| F-13 | Path *names* (NUL, control bytes, 4 000-segment paths) | fixed | `analyze-change` | `fuzz/fuzz_targets/paths.rs`, `hardening::paths` |
| F-14 | A regex compiled from repository content | n/a (asserted) | `analyze-change` | one `Regex::new` in product code, over built-in shapes only |
| F-15 | Decompression bomb | n/a | `ingest-git` | see §4.15 |
| F-16 | Ledger contents as an injection into a terminal | accepted | `analyze-agents`, `tui-views` | see §4.16 |
| F-17 | The gated write tools' defaults | fixed earlier | `tui-shell` | see §4.17 |

## 3. The attacker's five questions

Every row above answers one of five questions, and a new finding has to answer one of them too:

1. **Where do you write?** F-1, F-12. Only inside the canonicalised repository root, under `.fathom/`.
2. **What do you run?** F-3, F-8, F-11. Nothing that the repository named, ever; one program at all, and it
   is named by a flag, the environment, or the *user's* config.
3. **What do you read, and how much?** F-4, F-5, F-6, F-7, F-9, F-13, F-15. Bounded by a published budget in
   every case, with the refusal recorded rather than the input silently discarded.
4. **What do you send, and to whom?** F-2. One endpoint, parsed not prefix-matched, no redirects, capped response.
5. **What do you print?** F-10, F-16. Anything attachable to a bug report goes through the redactor.

## 4. The findings

### 4.1 F-1 — the symlink escape, in both directions

Every write path Fathom builds is `root.join(".fathom")…`, so a literal `..` cannot appear. The escape was
never in the string; it was underneath it. `git` stores a symlink as ordinary file content, so a clone can
contain:

```text
.fathom -> /home/taylor/scratch          # index writes a store there
.fathom/reviews -> /home/taylor/.ssh     # `review sign` appends to authorized_keys
.fathom/reviews/notes.md -> id_rsa       # `review status` reads a private key into a report
```

The third is the one people miss: containment is about **reads** too, because the ledger's contents are
rendered into a review verdict a person is supposed to trust.

**Control.** [`sandbox::Root`](../../crates/fathom-cli/src/sandbox.rs) canonicalises the repository root once
per run and refuses any write target whose resolved path is not inside it — refusing *resolution*, not
symlinks, so a store moved within the working tree with a link keeps working. The check runs before anything
is created, so the remedy is printed while nothing is half-written. On the read side
`ledger::format::parse` refuses any ledger file that does not resolve inside the repository.

**Verified by.** `a_fathom_directory_that_points_outside_the_repository_is_refused`,
`a_ledger_that_points_outside_the_repository_is_never_appended_to` (it seeds an `authorized_keys`-shaped file
and asserts the bytes are unchanged), `a_link_that_stays_inside_the_repository_is_allowed` (the no-false-positive
case), and `a_ledger_that_resolves_outside_the_repository_is_not_read`. Removing the check makes the first
test fail, which is how this file knows the test has teeth.

### 4.2 F-2 — SSRF through an allow-list that was a prefix

`deps.advisories.mode = "refresh"` is consent-gated, but the endpoint check it passed was
`endpoint.startswith("https://api.osv.dev")`, which `https://api.osv.dev.evil.test`,
`https://api.osv.dev@evil.test` and `https://evil.test#https://api.osv.dev` all satisfy. A cloned
`fathom.toml` cannot set the endpoint (there is deliberately no key for it — asserted by
`test_the_endpoint_is_not_configurable`), so this needed a local misconfiguration plus a consent click; it
was still a way for an "allowed endpoint" to name a host that was never allowed.

**Control.** `_validate_endpoint` *parses* the URL and refuses on scheme, userinfo, host, port, path, query or
fragment; the request URL is then reassembled from the checked parts rather than from the input string.
`_NoRedirect` turns a 3xx into an error — a redirect is the one mechanism by which an allowed endpoint
names a host nobody allowed. `MAX_RESPONSE_BYTES` (4 MiB) makes an oversized response a refusal, because half
an advisory is a guess about a vulnerability.

### 4.3 F-4 — the glob matcher was a denial of service with a plausible syntax

`noise.globs`, `test.globs`, `test.untested.exclude`, `non_code.patterns` and
`taxonomy.classes.<class-name>` are all keys a **repository** file may set, and the matcher they fed
translated a glob into `re` — `**` to `.*`, `*` to `[^/]*`. `**a**a**a**a**a**a**a**a` against
`"a" * 200 + "b"` is exponential in the number of wildcards, and both the pattern and the path are the
clone's. One `fathom index` on such a repository hangs forever, with an exit code of "still thinking".

**Control.** [`globs.py`](../../src/recommender/config/globs.py) now has one tokenizer feeding two engines.
Patterns with at most `WILDCARD_BUDGET = 2` unbounded wildcards, matched against a path no longer than
`REGEX_PATH_LIMIT = 4096`, use the old anchored regex — its worst case is then O(path²), and the two
constraints are what make that a bound rather than a hope. Everything else runs on a state-set simulation
with nothing to backtrack: O(pattern × path), always. `test_a_wildcard_dense_pattern_does_not_burn_time`
holds the four hostile shapes to 50 ms; `test_both_glob_engines_accept_the_same_language` proves the two
engines accept the same language, so moving a pattern between them cannot change which paths it matches.

**Why not "just use `fnmatch`"?** `fnmatch` has no `**`, matches `?` across `/`, and is wrong for the leading
`!` demotion this syntax needs; `pathlib` is wrong about `**` crossing directories. And a fast path was not
optional: the taxonomy matches 400+ patterns per changed file inside a published per-file budget
(`test_cold_lookups_fit_a_per_changed_file_budget`), which the pure-Python engine alone would not fit. That
is what the two-engine split buys, and the budget test is what keeps it honest.

**Deliberate behaviour change.** `[!a]` now means the glob complement. The old translator passed the class
body straight to `re`, where it is the two-character set `!` and `a`. An empty class (`[]`) is now a
`BadPattern` — a warning naming the key — rather than whatever `re` made of it. Both are named in
`test_glob_engines_agree_with_the_legacy_translation` so they cannot be rediscovered as regressions.

### 4.4 F-3, F-11, F-12 — the program, the commit subject, and the config path

Three findings that share one control: **repository content is data, and there is exactly one place each kind
of data is allowed to end up.**

* **F-3, the program.** `fathom open`, the shell's `E` key and the difftool handoff resolve an executable from
  a flag, then the environment, then *the user's* config, then a platform default — and the repository's
  `fathom.toml` is deliberately absent from that list, with a printed notice when a repository tries
  ([`handoff.rs`](../../crates/fathom-cli/src/handoff.rs), `a_repository_cannot_name_an_editor_for_the_reviewer`).
* **F-11, the commit subject.** No shell is involved anywhere: every program is started as
  `Command::new(program).args(…)` with an argv array, so a subject that *is* a shell command is a string. The
  register keeps this row because it is cheap to break — one `sh -c` in a diff view would undo it — and
  `a_commit_message_carrying_a_command_is_still_just_text` is the tripwire.
* **F-12, the config path.** A cloned `fathom.toml` pointing `paths.*` outside the repository is refused by
  `globs.escapes_root` and `layer._check_paths_contained`, textually rather than by resolving symlinks — the
  check must not depend on the path existing, and a check that resolves is a check that races.

### 4.5 F-5 — recursion as a stack overflow

The YAML and XML readers in `fathom-analyze/src/deps/` are hand-written recursive descent, which is the right
way to read a lockfile and the wrong way to meet 5 000 nested mappings: the failure is a `SIGSEGV` with no
message, no exit code and no typed `Unparsed` — a review tool that died on somebody's clone rather than a
repository that failed to analyse.

**Control.** Both readers carry `MAX_DEPTH = 100` and return an error past it, which the caller turns into
the same typed "could not parse this" that any malformed file gets. Real lockfiles nest a handful deep;
`nesting_beyond_the_depth_cap_is_refused_rather_than_overflowing_the_stack` asserts both the refusal *at* the
cap and a successful parse just inside it, so the bound stays a guard rail and not a wall. The JSON and TOML
readers use parsers that carry their own depth limits and are covered by the same fuzz target.

### 4.6 F-6 — size, not cleverness

Huge input needed no fix: `Limits` caps a blob at `ingest.file_size_cap.mib` (8 MiB) before it enters memory
and records the file as `NotExamined::Oversized` rather than pretending it was read — the ADR-004 §7.7 rule
that a decision not to look is itself a fact to store. This row exists in the register because "already
handled" is a finding too, and because the fuzz corpus's 4 MB single-line and 500 000-separator shapes are
the regression test for anyone who later moves a cap.

### 4.7 F-7 — the ledger is markdown, and markdown can be anything

`.fathom/reviews/*.md` is a directory a clone can populate with anything. `parse` now refuses, as a
`Problem`, a file that does not resolve inside the repository (§4.1), a file that is not a regular file, and
a file over `MAX_LEDGER_BYTES` (32 MiB). Refusal is a `Problem` and never a silence: "the ledger holds zero
decisions" and "the ledger was refused" have to be two different sentences on a screen.

### 4.8 F-8 — the hook a clone installs

A clone can write `.git/hooks/pre-commit`. Git only fires hooks for commands that fire them, and everything
Fathom runs is read-only plumbing (`log`, `diff`, `rev-parse`, `config`, `var`), so this was already true —
a property of the command list, and therefore not a property. `a_clone_owns_no_git_hook_that_fathom_runs`
installs three hooks that touch a marker file and fails if the marker appears, so the day someone adds a
`git worktree` or `git checkout` to that list, this test is the one that says so.

### 4.9 F-9 — the harness stream

`.fathom/events.jsonl` is written by a coding agent and read by Fathom, so it is untrusted input from a third
direction ([ADR-005 §6.3](../adr/0005-harness-contract.md)). Torn lines, embedded NULs, absurd field lengths
and path-shaped strings all run through `fathom_ingest::hardening::hook_event`, which is the same entry point
the `hooks` fuzz target calls; `crates/fathom-ingest/tests/hooks_hardening.rs` replays the corpus and a
proptest stream on stable Rust with no nightly toolchain.

### 4.10 F-10 — what a bug report may contain

Anything attachable — `fathom doctor`, `fathom export`, `fathom config explain` — goes through
[`Redactor`](../../crates/fathom-cli/src/sanitize.rs): the repository root and the home directory are replaced
with relative spellings, URL userinfo is dropped, and any `key = value` whose **key name** is on the secret
list (`token`, `secret`, `password`, `key`, …) is printed as `key=<redacted>`. Key-name matching is a
deliberate limit: a redactor that guesses what a secret *looks* like is a redactor that leaks the first secret
with a boring name. Secrets a repository names are config values the user chose to put in a file; Fathom's
promise is narrower and kept — it does not put them in its own output.

### 4.13 F-13 — path names that are not paths

NUL bytes, control characters, a 4 000-segment path, `Cargo.lock/../../etc/shadow`, a backslash on a platform
that treats it as an ordinary character: all of it reaches `hardening::paths` in the `paths` fuzz target and
in `tests/hardening.rs`, because the readers that take a *name* (recognition, the scope glob set, the scope
deviation comparison) are the ones a `--path` argument or a `fathom.toml` entry can reach without any file
existing.

### 4.14 F-14 — no regex is compiled from repository content

Product code contains exactly one `Regex::new`, in `detectors/secret_string.rs`, over shapes built into the
detector. Repository-supplied *globs* on the Rust side go through `globset`/`regex`, which is an automaton:
linear in the input by construction, so the ReDoS class does not exist there. The Python side did not have
that protection, which is why F-4 was a finding and why `globs.py` is the only place config globs are
compiled — asserted by `test_a_bad_glob_in_any_repo_settable_key_is_a_warning`, for every key the schema
declares as a repo-settable `glob_list`.

### 4.15 F-15 — decompression, honestly

There is no archive or compressed-stream reader in the product: no zip, no gzip-in-a-path-Fathom-reads, no
`tar`. The one inflation that happens is git's own zlib inflation of loose objects and packfiles, performed by
`gix`, bounded by git's own formats and by the object and pack budgets ADR-004 §11 publishes. **Accepted**
for `e7.8`, on the reading that a decompression-bomb control is only meaningful once something decompresses;
if a future ticket reads an archive (an export bundle, an advisory cache), this row becomes a finding and the
budget goes in `benchmarks/budgets.json` with the rest.

### 4.16 F-16 — ledger contents as terminal output — accepted

The ledger is markdown assembled from episode ids, commit subjects and reviewer notes, and it is printed to a
terminal. Terminal escape sequences inside a commit subject could therefore reach a terminal. **Accepted** for
`e7.8`, because the mitigation belongs to the renderer rather than the ledger: the TUI draws through its own
buffer, and the plain-text verbs are where a future change would strip or escape control bytes. Tracked here,
not in code, so that the next person to touch `render` finds the question already asked. The same reasoning
covers markdown injection into an exported artefact: HTML export escapes its text, and the JSON contract is
data.

### 4.17 F-17 — the gated write tools

`fathom open`, the shell's `E` key and the difftool handoff launch a program with the reviewer's rights — the
one place in the product where "unsafe default" would mean something. The precedence excludes exactly one
layer, the repository's `fathom.toml`, and says so out loud when a repository tries
(`fathom.toml's [ui] editor = … was ignored: a clone does not get to name a program this process runs`).
Writes the tools make are inside `.fathom/` or are the reviewer's own edit in their own editor. The
verification is `handoff.rs`'s own tests plus `a_run_creates_nothing_outside_the_state_directory`.

## 5. Fuzzing

Three targets, in [their own cargo workspace](../../fuzz/README.md) so that nightly, `-Zsanitizer` and
`libfuzzer-sys` stay out of the shipped dependency graph and out of `cargo build --workspace`:

| Target | Driver | What it reaches |
|---|---|---|
| `lockfile` | `fathom_analyze::hardening::lockfile` | every ecosystem reader, via `<name>\0<content>` |
| `hooks` | `fathom_ingest::hardening::hook_event` | the hook-event reader, torn lines included |
| `paths` | `fathom_analyze::hardening::paths` | recognition, the scope glob set, the deviation comparison |

The driver lives in the crate that owns the parsers, **not** in the fuzz directory, so the fuzz target and the
test suite cannot drift apart about what they call. `fuzz/corpus/` is generated by
[`tools/gen_fuzz_corpus.py`](../../tools/gen_fuzz_corpus.py) and `check`-ed in CI, which is what makes the
corpus reviewable instead of a pile of artefacts. The same seeds run on stable in
`crates/fathom-analyze/tests/hardening.rs` and `crates/fathom-ingest/tests/hooks_hardening.rs` next to
proptest streams, so a shape that ever crashed the fuzzer is a per-commit regression test with no nightly
toolchain involved.

[`.github/workflows/fuzz.yml`](../../.github/workflows/fuzz.yml) runs one minute per target on pulls that
touch the readers, and 30 minutes per target nightly. A crash artifact becomes a seed and a test; it is not
committed raw, because `gen_fuzz_corpus.py check` is the gate that keeps the corpus reproducible.

## 6. Adding a finding

1. One row in §2 with an ID, an owner from [ADR-002 §9](../adr/0002-workspace.md) and a link to the evidence.
2. A subsection in §4 that says what the attacker controls, what the control is, and which test fails when the
   control is removed. Removing the control and watching the test fail is part of writing the finding.
3. If it is **accepted**, the rationale has to name the thing that would change the decision — "we will get to
   it" is not a rationale, and §4.15 and §4.16 are the pattern.
4. If it is reachable from repository content at scale, a seed in `tools/gen_fuzz_corpus.py`.
