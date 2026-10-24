#!/usr/bin/env python3
"""Generate the shell artefacts from the CLI definition, and fail on drift (`e7.4`, **reco-3q4**).

The ticket's criterion is the sharp one: "*completions and man page are generated from clap definitions in
CI (drift is impossible)*". Two things have to be true for that sentence to mean something, and this file
is both of them:

1. **The generator is the CLI, not a copy of it.** It runs `fathom completions …`, which renders
   `crates/fathom-cli/src/verbs.rs` through `clap_complete` and `clap_mangen` — the same `clap::Command`
   `main` parses with. There is no second list of flags here to forget to update, and no Python parsing of
   `--help` text, which is the other way this kind of tool ends up half right.
2. **The generated files are shipped, and CI re-diffs them.** Generating into a build directory that
   nobody publishes would satisfy the letter of the criterion and none of its intent, so the output lives
   in the tree — `packaging/completions/` and `packaging/man/` — where `dist-workspace.toml`'s `include`
   puts it into every release archive, and where a reviewer can read it. `check` regenerates into a
   temporary directory and compares, so a flag added without a regeneration is a red build.

    python3 tools/gen_shell_artifacts.py write      # refresh packaging/ (needs a built binary)
    python3 tools/gen_shell_artifacts.py check      # what CI runs; exit 1 on any difference
    python3 tools/gen_shell_artifacts.py self-test  # prove `check` can go red, on throwaway copies

The binary is `target/debug/fathom` unless `--bin` or `$FATHOM_BIN` says otherwise, and it must be a
**default-feature** build: that is the shipped product (ADR-002 §6.1), and it is the same build
`tools/gen_docs.py --page cli` reads, so the two generators cannot describe different binaries. The verb
grammar itself does not depend on features — a verb compiled out refuses rather than disappearing — so
what this writes is correct for every build of this crate.

A version bump moves the version baked into every man page header, so a bump without a `write` is a red
build here and in `tools/check_release.py`. That is the point of §3 of the release runbook, not a
footgun: the artefacts a user installs must say the version they came from.

Exit: 0 clean, 1 drift or a missing input. One line per problem, in the style of every other gate here.
"""
from __future__ import annotations

import argparse
import contextlib
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
PACKAGE = ROOT / "packaging"
COMPLETIONS = PACKAGE / "completions"
MAN = PACKAGE / "man"

#: The two trees this generator owns — the only places a stale file may be pruned from. `packaging/` also
#: holds a hand-written release script and its own `README.md`, and `write` deletes whatever it no longer
#: renders, so "everything under packaging/" is a generator that eats a release. See `on_disk`.
OWNED = ("packaging/completions", "packaging/man")

#: `(shell, file name)`, in the order the release document lists them. The file names are what each
#: shell's own loader looks for: bash-completion loads a file named after the command, zsh sources a
#: leading-underscore name out of `$fpath`, and fish reads `vendor_completions.d/<command>.fish`.
SHELLS = [("bash", "fathom"), ("zsh", "_fathom"), ("fish", "fathom.fish")]

#: Where each shell is *meant* to put the file, for the document and for the `install` hint. These are
#: the distro-system locations; a user on a Mac with Homebrew has a prefix in front of all of them, and
#: `docs/concepts/release.md` says so instead of pretending one path fits all.
DESTINATIONS = {
    "bash": "/usr/share/bash-completion/completions/fathom",
    "zsh": "/usr/share/zsh/site-functions/_fathom",
    "fish": "/usr/share/fish/vendor_completions.d/fathom.fish",
}


class Drift(Exception):
    """One difference between the tree and what the binary renders. Carries the message only."""


def binary(args: argparse.Namespace) -> pathlib.Path:
    """The binary to ask, resolved the way `gen_docs.py` resolves it — and refused the same way."""
    candidate = args.bin or os.environ.get("FATHOM_BIN") or "target/debug/fathom"
    path = pathlib.Path(candidate)
    if not path.is_absolute():
        path = ROOT / path
    if not path.is_file():
        raise SystemExit(
            "no binary at %s; run `cargo build -p fathom-cli` first (default features: that is the\n"
            " shipped product, and the same build tools/gen_docs.py --page cli reads)" % path
        )
    return path


def run(bin_path: pathlib.Path, *args: str) -> bytes:
    """One `fathom completions …` invocation, stdout as bytes."""
    result = subprocess.run([str(bin_path), *args], capture_output=True, cwd=ROOT, check=False)
    if result.returncode != 0:
        raise SystemExit("fathom %s exited %d: %s"
                         % (" ".join(args), result.returncode, result.stderr.decode(errors="replace")))
    return result.stdout


def render(bin_path: pathlib.Path) -> dict[str, bytes]:
    """Every generated file, as `{repo-relative path: bytes}`.

    The man pages come back from `fathom completions man --out DIR`, which prints the paths it wrote, so
    this tool never has to know how many pages there are or how they are named: the binary owns that, and
    a verb added in Rust appears here on the next run without a change to this file.
    """
    out: dict[str, bytes] = {}
    for shell, name in SHELLS:
        script = run(bin_path, "completions", shell)
        if not script.strip():
            raise Drift("completions/%s/%s came out empty" % (shell, name))
        out["packaging/completions/%s/%s" % (shell, name)] = script

    with temp_dir() as work:
        printed = run(bin_path, "completions", "man", "--out", str(work)).decode()
        pages = [line for line in printed.splitlines() if line.strip()]
        if not pages:
            raise Drift("`fathom completions man --out` wrote no pages")
        for line in pages:
            page = pathlib.Path(line)
            rel = pathlib.Path("packaging/man/man1") / page.name
            out[rel.as_posix()] = page.read_bytes()
    return out


