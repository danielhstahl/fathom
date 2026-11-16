#!/usr/bin/env python3
"""Check `.github/workflows/*.yml` compiles against the repository it lives in (**reco-yuk**).

Six of the eleven workflows in this directory were red when this file was written, and four of them were
red for a reason a compiler would have caught and a reviewer cannot see in a diff:

* `ci.yml` named `axodotdev/cargo-dist/action@v1`, an action path that has never existed in that
  repository. The job died in *Set up job* — before one line of its own ran — which is the only failure
  mode with no log to read;
* `fuzz.yml` ran `cargo fuzz` with `working-directory: fuzz`, which makes cargo-fuzz look for
  `fuzz/fuzz/fuzz_targets/`, and it read a step output as `steps.minutes.value` instead of
  `steps.minutes.outputs.value` — a typo that is not an error in a GitHub expression, only an empty
  string, which then reached `$(( … ))`;
* `ci.yml` built `--all-targets` of a crate whose test suite needs a store backend, on a feature set
  without one;
* the committed `main.wxs` had been whitespace-touched, and `dist plan` refuses to start on an
  out-of-date generated file, so the whole release pipeline was stopped by eight lines of whitespace;
* and the gate went red in CI while passing on every laptop: `perf.yml` reads `benchmarks/trend/trend.md`,
  which the perf gate *writes* and `.gitignore` excludes, so "does this path exist" was a question about
  the developer's working tree — and a checkout is not a working tree (`reco-8myi`).

None of that is visible in the YAML. All of it is visible by *reading the rest of the repository*: the
action does not exist, the directory does not hold what the tool expects, the target needs a feature the
step did not name, the generated file is not its generator's output. That is what this file does, offline,
with no network and no toolchain.

    python3 tools/check_workflows.py             # what CI runs
    python3 tools/check_workflows.py --verbose   # the passing rules too
    python3 tools/check_workflows.py --network   # ask GitHub that every action ref resolves
    python3 tools/check_workflows.py budgets     # the artefact budget, as the docs print it
    python3 tools/check_workflows.py triggers    # will CI run on the branch I am standing on?
    python3 tools/check_workflows.py self-test   # prove the checker can fail

**The rules**, named as they are named in the failure lines:

| Rule | What it refuses |
|---|---|
| `A01` | an action reference that is malformed, or not on the inventory below (`--network` proves each inventoried ref resolves) |
| `A02` | a `cargo-dist` installer URL naming a version other than `dist-workspace.toml`'s pin |
| `D01` | a `dist build --artifacts=local` (or `=all`) step that names no `--target` — `dist` then builds *every* triple in `dist-workspace.toml` on that one runner, and refuses to cross-compile to macOS |
| `E01` | `steps.<id>.<prop>` where `<prop>` is neither `outputs` nor a real step property — the shape of the `fuzz.yml` bug |
| `E02` | `steps.<id>` naming a step with no `id:` in that job |
| `E03` | `needs.<job>` that does not exist, or `needs.<job>.outputs.<name>` that job never declares |
| `E04` | `inputs.<name>` read without being declared by either trigger |
| `P01` | a repository path inside a `run:` step that the runner will not have: absent from the tree *and* not ignored by `.gitignore` — an ignored path is the run's own output, not a claim about the checkout |
| `P02` | a `working-directory:` the runner will not have, on the same test |
| `P03` | a `cargo fuzz` step run from inside `fuzz/` |
| `P04` | a workflow that runs `cargo fuzz` while `fuzz/Cargo.toml` is not marked as a fuzz manifest — the marker cargo-fuzz refuses to start without |
| `T01` | `python3 tools/<tool>.py <subcommand>` where the tool's argparse offers no such subcommand |
| `T02` | `cargo … -p <pkg>` where `<pkg>` is not a workspace member |
| `T03` | `--features` naming a feature that package does not declare |
| `T04` | `--test <name>` / `--example <name>` with no such target in that package |
| `J01` | a job that `needs:` a job that does not exist |
| `J02` | a workflow with no `permissions:` anywhere — an unset permission is the organisation default, a policy written where this file cannot see it |
| `J03` | an artefact name uploaded twice in one workflow, or a `download-artifact` pattern that matches nothing uploaded there |
| `R01` | a job with no `timeout-minutes`, or one above the ceiling — the default is six hours, and four hung jobs were 43 % of a month's runner-minutes (`e11.1` §2.3) |
| `R02` | a `pull_request` workflow with no `concurrency` group, a group that is not per-workflow-and-per-ref, or a `cancel-in-progress` that is neither `true` nor excused by a `tolerates:` note |
| `R03` | a `continue-on-error` with no `tolerates:` note naming what it tolerates and why — and a `tolerates:` note whose tolerance is gone |
| `R04` | cache discipline: a job that compiles without restoring the cargo cache; a `rust-cache` step that writes its own key instead of taking the bust lever from `env.CACHE_BUST`, that does not save on failure, or that leaves a second workspace it builds uncached; a hand-rolled `actions/cache` over cargo paths |
| `R05` | an upload outside its artefact budget: an artefact no budget names, no `retention-days` or one longer than its budget, a path the budget does not describe, no size guard before it, or a guard whose limit is above the declared one, that runs under a different `if:`, that cannot fail, or that does not say which step produced the bytes |
| `R06` | scheduled-only work on the pull-request path: a step whose entry in `CI_REQUIRED_STEPS` is `scheduled` running in a workflow that triggers on `pull_request` — the rot `e11.2` would otherwise be free to grow back |
| `R07` | a gate that runs on no branch the work reaches: a `push` trigger narrowed to a list of branch names (`branches: [master]`, the shape that watched a frozen default branch for three weeks — `e11.1` §5.2, `reco-kwg6`), or a workflow that triggers on every pull request and on no branch push at all, so a branch with no open pull request is never measured |

**The `tolerates:` note.** Two rules read a comment, and they read exactly one shape of comment, because "say why in a
comment" is not checkable until the comment has a name a checker can look for:

```yaml
# tolerates: a Pages deploy that is midway, because tearing one down under the runner can leave
# half a merge published on the site
```

`R02` accepts a `cancel-in-progress` that is not literally `true` only when such a note sits above it, and `R03`
refuses a `continue-on-error` without one. The note has to name a *why*, not only a *what*, and a note whose
tolerance has been deleted is itself a failure: an explanation of a tolerance nobody still has is the same kind
of lie in the other direction.

**The branch a gate runs on.** `R07` exists because of a fact the census could only read off the API: for the
whole thirty-day window, no run of any of these files was ever started by the work this repository was doing.
`master` was frozen at `e194a63d9d`, the branch the loop worked on was never pushed, and every file said
`push: branches: [master]` — a true sentence about where merges happened that had quietly gone false, with
nothing in CI left to notice (`e11.1` §5.2, closed by `reco-kwg6`). A list of branch names is the shape the
rule refuses, because a list of names is a claim about the past: write `branches: ["**"]`, or no filter at
all. `python3 tools/check_workflows.py triggers` answers the same question from the other side — for the
branch you are standing on, which gates will run, and whether that branch is even on `origin`.

**Generated files are exempt from `R01`, `R04` and `R05`, and only those three.** `.github/workflows/release.yml`
is `cargo-dist`'s output — `dist generate --check` in `ci.yml` refuses a hand edit, and `check_release.py`'s
`R07` requires that gate to exist — so a line added there is a red build on the next `dist` bump, not a fix.
The exemption is keyed to the generator's own banner and to `A02`, which already requires the file's installer
URL to name the version `dist-workspace.toml` pins; it covers the line-shape rules a generator owns (a timeout,
a cache, an artefact step) and nothing else: a generated file is still read for actions, expressions, paths,
cadence and tolerated failures.

`self-test` writes deliberately broken workflows into a temp directory and requires every rule to fire: a
gate that has never said no is a gate that cannot say yes.

Exit: 0 clean, 1 with one line per problem.
"""
from __future__ import annotations

import argparse
import json
import pathlib
import re
import string
import sys
import tomllib
import urllib.error
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[1]
WORKFLOWS = ROOT / ".github" / "workflows"

#: Every third-party action this repository's workflows are allowed to run, and what it is for.
#:
#: The list is the point. GitHub resolves an action reference over the network at run time, so a typo is
#: not a lint error, it is a job that dies before its first step with nothing in the log. Adding a line
#: here is the whole cost of adopting an action; `--network` proves the refs are real. Owner names match
#: case-insensitively because GitHub's do.
ACTION_INVENTORY: dict[str, str] = {
    "actions/checkout": "the clone",
    "actions/setup-python": "the interpreter the python gates run under",
    "actions/upload-artifact": "artefacts",
    "actions/download-artifact": "the scratch artefacts `release.yml` hands between jobs",
    "actions/upload-pages-artifact": "the Pages bundle `docs.yml` builds",
    "actions/deploy-pages": "the Pages deploy",
    "Swatinem/rust-cache": "the cargo cache",
    "EmbarkStudios/cargo-deny-action": "the licence and advisory gate",
    "dtolnay/rust-toolchain": "an explicit toolchain, for a job that must not take `rust-toolchain.toml`",
    "github/codeql-action": "the SARIF uploader in the reusable workflow",
    "rui314/setup-mold": "the linker the fuzzing builds use",
}

#: The properties of a `steps.<id>` object that are not under `outputs`.
STEP_PROPERTIES = {"outputs", "conclusion", "outcome", "name", "id", "key"}

#: A file's own statement that no human writes it. `R01`, `R04` and `R05` stand down in a generated file
#: because the line they want belongs to the generator: hand-editing `.github/workflows/release.yml` to add
#: a `timeout-minutes` is not a fix, it is a red build the next time `dist generate --check` runs.
GENERATED_BY = re.compile(r"#\s*this file was autogenerated by ([a-z0-9-]+)", re.I)

#: The ceiling on a declared timeout, in minutes. The floor is the point of `R01` and the ceiling is its
#: other half: `e11.1` §2.3 found four jobs that had run to GitHub's six-hour default because nobody had
#: written a number, and a declared 300 is the same hole with a paper label on it. Nothing here needs an
#: hour and a half — the slowest measured job in the thirty-day census, hangs aside, was `check-gate` at
#: 17.7 minutes — so anything past two hours is a decision to be argued somewhere other than a YAML key.
TIMEOUT_CEILING_MINUTES = 120

#: The one caching action this repository's jobs use, and the env var that carries the bust lever.
#: `Swatinem/rust-cache` derives its key from the lock files, the toolchain in `rustc -Vv`, the cargo env,
#: the workflow and the job; `prefix-key` is its documented lever for starting a new cache by hand, and
#: `cache-on-failure` is its documented answer to the census's "102 of `ci.yml`'s 233 cache saves never
#: happened because the job failed". *Cache discipline* in docs/concepts/ci-gate.md is the rule those
#: three add up to.
CACHE_ACTION = "Swatinem/rust-cache"
CACHE_BUST_INPUT = "prefix-key"
CACHE_BUST_ENV = "CACHE_BUST"
CACHE_SAVE_ON_FAILURE_INPUT = "cache-on-failure"
#: Anything under these prefixes must not be hand-cached: a second cache over the same paths is a second
#: key over them, and two keys that can disagree is how a warm cache starts lying.
CACHE_OWNED_PATHS = ("~/.cargo", "./target", "target/")
CACHE_FOREIGN_ACTIONS = ("actions/cache",)

#: The artefact budget (`e11.4`, `reco-374v.4`). One row per artefact name a workflow uploads — a name may
#: carry `*`, and the first row that matches a name is the one that governs it — and the row says four
#: things: the size the artefact is allowed to be, how long it is kept, what it *is*, and which paths it is
#: made of. The last two are there because a budget with no description is a number: the thing `R05` can
#: actually refuse is an upload whose paths are not the paths the row describes, which is how an artefact
#: turns into a second, undocumented artefact that happens to share a name.
#:
#: `R05` checks every `upload-artifact` step against its row. The row must exist; no row may describe an
#: artefact nothing uploads (a budget for a ghost is how budgets get ignored); the step must declare
#: `retention-days` at or under the row's; its paths must be the row's paths; and a step before it must
#: measure the bytes against the row's size and fail past it. Change a budget in one commit — here, at the
#: upload, and in *The artefact budget* in docs/concepts/ci-gate.md — because `R05` refuses the three to
#: disagree.
#: The release matrix's artefacts (`artifacts-*`, `cargo-dist-cache`) are deliberately absent: they live in
#: the generated file, they are the shipped product rather than a report about it, and their retention is
#: the GitHub release they are attached to. *The artefact budget* in docs/concepts/ci-gate.md says what
#: they cost instead.
ARTEFACT_BUDGET: dict[str, dict[str, object]] = {
    # pattern: what it is, the size it may be, how long it is kept, and the paths it is made of
    "fathom-check-sarif": {
        "what": "`fathom check` over one range, as SARIF; read once, by whoever the gate objected to",
        "max_mb": 10, "retention_days": 14,
        "paths": ("fathom.sarif",),
    },
    "docs-site-preview": {
        "what": "the rendered docs site, kept as a pull-request preview",
        "max_mb": 50, "retention_days": 7,
        "paths": ("${{ runner.temp }}/docs-site",),
    },
    "privacy-offline": {
        "what": "one JSON document: the egress probe's verdict on the default binary",
        "max_mb": 2, "retention_days": 14,
        "paths": ("artifacts/privacy-offline.json",),
    },
    "fathom-dogfood-corpus": {
        "what": "the weekly gate's reports and SARIF, one pair per repository in the pinned corpus",
        "max_mb": 60, "retention_days": 30,
        "paths": ("fathom-*.check.json", "fathom-*.sarif"),
    },
    "fathom-perf-trend": {
        "what": "the perf trend files; `trend.jsonl` grows one row a day, which is what the 30 days pays for",
        "max_mb": 20, "retention_days": 30,
        "paths": ("benchmarks/trend/latest.json", "benchmarks/trend/trend.jsonl",
                  "benchmarks/trend/trend.md", "benchmarks/trend/latest-summary.txt"),
    },
    "fuzz-nightly-crash-*": {
        "what": "a crash input and its reproducer, uploaded only when a target found one",
        "max_mb": 200, "retention_days": 30,
        "paths": ("fuzz/artifacts/${{ matrix.target }}",),
    },
    "fathom-self-review-*": {
        "what": "what a release published about itself; kept as long as the release it is attached to",
        "max_mb": 30, "retention_days": 90,
        "paths": ("fathom-self-review-*.md", "fathom-doctor-*.json", "self-review.json",
                  "self-check.json", "self-check.sarif", "self-architecture.txt", "self-ratchet.txt",
                  "self-review-status.txt"),
    },
}
#: An upload step's size guard names itself, its bytes, its limit and its producer through these four env
#: keys: they are what `R05` reads, and the reason it can read them without parsing shell.
ARTEFACT_GUARD_KEYS = ("ARTEFACT_NAME", "ARTEFACT_PATH", "ARTEFACT_MAX_KB", "ARTEFACT_PRODUCER")

