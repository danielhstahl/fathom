# Extensibility: detectors you did not write, and the events that say what ran

> **Status: shipped.** `e6.7`, ticket `reco-raw`. Three things ship here: a **detector API** a third party
> can implement out of tree ([`fathom_analyze::plugin`]), a **finding schema** that carries a namespace and a
> version so an unknown finding is ignorable instead of fatal, and an **event bus** that publishes one
> analysis run as versioned JSONL an external tool can read. Detector *selection* — `detectors.enabled`,
> `detectors.disable`, `--detectors` — ships with them, because an extensible catalogue nobody can switch off
> is a liability rather than a feature.
>
> [ADR-002]: ../adr/0002-workspace.md
> [the detector catalogue]: pathology-detectors.md
> [the delta model]: delta-model.md

A detector is a pure function over one episode. That is the whole reason any of this is possible: a function
with no capabilities can be called by code this repository has never seen, and its output can be checked by a
fixture instead of a trust relationship. Everything below follows from that one property.

```console
$ fathom analyze HEAD --json > /tmp/episode.json
$ acme-detectors --delta /tmp/episode.json --json
{"schema_version":1,"finding_type":"acme.untested_agent_logic","plugin":"acme-detectors 0.1.0",…
```

Pipe the first command's stdout into the second's stdin and it is the same two commands with a file
deleted; the file is spelled out here because every command in these pages is parsed by the parser, and a
pipe is a thing a reader has to be told about rather than shown in a shell metacharacter.

## Three kinds of "detector", in order of what they cost

| | who writes it | where it runs | how it is chosen |
|---|---|---|---|
| **built-in** | this repository, in [`detectors/`](../../crates/fathom-analyze/src/detectors) | inside `fathom` | on by default, or `detectors.enabled` |
| **plugin detector** | anyone, against the published `fathom-analyze` crate | inside the binary that registers it | by name, always: `--detectors +acme.thing` |
| **out-of-tree program** | anyone, against the [delta document](delta-model.md) | its own process, reading `fathom analyze --json` | whatever that program does |

The third row is the one most people actually want, and it needs no API at all — the delta model is
versioned, published and stable, so `fathom analyze --json \| your-thing` is a supported integration surface
today. The middle row exists for the case where the plugin is linked into a host that wants plugin findings
in *its* report, with the same suppression, capping and provenance rules as a built-in.

## The plugin API

[`fathom_analyze::plugin`](../../crates/fathom-analyze/src/plugin/mod.rs) is the public surface. Two traits'
worth of API, and no capabilities:

```rust
pub trait Detector {
    fn meta(&self) -> Meta;
    fn detect(&self, input: &Input<'_>) -> Vec<Finding>;
}
```

`Input` borrows an [`EpisodeDelta`] — the same document `fathom analyze --json` prints — plus the
prior `EpisodeRef` history where the host has it. That is the entire capability list. A plugin detector
gets no store, no `git`, no clock and no network, **by shape rather than by policy**: there is nothing to
call. A plugin that needs a fact the delta does not carry has to say so in its finding, which is the same
honesty rule [the built-ins](pathology-detectors.md) follow when content is `absent`.

`Meta` is what a host prints without running anything:

| field | meaning |
|---|---|
| `namespace`, `id` | become the finding type `namespace.id`; both are validated at registration |
| `version` | the detector's own version, carried in every finding it produces |
| `api_version` | the `plugin` API major version the detector was written against |
| `title`, `asks` | the one-line claim and the question it answers, for a `--list`-shaped print |
| `default_on` | always `false` for a plugin: a plugin opts in by name and `all` never claims one |
| `finding_types` | the namespaced types this detector may emit; anything else is refused |

[`Registry`] holds them. `Registry::run(input, selection, limits)` returns a [`Report`] with **one row per
registered detector, always every one of them** — the same accounting rule the built-in catalogue follows, so
"it found nothing" and "it did not run" are never the same line.

### What the registry refuses, and why each refusal is a rule and not a mood

A plugin is untrusted code producing a finding that a human will act on. Eight rules, all of them in
[`plugin/finding.rs`](../../crates/fathom-analyze/src/plugin/finding.rs), all of them tested in
[`tests/plugin_api.rs`](../../crates/fathom-analyze/tests/plugin_api.rs):

