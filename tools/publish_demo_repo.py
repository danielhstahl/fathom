#!/usr/bin/env python3
"""Assemble the publishable demo repository (`m2`, **reco-by2**).

`publish/demo-repo/` holds the four files a demo repository needs on top of the miniature: the tour
(`DEMO.md`), the gate's policy (`fathom.toml`), the CI job that calls Fathom's reusable workflow, and the
README note that points a visitor at all three. This tool puts them on top of the *real* history — the
one packed in `crates/fathom-cli/assets/fathom-demo.pack`, the one `fathom --demo` unpacks — and commits
them as a twenty-second commit, so the published repository is the miniature plus its own gate rather
than a directory that merely has the miniature inside it.

    python3 tools/publish_demo_repo.py build --out DIR    # assemble (and commit)
    python3 tools/publish_demo_repo.py check              # assemble into a temp dir and audit it
    python3 tools/publish_demo_repo.py show               # what the commit would touch

Why the pack and not a re-generation: the whole point of a *demo* repository is that a person who clones
it sees what the binary shows them. `gen_demo_repo.py write` is the generator and `gen_demo_repo.py
check` is what keeps the pack honest; this tool reads the committed pack, so the two can never disagree
about which history is canonical.

Three things the assembly does that a plain copy would not:

* **the index is rebuilt**, by `unpack`, or `git status` in the published clone reports every tracked file
  as a staged deletion (the pack carries no index on purpose — see `gen_demo_repo.py`'s own docstring);
* **the ledger stays visible**: the miniature's `.gitignore` ignores `.fathom/` wholesale, which would
  make `fathom review sign`'s output uncommittable. The demo commit rewrites that line to ignore the
  disposable store while keeping `.fathom/reviews/` tracked — ADR-004 §12's sentence, made a fact in a
  repository somebody else clones;
* **the commit is attributable and repeatable**: a fixed author and a fixed message, so `check` can prove
  the assembly is a function of its inputs and nobody has to hand-fix a drift they cannot name.
"""
from __future__ import annotations

import argparse
import pathlib
import shutil
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
import gen_demo_repo  # noqa: E402  (same tree, same rules, and the pack format is its business)

TEMPLATE = ROOT / "publish" / "demo-repo"

#: What the extra commit is signed as. Fixed, so two runs are comparable (`check` below relies on it).
AUTHOR = "Fathom Demo <demo@fathom.test>"
MESSAGE = (
    "the demo's own tour and gate\n\n"
    "Written by tools/publish_demo_repo.py from publish/demo-repo: the tour (DEMO.md), the gate's\n"
    "policy (fathom.toml), the job that calls Fathom's reusable fathom-check workflow, and the\n"
    "README note that points at them. The 21 commits underneath are the miniature from\n"
    "crates/fathom-cli/assets/fathom-demo.pack, unchanged; this is the twenty-second, and it is the\n"
    "only one the demo repository adds to it."
)

#: The line the miniature's own `.gitignore` carries, and its replacement.
IGNORE_STORE = ".fathom/"
IGNORE_LEDGER = """\
.fathom/*
# ADR-004 §12: the store is disposable, the ledger is not. The reviews are what a sign-off commits
# to, so they are tracked — a demo whose sign-offs vanish on clone teaches the wrong lesson.
!.fathom/reviews/
"""


def git(cwd: pathlib.Path, *args: str) -> str:
    proc = subprocess.run(
        ["git", *args], cwd=cwd, capture_output=True, text=True, check=False
    )
    if proc.returncode != 0:
        raise SystemExit(
            f"git {' '.join(args)} failed in {cwd} ({proc.returncode}):\n{proc.stderr.strip()}"
        )
    return proc.stdout


def overlay(dest: pathlib.Path) -> list[str]:
    """Copy the template over the unpacked miniature, appending where the miniature already has a file."""
    touched: list[str] = []
    for source in sorted(TEMPLATE.rglob("*")):
        if not source.is_file():
            continue
        if source.name == "README-note.md":
            # Not a file the published repository carries — it is the text appended to the miniature's
            # own README below, and a second copy of it would be a second thing to keep in step.
            continue
        relative = source.relative_to(TEMPLATE)
        target = dest / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)
        touched.append(relative.as_posix())
    readme = dest / "README.md"
    note = TEMPLATE / "README-note.md"
    if readme.is_file() and note.is_file():
        body = readme.read_text(encoding="utf-8").rstrip("\n")
        readme.write_text(
            body + "\n\n" + note.read_text(encoding="utf-8"), encoding="utf-8"
        )
        touched.append("README.md")
    ignore = dest / ".gitignore"
    if ignore.is_file():
        text = ignore.read_text(encoding="utf-8")
        if IGNORE_STORE in text:
            text = text.replace(IGNORE_STORE, IGNORE_LEDGER)
        else:
            text = text.rstrip("\n") + "\n" + IGNORE_LEDGER
        ignore.write_text(text, encoding="utf-8")
        touched.append(".gitignore")
    return sorted(set(touched))