#: The manifest a `cargo fuzz` invocation run from the repository root ends up reading. The self-test moves
#: this the way it moves `WORKFLOWS`, so the rule that reads it can be shown to fire.
FUZZ_MANIFEST = ROOT / "fuzz" / "Cargo.toml"

#: The ignore file that says which paths this repository does *not* ship. `P01`/`P02` read it to tell a
#: path that was deleted from a path the runner is never given in the first place; the self-test moves it
#: the way it moves `WORKFLOWS`, so the exemption can be shown to be live rather than assumed.
GITIGNORE = ROOT / ".gitignore"

#: `cargo fuzz`, in either spelling of the toolchain pin (`cargo fuzz` / `cargo +nightly fuzz`).
CARGO_FUZZ = re.compile(r"\bcargo\s+(?:\+\S+\s+)?fuzz\b")

#: A `dist build` step and the rest of its line (`cargo dist build` reads as one too).
DIST_BUILD = re.compile(r"(?<![\w-])dist\s+build\b(.*)")
#: The `--artifacts=<mode>` it was given; absent means `host`, the fuzzy "this machine, roughly" mode.
DIST_ARTIFACT_MODE = re.compile(r"--artifacts\b(?:[= ]+([a-z]+))?")
#: The triple the step named, in any spelling (`--target=x`, `--target x`, `"--target=x"`).
DIST_TARGET_FLAG = re.compile(r"--target\b[= ]*\"?([^\s\"]+)")

#: Prefixes that make a token in a shell line a claim about this repository's tree.
REPO_DIRS = ("tools", "ci", "docs", "schemas", "benchmarks", "crates", "plugins", "fuzz",
             "packaging", "sbom", "publish", "assets", ".plan")


# ---------------------------------------------------------------------------
# reading a workflow: the subset of YAML these files use
# ---------------------------------------------------------------------------


def _split_flow(text: str) -> list[str]:
    """Split `[a, b, {c: d}]` on top-level commas."""
    out, depth, quote, current = [], 0, "", []
    for ch in text:
        if quote:
            current.append(ch)
            if ch == quote:
                quote = ""
            continue
        if ch in "\"'":
            quote = ch
            current.append(ch)
        elif ch in "[{":
            depth += 1
            current.append(ch)
        elif ch in "]}":
            depth -= 1
            current.append(ch)
        elif ch == "," and depth == 0:
            out.append("".join(current))
            current = []
        else:
            current.append(ch)
    if "".join(current).strip():
        out.append("".join(current))
    return out


def scalar(token: str):
    token = token.strip()
    if token in ("", "~", "null"):
        return ""
    if token.startswith("[") and token.endswith("]"):
        return [scalar(part) for part in _split_flow(token[1:-1]) if part.strip()]
    if token.startswith("{") and token.endswith("}"):
        out = {}
        for part in _split_flow(token[1:-1]):
            key, _, value = part.partition(":")
            out[scalar(key)] = scalar(value)
        return out
    if len(token) >= 2 and token[0] == token[-1] and token[0] in "\"'":
        return token[1:-1]
    return token


def strip_comment(line: str) -> str:
    """Drop a trailing `# …` that is not inside quotes — `run: sign "$KEY" # note` keeps its `$KEY`."""
    quote = ""
    for i, ch in enumerate(line):
        if quote:
            if ch == quote:
                quote = ""
        elif ch in "\"'":
            quote = ch
        elif ch == "#" and (i == 0 or line[i - 1] in " \t"):
            return line[:i].rstrip()
    return line


class Parser:
    """The YAML these workflows use: block mappings, block sequences, block scalars, flow scalars.

    Not a YAML implementation — PyYAML is not a dependency of this repository's tooling and a workflow
    gate is not the reason to start. What it does have to get right is indentation-as-found (a reformat of
    a file must not blind a rule), `|-`/`>` step bodies, inline comments after a value, and `"on:"` as a
    key.
    """

    def __init__(self, text: str) -> None:
        self.lines = text.splitlines()
        self.i = 0

    def peek(self) -> tuple[int, str] | None:
        while self.i < len(self.lines):
            line = strip_comment(self.lines[self.i])
            if not line.strip():
                self.i += 1
                continue
            return len(line) - len(line.lstrip()), line.strip()
        return None

    def block_scalar(self, indent: int, style: str) -> str:
        """The lines after `key: |` / `key: >-`, folded or literal as GitHub folds them."""
        collected: list[str] = []
        while self.i < len(self.lines):
            line = self.lines[self.i]
            if line.strip() and (len(line) - len(line.lstrip())) <= indent:
                break
            collected.append(line)
            self.i += 1
        while collected and not collected[-1].strip():
            collected.pop()
        width = min((len(l) - len(l.lstrip()) for l in collected if l.strip()), default=0)
        body = [line[width:] if line.strip() else "" for line in collected]
        if style.startswith("|"):
            return "\n".join(body) + ("" if style.endswith("-") else "\n")
        return " ".join(part.strip() for part in body if part.strip()) + ("\n" if not style.endswith("-") else "")

    def plain_scalar(self, indent: int) -> str:
        """A value on the lines *after* `key:`, with no `|` or `>` — folded like a `>` block.

        `ci.yml` writes one clippy step this way, and a reader that mistakes it for a mapping turns the
        second line of a command line into a key of a dict nobody asked for.
        """
        collected: list[str] = []
        while self.i < len(self.lines):
            line = strip_comment(self.lines[self.i])
            if not line.strip() or (len(line) - len(line.lstrip())) <= indent:
                break
            collected.append(line.strip())
            self.i += 1
        return " ".join(collected)

    def parse(self, indent: int):
        first = self.peek()
        if first is None or first[0] < indent:
            return {}
        if first[1].startswith("- "):
            return self.sequence(indent)
        return self.mapping(indent)

    def sequence(self, indent: int) -> list:
        out: list = []
        while True:
            here = self.peek()
            if here is None or here[0] < indent or not here[1].startswith("- "):
                break
            _, content = here
            self.i += 1
            item = content[2:].strip()
            if not item:
                out.append(self.parse(indent + 1))
                continue
            if item.endswith(":") or ":" in item:
                # `- key: value` starts a mapping whose first key sits on the dash line.
                step_indent = indent + 2
                entry: dict = {}
                key, _, value = item.partition(":")
                value = value.strip()
                if value.startswith(("|", ">")):
                    entry[scalar(key)] = self.block_scalar(step_indent, value)
                elif value:
                    entry[scalar(key)] = scalar(value)
                else:
                    nxt = self.peek()
                    if nxt and nxt[0] > step_indent:
                        entry[scalar(key)] = self.parse(nxt[0])
                    elif nxt and nxt[0] == step_indent and nxt[1].endswith(":"):
                        entry[scalar(key)] = self.mapping(nxt[0])
                    else:
                        entry[scalar(key)] = ""
                out.append(entry)
                while True:
                        more = self.peek()
                        if more is None or more[0] != step_indent or more[1].startswith("- "):
                            break
                        key2, _, value2 = more[1].partition(":")
                        value2 = value2.strip()
                        self.i += 1
                        if value2.startswith(("|", ">")):
                            entry[scalar(key2)] = self.block_scalar(step_indent, value2)
                        elif value2:
                            entry[scalar(key2)] = scalar(value2)
                        else:
                            nxt = self.peek()
                            if nxt and nxt[0] > step_indent:
                                entry[scalar(key2)] = (self.plain_scalar(step_indent) if not looks_structural(nxt[1])
                                                        else self.parse(nxt[0]))
                            else:
                                entry[scalar(key2)] = ""
                continue
            out.append(scalar(item))
        return out

    def mapping(self, indent: int) -> dict:
        out: dict = {}
        while True:
            here = self.peek()
            if here is None or here[0] < indent or here[1].startswith("- "):
                break
            if here[0] > indent:
                raise ValueError(f"unexpected indentation at line {self.i + 1}: {here[1][:60]}")
            key, _, value = here[1].partition(":")
            value = value.strip()
            self.i += 1
            if value.startswith(("|", ">")):
                out[scalar(key)] = self.block_scalar(indent, value)
                continue
            if value:
                out[scalar(key)] = scalar(value)
                continue
            nxt = self.peek()
            if nxt is None or nxt[0] <= indent:
                out[scalar(key)] = ""
            elif not looks_structural(nxt[1]) and not nxt[1].startswith("- "):
                out[scalar(key)] = self.plain_scalar(indent)
            else:
                out[scalar(key)] = self.parse(nxt[0])
        return out


def looks_structural(content: str) -> bool:
    """Does this line start a `key:` of its own? `clippy::unwrap_used` must not read as one."""
    return bool(re.match(r"^[^:]+:(\s|$)", content)) or content.endswith(":")


# ---------------------------------------------------------------------------
# the shape a rule wants
# ---------------------------------------------------------------------------


def generated_by(text: str) -> str:
    """What generator wrote this file, from its own banner, or empty when a human did.

    `cargo-dist` writes "# This file was autogenerated by dist" as its first line and `dist generate --check`
    — the gate `ci.yml` runs and `check_release.py`'s `R07` requires — refuses any other content. So the
    banner is not a comment that happens to be there, it is the assertion that no edit below it is possible
    without a red build, and that is the only reason three of the runner-hygiene rules stand down.
    """
    for line in text.splitlines()[:5]:
        match = GENERATED_BY.search(line)
        if match:
            return match.group(1)
    return ""


def mapping_of(value) -> dict[str, str]:
    """A `with:`/`env:` block as strings, whatever the file wrote in it."""
    if isinstance(value, dict):
        return {str(k): str(v) for k, v in value.items()}
    if isinstance(value, str) and value.strip() and value.strip() != "{}":
        # The plain-scalar path in `Parser` can hand back a folded one-liner (`env: FOO: 1`), which is not
        # a mapping; a rule that wants the keys is out of luck and has to say so rather than read nothing.
        return {}
    return {}


class Step:
    def __init__(self, job: "Job", index: int, data: dict) -> None:
        self.job = job
        self.index = index
        self.data = data
        self.id = str(data.get("id") or "")
        self.name = str(data.get("name") or "")
        self.uses = str(data.get("uses") or "")
        self.run = str(data.get("run") or "")
        self.if_text = str(data.get("if") or "")
        self.working_directory = str(data.get("working-directory") or "")
        self.continue_on_error = str(data.get("continue-on-error") or "").strip()
        with_ = data.get("with") or {}
        self.with_: dict[str, str] = {str(k): str(v) for k, v in with_.items()} if isinstance(with_, dict) else {}
        # A guard step declares its numbers under `env:` precisely so that a rule can read them without
        # parsing shell (`R05`); the same block is where a tolerated failure gets its reason in prose.
        self.env = mapping_of(data.get("env"))

    @property
    def where(self) -> str:
        return f"{self.job.workflow.name}: job `{self.job.id}` step {self.index + 1} ({self.label()})"

    @property
    def where_file(self) -> str:
        """`where`, with the file named — for the rules that read a line rather than a structure."""
        return (f"{self.job.workflow.name} (`{self.job.workflow.filename}`): job `{self.job.id}` "
                f"step {self.index + 1} ({self.label()})")

    def label(self) -> str:
        return self.name or self.uses or self.id or "unnamed"

    def text(self) -> str:
        return " ".join([self.run, self.if_text, json.dumps(self.with_), self.uses])


class Job:
    def __init__(self, workflow: "Workflow", job_id: str, data: dict) -> None:
        self.workflow = workflow
        self.id = job_id
        self.data = data
        self.name = str(data.get("name") or job_id)
        self.uses = str(data.get("uses") or "")
        self.if_text = str(data.get("if") or "")
        self.timeout = str(data.get("timeout-minutes") or "").strip()
        self.continue_on_error = str(data.get("continue-on-error") or "").strip()
        needs = data.get("needs") or []
        self.needs = [needs] if isinstance(needs, str) else [str(n) for n in needs]
        outputs = data.get("outputs") or {}
        self.outputs = set(outputs) if isinstance(outputs, dict) else set()
        permissions = data.get("permissions") or {}
        self.permissions = permissions if isinstance(permissions, dict) else {"": str(permissions)}
        steps = data.get("steps") or []
        self.steps = [Step(self, i, s) for i, s in enumerate(steps) if isinstance(s, dict)]