| rule | what happens | why |
|---|---|---|
| **R1** unknown `schema_version` | the finding is dropped, by name, and the run keeps going | a plugin built against a newer API must not corrupt a report it cannot describe |
| **R2** a finding type the detector did not declare | dropped, with the type named | a finding that can name any type can name a built-in's and steal its owner |
| **R3** empty subject or message | dropped | a row nobody can act on is noise with a severity attached |
| **R4** `high` severity with no evidence | demoted to `medium`, and the demotion is stated | `high` is the severity a reviewer stops reading everything else for |
| **R5** confidence outside `0..=1`, and `1.0` for an inferred claim | clamped, and the ceiling is the built-in price list | an assertion of certainty from unverifiable code is a bug, not a claim |
| **R6** a suppression key outside the plugin's own namespace | rewritten into it, and the rewrite is stated | a plugin must not be able to mute a built-in detector's findings |
| **R7** a `jump` naming a view this build does not have | replaced by the episode jump | a dead button is worse than a dull one |
| **R8** more rows than the cap | counted as `hidden`, never silently truncated | a cap that is invisible is a lie about volume |

Nothing in that list is fatal. **A hostile or broken plugin costs the reader its own findings and nothing
else** — the same containment the event bus applies to a panicking subscriber.

### The finding's wire format

`schemas/plugin-finding.schema.json` is generated from the Rust type
([`plugin::schema`]), blessed by a test, and diffed like every other published schema here. One finding:

```json
{"schema_version": 1, "finding_type": "acme.untested_agent_logic", "plugin": "acme-detectors",
 "plugin_version": "0.1.0", "episode_id": "9f2c…", "subject": "src/core/mod.rs",
 "message": "28 lines of source were added by an agent turn …", "severity": "medium",
 "confidence": 620, "evidence": [{"kind": "lines", "path": "src/core/mod.rs", "line_start": 1}],
 "jump": {"view": "diff", "path": "src/core/mod.rs"}, "suppression_key": "acme.untested_agent_logic:src/core/mod.rs"}
```

Three things about it are load-bearing:

* **`finding_type` is namespaced**, `namespace.id`, and a namespace may not collide with a built-in detector
  id or with a reserved namespace. A built-in id is never dotted, so a dotted type is always a plugin's and a
  report can say which it is looking at without a registry in hand.
* **`schema_version` is in the payload**, not only in the schema's `$id`. A reader that cannot read a version
  refuses it by name (R1), which is what makes "additive stays v1" safe to promise.
* **`suppression_key` is inside the plugin's namespace** (R6), so `detectors.suppress` mutes a plugin's
  family exactly as it mutes a built-in's, and no plugin can reach outside its own.

## Switching a detector off

Two switches, and the difference between them is the whole point of having both:

| | what it turns off | what the report says |
|---|---|---|
| `detectors.disable`, `--disable-detector`, `--detectors -id` | the detector: it does not run | its row is `disabled`, with the count of findings it would have made unavailable |
| `detectors.suppress`, `detectors.suppress = ["family:*"]` | the finding: the detector still runs | the row is `suppressed` and the count is kept |

Disable it when you have decided the detector is wrong for this repository. Suppress it when you have decided
*this family of findings* is noise here. Both are per-repository, both are read from `fathom.toml`, and
`disable` is applied after `enabled`, so a repository can say "everything except that" without a comment
explaining a precedence.

```toml
[detectors]
enabled = ["test_deletion", "untested_new_logic", "debug_leftover"]
disable = ["secret_string"]          # our fixtures are full of fake keys; the row still shows
suppress = ["todo_debt:*"]           # we track TODOs elsewhere
```

The grammar, which is the same in the file and in the flags
([`detectors::policy::Selection`](../../crates/fathom-analyze/src/detectors/policy.rs)):

| token | meaning |
|---|---|
| `id`, `+id` | run this one as well |
| `-id` | do not run this one, even if it ships on |
| `all` | the base is every built-in detector, defaults aside |
| `none` | the base is nothing; list what you want with `+id` |

so `--detectors none,+churn_loop` is one detector and `--detectors all,-secret_string` is eleven. A
namespaced token (`+acme.orphan_test`) names a plugin detector: `all` never claims a plugin, and a plugin
that is not linked into the running binary is reported as such rather than passing for a detector that found
nothing. Resolution order is the one every knob in this product uses — built-in default set, then
`fathom.toml`, then the command line — and a name that matches nothing is a notice naming it, never a
failure. A repository that upgrades must never be unable to ask a question.

Disabling is visible in every surface that prints a scan: `fathom why` prints the row, `fathom export`'s
JSON carries `outcome: "disabled"`, and the event stream says `detector.skipped` with the reason. The row is
kept **because** it was switched off — "did the tool look for that?" is a question a review has to keep being
able to answer.

## The event bus

[`fathom_core::events`] is the bus, and the contract. It lives in `fathom-core` and opens no file and reads
no clock (ADR-002 §4.1); [`crate::events`] in the CLI is the half that may, and it is where `[events]` is
read and the file is opened.

