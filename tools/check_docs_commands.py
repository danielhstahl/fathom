#!/usr/bin/env python3
"""Check that a `fathom …` command written in the docs is a command the binary accepts.

Part of `e7.5` (`reco-60m`). A recipe whose command does not type is worse than a recipe with no command:
the reader finds out, not the author. Every invocation in a *command* block is handed to clap itself with
`--help` appended, which parses the grammar and prints help **without running the verb** — so this catches
the two failures a documentation review misses:

* a **subcommand that does not exist** (`fathom review start`, `fathom review sign-off`, `fathom ratchet`),
  which is what a page writes when it copies an ADR's vocabulary instead of the CLI's; and
* a **flag that does not exist** (`fathom review status --all`, `--verdict`, `--episode` on a positional),
  which is what a page writes when it guesses.

Only blocks that claim to be commands are checked: a fence tagged `bash`/`sh`/`console`, or a line prefixed
with `$ `. A fence with no language is prose or transcribed output, and speculative verb lists elsewhere in
`docs/` live in those — this rule is about the commands a page tells you to type.

Arguments are passed as an argv list with no shell, so `$(…)` and `*` inside an example are inert literals
rather than something this check executes. Lines with an ellipsis placeholder are skipped.
"""
from __future__ import annotations

import argparse
import pathlib
import shlex
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
DOCS = ROOT / "docs"
COMMAND_FENCES = {"bash", "sh", "console", "shell"}
CUT_AT = ("|", ">", ">>", "2>&1", "&&", "||", "<")
# A line that says it belongs to a build that does not exist yet is documentation about the future, not a
# command the reader is told to type, so it is exempt. The word is on the line rather than inferred from
# the page because "this verb does not exist" and "this verb will change" get discussed in the same page.
FUTURE_MARKERS = ("planned", "not built", "not implemented", "will replace", "does not exist")


def commands(text: str) -> list[tuple[int, str]]:
    """Every command a page tells the reader to type, with its line number."""
    out: list[tuple[int, str]] = []
    fence: str | None = None
    for number, raw in enumerate(text.splitlines(), 1):
        stripped = raw.strip()
        if fence is None:
            if stripped.startswith("```"):
                fence = stripped[3:].strip().lower()
            continue
        if stripped.startswith("```"):
            fence = None
            continue
        line = stripped[2:].strip() if stripped.startswith("$ ") else stripped
        if line.startswith("fathom") and (fence in COMMAND_FENCES or stripped.startswith("$ ")):
            out.append((number, line))
    return out


def argv_of(line: str) -> list[str] | None:
    line = line.rstrip("\\").strip()
    if any(marker in line for marker in FUTURE_MARKERS):
        return None
    line = line.split(";")[0].strip()
    if "…" in line or "`" in line:
        return None
    try:
        tokens = shlex.split(line, posix=True)
    except ValueError:
        return None
    kept: list[str] = []
    for token in tokens:
        if token in CUT_AT or token.startswith("$(") or token.startswith("#"):
            break
        if token.startswith("#"):
            break
        kept.append(token)
    if not kept or kept[0] != "fathom":
        return None
    return kept[1:]


def problems(binary: pathlib.Path, docs: pathlib.Path = DOCS) -> list[str]:
    """Every documented command clap will not accept, as `path:line: command` plus clap's own sentence."""
    out: list[str] = []
    for path in sorted(docs.rglob("*.md")):
        if path.name == "cli.md":
            continue  # the generated page quotes clap verbatim; it cannot disagree with itself
        text = path.read_text(encoding="utf-8")
        for number, line in commands(text):
            rest = argv_of(line)
            if rest is None:
                continue
            probe = subprocess.run([str(binary)] + rest + ["--help"],
                                   capture_output=True, text=True)
            err = (probe.stderr or "").strip()
            if any(word in err for word in ("unrecognized subcommand", "unexpected argument",
                                            "invalid subcommand")):
                out.append("%s:%d: %s\n    %s"
                           % (path.relative_to(ROOT), number, line,
                              err.splitlines()[0] if err else "clap refused it"))
    return out


def count(docs: pathlib.Path = DOCS) -> int:
    total = 0
    for path in sorted(docs.rglob("*.md")):
        if path.name == "cli.md":
            continue
        for number, line in commands(path.read_text(encoding="utf-8")):
            if argv_of(line) is not None:
                total += 1
    return total


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--bin", default=str(ROOT / "target/debug/fathom"))
    ap.add_argument("-v", "--verbose", action="store_true")
    args = ap.parse_args(argv)
    binary = pathlib.Path(args.bin)
    if not binary.exists():
        print("note: %s not built; no command check performed" % binary)
        return 0
    found = problems(binary)
    for problem in found:
        print(problem)
    if found:
        print("\n%d documented command(s) the binary will not accept" % len(found))
        return 1
    print("documented commands OK: %d invocation(s) parsed by clap" % count())
    return 0


if __name__ == "__main__":
    sys.exit(main())
