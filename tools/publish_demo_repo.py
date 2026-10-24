#!/usr/bin/env python3
"""Assemble the publishable demo repository (`m2`, **reco-by2**; the exhibits and the tag, **reco-3qr**).

`publish/demo-repo/` holds the files a demo repository needs on top of the miniature: the tour
(`DEMO.md`), the answer key (`PATHOLOGIES.md`), the gate's policy (`fathom.toml`), the CI job that calls
Fathom's reusable workflow, and the README note that points a visitor at all three. This tool puts them
on top of the *real* history — the one packed in `crates/fathom-cli/assets/fathom-demo.pack`, the one
`fathom --demo` unpacks — after the exhibits the five-minute tour walks, and commits the template as the
last commit, so the published repository is the miniature plus its own exhibits and gate rather than a
directory that merely has the miniature inside it.

    python3 tools/publish_demo_repo.py build --out DIR    # assemble, commit, tag
    python3 tools/publish_demo_repo.py check              # assemble into a temp dir and audit it
    python3 tools/publish_demo_repo.py show               # what the assembly would touch

Why the pack and not a re-generation: the whole point of a *demo* repository is that a person who clones
it sees what the binary shows them. `gen_demo_repo.py write` is the generator and `gen_demo_repo.py
check` is what keeps the pack honest; this tool reads the committed pack, so the two can never disagree
about which history is canonical. That is also why the **exhibit commits are appended and the miniature
commits are not touched**: everything under the miniature's own tip is byte-identical to what the binary
unpacks, and `check` proves it by counting the commits between the pack's head and the assembled head.
The exhibits (`tools/demo_exhibits.py`) are the two shapes the miniature has no room for — a run of
episodes on one region, and a commit that is provably whitespace only.

Four things the assembly does that a plain copy would not:

* **the index is rebuilt**, by `unpack`, or `git status` in the published clone reports every tracked file
  as a staged deletion (the pack carries no index on purpose — see `gen_demo_repo.py`'s own docstring);
* **the ledger stays visible**: the miniature's `.gitignore` ignores `.fathom/` wholesale, which would
  make `fathom review sign`'s output uncommittable. The demo commit rewrites that line to ignore the
  disposable store while keeping `.fathom/reviews/` tracked — ADR-004 §12's sentence, made a fact in a
  repository somebody else clones;
* **the commit is attributable and repeatable**: a fixed author and a fixed message, so `check` can prove
  the assembly is a function of its inputs and nobody has to hand-fix a drift they cannot name;
* **the repository carries its version as a tag**, `demo-v<major>.<minor>`, derived from the crate's own
  version rather than typed. That is the acceptance criterion that says the demo is *re-tagged and
  re-recorded at each minor release*: a tag is the only place that sentence can be checked, and
  `docs/concepts/release.md` is where the release checklist says to move it.
"""
from __future__ import annotations

import argparse
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
import gen_demo_repo  # noqa: E402  (same tree, same rules, and the pack format is its business)
import demo_exhibits  # noqa: E402  (the seeded pathologies, and the commits that seed them)

TEMPLATE = ROOT / "publish" / "demo-repo"

#: What the extra commit is signed as. Fixed, so two runs are comparable (`check` below relies on it).
AUTHOR = "Fathom Demo <demo@fathom.test>"
MESSAGE = (
    "the demo's own tour, answer key and gate\n\n"
    "Written by tools/publish_demo_repo.py from publish/demo-repo: the tour (DEMO.md), the answer key\n"
    "(PATHOLOGIES.md), the gate's policy (fathom.toml), the job that calls Fathom's reusable\n"
    "fathom-check workflow, and the README note that points at them. Underneath are the miniature\n"
    "commits from crates/fathom-cli/assets/fathom-demo.pack, unchanged, and the exhibit commits from\n"
    "tools/demo_exhibits.py that seed the five pathologies the tour walks."
)