class Workflow:
    def __init__(self, path: pathlib.Path) -> None:
        self.path = path
        self.filename = path.name
        # The file as written, for the rules that read text rather than structure (a `dist` installer URL is
        # a URL, not a key). Parsed and raw are both kept: the parser loses nothing these rules need, but the
        # pin rule wants the string a grep would have found.
        self.text = path.read_text(encoding="utf-8")
        document = Parser(self.text).parse(0)
        self.name = str(document.get("name") or path.name)
        triggers = document.get("on") if "on" in document else document.get(True)
        self.triggers: dict[str, str] = {}
        if isinstance(triggers, dict):
            self.triggers = {str(k): str(v or "") for k, v in triggers.items()}
        elif isinstance(triggers, list):
            self.triggers = {str(t): "" for t in triggers}
        elif triggers:
            self.triggers = {str(triggers): ""}
        # The *tree* under each trigger (`{"push": {"branches": ["**"]}}`), kept beside the stringified
        # `triggers` above rather than replacing it: every other rule wants "does this trigger exist", and
        # only `R07` wants the list a `branches:` key holds — and it has to be a list it can match, not the
        # repr of one. `declared_inputs` re-reads its block out of the text for the same reason; this saves
        # the second parse.
        self.trigger_tree: dict[str, object] = ({str(k): v for k, v in triggers.items()}
                                                if isinstance(triggers, dict) else {})
        permissions = document.get("permissions") or {}
        self.permissions = permissions if isinstance(permissions, dict) else {"": str(permissions)}
        # Workflow-level `env:` and `concurrency:`: one carries the cache bust lever, the other is what `R02`
        # is about. `concurrency` may be a bare group name (`concurrency: pages`), which is legal YAML and
        # means "grouped, never cancelled" — a shape with no `cancel-in-progress` key in it at all.
        self.env = mapping_of(document.get("env"))
        raw_concurrency = document.get("concurrency")
        self.concurrency: dict[str, str] = {}
        if isinstance(raw_concurrency, dict):
            self.concurrency = {str(k): str(v) for k, v in raw_concurrency.items()}
        elif str(raw_concurrency or "").strip():
            self.concurrency = {"group": str(raw_concurrency).strip()}
        self.generated = generated_by(self.text)
        self.jobs: dict[str, Job] = {}
        for job_id, data in (document.get("jobs") or {}).items():
            self.jobs[str(job_id)] = Job(self, str(job_id), data if isinstance(data, dict) else {})

    def declared_inputs(self, trigger: str) -> set[str]:
        """The `inputs:` names one of this workflow's triggers declares."""
        block = {}
        # The trigger's own sub-tree, re-read from the source: `Parser` put it in the value when it was a
        # mapping, and `on:` arrives as `"on"` or as `True` depending on who wrote the file, so the source
        # is the one spelling that is stable.
        match = re.search(rf"^  {trigger}:\n((?:^    .*\n?)*)", self.text, re.M)
        if match:
            block = Parser(match.group(0).replace(f"  {trigger}:", f"{trigger}:")).parse(0).get(trigger) or {}
        if isinstance(block, dict):
            inputs = block.get("inputs") or {}
            if isinstance(inputs, dict):
                return {str(k) for k in inputs}
        return set()

    def run_steps(self) -> list[Step]:
        return [s for job in self.jobs.values() for s in job.steps if s.run]

    def every_step(self) -> list[Step]:
        return [s for job in self.jobs.values() for s in job.steps]


# ---------------------------------------------------------------------------
# the rules
# ---------------------------------------------------------------------------


def rule_actions(wf: Workflow, problems: list[str]) -> None:
    """`A01` — every action reference is well-formed and on the inventory."""
    targets = [(job.uses, f"{wf.name}: job `{job.id}`") for job in wf.jobs.values() if job.uses]
    targets += [(step.uses, step.where) for step in wf.every_step() if step.uses]
    for ref, where in targets:
        if ref.startswith(("./", "docker://")):
            continue
        if "@" not in ref:
            problems.append(f"A01 {where}: `{ref}` names no ref (`owner/repo[/path]@ref`)")
            continue
        path, _, name = ref.partition("@")
        parts = path.split("/")
        if len(parts) < 2:
            problems.append(f"A01 {where}: `{ref}` is not `owner/repo[/path]@ref`")
            continue
        owner, repo = parts[0], parts[1]
        key = next((k for k in ACTION_INVENTORY if k.lower() == f"{owner}/{repo}".lower()), None)
        if key is None:
            problems.append(
                f"A01 {where}: `{ref}` is not on the action inventory. Either it does not exist — and "
                f"the job dies in *Set up job* with no log — or it is new here: prove it resolves "
                f"(`python3 tools/check_workflows.py --network`), then add `{owner}/{repo}` to "
                f"`ACTION_INVENTORY` in `tools/check_workflows.py`"
            )
        elif name not in ("master", "main", "stable") and not re.fullmatch(r"v?\d+(\.\d+)*", name):
            problems.append(
                f"A01 {where}: `{ref}` pins `{key}` to `{name}`; the inventory is keyed by major tag, so "
                f"that a bump is one visible edit rather than a moved branch"
            )


def dist_pin() -> str:
    config = ROOT / "dist-workspace.toml"
    if not config.is_file():
        return ""
    try:
        return str(tomllib.loads(config.read_text(encoding="utf-8"))["dist"]["cargo-dist-version"])
    except (KeyError, TypeError, tomllib.TOMLDecodeError):
        return ""


def rule_dist_pin(wf: Workflow, problems: list[str]) -> None:
    """`A02` — a `dist` installer URL names the pinned version, or derives it at run time."""
    pin = dist_pin()
    if not pin:
        return
    for match in re.finditer(r"cargo-dist/releases/download/v?([^/\s\"']+)/cargo-dist-installer\.sh", wf.text):
        version = match.group(1)
        if "$" in version:
            continue  # derived from the pin when the step runs, which is better than a copy of it
        if version != pin:
            problems.append(
                f"A02 {wf.name}: fetches cargo-dist v{version}; `dist-workspace.toml` pins v{pin}. One "
                f"version, in the config the generated workflow is built from — a `dist` that is not the "
                f"pin generates a workflow the pin then refuses"
            )


def dist_target_count() -> int:
    """How many triples `dist-workspace.toml` asks for; the number in the `D01` sentence."""
    config = ROOT / "dist-workspace.toml"
    try:
        return len(tomllib.loads(config.read_text(encoding="utf-8"))["dist"]["targets"])
    except (OSError, KeyError, TypeError, tomllib.TOMLDecodeError):
        return 0


def rule_dist_targets(wf: Workflow, problems: list[str]) -> None:
    """`D01` — a `dist build` of the *per-target* artefacts has to say which triple it means.

    `--artifacts=local` reads, to whoever writes a step that runs on one machine, as "the local archive".
    To `dist` it means "the target-specific half of the release", and with no `--target` on the command
    line `dist` takes its triples from `dist-workspace.toml` — the whole set — because that reading is
    what makes `--artifacts=global` mean the whole release (`cargo-dist`'s `tasks.rs`: "Otherwise assume
    the user wants all targets"). One runner is not six machines, and the first macOS triple it reaches
    stops the job with

        × Cross-compilation from x86_64-unknown-linux-gnu to aarch64-apple-darwin is not supported

    which is `dist` being right — making Mach-O on Linux is a road paved with sadness — about a target the
    step never asked for. `reco-hzr` is the sighting: `ci.yml`'s packaging job, which needs this runner's
    archive and only this runner's archive, died on it.

    `global` is exempt: the installers, the source tarball and the checksum file are one set for the
    release rather than one per triple, so asking for them without a triple is the request. `host` is
    exempt for the opposite reason — it is the mode that *is* about this machine. A line whose flags
    arrive through `${{ … }}` is exempt too: that is `release.yml`, whose triples come from `dist plan`'s
    own matrix.
    """
    for step in wf.run_steps():
        for line in step.run.splitlines():
            match = DIST_BUILD.search(line)
            if match is None:
                continue
            rest = match.group(1)
            mode = DIST_ARTIFACT_MODE.search(rest)
            if mode is None or (mode.group(1) or "host") not in ("local", "all"):
                continue
            if "${{" in rest:
                continue  # the triple comes from the plan's matrix, which is the other honest way
            named = DIST_TARGET_FLAG.search(rest)
            if named is not None and named.group(1).strip():
                continue
            problems.append(
                f"D01 {step.where}: `dist build --artifacts={mode.group(1)}` names no `--target`, so "
                f"`dist` takes its triples from `dist-workspace.toml` and builds all "
                f"{dist_target_count()} of them on this one runner — the first macOS one stops the job "
                f"with `Cross-compilation from … to aarch64-apple-darwin is not supported`. Name the "
                f"triple this runner is (`--target=\"$(rustc -vV | sed -n 's/^host: //p')\"` next to "
                f"`triple=$(rustc -vV …)`), or the matrix's triple if the step is per-target"
            )


def rule_expressions(wf: Workflow, problems: list[str]) -> None:
    """`E01`–`E04` — every context path in a `${{ … }}` resolves to something this file declares."""
    declared = {job.id: {s.id for s in job.steps if s.id} for job in wf.jobs.values()}
    every_input = wf.declared_inputs("workflow_dispatch") | wf.declared_inputs("workflow_call")
    for job in wf.jobs.values():
        text = " ".join([job.if_text, job.uses, json.dumps(job.data.get("with") or {})] +
                        [step.text() for step in job.steps])
        for match in re.finditer(r"\$\{\{([^}]*)\}\}", text):
            expr = match.group(1)
            for ref in re.finditer(r"\bsteps\.([a-zA-Z0-9_-]+)(?:\.([a-zA-Z0-9_.-]+))?", expr):
                step_id, prop = ref.group(1), ref.group(2)
                if step_id not in declared[job.id]:
                    problems.append(
                        f"E02 {wf.name}: job `{job.id}` reads `steps.{step_id}`, and no step of that job "
                        f"has that `id:` — a step that never published anything, read as the empty string"
                    )
                    continue
                head = (prop or "").split(".")[0]
                if head and head not in STEP_PROPERTIES:
                    problems.append(
                        f"E01 {wf.name}: job `{job.id}` reads `steps.{step_id}.{prop}`; a step output is "
                        f"`steps.{step_id}.outputs.{head}`. GitHub does not raise on a missing context "
                        f"path: it yields the empty string, which is worse than an error"
                    )
            for ref in re.finditer(r"\bneeds\.([a-zA-Z0-9_-]+)(?:\.outputs\.([a-zA-Z0-9_-]+))?", expr):
                need_job, out = ref.group(1), ref.group(2)
                if need_job not in wf.jobs:
                    problems.append(f"E03 {wf.name}: job `{job.id}` reads `needs.{need_job}`; no such job")
                elif out and out not in wf.jobs[need_job].outputs:
                    problems.append(
                        f"E03 {wf.name}: job `{job.id}` reads `needs.{need_job}.outputs.{out}`, which "
                        f"`{need_job}` does not declare under `outputs:`"
                    )
    for match in re.finditer(r"(?<!event\.)\binputs\.([a-zA-Z0-9_-]+)", wf.text):
        name = match.group(1)
        if name not in every_input:
            problems.append(
                f"E04 {wf.name}: reads `inputs.{name}`, which no trigger of this file declares — so it "
                f"is the empty string on every run, whatever the caller passes"
            )


# ---------------------------------------------------------------------------
# .gitignore: the paths the checkout is allowed not to contain
# ---------------------------------------------------------------------------


def _gitignore_regex(pattern: str) -> str:
    """One `.gitignore` pattern body as a regex for a whole repository-relative path.

    `*` and `?` stop at `/`, `**` does not (`**/` eats whole leading directories), `[…]` is a class, and a
    backslash escapes — which is the whole of the syntax these files use, and enough for any `.gitignore`
    a normal repository writes.
    """
    out: list[str] = []
    i, n = 0, len(pattern)
    while i < n:
        char = pattern[i]
        if char == "*" and pattern.startswith("**", i):
            if pattern.startswith("**/", i):
                out.append("(?:.*/)?")
                i += 3
                continue
            out.append(".*")
            i += 2
            continue
        if char == "*":
            out.append("[^/]*")
        elif char == "?":
            out.append("[^/]")
        elif char == "[":
            close = pattern.find("]", i + 1)
            if close < 0:
                out.append(re.escape(char))
                i += 1
                continue
            body = pattern[i + 1 : close].replace("^", r"\^")
            out.append("[" + ("^" + body[1:] if body.startswith("!") else body) + "]")
            i = close + 1
            continue
        elif char == "\\" and i + 1 < n:
            out.append(re.escape(pattern[i + 1]))
            i += 2
            continue
        else:
            out.append(re.escape(char))
        i += 1
    return "".join(out)


def gitignore_rules(path: pathlib.Path) -> list[tuple[bool, bool, bool, re.Pattern]]:
    """The rules in a `.gitignore`, in file order, as `(negated, anchored, directory, pattern)`.

    A pattern with a `/` in it is anchored to the directory the file sits in (here: the repository root);
    one without matches a name at any depth. A trailing `/` means "directory, and everything under it".
    """
    rules: list[tuple[bool, bool, bool, re.Pattern]] = []
    try:
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        return rules  # no ignore file to read (a tarball, the self-test's temp directory): nothing is ignored
    for line in lines:
        line = line.rstrip()
        if not line.startswith("#") and line.strip():
            negated = line.startswith("!")
            body = line[1:] if negated else line
            directory = body.endswith("/")
            # A leading `/` anchors the pattern to this file's directory, exactly as a `/` anywhere else in
            # it does; git's `/target/` is the root's `target/`, not every crate's.
            anchored = body.startswith("/") or "/" in body.lstrip("/").rstrip("/")
            body = body.lstrip("/").rstrip("/")
            if body:
                rules.append((negated, anchored, directory, re.compile(_gitignore_regex(body))))
    return rules


def path_is_ignored(rel: str, rules: list[tuple[bool, bool, bool, re.Pattern]] | None = None) -> bool:
    """Whether `.gitignore` says `rel` (repository-relative) is not part of this repository.

    Two rules from git's own semantics carry the weight here: the *last* matching line decides, and an
    excluded directory excludes everything beneath it with no way back — so an ancestor that matches a
    plain directory rule settles the answer (`benchmarks/trend/` under `.gitignore` is what makes the
    trend file not a claim about the tree).
    """
    if rules is None:
        rules = gitignore_rules(GITIGNORE)
    parts = rel.strip("/").split("/")
    for index, part in enumerate(parts[:-1]):
        prefix = "/".join(parts[: index + 1])
        for negated, anchored, directory, pattern in rules:
            if not negated and directory and pattern.fullmatch(prefix if anchored else part):
                return True
    ignored = False
    for negated, anchored, directory, pattern in rules:
        # A directory rule is allowed to answer for a name that is not here: "not here" is the case being asked
        # about, and `benchmarks/trend/` has to count whether or not somebody ran the perf gate locally.
        if pattern.fullmatch(rel.strip("/") if anchored else parts[-1]):
            ignored = not negated
    return ignored


def will_be_in_checkout(token: str, rules: list[tuple[bool, bool, bool, re.Pattern]] | None = None) -> bool:
    """Whether the path a step names is something the runner will have.

    The question `P01` and `P02` actually ask is about the checkout `actions/checkout` produces, not about
    whatever happens to be on the machine that last ran a build. So a token is fine when it is here, and
    also when `.gitignore` says it is not part of the repository: such a path is build or run output —
    `benchmarks/trend/trend.md`, written by the perf gate and uploaded as a job artefact — which is
    exactly the path a step has to guard for, and exactly the path a gate that only looks at the working
    tree flags on the one machine out of eleven runs where it exists.
    """
    if (ROOT / token).exists():
        return True
    return path_is_ignored(token, rules)


