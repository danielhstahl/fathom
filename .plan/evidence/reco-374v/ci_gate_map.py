#!/usr/bin/env python3
"""Every gate the workflows actually execute, and where it moved to.

`e11.1`'s census (`.plan/ci-triage-reco-374v.md`) counted runs and reds from the Actions API. This counts
the *other* half: the invocations the workflow files execute, at a given revision, and the diff between two
revisions. It exists to answer the epic's third constraint — "nothing here lowers a quality bar; it moves
when a bar is paid. Every removal names the job that still pays it" — with a set difference rather than with
a memory, and to reprint the duplicate list `e11.1` §3 wrote by hand.

It reuses `tools/check_workflows.py`'s parser on purpose: the same `Workflow`/`Job`/`Step` objects that
`R01`-`R06` read, so "executed" means the same thing here as it does to rule `C01` — `run:`, `uses:` and
`with:`, never a comment.

Two keys per executed command, because two questions get asked of it:

* the **gate** is the command line as written, modulo whitespace and runner-only flags — what `dupes`
  matches on, because "the same question asked twice" means the same *question*: `gen_docs.py check --page
  cli` and `gen_docs.py check --page cheatsheet --page detectors --page features` are two questions.
* the **family** is the tool and its verb — what `diff` matches on, because a gate that survived the epic
  with one more flag on it is still the same gate.

    python3 .plan/evidence/reco-374v/ci_gate_map.py summary [--ref REF] [--json]
    python3 .plan/evidence/reco-374v/ci_gate_map.py map    [--ref REF]
    python3 .plan/evidence/reco-374v/ci_gate_map.py dupes  [--ref REF]
    python3 .plan/evidence/reco-374v/ci_gate_map.py diff --from REF [--to REF]

`--ref` reads `.github/workflows` out of a git revision (`git show`), so the "before" is the tree the epic
started from and not whatever is checked out now.
"""

from __future__ import annotations

import argparse
import collections
import json
import pathlib
import re
import subprocess
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[3] / "tools"))
import check_workflows as WF  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parents[3]
WORKFLOWS = ".github/workflows"

#: Words that say *what is proven* rather than *how this runner was told to build*, so they stay in a gate's
#: id; everything else a step passes (`--locked`, `-j 2`, `--quiet`, a redirected path) is runner talk.
MEANING = ("minimal", "langs-all", "sqlite-bundled", "all-targets", "all-features", "no-default-features",
           "release", "self-test", "audit", "lint", "check", "draft", "verify", "strict", "example",
           "page", "no-progress", "locked-head")

CARGO_VERBS = ("fmt", "clippy", "test", "build", "check", "deny", "tree", "fuzz", "doc", "run", "install",
               "llvm-cov", "miri", "clean", "package")

#: `(kind, regex)`, tried in order over each command line of a `run:` block. `cargo` is first because a
#: line can hold `cargo build && python3 tools/x.py`, and the *subject* of a step is the left-hand end.
PATTERNS: list[tuple[str, re.Pattern]] = [
    ("cargo", re.compile(r"(?<![\w/.-])cargo\s+(?P<rest>\+\S+\s+\S[^\n|&;]*|(?:" + "|".join(CARGO_VERBS) + r")\b[^\n|&;]*)")),
    ("tool", re.compile(r"python3?\s+(?:PYTHON[^ ]*\s+\S+\s+)?(?P<rest>tools/[\w/]+\.py[^\n|&;]*)")),
    ("ci-script", re.compile(r"(?:^|[\s;&(])(?:bash\s+|\.\/)?ci/(?P<rest>[\w./-]+\.sh[^\n|&;]*)")),
    ("module", re.compile(r"(?<![\w-])python3?\s+(?P<rest>-m\s+\w[\w.]*[^\n|&;]*)")),
    ("dist", re.compile(r"(?<![\w-])(?P<rest>dist\s+(?:plan|generate|build-local-artifacts|publish)\b[^\n|&;]*)")),
    ("recommender", re.compile(r"(?<![\w/.-])(?P<rest>recommender\.\w+\s+\w[\w-]*[^\n|&;]*)")),
    ("fathom", re.compile(r"(?<![\w/.-])fathom\s+(?P<rest>\w[\w-]*(?:\s+\w[\w-]*)?[^\n|&;]*)")),
]