#: The miniature's commits occupy clock slots 0..20 in `gen_demo_repo.stamp`, so the exhibits start the
#: next hour after its merge. Same generator, same hour-a-part cadence, same determinism.
EXHIBIT_BASE = 21


def crate_version() -> str:
    """The crate's own version, read rather than remembered.

    The workspace root owns it (`[workspace.package] version`, which every crate inherits with
    `version.workspace = true`), so that is the one file to ask. `tools/check_release.py` is the gate that
    keeps the crates and the root agreeing; this just does not duplicate the number.
    """
    import tomllib

    document = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    version = document.get("workspace", {}).get("package", {}).get("version")
    if not version:
        raise SystemExit("Cargo.toml carries no [workspace.package] version to tag the demo with")
    return str(version)


def demo_tag() -> str:
    """The tag the demo repository carries: `demo-v<major>.<minor>`.

    Minor, not patch, because the acceptance criterion is "tagged and re-recorded at each minor
    release" — a patch release that does not change what the tour shows must not re-record a demo, and a
    tag that moved on a patch would make the recording look older than it is.
    """
    base, _, _ = crate_version().partition("-")
    major, minor, *_ = base.split(".")
    return f"demo-v{major}.{minor}"

#: The line the miniature's own `.gitignore` carries, and its replacement.
IGNORE_STORE = ".fathom/"
IGNORE_LEDGER = """\
.fathom/*
# ADR-004 §12: the store is disposable, the ledger is not. The reviews are what a sign-off commits
# to, so they are tracked — a demo whose sign-offs vanish on clone teaches the wrong lesson.
!.fathom/reviews/
"""


