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
        with_ = data.get("with") or {}
        self.with_: dict[str, str] = {str(k): str(v) for k, v in with_.items()} if isinstance(with_, dict) else {}

    @property
    def where(self) -> str:
        return f"{self.job.workflow.name}: job `{self.job.id}` step {self.index + 1} ({self.label()})"

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
        permissions = document.get("permissions") or {}
        self.permissions = permissions if isinstance(permissions, dict) else {"": str(permissions)}
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
    if network:
        notes.extend(verify_inventory_network(problems))
    if verbose:
        notes.append(f"{len(files)} workflow files · {len(members)} crates · "
                     f"{sum(len(w.jobs) for w in files)} jobs · "
                     f"{sum(len(s) for w in files for s in [w.every_step()])} steps read")
        notes.append(f"{len(gitignore_rules(GITIGNORE))} `.gitignore` rule(s) read — a path a step names "
                     "that one of them excludes is the run's own output, not a missing file (`P01`)")
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
                             "P04", "T01", "T02", "T03", "T04", "J01", "J02", "J03")
                if not any(p.startswith(rule) for p in problems)]
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
          "working-directory, `cargo fuzz` in the "
          "wrong directory and pointed at a manifest it will not accept, an unknown crate, feature, test, "
          "example and tool subcommand, a duplicate artefact name, an unmatchable download pattern, a "
          "stray needs and a missing permissions block — and a step that reads an ignored, generated path "
          "was left alone")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    parser.add_argument("--verbose", action="store_true", help="print what passed as well")
    parser.add_argument("--network", action="store_true", help="verify every action ref resolves on GitHub")
    parser.add_argument("mode", nargs="?", default="check", choices=["check", "self-test"])
    args = parser.parse_args()
    if args.mode == "self-test":
        return self_test()
    problems, notes = check(args.verbose, args.network)
    for note in notes:
        print(note)
    if problems:
        for problem in problems:
            print(problem, file=sys.stderr)
        print(f"\n{len(problems)} problem(s)", file=sys.stderr)
        return 1
    print("workflows OK: action inventory, expressions, paths, fuzz manifest, dist target selection, "
          "cargo targets, job wiring, artefact names")
    return 0


if __name__ == "__main__":
    sys.exit(main())