@contextlib.contextmanager
def temp_dir():
    path = pathlib.Path(tempfile.mkdtemp(prefix="gen-shell-artifacts-"))
    try:
        yield path
    finally:
        shutil.rmtree(path, ignore_errors=True)


def written(rel: str) -> pathlib.Path:
    return ROOT / rel


def command_write(args: argparse.Namespace) -> int:
    bin_path = binary(args)
    try:
        files = render(bin_path)
    except Drift as drift:
        print("  %-4s %s" % ("G01", drift))
        return 1
    for rel, body in sorted(files.items()):
        target = written(rel)
        target.parent.mkdir(parents=True, exist_ok=True)
        if not target.exists() or target.read_bytes() != body:
            target.write_bytes(body)
            print("  wrote %s" % rel)
    stale = sorted(set(on_disk()) - set(files))
    for rel in stale:
        (ROOT / rel).unlink()
        print("  pruned %s (the binary no longer renders it)" % rel)
    print("packaging/: %d completions, %d man pages, from %s"
          % (len(SHELLS), sum(1 for rel in files if rel.startswith("packaging/man")), bin_path))
    return 0


def on_disk() -> list[str]:
    """The files under the two trees this generator owns — and only those.

    The scope is the point. `packaging/` also holds things a release hand-writes (`linux-packages.sh`,
    this directory's `README.md`), and `write` prunes whatever it renders no more, so a generator that
    looks at all of `packaging/` deletes a release script the first time it runs. That happened, on the
    author's tree, on the first run; the narrow scope is the lesson, kept where it can be read.
    """
    found: list[str] = []
    for owned in OWNED:
        for path in (ROOT / owned).rglob("*"):
            if path.is_file():
                found.append(path.relative_to(ROOT).as_posix())
    return sorted(found)


def command_check(args: argparse.Namespace) -> int:
    bin_path = binary(args)
    try:
        files = render(bin_path)
    except Drift as drift:
        print("  %-4s %s" % ("G01", drift))
        return 1
    problems: list[str] = []
    for rel, body in sorted(files.items()):
        target = written(rel)
        if not target.exists():
            problems.append("%s is missing; run `python3 tools/gen_shell_artifacts.py write`" % rel)
        elif target.read_bytes() != body:
            problems.append("%s is not what the binary renders; run `python3 tools/gen_shell_artifacts.py write`"
                            % rel)
    for rel in sorted(set(on_disk()) - set(files)):
        problems.append("%s is committed but nothing renders it; delete it" % rel)
    if problems:
        for problem in problems:
            print("  %-4s %s" % ("G01", problem))
        print("\n%d problem(s)" % len(problems))
        return 1
    print("shell artefacts OK: %d completions + %d man pages are the binary's own grammar (%s)"
          % (len(SHELLS), sum(1 for rel in files if rel.startswith("packaging/man")), bin_path))
    return 0


def command_self_test(args: argparse.Namespace) -> int:
    """Prove `check` can go red, the way every gate in this repository is required to demonstrate.

    Each case copies `packaging/` and the checker's own inputs into a throwaway tree, breaks one thing,
    and demands the checker says so. Nothing here touches the real tree.
    """
    bin_path = binary(args)
    cases = [
        ("an empty completion file", lambda root: (root / "packaging/completions/zsh/_fathom")
            .write_bytes(b"")),
        ("a man page that lost its version", lambda root: _rewrite_first_man(root, _drop_version)),
        ("a man page for a verb that no longer exists", lambda root: (
            root / "packaging/man/man1/fathom-sideways.1").write_bytes(b".TH X 1\n")),
        ("a completion file that was deleted", lambda root: (
            root / "packaging/completions/fish/fathom.fish").unlink()),
    ]
    failures = 0
    with temp_dir() as base:
        for index, (what, mutate) in enumerate(cases):
            tree = base / ("case-%02d" % index)
            shutil.copytree(PACKAGE, tree / "packaging")
            mutate(tree)
            problems = _compare(tree, render(bin_path))
            if problems:
                print("  ok    check goes red when %s" % what)
            else:
                print("  FAIL  check stayed green when %s" % what)
                failures += 1
    if failures:
        print("\n%d self-test case(s) failed" % failures)
        return 1
    print("self-test OK: %d ways to be red, all of them detected" % len(cases))
    return 0


def _compare(tree: pathlib.Path, files: dict[str, bytes]) -> list[str]:
    """The same comparison `check` makes, against an arbitrary tree instead of the repository."""
    problems: list[str] = []
    for rel, body in sorted(files.items()):
        target = tree / rel
        if not target.exists():
            problems.append("%s missing" % rel)
        elif target.read_bytes() != body:
            problems.append("%s differs" % rel)
    on_tree = {str(p.relative_to(tree).as_posix()) for p in (tree / "packaging").rglob("*")
               if p.is_file() and p.name != "README.md"}
    problems.extend("%s extra" % rel for rel in sorted(on_tree - set(files)))
    return problems


def _rewrite_first_man(tree: pathlib.Path, edit) -> None:
    page = sorted((tree / "packaging/man/man1").glob("*.1"))[0]
    page.write_text(edit(page.read_text()))


def _drop_version(text: str) -> str:
    return text.replace("fathom 0", "fathom 9", 1)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="gen_shell_artifacts.py", description=__doc__.splitlines()[0])
    parser.add_argument("command", choices=["write", "check", "self-test"],
                        help="write packaging/, check it, or prove the check can fail")
    parser.add_argument("--bin", help="the fathom binary to generate from (default target/debug/fathom)")
    args = parser.parse_args(argv)
    if args.command == "write":
        return command_write(args)
    if args.command == "self-test":
        return command_self_test(args)
    return command_check(args)


if __name__ == "__main__":
    sys.exit(main())