def referenced_repo_paths(shell: str) -> list[str]:
    """Tokens in a step's shell that are shaped like paths into this repository."""
    out: list[str] = []
    for token in re.findall(r"[A-Za-z0-9_.\-][A-Za-z0-9_.\-/]*", shell):
        if token.startswith(("./", "../", "http://", "https://")) or "$" in token:
            continue
        if not token.startswith(tuple(f"{d}/" for d in REPO_DIRS)):
            continue
        if not re.search(r"[./][A-Za-z0-9_][A-Za-z0-9_.\-]*$", token):
            continue  # `crates/` alone, or a directory in prose
        out.append(token)
    return out


def rule_paths(wf: Workflow, problems: list[str]) -> None:
    """`P01`, `P02`, `P03` — the tree the steps name is the tree the runner gets."""
    rules = gitignore_rules(GITIGNORE)
    for step in wf.run_steps():
        if step.working_directory and not (ROOT / step.working_directory).is_dir():
            if not path_is_ignored(step.working_directory, rules):
                problems.append(
                    f"P02 {step.where}: `working-directory: {step.working_directory}` is not a directory"
                )
        if CARGO_FUZZ.search(step.run) and step.working_directory:
            problems.append(
                f"P03 {step.where}: runs `cargo fuzz` from `{step.working_directory}`. cargo-fuzz derives "
                f"its fuzz directory from the package it finds in the working directory, and only the "
                f"`cargo-fuzz = true` marker in `fuzz/Cargo.toml` (`P04`) is what makes it walk back out "
                f"of `fuzz/` instead of looking for `fuzz/fuzz/`. Every other path in the step — "
                f"`fuzz/artifacts/<target>`, the seed copy — is written from the root anyway, so run it "
                f"from the repository root"
            )
        for token in referenced_repo_paths(step.run):
            if not will_be_in_checkout(token, rules):
                problems.append(f"P01 {step.where}: names `{token}`, which does not exist")


def fuzz_manifest_problem(path: pathlib.Path) -> str:
    """Empty when cargo-fuzz would accept `path` as the fuzz workspace's manifest; the reason when not.

    The check copies cargo-fuzz's own `is_fuzz_manifest`: a `[package]` table whose `metadata` table says
    `cargo-fuzz = true`. Nothing else about the manifest matters to that check — which is exactly why a
    fuzz workspace that builds perfectly on a laptop can still be refused on the first line of a CI run.
    """
    if not path.is_file():
        return f"{path} does not exist"
    try:
        data = tomllib.loads(path.read_text(encoding="utf-8"))
    except tomllib.TOMLDecodeError as exc:
        return f"{path} is not parseable TOML ({exc})"
    package = data.get("package")
    if not isinstance(package, dict):
        return f"{path} has no `[package]` table, so it is a virtual manifest, not a fuzz workspace"
    metadata = package.get("metadata")
    if not isinstance(metadata, dict):
        return f"{path} has no `[package.metadata]` table"
    if metadata.get("cargo-fuzz") is not True:
        return f"{path} does not say `cargo-fuzz = true` under `[package.metadata]`"
    return ""


def rule_fuzz_manifest(wf: Workflow, problems: list[str]) -> None:
    """`P04` — a workflow that runs `cargo fuzz` needs a manifest cargo-fuzz recognises.

    cargo-fuzz reads this before it compiles a single crate, and the whole of its complaint is three lines:

        Error: manifest fuzz/Cargo.toml does not look like a cargo-fuzz manifest. Add following lines to
        override:
        [package.metadata]
        cargo-fuzz = true

    which is what every job of `.github/workflows/fuzz.yml` printed (`reco-com`) while the marker existed
    nowhere in this repository. The marker is invisible to every other tool — `cargo build`, `cargo clippy`,
    `cargo deny` and this repository's own corpus gate all read the manifest happily without it — so it can
    go missing in a refactor that leaves nothing else red.
    """
    if not any(CARGO_FUZZ.search(step.run) for step in wf.run_steps()):
        return
    problem = fuzz_manifest_problem(FUZZ_MANIFEST)
    if problem:
        problems.append(
            f"P04 {wf.name}: runs `cargo fuzz`, but {problem}, so cargo-fuzz stops with `does not look "
            f"like a cargo-fuzz manifest` before it builds a target"
        )


def tool_subcommands(tool: pathlib.Path) -> set[str] | None:
    """The subcommands a tool offers, or None when it offers none.

    Three spellings, because the repository's tools are old and were not all written by the same hand:
    `argparse`'s `add_parser`/`choices=`, and the two bare-`sys.argv` idioms that predate them.
    """
    text = tool.read_text(encoding="utf-8", errors="replace")
    names = set(re.findall(r"add_parser\(\s*[\"']([^\"']+)[\"']", text))
    for choices in re.findall(r"choices\s*=\s*[\[\(\{]([^\]\)\}]*)[\]\)\}]", text):
        names |= {c.strip().strip("\"'") for c in choices.split(",") if c.strip()}
    names |= set(re.findall(r"sys\.argv\[1\]\s*(?:==|!=)\s*[\"']([a-z][a-z-]*)[\"']", text))
    names |= set(re.findall(r"args\.(?:mode|command|verb|subcommand)\s*==\s*[\"']([a-z][a-z-]*)[\"']", text))
    return {n for n in names if n and all(c in string.ascii_lowercase + "-0123456789." for c in n)} or None


def rule_tool_calls(wf: Workflow, problems: list[str]) -> None:
    """`T01` — a tool invoked by name is invoked with a subcommand it has."""
    for step in wf.run_steps():
        # Per line, never across lines: `python3 tools/a.py\npython3 tools/b.py check` is two commands,
        # and a reader that lets a newline stand for a space reads the second one's `python` as the
        # first one's subcommand.
        for line in step.run.splitlines():
            for match in re.finditer(r"python3?\s+(?:-\S+\s+)*(tools/[a-z_0-9]+\.py)\s+([a-z][a-z-]*)?", line):
                tool_rel, word = match.group(1), match.group(2)
                tool = ROOT / tool_rel
                if not tool.is_file() or not word:
                    continue
                offered = tool_subcommands(tool)
                if offered and word not in offered:
                    problems.append(
                        f"T01 {step.where}: `{tool_rel} {word}`; its argparse offers "
                        f"{', '.join(sorted(offered))}"
                    )


def workspace_members() -> dict[str, dict]:
    """Each workspace member's directory, features and target names, read out of the manifests."""
    root = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    members: dict[str, dict] = {}
    for entry in root.get("workspace", {}).get("members", []):
        for path in sorted(ROOT.glob(entry)):
            manifest = path / "Cargo.toml"
            if not manifest.is_file():
                continue
            data = tomllib.loads(manifest.read_text(encoding="utf-8"))
            name = data.get("package", {}).get("name")
            if not name:
                continue
            features = set(data.get("features", {}))
            for spec in data.get("dependencies", {}).values():
                if isinstance(spec, dict) and spec.get("optional"):
                    features.add(str(spec.get("package") or spec.get("path") or "").rsplit("/", 1)[-1])
            members[name] = {
                "dir": path,
                "features": features,
                "tests": {p.stem for p in (path / "tests").glob("*.rs")} if (path / "tests").is_dir() else set(),
                "examples": {p.stem for p in (path / "examples").glob("*.rs")} if (path / "examples").is_dir() else set(),
            }
    return members


def cargo_invocations(shell: str) -> list[tuple[str, str]]:
    """`(verb, rest)` for each `cargo <verb>` in a step's shell, one line at a time."""
    out = []
    for line in shell.splitlines():
        out.extend((m.group(1), m.group(2)) for m in re.finditer(
            r"cargo\s+(?:\+\S+\s+)?(build|test|run|check|clippy)\b(.*)", line))
    return out


def rule_cargo_calls(wf: Workflow, members: dict[str, dict], problems: list[str]) -> None:
    """`T02`–`T04` — the packages, features and targets a `cargo` step names exist."""
    for step in wf.run_steps():
        if "--manifest-path" in step.run:
            continue  # another workspace's manifest (`plugins/acme-detectors`); not this member list
        for _verb, rest in cargo_invocations(step.run):
            tokens = rest.split()
            package, features, tests, examples = "", [], [], []
            for i, token in enumerate(tokens):
                nxt = tokens[i + 1] if i + 1 < len(tokens) else ""
                if token in ("-p", "--package"):
                    package = nxt
                elif token == "--features":
                    features += [f for f in nxt.split(",") if f]
                elif token == "--test":
                    tests.append(nxt)
                elif token == "--example":
                    examples.append(nxt)
            if package and package not in members:
                problems.append(
                    f"T02 {step.where}: `-p {package}` is not a member of this workspace "
                    f"({', '.join(sorted(members))})"
                )
                continue
            if not package:
                continue
            scope = members[package]
            unknown = [f for f in features if f not in scope["features"]]
            if unknown:
                problems.append(
                    f"T03 {step.where}: asks `{package}` for "
                    f"{', '.join(unknown)}, which its manifest does not declare"
                )
            for name in tests:
                if name and name not in scope["tests"]:
                    problems.append(
                        f"T04 {step.where}: `--test {name}`; `{package}` has no tests/{name}.rs"
                    )
            for name in examples:
                if name and name not in scope["examples"]:
                    problems.append(
                        f"T04 {step.where}: `--example {name}`; `{package}` has no examples/{name}.rs"
                    )


def rule_job_wiring(wf: Workflow, problems: list[str]) -> None:
    """`J01`, `J02`."""
    if not wf.permissions and not any(job.permissions for job in wf.jobs.values()):
        problems.append(
            f"J02 {wf.name}: no `permissions:` anywhere, so every job runs with whatever the "
            f"organisation defaults to today — a policy written outside this repository"
        )
    for job in wf.jobs.values():
        for need in job.needs:
            if need and need not in wf.jobs:
                problems.append(f"J01 {wf.name}: job `{job.id}` needs `{need}`, which does not exist")


def _matches(pattern: str, name: str) -> bool:
    regex = "^" + re.escape(pattern).replace("\\*", ".*").replace("\\?", ".") + "$"
    return bool(re.match(regex, name))


def rule_artifacts(wf: Workflow, problems: list[str]) -> None:
    """`J03` — a name uploaded once, and a download pattern that matches an upload in the same file."""
    uploads: list[str] = []
    downloads: list[str] = []
    for step in wf.every_step():
        if "upload-artifact" in step.uses:
            uploads.append(str(step.with_.get("name", "")))
        if "download-artifact" in step.uses:
            downloads.append(str(step.with_.get("pattern") or step.with_.get("name") or ""))
    def render(name: str) -> str:
        return re.sub(r"\$\{\{[^}]*\}\}", "*", name)
    seen: set[str] = set()
    for name in uploads:
        rendered = render(name)
        if rendered and rendered in seen and "*" not in rendered:
            problems.append(
                f"J03 {wf.name}: two steps upload artefact `{name}`; v4 fails the second one — an "
                f"artefact name has to be unique in a run"
            )
        seen.add(rendered)
    for pattern in downloads:
        if not pattern:
            continue
        if not any(_matches(render(pattern), render(up)) for up in uploads):
            problems.append(
                f"J03 {wf.name}: `download-artifact` asks for `{pattern}`, which no `upload-artifact` "
                f"step in this workflow publishes"
            )


# ---------------------------------------------------------------------------
# runner hygiene: the timeout, the queue, the tolerated failure, the cache, the budget
#
# All six rules here come out of one measurement (`e11.1`, `.plan/ci-triage-reco-374v.md`): thirty days of
# this fleet, read to the step. 43 % of the runner-minutes in that window went to four jobs that had run to
# GitHub's six-hour default because no job in `ci.yml` declared a timeout; 7 more red runs were runs merely
# superseded by a push nobody had cancelled them for; 102 of `ci.yml`'s 233 cargo-cache saves never happened
# because the job was red when the cache would have been written; and the two steps that were
# `continue-on-error` had been read by nobody. `R06` is the one rule here that comes from the following
# ticket rather than from the census: `e11.2` moved the long tail off the pull request, and a step can walk
# back without anybody noticing unless something is watching the cadence.
#
# They are `R` for *runner*, and every one of them is about the shape of a job rather than about what the job
# runs: nothing in this section cares whether a step's command is correct.
# ---------------------------------------------------------------------------

#: The one comment shape a tolerated failure may be explained in, and the words that make an explanation a
#: reason rather than a shrug. `R02` and `R03` are the only readers, and *Tolerated failures* in
#: docs/concepts/ci-gate.md is the prose.
TOLERANCE_NOTE = re.compile(r"^\s*#\s*tolerates:\s*(?P<body>.+)$", re.I)
TOLERANCE_CAUSE = re.compile(r"\b(because|since|so that|which is why)\b", re.I)
#: How far above its tolerance a note has to sit, in lines, and how much it has to say.
TOLERANCE_WINDOW = 12
TOLERANCE_MIN_CHARS = 40

#: A step that makes the compiler work, as opposed to asking it a question. `cargo fmt`, `cargo metadata`
#: and `cargo install` are not in the list: the first two compile nothing of the workspace, and the third
#: builds tooling that has no business in a workspace cache.
CACHE_COMPILES = re.compile(r"cargo\s+(?:\+\S+\s+)?(build|test|check|clippy|run|fuzz)\b")
#: A second workspace, named one of the two ways a step can name it.
CACHE_MANIFEST_FLAG = re.compile(r"--manifest-path\s+[\"']?(\S+?)/Cargo\.toml")
#: The one spelling of the bust lever `R04` accepts: the lever is a workflow-level `env:` line rather than a
#: string copy-pasted into every cache step, so that a bust is one visible edit in one place per file.
CACHE_BUST_INPUT_VALUE = re.compile(r"^\$\{\{\s*env\." + CACHE_BUST_ENV + r"\s*\}\}$")
#: A bust value is a date, because the honest question when one is raised is "when, and what moved it", and
#: a bare number answers neither. `2026-11`, or `2026-11.2` for a second bust in the same month.
CACHE_BUST_VALUE = re.compile(r"^\d{4}-\d{2}(\.\d+)?$")