def assemble() -> tuple[pathlib.Path, dict]:
    work = pathlib.Path(tempfile.mkdtemp(prefix="fathom-demo-repo-"))
    dest = work / "signal"
    gen_demo_repo.unpack(dest, gen_demo_repo.PACK.read_bytes())
    head_before = git(dest, "rev-parse", "HEAD").strip()
    touched = overlay(dest)
    git(dest, "config", "user.name", "Fathom Demo")
    git(dest, "config", "user.email", "demo@fathom.test")
    git(dest, "add", "--all")
    git(dest, "commit", "--quiet", "--author", AUTHOR, "--date", "@0 +0000", "-m", MESSAGE)
    facts = {
        "miniature_head": head_before,
        "demo_head": git(dest, "rev-parse", "HEAD").strip(),
        "commits": int(git(dest, "rev-list", "--count", "HEAD").strip()),
        "branch": git(dest, "rev-parse", "--abbrev-ref", "HEAD").strip(),
        "touched": touched,
        "clean_after": git(dest, "status", "--porcelain").strip() == "",
    }
    return dest, facts


def check(dest: pathlib.Path, facts: dict) -> list[str]:
    """Every promise the assembly makes, as a failure someone else cannot quietly skip."""
    problems: list[str] = []
    manifest = gen_demo_repo.MANIFEST
    if manifest.is_file():
        import json

        packed_head = json.loads(manifest.read_text(encoding="utf-8")).get("head")
        if packed_head and not facts["miniature_head"].startswith(packed_head[:12]):
            problems.append(
                f"the miniature underneath is {facts['miniature_head'][:12]}, the committed manifest "
                f"says {packed_head[:12]} — rebuild the pack or republish from it"
            )
    if facts["commits"] != 22:
        problems.append(f"expected 22 commits (21 miniature + the demo's own), found {facts['commits']}")
    if not facts["clean_after"]:
        problems.append("`git status` is dirty after the assembly commit")
    shown = git(dest, "show", "--stat", "--name-only", "--pretty=format:", "HEAD").split()
    for expected in ("DEMO.md", "fathom.toml", "README.md", ".gitignore"):
        if expected not in shown:
            problems.append(f"the demo commit does not touch {expected}")
    if not (dest / ".github" / "workflows" / "fathom.yml").is_file():
        problems.append("the published repository would arrive without its gate")
    workflow = (dest / ".github" / "workflows" / "fathom.yml").read_text(encoding="utf-8")
    if "fathom-check.yml" not in workflow:
        problems.append("the demo's workflow does not call Fathom's reusable workflow")
    if "fathom/" not in (dest / ".gitignore").read_text(encoding="utf-8"):
        problems.append(".gitignore no longer says what happens to .fathom/")
    if "!.fathom/reviews/" not in (dest / ".gitignore").read_text(encoding="utf-8"):
        problems.append("the review ledger is uncommittable in the published clone")
    return problems


def main() -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    parser.add_argument("action", choices=["build", "check", "show"])
    parser.add_argument("--out", type=pathlib.Path, help="build: where to write the repository")
    args = parser.parse_args()

    if not gen_demo_repo.PACK.is_file():
        print(f"{gen_demo_repo.PACK} is missing; run `tools/gen_demo_repo.py write`", file=sys.stderr)
        return 1
    if not TEMPLATE.is_dir():
        print(f"{TEMPLATE} is missing its template files", file=sys.stderr)
        return 1

    dest, facts = assemble()
    if args.action == "show":
        for key, value in facts.items():
            print(f"{key}: {value}")
        shutil.rmtree(dest.parent, ignore_errors=True)
        return 0

    if args.action == "check":
        problems = check(dest, facts)
        shutil.rmtree(dest.parent, ignore_errors=True)
        if problems:
            for problem in problems:
                print(f"publish/demo-repo: {problem}", file=sys.stderr)
            return 1
        print(
            f"demo repository assembles: {facts['commits']} commits, "
            f"{len(facts['touched'])} template file(s), clean status, gate wired"
        )
        return 0

    if args.out is None:
        print("build needs --out", file=sys.stderr)
        return 2
    if args.out.exists() and any(args.out.iterdir()):
        print(f"{args.out} exists and is not empty; refusing to overwrite", file=sys.stderr)
        return 1
    args.out.parent.mkdir(parents=True, exist_ok=True)
    shutil.move(str(dest), str(args.out))
    shutil.rmtree(dest.parent, ignore_errors=True)
    problems = check(args.out, facts)
    print(
        f"wrote {args.out}: {facts['commits']} commits, head {facts['demo_head'][:12]}, "
        f"demo commit touches {', '.join(facts['touched'])}"
    )
    for problem in problems:
        print(f"  note: {problem}", file=sys.stderr)
    print(
        "to publish it:\n"
        f"  git -C {args.out} remote add origin <your-remote>\n"
        f"  git -C {args.out} push -u origin {'HEAD'}:main"
    )
    return 0 if not problems else 1


if __name__ == "__main__":
    sys.exit(main())