**The wire format is JSONL**: one JSON object per line, each carrying `contract`
(`fathom-analysis-events`), `schema_version` (currently `1`), `kind`, `seq`, `run`, and then the fields that
kind fills in — `episode`, `detector`, `subject`, `outcome`, `severity`, `counts`, `note`. Unknown fields are
ignored on read, which is what makes additive versioning a promise instead of a slogan; a line whose
`schema_version` is newer than this build is refused **by name**, and a line whose `kind` this build does not
know is kept as a kind. A stream from a newer build is a stream an older reader can still print.
[`schemas/fathom-analysis-events.schema.json`](../../schemas/fathom-analysis-events.schema.json) publishes
that shape for a consumer who would rather validate than trust — generated from the types in `events`, blessed
like every other schema here, and describing **one line** rather than the file.

The declared kinds — this list is the contract, and
[`tests/test_extensibility_contract.py`](../../tests/test_extensibility_contract.py) fails if the code and
this page part:

| kind | what it says |
|---|---|
| `run.started` | a verb began; `run` names it |
| `selection.resolved` | which detectors this run was asked for, after the file and the flags |
| `analysis.started` | one episode's scan began; `counts.detectors` |
| `analysis.finished` | it ended: `counts.findings`, `counts.detectors`, `counts.gaps` |
| `detector.finished` | one detector answered: `detector`, `outcome`, `counts.findings`, `counts.hidden`, `counts.suppressed` |
| `detector.skipped` | one detector did not: `outcome` is `disabled`, `not_examined` or `suppressed` |
| `finding.raised` | a finding arrived: `detector`, `severity`, `subject`, `note` |
| `notice.raised` | something the run has to admit and no other kind carries |
| `plugin.registered` | a plugin detector was registered in this process |
| `plugin.rejected` | a plugin, or one of its findings, was refused — `note` names the rule |
| `index.started` | an index pass began |
| `index.finished` | an index pass ended: `counts.episodes`, `counts.errors` |
| `run.finished` | the verb ended, with `counts.code` |
| `run.failed` | the verb gave up, with the reason in `note` |

Three properties are worth naming because they are the ones a bus usually gets wrong:

* **A panicking subscriber is counted, not propagated.** One bad subscriber may not stop an analysis, and the
  panic is reported as a stat (`stats.panics`) rather than swallowed.
* **Publication is free when nobody is listening.** Every verb constructs a bus and publishes on it; with no
  subscribers that is one lock on an empty list, which is why there is no `--events`-shaped cost when the
  feature is off.
* **Subscribers are capped** (`MAX_SUBSCRIBERS`, 16) and refused past that with a number attached, so a leak
  in a long-running host is a visible refusal rather than an unbounded queue.

### Turning it on

```toml
[events]
enabled = true
on = ["detector.*", "finding.raised"]      # a family, or an exact kind; empty means everything
```

The stream is appended to `paths.analysis_events` (default `.fathom/analysis-events.jsonl`). `--events
<file>` turns it on for one run, over whatever the file says, and `--events -` sends the same lines to
stderr — to stderr, never stdout, because `--json` promises stdout holds exactly one document:

```console
$ fathom why HEAD --json --events -
$ fathom events --tail 3
run.finished         ok why-1780000000000                    code=0
finding.raised       high   src/harbour/ledger.py  untested_new_logic [high] confidence=620
detector.finished    ran                                    untested_new_logic findings=1 hidden=0 suppressed=0
```

Above, `--events -` is what the reviewer typed *and* where it went: the lines land on stderr, `71` on
stdout, and `jq` may read the stdout it was promised. Off is the default, and a run that writes nothing says nothing about it: the product does not create files a
repository did not ask for (ADR-001 §4.6). Two refusals close it: a *repository's* config may not point the
stream outside the repository (the `paths.*` rule — a cloned file does not choose where your process
writes), and every line is redacted on the way out through the same [`crate::sanitize`] redactor the rest of
the CLI uses, because an event log in CI is a disclosure if it carries an absolute build path or a
secret-shaped string.

`fathom events` is the reader the product ships, and it is deliberately the same reader the bus's own tests
use: `--json` for the raw lines, `--on` to filter, `--tail` for the end, `--explain` to print where the
stream is, whether it is on, and what was refused. A line it cannot read is reported and skipped, never
fatal — a run killed mid-write leaves one torn line at the end, and every event before it is still worth
having.

## The example plugin, out of tree