#: What a per-*thing* concurrency group has to name. `github.workflow` keeps two workflows out of one queue;
#: the second keeps two pull requests, or two branches, out of one queue with each other.
CONCURRENCY_PER_REF = ("github.ref", "github.event.pull_request.number", "github.head_ref")

#: The one `push:` branch filter `R07` reads as "every branch the work can be on". `*` is deliberately not a
#: catch-all: GitHub's branch globs stop at a `/`, so `*` describes `master` and `trunk` and refuses
#: `feature/more_functionality` — the exact shape of name the work happens on here.
BRANCH_CATCH_ALL = "**"


def branch_glob_re(pattern: str) -> re.Pattern:
    """One GitHub branch-glob pattern as a regex, with `/` a boundary that only `**` crosses.

    Not `_glob_re` above, which is for artefact *paths* and lets `*` cross a separator on purpose: a branch
    filter that read `*` as `**` would wave through `branches: [*]` and still never match
    `feature/more_functionality`, which is the shape this rule exists to refuse. `?` is one character short
    of a separator, `+` is GitHub's "any set of characters, `/` included", and a leading `!` is handled by
    the caller because it is a veto over the whole list rather than one pattern's property.
    """
    out: list[str] = []
    i = 0
    while i < len(pattern):
        if pattern[i:i + 2] == "**":
            out.append(".*")
            i += 2
            continue
        out.append({"*": "[^/]*", "?": "[^/]", "+": ".+"}.get(pattern[i]) or re.escape(pattern[i]))
        i += 1
    return re.compile("".join(out))


def branch_glob_matches(pattern: str, branch: str) -> bool:
    """Whether one glob — no leading `!`, that is the caller's business — names one branch."""
    return bool(branch_glob_re(pattern).fullmatch(branch))


def branch_pattern_matches(pattern: str, branch: str) -> bool:
    """Whether one `branches:` entry admits one branch name, `!negation` included.

    A `!` entry admits every branch except the ones its glob names — that is what a negated entry *is* — and
    `branch_filter_matches` is where it becomes a veto over the whole list rather than a rule of its own.
    """
    if pattern.startswith("!"):
        return not branch_glob_matches(pattern[1:], branch)
    return branch_glob_matches(pattern, branch)


def branch_filter_matches(patterns: list[str], branch: str) -> bool:
    """GitHub's answer for a whole `branches:` (or `branches-ignore:`) list.

    A `!` entry vetoes whatever else matched; an all-positive list admits only what one of its globs names;
    an empty list admits everything.
    """
    if any(p.startswith("!") and branch_glob_matches(p[1:], branch) for p in patterns):
        return False
    positives = [p for p in patterns if not p.startswith("!")]
    return not positives or any(branch_glob_matches(p, branch) for p in positives)


def trigger_lists(wf: Workflow, trigger: str, key: str) -> list[str]:
    """One list (`branches:`, `branches-ignore:`, `paths:`) out of one trigger's block.

    GitHub accepts a bare scalar where it accepts a list — `push: branches: master` is legal, and it is the
    most literal spelling of the mistake `R07` refuses — so a scalar arrives as a one-item list.
    """
    block = wf.trigger_tree.get(trigger)
    if not isinstance(block, dict):
        return []
    for name, value in block.items():
        if str(name) != key:
            continue
        if isinstance(value, list):
            return [str(v) for v in value]
        if value in (None, "", "{}"):
            return []
        return [str(value)]
    return []


def push_filter(wf: Workflow) -> tuple[list[str], list[str]] | None:
    """The workflow's `push:` branch filter, or `None` when it has no `push:` trigger at all."""
    if "push" not in wf.triggers:
        return None
    return trigger_lists(wf, "push", "branches"), trigger_lists(wf, "push", "branches-ignore")


def filter_reaches_every_branch(branches: list[str], ignores: list[str]) -> bool:
    """Whether a filter admits every branch there is, which is the only answer `R07` accepts.

    Not "admits the branches that exist today" — a filter narrowed to names is a claim about where the work
    was, and the census's whole finding was that a claim like that goes false in silence.
    """
    if ignores:
        return False
    if not branches:
        return True
    return BRANCH_CATCH_ALL in branches and not any(b.startswith("!") for b in branches)


def push_reaches(wf: Workflow, branch: str) -> bool:
    """Whether a push of `branch` starts a run of this workflow."""
    seen = push_filter(wf)
    if seen is None:
        return False
    branches, ignores = seen
    if ignores and branch_filter_matches(ignores, branch):
        return False
    return branch_filter_matches(branches, branch)


def is_merge_gate(wf: Workflow) -> bool:
    """Whether this workflow answers "may this merge?" for *any* pull request.

    A `pull_request:` trigger narrowed by `paths:` is not that: it is a specialist that was already priced
    by a decision about which files it is about, and `R06` and `C01` hold it to its cadence. The one that
    fires on every pull request is the gate, and a gate a pushed branch cannot reach is a gate nobody sees.
    """
    return "pull_request" in wf.triggers and not trigger_lists(wf, "pull_request", "paths")


def render_artefact(name: str) -> str:
    """An artefact name with its `${{ … }}` parts replaced by the `*` the budget table is keyed with."""
    return re.sub(r"\$\{\{[^}]*\}\}", "*", name)


def budget_for(name: str) -> tuple[str, dict[str, object]] | None:
    """The budget row that governs an artefact name, or None when nothing budgets it."""
    rendered = render_artefact(name)
    for pattern, row in ARTEFACT_BUDGET.items():
        if _matches(pattern, rendered):
            return pattern, row
    return None


def _glob_re(pattern: str) -> re.Pattern:
    """A path pattern as a regex, with `*` and `**` both allowed to cross a directory separator.

    Deliberately lax about `*` versus `**`: every pattern this table holds comes from a workflow that wrote
    `${{ matrix.target }}` (rendered to `*`) or a shell glob, and the question asked of them is only "is this
    the file the row is talking about".
    """
    return re.compile("".join(".*" if ch == "*" else "." if ch == "?" else re.escape(ch)
                                for ch in pattern.strip().strip("/")) + "(?:/.*)?")


def paths_describe(row_paths: list[str], uploaded: str) -> bool:
    """Whether a budget row describes one uploaded path — by name, by glob, or by being its parent directory."""
    rendered = uploaded.strip().strip("/")
    for entry in row_paths:
        entry = entry.strip().strip("/")
        if entry == rendered or _glob_re(entry).match(rendered) or _glob_re(rendered).match(entry):
            return True
    return False


def paths_agree(left: list[str], right: list[str]) -> bool:
    """Whether two spellings of "the bytes in this artefact" name the same set of paths."""
    clean = [p.strip().strip("/") for p in left if p.strip()]
    other = [p.strip().strip("/") for p in right if p.strip()]
    return (len(clean) == len(other)
            and all(any(paths_describe([b], a) for b in other) for a in clean))


def _normalise_if(text: str) -> str:
    """`if: ${{ always() }}` and `if: always()` are the same condition."""
    return re.sub(r"[\s{}$]", "", text or "")


def _line_context(lines: list[str], index: int) -> str:
    """The step or job a source line belongs to, for a message that has to quote a line number."""
    for back in range(index, -1, -1):
        step = re.match(r"^\s*(?:-\s+)?(?:name|uses):\s*(.+)$", lines[back])
        if step:
            return f"step `{step.group(1).strip()[:56]}`"
        job = re.match(r"^  ([a-zA-Z0-9_-]+):\s*(?:#.*)?$", lines[back])
        if job:
            return f"job `{job.group(1)}`"
    return "the workflow"


def tolerance_sites(wf: Workflow) -> list[tuple[int, str, str]]:
    """Every `(line, key, value)` in a file where a failure is allowed to be tolerated.

    Read from the source and not from the parsed jobs and steps because these rules are about a *comment*
    above a key, and a comment is exactly what a YAML parser does not keep. A commented-out line is skipped,
    so a rule cannot be satisfied by a tolerance somebody deleted and left behind as prose.
    """
    sites: list[tuple[int, str, str]] = []
    for index, raw in enumerate(wf.text.splitlines()):
        if raw.lstrip().startswith("#"):
            continue
        line = strip_comment(raw)
        for key in ("continue-on-error", "cancel-in-progress"):
            match = re.match(rf"^\s*{key}\s*:\s*(.+)$", line)
            if match is None:
                continue
            value = match.group(1).strip()
            if key == "continue-on-error" and value.lower() in ("false", "~", ""):
                continue
            if key == "cancel-in-progress" and value.lower() == "true":
                continue
            sites.append((index + 1, key, value))
    return sites


def tolerance_notes(wf: Workflow) -> list[tuple[int, str]]:
    """Every `(line, body)` `tolerates:` note in a file."""
    out: list[tuple[int, str]] = []
    for index, raw in enumerate(wf.text.splitlines()):
        match = TOLERANCE_NOTE.match(raw)
        if match:
            out.append((index + 1, match.group("body").strip()))
    return out


def _note_above(notes: list[tuple[int, str]], lineno: int) -> str:
    """The body of the nearest `tolerates:` note within the window above a tolerance, or empty."""
    near = [body for line, body in notes if line < lineno and lineno - line <= TOLERANCE_WINDOW]
    return near[-1] if near else ""


def rule_timeouts(wf: Workflow, problems: list[str]) -> None:
    """`R01` — every job says how long it is allowed to take."""
    if wf.generated:
        return  # `dist`'s output; see the module docstring and *Timeouts* in docs/concepts/ci-gate.md
    for job in wf.jobs.values():
        where = f"{wf.name} (`{wf.filename}`): job `{job.id}`"
        if not job.timeout:
            problems.append(
                f"R01 {where}: no `timeout-minutes`, so a hung cargo or a hung git holds a runner for "
                f"GitHub's six-hour default and looks exactly like a flake. `e11.1` §2.3: four jobs ran to "
                f"360 minutes and were then cancelled, which was 43 % of a month's runner-minutes. Set the "
                f"number from the census (`.plan/ci-triage-reco-374v.md`, per-job `dur_min_p95`) with room "
                f"above it — five times the p95 is a good rule of thumb, and a too-small number is a new "
                f"class of red"
            )
            continue
        if not job.timeout.isdigit():
            problems.append(f"R01 {where}: `timeout-minutes: {job.timeout}` is not a whole number of minutes")
        elif int(job.timeout) > TIMEOUT_CEILING_MINUTES:
            problems.append(
                f"R01 {where}: `timeout-minutes: {job.timeout}` is past the "
                f"{TIMEOUT_CEILING_MINUTES}-minute ceiling. The slowest finished job in the thirty-day "
                f"census took 17.7 minutes; a job that wants hours is a job whose *shape* is the news, and "
                f"that sentence belongs in a document rather than in a YAML key"
            )


def rule_concurrency(wf: Workflow, problems: list[str]) -> None:
    """`R02` — a pull request cancels the run it has already made obsolete."""
    if "pull_request" not in wf.triggers:
        return
    name = f"{wf.name} (`{wf.filename}`)"
    notes = tolerance_notes(wf)
    if not wf.concurrency:
        problems.append(
            f"R02 {name}: triggers on `pull_request` with no `concurrency:` group, so a pushed branch pays "
            f"for every superseded run in full — `e11.1` §2.4 counted 7 red runs whose only fault was being "
            f"superseded. Three lines fix it:\n"
            f"      concurrency:\n"
            f"        group: ${{{{ github.workflow }}}}-${{{{ github.event.pull_request.number || github.ref }}}}\n"
            f"        cancel-in-progress: true"
        )
        return
    group = wf.concurrency.get("group", "").strip()
    if not group:
        problems.append(f"R02 {name}: a `concurrency:` block with no `group:` in it")
    else:
        if "github.workflow" not in group:
            problems.append(
                f"R02 {name}: `concurrency: {group}` does not name `github.workflow`, so this workflow "
                f"shares a queue with whatever else spells the group the same — and cancels it"
            )
        if not any(token in group for token in CONCURRENCY_PER_REF):
            problems.append(
                f"R02 {name}: `concurrency: {group}` is not per-ref, so two pull requests share one queue "
                f"and the second cancels the first's run. Name "
                f"`${{{{ github.event.pull_request.number || github.ref }}}}`"
            )
    cancel = wf.concurrency.get("cancel-in-progress", "").strip()
    if cancel.lower() == "true":
        return
    lineno = next((line for line, key, _ in tolerance_sites(wf) if key == "cancel-in-progress"), 0)
    if "pull_request" not in cancel:
        problems.append(
            f"R02 {name}: `cancel-in-progress: {cancel or '(absent)'}` lets a superseded pull-request run "
            f"finish. Either say `true`, or write `${{{{ github.event_name == 'pull_request' }}}}` if a push "
            f"has a reason not to be cancelled — and write that reason in a `# tolerates:` note (`R03`)"
        )
    elif not _note_above(notes, lineno):
        problems.append(
            f"R02 {name} line {lineno}: `cancel-in-progress: {cancel}` cancels on a pull request and "
            f"nowhere else, which is a tolerated failure of a different shape — name what it tolerates and "
            f"why in a `# tolerates:` note above it"
        )


def rule_tolerated_failures(wf: Workflow, problems: list[str]) -> None:
    """`R03` — an allowed failure that is not named is a lie about the gate."""
    lines = wf.text.splitlines()
    sites = tolerance_sites(wf)
    notes = tolerance_notes(wf)
    for lineno, key, value in sites:
        if key != "continue-on-error":
            continue
        if not _note_above(notes, lineno):
            problems.append(
                f"R03 {wf.name} (`{wf.filename}`) line {lineno}: `{key}: {value}` in "
                f"{_line_context(lines, lineno - 1)} tolerates a failure without saying which one or why. An "
                f"allowed failure that is never read is a lie about the gate — `e11.1` §2.7 spent 778 minutes "
                f"on a job whose red could not matter. Either fail the step, or name the tolerance in "
                f"`# tolerates: <what> because <why>` above it and make the step write the failure to "
                f"`$GITHUB_STEP_SUMMARY`, so that someone reads it"
            )
    for lineno, body in notes:
        if len(body) < TOLERANCE_MIN_CHARS or not TOLERANCE_CAUSE.search(body):
            problems.append(
                f"R03 {wf.name} (`{wf.filename}`) line {lineno}: the `tolerates:` note “{body[:64]}” says "
                f"what but not why; it has to reach “because”, “since” or “so that” and carry at least "
                f"{TOLERANCE_MIN_CHARS} characters, or it is a label and not a reason"
            )
        if not [line for line, _key, _value in sites if lineno < line <= lineno + TOLERANCE_WINDOW]:
            problems.append(
                f"R03 {wf.name} (`{wf.filename}`) line {lineno}: a `tolerates:` note with nothing to "
                f"tolerate within {TOLERANCE_WINDOW} lines below it. The tolerance is gone, so the excuse has "
                f"to go with it — an explanation of a tolerance nobody still has reads exactly like one "
                f"nobody has ever read"
            )