def words_of(rest: str) -> list[str]:
    return re.findall(r"[^\s|&;]+", re.sub(r"\\\n", " ", rest))


def keep_flags(words: list[str]) -> list[str]:
    return sorted({w.lstrip("-") for w in words if w.startswith("-") and w.lstrip("-") in MEANING})


def gate_for(kind: str, rest: str) -> tuple[str, str]:
    """(family, gate) for one matched command tail."""
    words = words_of(rest)
    if not words:
        return "", ""
    if kind == "cargo":
        if words[0].startswith("+"):
            words = words[1:]
        verb, body = words[0], words[1:]
        pkg = body[body.index("-p") + 1] if "-p" in body else (
            body[body.index("--package") + 1] if "--package" in body else "")
        features: list[str] = []
        if "--features" in body:
            features = body[body.index("--features") + 1].split()
        parts = [verb] + ([pkg] if pkg else []) + sorted(set(features)) + keep_flags(body)
        return "cargo %s" % verb, "cargo %s" % " ".join(parts)
    if kind == "tool":
        head = words[0]
        parts = [head] + [w for w in words[1:]
                          if not w.startswith("-") or w.lstrip("-") in MEANING]
        return "tools/%s" % head.split("/")[-1], " ".join(parts)
    if kind == "ci-script":
        return "ci-script", "ci/" + words[0]
    if kind == "module":
        return "python-module", " ".join([words[1]] + [w for w in words[2:]
                                                       if not w.startswith("-") or w.lstrip("-") in MEANING])
    if kind == "dist":
        return "dist %s" % words[1], " ".join(words[:2] + keep_flags(words[2:]))
    if kind == "recommender":
        crate, verb = words[0].split(".", 1)
        sub = words[1] if len(words) > 1 else ""
        return "recommender.%s %s" % (crate, sub), " ".join([crate, sub] + keep_flags(words[2:]))
    head, sub = words[0], (words[1] if len(words) > 1 else "")
    if sub.startswith("-"):
        sub = ""
    return "fathom %s" % head, " ".join([head, sub] + keep_flags(words[1 + (1 if sub else 0):]))


def lines_of(text: str) -> list[str]:
    """The command lines of a `run:` block, joins unwrapped and comments dropped."""
    out: list[str] = []
    buffer = ""
    for raw in text.splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        buffer = (buffer + " " + line).strip() if buffer else line
        if buffer.endswith("\\"):
            buffer = buffer[:-1]
            continue
        out.append(buffer)
        buffer = ""
    if buffer:
        out.append(buffer)
    return out


def each_gate(workflows: dict[str, WF.Workflow]):
    """Yield (gate, family, place) for every command every step executes."""
    for wf in workflows.values():
        for job in wf.jobs.values():
            if job.uses:
                yield ("uses %s" % job.uses.split("@")[0], "uses", "%s · %s (reusable call)"
                       % (wf.filename, job.id))
            for step in job.steps:
                place = "%s · %s · %s" % (wf.filename, job.id, step.label())
                seen_gate: set[str] = set()
                if step.uses:
                    seen_gate.add("uses")
                    yield ("uses %s" % step.uses.split("@")[0], "uses", place)
                for line in lines_of(step.run):
                    for kind, regex in PATTERNS:
                        for match in regex.finditer(line):
                            family, gate = gate_for(kind, match.group("rest"))
                            if not gate or gate in seen_gate:
                                continue
                            seen_gate.add(gate)
                            yield gate, family, place


def index(workflows: dict[str, WF.Workflow], key: str) -> "collections.OrderedDict[str, list[str]]":
    """gate (or family) → the steps that execute it, in file order."""
    out: "collections.OrderedDict[str, list[str]]" = collections.OrderedDict()
    which = 0 if key == "gate" else 1
    for gate, family, place in each_gate(workflows):
        bucket = out.setdefault((gate, family)[which], [])
        if place not in bucket:
            bucket.append(place)
    return out


def tool_only(data: dict) -> dict:
    return {k: v for k, v in data.items() if k != "uses"}