def git(cwd: pathlib.Path, *args: str, env: dict[str, str] | None = None) -> str:
    proc = subprocess.run(
        ["git", *args], cwd=cwd, capture_output=True, text=True, check=False, env=env
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


def apply_exhibits(dest: pathlib.Path) -> list[str]:
    """Write the seeded pathologies on top of the miniature, one believable commit at a time.

    The clock and the identities come from `gen_demo_repo`, which is what makes them as repeatable as the
    miniature's own commits. The subjects are what `tools/demo_exhibits.py` promises, and
    `tools/demo_acceptance.py` is what runs the tour against them; this function only makes the bytes.
    """
    subjects: list[str] = []
    for offset, step in enumerate(demo_exhibits.commits(dest)):
        env = gen_demo_repo.stamp(EXHIBIT_BASE + offset)
        for path, content in step.writes.items():
            target = dest / path
            target.parent.mkdir(parents=True, exist_ok=True)
            if content is None:
                target.unlink(missing_ok=True)
            else:
                target.write_text(content, encoding="utf-8")
        gen_demo_repo.git(dest, "add", "--all", env=env)
        message = step.subject
        if step.body:
            message += "\n\n" + step.body
        for key, value in step.trailers:
            message += f"\n\n{key}: {value}"
        gen_demo_repo.git(
            dest,
            "-c",
            f"user.name={step.who[0]}",
            "-c",
            f"user.email={step.who[1]}",
            "commit",
            "--quiet",
            "-m",
            message,
            env=env,
        )
        subjects.append(step.subject)
    return subjects


def assemble() -> tuple[pathlib.Path, dict]:
    work = pathlib.Path(tempfile.mkdtemp(prefix="fathom-demo-repo-"))
    dest = work / "signal"
    gen_demo_repo.unpack(dest, gen_demo_repo.PACK.read_bytes())
    head_before = git(dest, "rev-parse", "HEAD").strip()
    git(dest, "config", "user.name", "Fathom Demo")
    git(dest, "config", "user.email", "demo@fathom.test")
    exhibits = apply_exhibits(dest)
    touched = overlay(dest)
    stamp = gen_demo_repo.stamp(EXHIBIT_BASE + len(exhibits))
    environment = {**os.environ, **stamp}
    git(dest, "add", "--all", env=environment)
    git(
        dest,
        "commit",
        "--quiet",
        "--author",
        AUTHOR,
        "--date",
        stamp["GIT_AUTHOR_DATE"],
        "-m",
        MESSAGE,
        env=environment,
    )
    tag = demo_tag()
    git(
        dest,
        "tag",
        "-a",
        tag,
        "-m",
        f"the demo repository at {crate_version()} — tour, exhibits and recording as of this commit",
        env={**os.environ, **stamp},
    )
    facts = {
        "miniature_head": head_before,
        "demo_head": git(dest, "rev-parse", "HEAD").strip(),
        "commits": int(git(dest, "rev-list", "--count", "HEAD").strip()),
        "on_top_of_miniature": int(
            git(dest, "rev-list", "--count", f"{head_before}..HEAD").strip()
        ),
        "exhibits": exhibits,
        "tag": tag,
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
    expected = 21 + len(facts["exhibits"]) + 1
    if facts["commits"] != expected:
        problems.append(
            f"expected {expected} commits (21 miniature + {len(facts['exhibits'])} exhibits + the "
            f"demo's own), found {facts['commits']}"
        )
    if facts["on_top_of_miniature"] != len(facts["exhibits"]) + 1:
        problems.append(
            f"{facts['on_top_of_miniature']} commits sit on the miniature's head, not "
            f"{len(facts['exhibits']) + 1}: a miniature commit moved, and the clone is no longer what "
            "the binary unpacks"
        )
    log = git(dest, "log", "--format=%s").splitlines()
    for row in demo_exhibits.CATALOGUE:
        for subject in row["commits"]:
            if subject not in log:
                problems.append(f"`{row['pathology']}` is seeded by `{subject}`, which is not in the history")
    if not facts["clean_after"]:
        problems.append("`git status` is dirty after the assembly commit")
    shown = git(dest, "show", "--stat", "--name-only", "--pretty=format:", "HEAD").split()
    for expected_file in ("DEMO.md", "PATHOLOGIES.md", "fathom.toml", "README.md", ".gitignore"):
        if expected_file not in shown:
            problems.append(f"the demo commit does not touch {expected_file}")
    if not (dest / ".github" / "workflows" / "fathom.yml").is_file():
        problems.append("the published repository would arrive without its gate")
    workflow = (dest / ".github" / "workflows" / "fathom.yml").read_text(encoding="utf-8")
    if "fathom-check.yml" not in workflow:
        problems.append("the demo's workflow does not call Fathom's reusable workflow")
    if "fathom/" not in (dest / ".gitignore").read_text(encoding="utf-8"):
        problems.append(".gitignore no longer says what happens to .fathom/")
    if "!.fathom/reviews/" not in (dest / ".gitignore").read_text(encoding="utf-8"):
        problems.append("the review ledger is uncommittable in the published clone")
    tags = git(dest, "tag", "--list", "demo-v*").split()
    if facts["tag"] not in tags:
        problems.append(f"the assembled repository carries no `{facts['tag']}` tag")
    if tags != [facts["tag"]]:
        problems.append(f"the assembled repository carries {tags}, not just `{facts['tag']}`")
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
            f"demo repository assembles: {facts['commits']} commits "
            f"(21 miniature + {len(facts['exhibits'])} exhibits + the demo's own), "
            f"tagged {facts['tag']}, {len(facts['touched'])} template file(s), clean status, gate wired"
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
        f"tagged {facts['tag']}, demo commit touches {', '.join(facts['touched'])}"
    )
    for problem in problems:
        print(f"  note: {problem}", file=sys.stderr)
    print(
        "to publish it:\n"
        f"  git -C {args.out} remote add origin <your-remote>\n"
        f"  git -C {args.out} push -u origin {'HEAD'}:main\n"
        f"  git -C {args.out} push origin {facts['tag']}"
        "   # the version tag is part of the artefact"
    )
    return 0 if not problems else 1


if __name__ == "__main__":
    sys.exit(main())