def rule_cache(wf: Workflow, problems: list[str]) -> None:
    """`R04` — one caching action, a derived key, one documented lever for busting it, and it saves when red.

    The rule, in the four sentences *Cache discipline* in docs/concepts/ci-gate.md prints:

    * every job that *compiles* the workspace restores the cache before its first compiling step, because a
      cache restored late caches only what the job had already built;
    * nobody writes a cache key. `Swatinem/rust-cache` derives one from the lock files, the toolchain in
      `rustc -Vv`, the cargo env, the workflow and the job — which covers every input a cargo build reads,
      and a hand-written key is a key that will not be updated when the derivation grows;
    * the one hand-written lever is the *prefix*, taken from `env.CACHE_BUST`, and it is for bytes that would
      be **wrong**, not merely stale (the comment required on that env line says where the derivation is
      blind: the registry, and the action's own layout — not the source, which cargo fingerprints);
    * `cache-on-failure: "true"`, because a red job's compiled objects are still correct objects, and the
      census counted 102 of `ci.yml`'s 233 saves that never happened for no other reason.
    """
    if wf.generated:
        return
    cached_any = False
    for job in wf.jobs.values():
        compiles = [s for s in job.steps if CACHE_COMPILES.search(s.run)]
        caches = [s for s in job.steps if CACHE_ACTION in s.uses]
        cached_any = cached_any or bool(caches)
        for step in job.steps:
            for action in CACHE_FOREIGN_ACTIONS:
                if step.uses.startswith(action):
                    problems.append(
                        f"R04 {step.where_file}: `{step.uses}` hand-caches cargo paths. One caching action per "
                        f"repository, and it is `{CACHE_ACTION}`: two actions over the same directories are "
                        f"two keys over them, and two keys that can disagree is how a warm cache starts "
                        f"lying about what it holds"
                    )
        if compiles and not caches:
            verbs = sorted({m.group(1) for s in compiles for m in [CACHE_COMPILES.search(s.run)] if m})
            problems.append(
                f"R04 {wf.name} (`{wf.filename}`): job `{job.id}` compiles in {len(compiles)} step(s) "
                f"(`cargo {', '.join(verbs)}`) and restores no cargo cache, so every run pays for the "
                f"dependency graph from scratch and then throws it away"
            )
        elif compiles and caches and min(s.index for s in caches) > min(s.index for s in compiles):
            problems.append(
                f"R04 {wf.name} (`{wf.filename}`): job `{job.id}` restores the cargo cache in step "
                f"{min(s.index for s in caches) + 1}, after its first compiling step "
                f"(step {min(s.index for s in compiles) + 1}), so the bytes it saves are the ones the job "
                f"had already built. Restore it first — and after the toolchain it wants, because the action "
                f"hashes `rustc -Vv` as found when the step runs"
            )
        for step in caches:
            bust = step.with_.get(CACHE_BUST_INPUT, "")
            if not CACHE_BUST_INPUT_VALUE.match(bust.strip()):
                problems.append(
                    f"R04 {step.where_file}: `{CACHE_BUST_INPUT}: {bust or '(unset)'}`. The bust lever is "
                    f"`${{{{ env.{CACHE_BUST_ENV} }}}}`, set once per workflow under `env:` — a key written "
                    f"into each step is a key nobody busts consistently"
                )
            if step.with_.get(CACHE_SAVE_ON_FAILURE_INPUT, "").strip().lower() != "true":
                problems.append(
                    f"R04 {step.where_file}: does not set `{CACHE_SAVE_ON_FAILURE_INPUT}: \"true\"`, so a red run "
                    f"keeps its compiled objects to itself and the next run rebuilds them. `e11.1`: 102 of "
                    f"`ci.yml`'s 233 cache saves never happened because the job was red"
                )
        wanted = {s.working_directory for s in job.steps if s.working_directory}
        for step in job.steps:
            wanted.update(CACHE_MANIFEST_FLAG.findall(step.run))
        wanted = {w.strip("/\"'") for w in wanted if w.strip("/\"'")}
        if caches and wanted:
            kept = set()
            for step in caches:
                for entry in str(step.with_.get("workspaces", "")).splitlines():
                    token = entry.split("->")[0].strip()
                    if token:
                        kept.add(token)
            for extra in sorted(wanted - kept):
                problems.append(
                    f"R04 {wf.name} (`{wf.filename}`): job `{job.id}` builds `{extra}` (a `working-directory:` or a "
                    f"`--manifest-path`) and no `rust-cache` step names it under `workspaces:`, so that "
                    f"workspace's `target/` is rebuilt on every run and cached under a key that does not "
                    f"describe it"
                )
    if cached_any:
        where = f"{wf.name} (`{wf.filename}`)"
        bust = wf.env.get(CACHE_BUST_ENV, "")
        if not bust:
            problems.append(
                f"R04 {where}: uses `{CACHE_ACTION}` but declares no `{CACHE_BUST_ENV}:` under `env:`, so "
                f"there is no lever to bust the cache with"
            )
        elif not CACHE_BUST_VALUE.match(bust):
            problems.append(
                f"R04 {where}: `{CACHE_BUST_ENV}: \"{bust}\"` is not a date (`2026-11`, or `2026-11.2` for a "
                f"second bust in a month). The bust is dated because the question it raises is *when, and "
                f"what moved it*"
            )
        else:
            lines = wf.text.splitlines()
            at = next((i for i, raw in enumerate(lines)
                       if re.match(rf"^\s*{CACHE_BUST_ENV}\s*:", strip_comment(raw))), None)
            # The comment that belongs to the key is the run of comment lines sitting directly above it, of
            # however many lines it runs — a rule that only reads the last three would be satisfied by a
            # flourish and a shrug above it, and would refuse a paragraph that explains the thing properly.
            near = ""
            if at is not None:
                back = at - 1
                block: list[str] = []
                while back >= 0 and lines[back].lstrip().startswith("#"):
                    block.append(lines[back])
                    back -= 1
                near = " ".join(reversed(block))
            if at is None or "bust" not in near.lower():
                problems.append(
                    f"R04 {where}: `{CACHE_BUST_ENV}: \"{bust}\"` has no comment saying when it may be "
                    f"moved. The rule is one sentence — bust it when the bytes in the cache would be *wrong* "
                    f"rather than merely stale — and it has to be readable where the lever is"
                )


def artefact_guards(job: Job) -> tuple[dict[int, Step], list[Step]]:
    """A job's size guards (by step index) and its uploads (in order)."""
    guards: dict[int, Step] = {}
    uploads: list[Step] = []
    for step in job.steps:
        if all(key in step.env for key in ARTEFACT_GUARD_KEYS):
            guards[step.index] = step
        if "upload-artifact" in step.uses:
            uploads.append(step)
    return guards, uploads


def rule_artefact_budget(wf: Workflow, problems: list[str]) -> None:
    """`R05` — what is uploaded, how large it may be, how long it is kept, and a step that enforces all three."""
    if wf.generated:
        return
    for job in wf.jobs.values():
        guards, uploads = artefact_guards(job)
        for index, guard in guards.items():
            if not any(upload.index > index for upload in uploads):
                problems.append(
                    f"R05 {wf.name}: job `{job.id}` step {index + 1} measures an artefact "
                    f"(`{guard.env.get('ARTEFACT_NAME')}`) and no `upload-artifact` step follows it. A budget "
                    f"that guards nothing is a number with a step attached to it"
                )
        for upload in uploads:
            raw = upload.with_.get("name", "")
            where = f"{wf.name} (`{wf.filename}`): job `{job.id}` uploads `{render_artefact(raw)}`"
            row = budget_for(raw)
            if row is None:
                problems.append(
                    f"R05 {where}: no row in `ARTEFACT_BUDGET` budgets it. Say what it is, how large it may "
                    f"get, how long it is kept and which paths it is made of, in "
                    f"`tools/check_workflows.py::ARTEFACT_BUDGET`"
                )
                continue
            pattern, budget = row
            limit_mb = int(str(budget["max_mb"]))
            days = int(str(budget["retention_days"]))
            content = [render_artefact(str(p)) for p in budget["paths"]]
            retention = upload.with_.get("retention-days", "").strip()
            if not retention.isdigit():
                problems.append(
                    f"R05 {where}: no `retention-days`, so it is kept for whatever the repository defaults to "
                    f"today; its budget (`{pattern}`) says {days} days"
                )
            elif int(retention) > days:
                problems.append(
                    f"R05 {where}: kept {retention} days, past the {days} its budget “{pattern}” allows — "
                    f"{budget['what']}"
                )
            paths = [render_artefact(p) for p in str(upload.with_.get("path", "")).splitlines() if p.strip()]
            for path in paths:
                if not paths_describe(content, path):
                    problems.append(
                        f"R05 {where}: uploads `{path}`, which its budget row “{pattern}” does not describe "
                        f"(that row is {', '.join(content)}). An artefact that grows a path has grown a "
                        f"second artefact under one name: budget it, or stop uploading it"
                    )
            before = [g for index, g in guards.items()
                      if index < upload.index
                      and render_artefact(g.env.get("ARTEFACT_NAME", "")) == render_artefact(raw)]
            if not before:
                problems.append(
                    f"R05 {where}: nothing measures it. Put a size guard step before the upload — "
                    f"`{', '.join(ARTEFACT_GUARD_KEYS)}` under `env:` and five lines of shell "
                    f"(*The artefact budget* in docs/concepts/ci-gate.md has the block), so an artefact past "
                    f"its {limit_mb} MB budget fails the run instead of quietly costing the fleet its storage"
                )
                continue
            guard = before[-1]
            if _normalise_if(guard.if_text) != _normalise_if(upload.if_text):
                problems.append(
                    f"R05 {wf.name}: job `{job.id}`: the guard for `{render_artefact(raw)}` runs under "
                    f"`if: {guard.if_text or '(nothing)'}` and the upload under `if: "
                    f"{upload.if_text or '(nothing)'}`. An upload that runs when its guard did not is an "
                    f"unmeasured upload"
                )
            limit = guard.env.get("ARTEFACT_MAX_KB", "").strip()
            if not limit.isdigit():
                problems.append(f"R05 {guard.where}: `ARTEFACT_MAX_KB: {limit}` is not a number of kilobytes")
            elif int(limit) > limit_mb * 1024:
                problems.append(
                    f"R05 {guard.where}: `ARTEFACT_MAX_KB: {limit}` is above the {limit_mb} MB its budget row "
                    f"“{pattern}” declares. A step may not grant itself more than the table allows"
                )
            elif int(limit) * 4 < limit_mb * 1024:
                problems.append(
                    f"R05 {guard.where}: `ARTEFACT_MAX_KB: {limit}` is under a quarter of the {limit_mb} MB "
                    f"its budget row “{pattern}” declares. Two numbers for one limit, already disagreeing: "
                    f"copy the row's"
                )
            if "$ARTEFACT_PATH" not in guard.run and "${ARTEFACT_PATH}" not in guard.run:
                problems.append(
                    f"R05 {guard.where}: declares `ARTEFACT_PATH` and never measures it — the guard has to "
                    f"`du` the paths it names"
                )
            measured = [render_artefact(token)
                        for token in render_artefact(guard.env.get("ARTEFACT_PATH", "")).split()]
            if not paths_agree(paths, measured):
                problems.append(
                    f"R05 {guard.where}: measures `{guard.env.get('ARTEFACT_PATH')}` while the upload ships "
                    f"{', '.join(paths)}. A path in the artefact that the guard never measured is how a "
                    f"budget gets stepped over — and a path the guard measures that the upload dropped is a "
                    f"file the producer stopped writing, which is the same blind spot facing the other way"
                )
            producer = guard.env.get("ARTEFACT_PRODUCER", "").strip()
            if len(producer) < 12:
                problems.append(
                    f"R05 {guard.where}: `ARTEFACT_PRODUCER: \"{producer}\"` does not name the step whose "
                    f"output these bytes are — the line a reader consults to tell an empty artefact from a "
                    f"producer that stopped producing"
                )
            if "::error::" not in guard.run or "exit 1" not in guard.run:
                problems.append(
                    f"R05 {guard.where}: reports but cannot fail; there is no `::error::` and `exit 1` past "
                    f"the limit. A budget that prints a number in green is a report, not a budget"
                )


def rule_budget_table(files: list[Workflow], problems: list[str]) -> None:
    """`R05`'s other half: the table and the file set describe the same artefacts."""
    uploaded: list[str] = []
    for wf in files:
        if wf.generated:
            continue
        for job in wf.jobs.values():
            _guards, uploads = artefact_guards(job)
            uploaded.extend(upload.with_.get("name", "") for upload in uploads)
    for pattern in ARTEFACT_BUDGET:
        if not any(_matches(pattern, render_artefact(name)) for name in uploaded):
            problems.append(
                f"R05 `.github/workflows`: `ARTEFACT_BUDGET` budgets `{pattern}` and no workflow uploads "
                f"anything by that name. A budget for an artefact nobody makes is how a budget table starts "
                f"to be fiction — delete the row, or say what is coming"
            )


def required_step_cadences() -> tuple[tuple[str, ...], ...] | None:
    """`CI_REQUIRED_STEPS` from the workspace model, or None when the model cannot be read.

    Imported lazily and defensively: `check_workflows.py` is run standalone as often as `check_workspace.py`
    imports it, and a rule that cannot read its list says so rather than passing.
    """
    try:
        import workspace_model as model
    except Exception:
        return None
    return getattr(model, "CI_REQUIRED_STEPS", ()) or None