[`plugins/acme-detectors`](../../plugins/acme-detectors) is a second cargo workspace, excluded from the
root one (`workspace.members` does not list `plugins/*`, and `check_workspace.py` would otherwise lint it as
if it were a crate here). Its `Cargo.toml` depends on `fathom-analyze` the way a third party's does, and CI
builds and runs it in its own job (`ci.yml`'s `out-of-tree-plugin`), so "the published crate is usable" is a
checked fact rather than an intention. Two detectors live there:

* **`acme.untested_agent_logic`** — an agent-attributed episode that added source and no test file. Chosen
  because it needs one field of the delta (`author_kind`) and nothing else, which is the shape of most
  third-party rules.
* **`acme.unseen_reach`** — churn above a floor whose blast section was never measured. Chosen because it is
  a *negative* rule: it speaks when the data is missing, which is the case a naive detector silently scores
  as "no blast".

Run it, in the shape a reviewer would actually use it:

```console
$ cd plugins/acme-detectors
$ cargo run --quiet -- --list
acme.untested_agent_logic  medium  an agent added source and no test file  (default: off)
acme.unseen_reach          low     a change this size with no blast radius  (default: off)
$ cargo run --quiet -- --delta fixtures/miniature.json --check fixtures/miniature.expected.jsonl
```

Its test suite ([`tests/contract.rs`](../../plugins/acme-detectors/tests/contract.rs)) is written as the
contract a stranger would want: blessed fixture output, each rule's boundary, the CLI's flags, a `--list`
that prints what each detector declares, suppression by key, and a round trip through the JSON document to
catch the case where the model and the plugin disagree about what a field means.

## Stability policy

This is the promise a third party can build against. It is written down because an undocumented API is a
promise nobody can rely on, and an over-documented one is a promise nobody can keep.

**Stable now, and covered by the rules below:** the `Detector` trait's two methods; `Meta`'s and `Input`'s
existing fields; `Finding`'s existing fields and therefore `schemas/plugin-finding.schema.json`; every
`finding_type` grammar rule (R2, R6); the event kinds and fields above; every `Selection` token; the delta
document itself, under its own [version policy][delta model].

**Not stable:** anything not in that list. An undocumented field, a `Debug` shape, an error message's wording,
the order of rows in a `--list`, the wording of a notice. Message text is prose, and prose is not an ABI.

**The rules:**

1. **Additive only within a major.** A new field on `Meta`, `Input`, `Finding` or `Event` is allowed at the
   same version, and readers must ignore what they do not know — which is why nothing here uses
   `deny_unknown_fields`, and why the finding schema says `additionalProperties: true`. Removing, renaming
   or retyping a field, narrowing a type or an enum, or changing what a required key means is breaking.
2. **Breaking changes are versioned, in the payload.** `Finding.schema_version` and `Event.schema_version`
   are bumped with the document, and a reader that cannot read a version refuses it by name (R1) instead of
   half-understanding it. `Meta.api_version` states which major a detector was written against, so a host can
   say *which* plugin is behind rather than failing oddly.
3. **A built-in id is forever.** A detector id in the catalogue is a name in other people's
   `fathom.toml` (`enabled`, `disable`, `suppress`) and in other people's plugin namespaces (which may not
   collide with it). Retiring a built-in means marking it off-by-default and keeping the id, never reusing it.
4. **A deprecated thing ships with its replacement in the same release.** A field is deprecated by documenting
   it as such, naming the replacement, and keeping it working for one minor; only then is it removed, in a
   major. `cargo doc` is where the deprecation is stated, and
   [`tests/test_extensibility_contract.py`](../../tests/test_extensibility_contract.py) refuses a documented
   event kind that the code does not emit and a code-emitted kind this page does not name.
5. **Plugins do not get to extend this list.** A plugin's own findings, types and keys are its contract with
   its users. The API's contract is that the host keeps applying R1–R8 to it, which means a plugin never has
   to be told that its confidence of `1.0` meant something else — the host already said so, in the report.

The pragmatic consequence for a plugin author: read `finding_type`, `severity`, `subject` and `message`,
ignore everything else you do not know, and never assume a field you have not seen before is absent in an
older build. That is the same discipline the built-in surfaces apply to the delta document, and it is why
both can move without a coordinated release.

## What is deliberately not pluggable

Three things, each for a reason rather than for want of time:

* **No dynamic loading.** No `.so`, no `dlopen`, no plugin search path. A plugin is compiled and registered
  by a host binary, which makes it reviewable, reproducible, and covered by the build's own licence and
  advisory gates (`cargo deny`) — and it means `fathom` links no third-party code it has not seen.
* **No plugin-supplied severity scale, no plugin-written findings in the built-in namespace.** Severity is a
  shared language: a report where one author's `high` means something else is a report nobody ranks. See R2,
  R5, R6.
* **No plugin access to the store, the repository, the clock or the network.** A plugin gets a delta document
  and a borrowed history. Anything else a plugin needs belongs in the delta model — which is versioned,
  reviewed and documented — rather than in a capability handed to unreviewed code.