def load(ref: str | None) -> dict[str, WF.Workflow]:
    """Parse every workflow, from the working tree (`ref=None`) or out of a revision."""
    if ref is None:
        return {p.name: WF.Workflow(p) for p in sorted((ROOT / WORKFLOWS).glob("*.yml"))}
    out_dir = pathlib.Path("/tmp/wf-%s" % ref.replace("/", "_").replace(".", "_"))
    out_dir.mkdir(exist_ok=True)
    names = subprocess.run(["git", "-C", str(ROOT), "ls-tree", "--name-only", ref, WORKFLOWS + "/"],
                           capture_output=True, text=True, check=True).stdout.split()
    workflows: dict[str, WF.Workflow] = {}
    for name in names:
        blob = subprocess.run(["git", "-C", str(ROOT), "show", "%s:%s" % (ref, name)],
                              capture_output=True, text=True, check=True).stdout
        path = out_dir / pathlib.Path(name).name
        path.write_text(blob)
        workflows[path.name] = WF.Workflow(path)
    return workflows


def counts(workflows: dict[str, WF.Workflow]) -> dict[str, int]:
    """The size of the fleet, and the size of the part a pull request pays for."""
    pr = {n: w for n, w in workflows.items() if "pull_request" in w.triggers}

    def jobs(files):
        return sum(len(w.jobs) for w in files)

    def steps(files):
        return sum(len(j.steps) for w in files for j in w.jobs.values())

    return {
        "files": len(workflows),
        "pr_files": len(pr),
        "jobs": jobs(list(workflows.values())),
        "pr_jobs": jobs(list(pr.values())),
        "steps": steps(list(workflows.values())),
        "pr_steps": steps(list(pr.values())),
        "gates": len(tool_only(index(workflows, "gate"))),
        "pr_gates": len(tool_only(index(pr, "gate"))),
    }


def cmd_summary(args) -> int:
    data = counts(load(args.ref))
    if args.json:
        print(json.dumps({args.ref or "worktree": data}, indent=1))
        return 0
    for key, value in data.items():
        print("  %-10s %d" % (key, value))
    return 0


def cmd_map(args) -> int:
    for gate, places in index(load(args.ref), "gate").items():
        print("  %-52s %s" % (gate, " | ".join(places)))
    return 0


def cmd_dupes(args) -> int:
    data = tool_only(index(load(args.ref), "gate"))
    print("gates whose exact command runs in more than one step:")
    for gate, places in sorted(data.items(), key=lambda kv: -len(kv[1])):
        if len(places) > 1:
            print("  %-50s %s" % (gate, " | ".join(places)))
    dupes = [g for g, p in data.items() if len(p) > 1]
    print("%d of %d tool gates run in more than one step" % (len(dupes), len(data)))
    return 0


def cmd_diff(args) -> int:
    before = index(load(args.ref), "family")
    after = index(load(args.to), "family")
    gone = [g for g in before if g not in after]
    new = [g for g in after if g not in before]
    print("gate families executed at `%s` and nowhere at `%s` (%d):" % (args.ref, args.to, len(gone)))
    for gate in gone:
        print("  - %-28s was: %s" % (gate, " | ".join(before[gate])))
    print("gate families first executed at `%s` (%d):" % (args.to, len(new)))
    for gate in new:
        print("  + %-28s now: %s" % (gate, " | ".join(after[gate])))
    moved = [g for g in before if g in after and before[g] != after[g]]
    print("gate families that changed hands (%d):" % len(moved))
    for gate in moved:
        print("  ~ %s\n      was: %s\n      now: %s" % (gate, " | ".join(before[gate]),
                                                       " | ".join(after[gate])))
    print("%d families moved, %d gone, %d new (of %d before, %d after)"
          % (len(moved), len(gone), len(new), len(before), len(after)))
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(prog="ci_gate_map.py")
    sub = ap.add_subparsers(dest="cmd", required=True)
    for name, func, help_ in (("summary", cmd_summary, "how big the fleet and the pull-request path are"),
                              ("map", cmd_map, "every gate and where it runs"),
                              ("dupes", cmd_dupes, "the gates that run in more than one step")):
        p = sub.add_parser(name, help=help_)
        p.add_argument("--ref", default=None)
        p.add_argument("--json", action="store_true")
        p.set_defaults(func=func)
    p = sub.add_parser("diff", help="which gate families were dropped, moved or added")
    p.add_argument("--from", dest="ref", required=True)
    p.add_argument("--to", default="HEAD")
    p.set_defaults(func=cmd_diff)
    return ap.parse_args().func(ap.parse_args())


if __name__ == "__main__":
    sys.exit(main())