def rule_step_cadence(wf: Workflow, problems: list[str]) -> None:
    """`R06` — work the model calls scheduled-only does not run on the pull-request path.

    `C01` (`tools/check_workspace.py`) already refuses a required step that survives *only* at the cadence it
    does not belong to. It cannot see the opposite accident, which is the one `e11.2` leaves behind: a step
    added back into a per-PR workflow that the model has already decided is a daily fact. That addition keeps
    `C01` green — the needle still appears in a scheduled file too — and quietly puts the long tail back on
    the queue `e11.2` cut it off. This is the rule that keeps `e11.2` from rotting, and it is `C01`'s mirror
    rather than a copy of it.
    """
    if "pull_request" not in wf.triggers:
        return
    model = required_step_cadences()
    if model is None:
        problems.append(
            f"R06 {wf.name} (`{wf.filename}`): cannot read `CI_REQUIRED_STEPS` from "
            f"`tools/workspace_model.py`, so no step in this file can be confirmed to belong on a pull "
            f"request"
        )
        return
    executable = "\n".join(
        [job.uses for job in wf.jobs.values()]
        + [part for job in wf.jobs.values() for step in job.steps
           for part in [step.uses, step.run, *step.with_.values()]]
    )
    for entry in model:
        name, needle, cadence = entry[0], entry[1], entry[2]
        if cadence != "scheduled" or needle not in executable:
            continue
        problems.append(
            f"R06 {wf.name} (`{wf.filename}`): runs `{needle}`, which the required-step model names "
            f"`scheduled` (`{name}`). A pull-request workflow may not run scheduled-only work: it puts the "
            f"cost back on every push and leaves `C01` green, because the needle also appears in a scheduled "
            f"file. Either move the step to `nightly.yml`, `perf.yml` or `fuzz.yml`, or change its cadence in "
            f"`tools/workspace_model.py` and say why in the same commit"
        )


def rule_branch_reach(wf: Workflow, problems: list[str]) -> None:
    """`R07` — every gate runs on a branch the work reaches.

    Two clauses, one failure behind them. `e11.1` §5.2 counted the runs this fleet made in thirty days and
    found not one of them started by the work: `master` had been frozen at `e194a63d9d` since 2026-10-26,
    the branch the loop worked on was not on the remote, and every file here said
    `push: branches: [master]`. Nothing failed, because nothing was watching: a list of branch names is a
    claim about where the work is, and such a claim expires the day the work moves. So a `push:` trigger may
    not narrow by branch name (clause one, which holds for every workflow that has one), and a workflow that
    gates every pull request may not be reachable only by a pull request (clause two, the same hole in a
    file with no `push:` at all). `reco-kwg6` is the bead; *Which branch a gate runs on* in
    docs/concepts/ci-gate.md is the prose.
    """
    seen = push_filter(wf)
    if seen is not None and not filter_reaches_every_branch(*seen):
        branches, ignores = seen
        wrote = ("branches-ignore: " + ", ".join(f"`{b}`" for b in ignores) if ignores
                 else "branches: [" + ", ".join(f"`{b}`" for b in branches) + "]")
        problems.append(
            f"R07 {wf.name} (`{wf.filename}`): its `push:` trigger is narrowed to `{wrote}` — a list of "
            f"names, which is a claim about where the work *was*. This file was unreachable for a whole "
            f"thirty-day census window for exactly that reason (`e11.1` §5.2, `reco-kwg6`). Write "
            f"`branches: [\"**\"]` — `*` will not do, it stops at a `/` and so refuses a branch named "
            f"`feature/<something>` — or drop the filter. A file that genuinely must not run on every branch "
            f"should say which branches and why in its own header and raise this rule in the same commit"
        )
    if is_merge_gate(wf) and seen is None:
        problems.append(
            f"R07 {wf.name} (`{wf.filename}`): triggers on every `pull_request` and on no branch `push`, so a "
            f"branch with no open pull request is never measured here — which is the shape a loop working on "
            f"an un-merged branch is stuck in forever (`e11.1` §5.2, `reco-kwg6`). Add `push:` with "
            f"`branches: [\"**\"]`; the per-ref `concurrency:` group `R02` already requires keeps a superseded "
            f"push from paying for itself twice"
        )


# ---------------------------------------------------------------------------
# driving it
# ---------------------------------------------------------------------------


def workflows() -> list[Workflow]:
    return [Workflow(path) for path in sorted(WORKFLOWS.glob("*.yml"))]


def check(verbose: bool = False, network: bool = False) -> tuple[list[str], list[str]]:
    problems: list[str] = []
    notes: list[str] = []
    members = workspace_members()
    files = workflows()
    for wf in files:
        rule_actions(wf, problems)
        rule_dist_pin(wf, problems)
        rule_dist_targets(wf, problems)
        rule_expressions(wf, problems)
        rule_paths(wf, problems)
        rule_fuzz_manifest(wf, problems)
        rule_tool_calls(wf, problems)
        rule_cargo_calls(wf, members, problems)
        rule_job_wiring(wf, problems)
        rule_artifacts(wf, problems)
        rule_timeouts(wf, problems)
        rule_concurrency(wf, problems)
        rule_tolerated_failures(wf, problems)
        rule_cache(wf, problems)
        rule_artefact_budget(wf, problems)
        rule_step_cadence(wf, problems)
        rule_branch_reach(wf, problems)
    rule_budget_table(files, problems)
    if network:
        notes.extend(verify_inventory_network(problems))
    if verbose:
        generated = [wf.filename for wf in files if wf.generated]
        notes.append(f"{len(files)} workflow files · {len(members)} crates · "
                     f"{sum(len(w.jobs) for w in files)} jobs · "
                     f"{sum(len(s) for w in files for s in [w.every_step()])} steps read")
        notes.append(f"{len(gitignore_rules(GITIGNORE))} `.gitignore` rule(s) read — a path a step names "
                     "that one of them excludes is the run's own output, not a missing file (`P01`)")
        notes.append(f"{sum(1 for wf in files if 'pull_request' in wf.triggers)} file(s) trigger on "
                     f"`pull_request` and are held to `R02` and `R06`; "
                     f"{sum(1 for wf in files for job in wf.jobs.values() if job.timeout)} job(s) declare a "
                     f"`timeout-minutes` (`R01`)")
        notes.append(f"{len(ARTEFACT_BUDGET)} artefact budget row(s), every one of them matched by a real "
                     "upload (`R05`); `python3 tools/check_workflows.py budgets` prints the table")
        if generated:
            notes.append(f"generated, so exempt from `R01`/`R04`/`R05`: {', '.join(generated)} — their "
                         "generator owns those lines and `dist generate --check` refuses the edit (`A02` "
                         "still holds them to the pinned version)")
    return problems, notes


def verify_inventory_network(problems: list[str]) -> list[str]:
    """`--network`: prove every action reference used in this repository resolves on GitHub.

    Off by default, because a CI gate that phones home fails the day the network is the thing being
    tested. With a token in `GH_TOKEN`/`GITHUB_TOKEN` it asks authenticated; without one it stops on the
    first rate limit and says how many refs it could not check, because an anonymous 403 is the API
    protecting itself and not a workflow being wrong.
    """
    import os

    token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN") or ""
    refs = set()
    for wf in workflows():
        for job in wf.jobs.values():
            for step in [job, *job.steps]:
                ref = str(getattr(step, "uses", ""))
                if ref and "@" in ref and not ref.startswith(("./", "docker://")):
                    refs.add(ref)
    notes = []
    unverified = 0
    for ref in sorted(refs):
        path, _, name = ref.partition("@")
        repo = "/".join(path.split("/")[:2])
        url = f"https://api.github.com/repos/{repo}/contents/action.yml?ref={name}"
        headers = {"User-Agent": "fathom-check-workflows"}
        if token:
            headers["Authorization"] = f"Bearer {token}"
        try:
            urllib.request.urlopen(urllib.request.Request(url, headers=headers), timeout=30)
            notes.append(f"network: {repo}@{name} resolves")
        except urllib.error.HTTPError as exc:
            if exc.code in (403, 429):
                unverified += 1
                continue
            problems.append(
                f"A01 {ref} does not resolve on GitHub (HTTP {exc.code}); the job would die in *Set up job*"
            )
        except OSError as exc:
            problems.append(f"A01 {ref} could not be checked: {exc}")
    if unverified:
        notes.append(f"network: {unverified} ref(s) left unchecked — the API rate-limited this run "
                     f"(export GH_TOKEN to check them)")
    return notes


# ---------------------------------------------------------------------------
# the checker, checked
# ---------------------------------------------------------------------------

BROKEN: dict[str, str] = {
    "a-action.yml": """
name: bogus
on: pull_request
permissions:
  contents: read
jobs:
  a:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: not_an_action/never_existed/action@v1
""",
    "e-outputs.yml": """
name: stepout
on:
  workflow_dispatch:
    inputs:
      real:
        default: "1"
jobs:
  a:
    runs-on: ubuntu-latest
    steps:
      - id: minutes
        run: echo "value=3" >> "$GITHUB_OUTPUT"
      - run: echo ${{ steps.minutes.value }} ${{ steps.never.outputs.value }} ${{ inputs.nope }}
""",
    "e-needs.yml": """
name: badneeds
on: pull_request
permissions:
  contents: read
jobs:
  a:
    runs-on: ubuntu-latest
    steps:
      - run: echo ${{ needs.other.outputs.thing }}
""",
    "p-paths.yml": """
name: badpaths
on: pull_request
permissions:
  contents: read
jobs:
  a:
    runs-on: ubuntu-latest
    steps:
      - run: python3 tools/no_such_tool.py check
        working-directory: crates/fathom-store/nope
""",
    "p-fuzz.yml": """
name: fuzzcwd
on: pull_request
permissions:
  contents: read
jobs:
  a:
    runs-on: ubuntu-latest
    steps:
      - run: cargo +nightly fuzz build lockfile
        working-directory: fuzz
""",
    "p-fuzz-manifest.yml": """
name: fuzzmanifest
on: pull_request
permissions:
  contents: read
jobs:
  a:
    runs-on: ubuntu-latest
    steps:
      - run: cargo +nightly fuzz run lockfile
""",
    "t-cargo.yml": """
name: badtargets
on: pull_request
permissions:
  contents: read
jobs:
  a:
    runs-on: ubuntu-latest
    steps:
      - run: cargo test --locked -p fathom-analyze --features no-such-feature --test no_such_test
      - run: cargo build -p not_a_crate
      - run: cargo run -p fathom-analyze --example no_such_example
      - run: python3 tools/tui_tables.py no-such-subcommand
      - run: python3 tools/gen_docs.py no-such-subcommand --page cli
""",
    "j-wiring.yml": """
name: noperm
on: pull_request
jobs:
  a:
    runs-on: ubuntu-latest
    needs: [missing]
    steps:
      - run: echo hi
""",
    "j-artifacts.yml": """
name: artifacts
on: pull_request
permissions:
  contents: read
jobs:
  a:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/upload-artifact@v4
        with:
          name: twice
      - uses: actions/upload-artifact@v4
        with:
          name: twice
      - uses: actions/download-artifact@v8
        with:
          name: never-uploaded
""",
    "d-dist-target.yml": """
name: disttarget
on: pull_request
permissions:
  contents: read
jobs:
  a:
    runs-on: ubuntu-latest
    steps:
      - run: dist build --artifacts=local
      - run: dist build --artifacts=all
""",
    "a-dist.yml": """
name: distpin
on: pull_request
permissions:
  contents: read
jobs:
  a:
    runs-on: ubuntu-latest
    steps:
      - run: curl -L https://github.com/axodotdev/cargo-dist/releases/download/v0.1.0/cargo-dist-installer.sh | sh
""",
    # --- `e11.4` (`reco-374v.4`): the runner-hygiene rules, one file each written to break one of them.
    # Each carries the concurrency group and the timeout it needs, so that a rule failing to fire means the
    # rule is broken, not merely out-shouted by another rule firing on the same file.
    "r-timeout.yml": """
name: timeouts
on: pull_request
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true
permissions:
  contents: read
jobs:
  no-number:
    runs-on: ubuntu-latest
    steps:
      - run: echo a hang here holds the runner for six hours and looks like a flake
  too-big-a-number:
    runs-on: ubuntu-latest
    timeout-minutes: 600
    steps:
      - run: echo five hours is the same hole with a paper label on it
""",
    "r-queue.yml": """
name: noqueue
on: pull_request
permissions:
  contents: read
jobs:
  a:
    runs-on: ubuntu-latest
    timeout-minutes: 10
    steps:
      - run: echo every push to the branch pays for the last one in full, again
""",
    "r-queue-shape.yml": """
name: sharedqueue
on: pull_request
concurrency:
  group: everyone
  cancel-in-progress: false
permissions:
  contents: read
jobs:
  a:
    runs-on: ubuntu-latest
    timeout-minutes: 10
    steps:
      - run: echo one queue for every workflow and every pull request, and nothing cancelled
""",
    "r-tolerate.yml": """
name: tolerate
on: pull_request
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true
permissions:
  contents: read
jobs:
  a:
    runs-on: ubuntu-latest
    timeout-minutes: 10
    steps:
      - name: a failure that is allowed and never explained
        continue-on-error: true
        run: exit 1
""",
    "r-tolerate-note.yml": """
name: toleratenote
on: pull_request
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true
permissions:
  contents: read
jobs:
  a:
    runs-on: ubuntu-latest
    timeout-minutes: 10
    steps:
      # tolerates: the old flake
      - run: echo the tolerance was deleted and the note was left behind, saying what it used to forgive
""",
    "r-cache.yml": """
name: nocache
on: pull_request
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true
permissions:
  contents: read
jobs:
  compiles-cold:
    runs-on: ubuntu-latest
    timeout-minutes: 10
    steps:
      - uses: actions/checkout@v4
      - run: cargo build --workspace
""",
    "r-cache-key.yml": """
name: cachekeys
on: pull_request
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true
permissions:
  contents: read
jobs:
  hand-written-keys:
    runs-on: ubuntu-latest
    timeout-minutes: 10
    steps:
      - uses: actions/checkout@v4
      - uses: Swatinem/rust-cache@v2
        with:
          key: week-of-the-year
      - run: cargo build --locked --manifest-path plugins/acme-detectors/Cargo.toml
      - uses: actions/cache@v4
        with:
          path: ~/.cargo
          key: my-own-idea
""",
    "r-artefact.yml": """
name: unbudgeted
on: pull_request
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true
permissions:
  contents: read
jobs:
  a:
    runs-on: ubuntu-latest
    timeout-minutes: 10
    steps:
      - run: dd if=/dev/zero of=nobody-budgeted-this.bin bs=1k count=16
      - uses: actions/upload-artifact@v4
        with:
          name: nobody-budgeted-this
          path: nobody-budgeted-this.bin
""",
    "r-artefact-budget.yml": """
name: budgetbreakers
on: pull_request
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true
permissions:
  contents: read
jobs:
  a:
    runs-on: ubuntu-latest
    timeout-minutes: 10
    steps:
      - run: |
          mkdir -p fathom-check-sarif extra
          dd if=/dev/zero of=fathom-check-sarif/fathom.sarif bs=1k count=16
          touch extra/the-second-artefact
      - uses: actions/upload-artifact@v4
        with:
          name: fathom-check-sarif
          path: |
            fathom-check-sarif/fathom.sarif
            extra/the-second-artefact
          retention-days: 400
""",
    "r-cadence.yml": """
name: tailbackonthepath
on: pull_request
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true
permissions:
  contents: read
jobs:
  a:
    runs-on: ubuntu-latest
    timeout-minutes: 10
    steps:
      - run: cargo run --release --locked --example s1_spike -- --check
""",
    # The exact file shape `e11.1` §5.2 found: a gate that names a branch instead of reaching one. `master`
    # was frozen three weeks before that census ran, and every run this file could have started went to a
    # commit nobody was working on.
    "r07-branch.yml": """
name: namedbranch
on:
  pull_request:
  push:
    branches: [master]
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true
permissions:
  contents: read
jobs:
  a:
    runs-on: ubuntu-latest
    timeout-minutes: 10
    steps:
      - run: echo the default branch stopped moving in October and this file never noticed
""",
    # The same hole in a file that has no `push:` trigger at all: only a pull request can reach it, and a
    # loop that never opens one is never measured.
    "r07-nopush.yml": """
name: pronly
on: pull_request
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true
permissions:
  contents: read
jobs:
  a:
    runs-on: ubuntu-latest
    timeout-minutes: 10
    steps:
      - run: echo a branch with no pull request is a branch nobody gates
""",
}

#: What the self-test's stand-in `.gitignore` claims is the run's own output, and the paths the workflow
#: below names from it. It is a directory rule because the real one is (`benchmarks/trend/`), and a
#: directory rule is the gitignore shape that decides everything under it. None of these exist in this
#: repository, which is the point: the exemption has to be what keeps them quiet, not a file left over
#: from somebody's build.
GENERATED_IGNORE_RULE = "tools/generated/"
GENERATED_PATHS = ("tools/generated/tables.py", "tools/generated/summary.md")

#: Workflows that must produce *no* problem, for the rule named in the key. `P01` fired on `perf.yml` in
#: CI and nowhere on a laptop (`reco-8myi`) because the only version of "does this path exist" a working
#: tree can answer is the wrong one; this is the case that keeps the fixed rule from being loosened into
#: silence.
EXEMPT: dict[str, str] = {
    "p-generated.yml": f"""
name: generated
on: pull_request
permissions:
  contents: read
jobs:
  a:
    runs-on: ubuntu-latest
    steps:
      - run: if [ -f {GENERATED_PATHS[0]} ]; then cat {GENERATED_PATHS[0]}; fi
      - run: ls {GENERATED_PATHS[1]}
""",
}


def self_test() -> int:
    """Require every rule to fire on a workflow written to break it, and the exemptions to hold."""
    import tempfile

    global WORKFLOWS, FUZZ_MANIFEST, GITIGNORE  # noqa: PLW0603 - the rules read the module's paths, so the test moves
    with tempfile.TemporaryDirectory() as work:
        temp = pathlib.Path(work)
        for name, text in BROKEN.items():
            (temp / name).write_text(text, encoding="utf-8")
        for name, text in EXEMPT.items():
            (temp / name).write_text(text, encoding="utf-8")
        # The ignore file the `P01` exemption reads. Without it nothing is ignored, and the exempt workflow
        # above would report two problems of its own — which is how this line proves it is being read.
        (temp / ".gitignore").write_text(GENERATED_IGNORE_RULE + "\n", encoding="utf-8")
        # `P04` reads a manifest rather than a workflow, so the self-test has to supply an unmarked one —
        # the exact file shape `reco-com` committed, and the reason cargo-fuzz refused to start.
        (temp / "fuzz").mkdir()
        (temp / "fuzz" / "Cargo.toml").write_text(
            '[workspace]\n\n[package]\nname = "fathom-fuzz"\nversion = "0.0.0"\n', encoding="utf-8"
        )
        workflows_dir, manifest, ignore = WORKFLOWS, FUZZ_MANIFEST, GITIGNORE
        WORKFLOWS, FUZZ_MANIFEST, GITIGNORE = temp, temp / "fuzz" / "Cargo.toml", temp / ".gitignore"
        try:
            problems, _ = check()
            rules = gitignore_rules(GITIGNORE)
        finally:
            WORKFLOWS, FUZZ_MANIFEST, GITIGNORE = workflows_dir, manifest, ignore
    failures = [f"rule {rule} did not fire on a workflow written to break it"
                for rule in ("A01", "A02", "D01", "E01", "E02", "E03", "E04", "P01", "P02", "P03",
                             "P04", "T01", "T02", "T03", "T04", "J01", "J02", "J03",
                             "R01", "R02", "R03", "R04", "R05", "R06", "R07")
                if not any(p.startswith(rule) for p in problems)]
    # `R05` has a second half that no workflow file can stage on its own: a budget row for an artefact
    # nothing uploads. It is staged here, on the table, because a budget allowed to accrete rows that
    # describe nothing is a budget nobody believes.
    ARTEFACT_BUDGET["nothing-uploads-this"] = {"what": "the self-test's ghost artefact", "max_mb": 1,
                                              "retention_days": 1, "paths": ("nothing-uploads-this",)}
    try:
        _again, _notes = check()
    finally:
        del ARTEFACT_BUDGET["nothing-uploads-this"]
    if not any(p.startswith("R05") and "nothing-uploads-this" in p for p in _again):
        failures.append("rule R05 did not refuse a budget row for an artefact no workflow uploads")
    # `R07` has two clauses and the list above fires the second one almost incidentally, so both are named
    # here: the shape the census actually found is the *narrowed* one, and a rule that only catches the
    # obvious half of itself is a rule that will be quietly satisfied tomorrow.
    for clause, needle in (("a `push:` narrowed to a list of branch names", "is narrowed to"),
                           ("a gate that only a pull request can reach", "and on no branch `push`")):
        if not any(p.startswith("R07") and needle in p for p in problems):
            failures.append(f"rule R07 did not fire on {clause}")
    if not rules:
        failures.append(
            f"the self-test's `.gitignore` ({GENERATED_IGNORE_RULE}) gave no rules, so the `P01`/`P02` "
            "exemption was never asked and passing means nothing"
        )
    for path in EXEMPT:
        for token in GENERATED_PATHS:
            if any(p.startswith(("P01", "P02")) and token in p for p in problems):
                failures.append(
                    f"`{token}` is ignored by `.gitignore`, so it is the run's own output and not a claim "
                    f"about the checkout ({path}); `P01` fired on it anyway"
                )
    if failures:
        for failure in failures:
            print(f"self-test: {failure}", file=sys.stderr)
        for problem in problems:
            print(f"  saw: {problem}", file=sys.stderr)
        return 1
    print(f"check_workflows self-test OK: {len(problems)} problems read from {len(BROKEN)} files written "
          "to break one rule each — a fake action, a stale dist pin, a `dist build` that named no triple, "
          "a misread step output, an unknown step, an undeclared needs and input, a missing path and "
          "working-directory, `cargo fuzz` in the wrong directory and pointed at a manifest it will not "
          "accept, an unknown crate, feature, test, example and tool subcommand, a duplicate artefact name, "
          "an unmatchable download pattern, a stray needs and a missing permissions block, a job with no "
          "timeout and one with a six-hour one, a pull-request workflow with no queue and one with the wrong "
          "queue, a tolerated failure with no reason and a reason with no tolerated failure, a compile with "
          "no cache and a cache with hand-written keys, an unbudgeted artefact, a budget stepped over "
          "(retention, paths, and a row for nothing), a scheduled-only step back on the pull-request "
          "path, a `push:` trigger that named branches instead of reaching them and a gate no pushed branch "
          "can reach — and a step that reads an ignored, generated path was left alone")
    return 0


#: What to tell a reader who finds their own branch absent from `origin`. The point of the line is that it is
#: the whole fix: nothing about `R07` can start a run for a branch nobody has pushed.
BRANCH_UNPUSHED = ("not visible as `origin/{branch}` in this clone, so no run of any kind will start for it "
                   "— `git push -u origin {branch}` (or `git fetch origin` first, if it is pushed and this "
                   "clone is merely stale)")


def git(*args: str) -> str:
    """A git question asked of this checkout, offline. Empty on any failure: the caller says what it assumed."""
    import subprocess

    try:
        found = subprocess.run(["git", "-C", str(ROOT), *args], capture_output=True, text=True,
                               timeout=30, check=False)
    except (OSError, subprocess.SubprocessError):
        return ""
    return found.stdout.strip() if found.returncode == 0 else ""


def check_branch_reach(branch: str = "") -> tuple[list[str], list[str]]:
    """Answer "will CI run on this branch?" from the checkout, with no network and no token.

    Printed by `python3 tools/check_workflows.py triggers`, and the reason the subcommand exists: the
    question `reco-kwg6` closes is one nobody could ask before it. A workflow that triggers on
    `pull_request` is not a fact about a branch until somebody opens the pull request, so the honest answer
    needs three parts — what each workflow's triggers admit, whether the branch is on the remote at all, and
    which of the merge gates would run if it were pushed.
    """
    branch = branch or git("branch", "--show-current")
    lines: list[str] = []
    problems: list[str] = []
    files = workflows()
    gates = [wf for wf in files if is_merge_gate(wf)]
    if not branch:
        lines.append("branch: (detached HEAD)")
        problems.append("HEAD is detached, so there is no branch to answer for; pass `--branch <name>`")
        return lines, problems
    sha = git("rev-parse", "--verify", "--quiet", f"refs/remotes/origin/{branch}")
    lines.append(f"branch: `{branch}` — " + (f"`{sha[:12]}` on `origin`, per this clone" if sha
                                             else BRANCH_UNPUSHED.format(branch=branch)))
    if not sha:
        problems.append(f"branch `{branch}` is {BRANCH_UNPUSHED.format(branch=branch)}")
    lines.append("")
    lines.append("| Workflow | Triggers | A push of this branch | A pull request |")
    lines.append("|---|---|---|---|")
    for wf in files:
        triggers = "; ".join(sorted(wf.triggers)) or "none"
        seen = push_filter(wf)
        if seen is None:
            push = "no `push:` trigger"
        elif push_reaches(wf, branch):
            push = "yes"
        else:
            push = "no — its filter excludes it"
        pr = ("n/a — no `pull_request` trigger" if "pull_request" not in wf.triggers
              else "yes, every path" if is_merge_gate(wf) else "only its `paths:` filter")
        lines.append(f"| `{wf.filename}` | {triggers} | {push} | {pr} |")
        if is_merge_gate(wf) and not push_reaches(wf, branch):
            problems.append(f"`{wf.filename}` gates every pull request and its `push:` trigger does not "
                            f"reach `{branch}`, so pushing this branch starts no run there")
    lines.append("")
    reached = sum(1 for wf in gates if push_reaches(wf, branch))
    lines.append(f"{len(gates)} merge-gate workflow(s) — the ones that fire on every pull request — and "
                 f"{reached} of them reach `{branch}`. A gate that reaches the branch is a gate whose "
                 "verdict you can read; a file below that does not is scheduled or dispatch-only by design.")
    return lines, problems


def render_budgets() -> str:
    """The artefact budget as a markdown table, with the workflows that upload each one.

    Printed by `python3 tools/check_workflows.py budgets` so that the artefact-budget table in
    docs/concepts/ci-gate.md can quote the same table `R05` enforces rather than a copy of it, and so a
    reviewer can diff the two by eye.
    """
    uploaded: dict[str, set[str]] = {}
    for wf in workflows():
        for job in wf.jobs.values():
            for upload in artefact_guards(job)[1]:
                row = budget_for(upload.with_.get("name", ""))
                if row:
                    uploaded.setdefault(row[0], set()).add(wf.filename)
    lines = ["| Artefact | What it is | Limit | Kept | Where it is uploaded |",
             "|---|---|---|---|---|"]
    for pattern, row in ARTEFACT_BUDGET.items():
        where = ", ".join(f"`{name}`" for name in sorted(uploaded.get(pattern, {"—"})))
        lines.append(f"| `{pattern}` | {row['what']} | {row['max_mb']} MB | {row['retention_days']} days "
                     f"| {where} |")
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    parser.add_argument("--verbose", action="store_true", help="print what passed as well")
    parser.add_argument("--network", action="store_true", help="verify every action ref resolves on GitHub")
    parser.add_argument("--branch", default="", help="`triggers` mode: the branch to answer for "
                                                   "(default: the one HEAD is on)")
    parser.add_argument("mode", nargs="?", default="check", choices=["check", "self-test", "budgets",
                                                                    "triggers"])
    args = parser.parse_args()
    if args.mode == "self-test":
        return self_test()
    if args.mode == "budgets":
        print(render_budgets())
        return 0
    if args.mode == "triggers":
        lines, problems = check_branch_reach(args.branch)
        for line in lines:
            print(line)
        for problem in problems:
            print(f"triggers: {problem}", file=sys.stderr)
        if problems:
            print(f"\n{len(problems)} problem(s): this branch starts no run, or not every gate can see it",
                  file=sys.stderr)
            return 1
        print("\nthis branch reaches every merge gate: pushing it starts a run of each one listed above")
        return 0
    problems, notes = check(args.verbose, args.network)
    for note in notes:
        print(note)
    if problems:
        for problem in problems:
            print(problem, file=sys.stderr)
        print(f"\n{len(problems)} problem(s)", file=sys.stderr)
        return 1
    print("workflows OK: action inventory, expressions, paths, fuzz manifest, dist target selection, "
          "cargo targets, job wiring, artefact names, timeouts, concurrency, tolerated failures, cache "
          "discipline, artefact budgets, step cadence and branch reach")
    return 0


if __name__ == "__main__":
    sys.exit(main())
