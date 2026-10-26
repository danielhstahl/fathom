#!/usr/bin/env python3
"""One version, seven places, one command.

The version is *declared* once — `[workspace.package] version` in `Cargo.toml` — and *derived* in six
other places. Every one of them is a red build, a misnamed artefact or a broken install if it gets left
behind, which is why a bump has never been one edit:

| Where the version ends up | Who refuses the drift |
|---|---|
| the seven path-dependency requirements in `Cargo.toml` | `R05` / `P02` (and `cargo build`, loudly: a path dep requiring `0.1.0` does not resolve against a `0.1.0-alpha.1` crate) |
| the eight `Cargo.lock` member entries | `P03` (and `tests/test_release_contract.py`) |
| the four `.deb`/`.rpm` names in `dist-workspace.toml` | `R15` / `P04` |
| every man page's `.TH` header | `gen_shell_artifacts.py check` / `P05` |
| README's two install one-liners | `R09` / `P06` |
| `benchmarks/reports/dist-release-plan.json` | `R10` / `P07` (only `dist` can refresh it) |
| `CHANGELOG.md`'s `## [version]` section | `R13` / `P08` (and `cargo-dist`, at publish time — the worst moment) |

Three commands:

* `prepare --version X` moves all seven, in the order the generators require (`docs/concepts/release.md`
  §3 step 3), and finishes with the two gates so the tree is CI-green before it is committed;
* `check` re-derives the same seven offline — no `dist`, no build, no network — and is what CI asserts;
* `self-test` mutates a throwaway tree one file at a time and requires `check` to name the rule, because
  a gate nobody has seen fail is decoration.

The **tag is not this tool's job**. The version on the default branch *is* the trigger:
`.github/workflows/publish.yml` dispatches `release.yml` for the version the tree carries, and
`release.yml` creates the tag and the GitHub Release together — so the tag can never name a version the
tree does not carry, and no one hand-writes a tag again. `prepare` therefore refuses to stop at "edited":
it prints what the push will set off.

Run: python3 tools/release.py prepare --version 0.1.0-alpha.2
     python3 tools/release.py check [--version X]
     python3 tools/release.py self-test
Exit: 0 clean, 1 problems found (one line per problem, rule id first).
"""
from __future__ import annotations

import argparse
import json
import pathlib
import re
import subprocess
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "tools"))

from package_release import package_version as debian_version  # noqa: E402  (the `~` spelling, once)

SEMVER = re.compile(r"^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$")

CARGO = "Cargo.toml"
LOCK = "Cargo.lock"
DIST_CONFIG = "dist-workspace.toml"
README = "README.md"
CHANGELOG = "CHANGELOG.md"
PLAN = "benchmarks/reports/dist-release-plan.json"
MAN = "packaging/man/man1"
CRATES = "crates"


class Problem:
    """One refusal. `rule` is `P01`–`P08` so a message can be looked up in this file."""

    def __init__(self, rule: str, where: str, why: str) -> None:
        self.rule, self.where, self.why = rule, where, why

    def line(self) -> str:
        return "%s %s: %s" % (self.rule, self.where, self.why)


def run(argv: list[str], cwd: pathlib.Path | None = None) -> int:
    """Run a generator in the open. No capture: the noise is the point of a release run."""
    print("   $ " + " ".join(argv))
    return subprocess.run(argv, cwd=str(cwd or ROOT)).returncode


def have(program: str) -> bool:
    from shutil import which
    return which(program) is not None


# ---------------------------------------------------------------------------
# reading the version out of the tree it is supposed to be in
# ---------------------------------------------------------------------------


def declared(root: pathlib.Path) -> str:
    """`[workspace.package] version`, as text: this must not need a `cargo` build."""
    text = (root / CARGO).read_text()
    match = re.search(r'^\[workspace\.package\][^[]*?^\s*version\s*=\s*"([^"]+)"', text, re.S | re.M)
    return match.group(1) if match else ""


def path_dep_requirements(root: pathlib.Path) -> list[str]:
    """Every `version = ` on a line that also carries `path = ` — the requirements that must move."""
    out = []
    for line in (root / CARGO).read_text().splitlines():
        if 'path = "' in line and 'version = "' in line:
            out.append(re.search(r'version = "([^"]*)"', line).group(1))
    return out


def locked_versions(root: pathlib.Path) -> dict[str, str]:
    """The lockfile's idea of each workspace member's version."""
    names = set()
    for manifest in sorted((root / CRATES).glob("*/Cargo.toml")):
        try:
            names.add(str(tomllib.loads(manifest.read_text()).get("package", {}).get("name", "")))
        except tomllib.TOMLDecodeError:
            continue
    if not (root / LOCK).exists():
        return {}
    lock = tomllib.loads((root / LOCK).read_text())
    return {str(p.get("name")): str(p.get("version", "")) for p in lock.get("package", [])
            if str(p.get("name")) in names}


def packaged_names(root: pathlib.Path) -> list[str]:
    """The `.deb`/`.rpm` file names `dist` is told to publish."""
    if not (root / DIST_CONFIG).exists():
        return []
    config = tomllib.loads((root / DIST_CONFIG).read_text())
    names = []
    for extra in config.get("dist", {}).get("extra-artifacts", []):
        for path in extra.get("artifacts", []):
            name = str(path).rsplit("/", 1)[-1]
            if name.endswith((".deb", ".rpm")):
                names.append(name)
    return names


def man_page_versions(root: pathlib.Path) -> dict[str, str]:
    """What each committed man page says it documents (`.TH NAME 1 "fathom <version>"`)."""
    out = {}
    for page in sorted((root / MAN).glob("*.1")):
        match = re.search(r'^\.TH\s+\S+\s+1\s+"fathom\s+([^"]+)"', page.read_text(), re.M)
        out[page.name] = match.group(1) if match else ""
    return out


def plan_version(root: pathlib.Path) -> str:
    """The version the captured `dist plan` announces, or `""` when nothing is captured."""
    path = root / PLAN
    if not path.exists():
        return ""
    captured = json.loads(path.read_text())
    return str(captured.get("announcement", {}).get("tag", "")).lstrip("v")


def changelog_has(root: pathlib.Path, version: str) -> tuple[bool, bool]:
    """(section exists, section is dated). The section *is* the release body."""
    if not (root / CHANGELOG).exists():
        return False, False
    for line in (root / CHANGELOG).read_text().splitlines():
        match = re.match(r"^## \[([^\]]+)\]\s*(.*)$", line.strip())
        if match and match.group(1) == version:
            return True, bool(match.group(2).strip(" -–"))
    return False, False


def readme_urls_current(root: pathlib.Path, version: str) -> bool:
    """README's install one-liners must name the release the installers will download from (`R09`)."""
    if not (root / README).exists():
        return False
    text = (root / README).read_text()
    return "/releases/download/v%s/" % version in text


# ---------------------------------------------------------------------------
# checking: the same seven, offline
# ---------------------------------------------------------------------------


def check(root: pathlib.Path, version: str | None = None) -> tuple[list[Problem], list[str]]:
    """Every place the version must have arrived, re-derived from the tree."""
    problems: list[Problem] = []
    notes: list[str] = []
    want = version or declared(root)
    if not SEMVER.match(want or ""):
        return [Problem("P01", CARGO,
                        "[workspace.package] version %r is not X.Y.Z(-prerelease)" % want)], notes

    stale = [v for v in path_dep_requirements(root) if v != want]
    if stale:
        problems.append(Problem("P02", CARGO, "%d path-dep requirement(s) still say %s; the workspace is %s"
                                % (len(stale), ", ".join(sorted(set(stale))), want)))

    locks = locked_versions(root)
    behind = {name: got for name, got in sorted(locks.items()) if got != want}
    if behind:
        problems.append(Problem("P03", LOCK, "%s; run `cargo update --workspace`"
                                % ", ".join("%s is locked at %s (workspace is %s)"
                                            % (name, got, want) for name, got in sorted(behind.items()))))
    elif not locks:
        problems.append(Problem("P03", LOCK, "no workspace members found in the lockfile"))

    spelled = debian_version(want)
    wrong = [n for n in packaged_names(root) if spelled not in n]
    if wrong:
        problems.append(Problem("P04", DIST_CONFIG, "%s does not carry the package version %s; dpkg and "
                                "dnf read the file name" % (", ".join(wrong), spelled)))

    pages = man_page_versions(root)
    if not pages:
        problems.append(Problem("P05", MAN, "no man pages; run `python3 tools/gen_shell_artifacts.py write`"))
    else:
        old = sorted(name for name, got in pages.items() if got != want)
        if old:
            problems.append(Problem("P05", MAN, "%d page(s) say %s, not %s (e.g. %s)"
                                    % (len(old), ", ".join(sorted(set(pages[n] for n in old))), want,
                                       old[0])))

    if not readme_urls_current(root, want):
        problems.append(Problem("P06", README, "does not quote `/releases/download/v%s/`, so the "
                                "documented install line is not the release being cut" % want))

    captured = plan_version(root)
    if not captured:
        problems.append(Problem("P07", PLAN, "no captured plan; run `dist plan --output-format=json | "
                                "python3 tools/check_release.py capture`"))
    elif captured != want:
        problems.append(Problem("P07", PLAN, "announces v%s while the tree carries %s; `dist` needs to "
                                "re-measure it (this cannot be faked offline)" % (captured, want)))

    seen, dated = changelog_has(root, want)
    if not seen:
        problems.append(Problem("P08", CHANGELOG, "no `## [%s]` section; `cargo-dist` builds the release "
                                "body out of that file" % want))
    elif not dated:
        problems.append(Problem("P08", CHANGELOG, "`## [%s]` has no date" % want))

    if not problems:
        notes.append("v%s is in all seven places: manifest, %d path deps, %d lock entries, %d package "
                     "name(s), %d man page(s), README, captured plan, changelog"
                     % (want, len(path_dep_requirements(root)), len(locks), len(packaged_names(root)),
                        len(pages)))
    return problems, notes


def command_check(root: pathlib.Path, version: str | None) -> int:
    problems, notes = check(root, version)
    for problem in problems:
        print(problem.line())
    for note in notes:
        print("ok: %s" % note)
    return 1 if problems else 0


# ---------------------------------------------------------------------------
# moving it
# ---------------------------------------------------------------------------


def bump_manifest(root: pathlib.Path, old: str, new: str, dry: bool) -> list[str]:
    """The declared version and the path-dep requirements, as text surgery (comments intact)."""
    path = root / CARGO
    text = path.read_text()
    moved = []
    head = re.compile(r'^(\[workspace\.package\][^[]*?^\s*version\s*=\s*")' + re.escape(old) + '(")',
                      re.S | re.M)
    if head.search(text):
        text = head.sub(lambda m: m.group(1) + new + m.group(2), text, count=1)
        moved.append("[workspace.package] version")
    kept = []
    for line in text.splitlines(keepends=True):
        if 'path = "' in line and 'version = "%s"' % old in line:
            line = line.replace('version = "%s"' % old, 'version = "%s"' % new)
            moved.append(line.strip().split(" ")[0].strip() + " requirement")
        kept.append(line)
    text = "".join(kept)
    if not dry:
        path.write_text(text)
    return moved


def bump_dist_config(root: pathlib.Path, old: str, new: str, dry: bool) -> list[str]:
    """The `.deb`/`.rpm` names inside `[[dist.extra-artifacts]]`, in both spellings.

    Scoped to that table on purpose: a blanket string replace over this file is how a future
    `cargo-dist-version` or a hand-written URL silently changes too.
    """
    path = root / DIST_CONFIG
    lines = path.read_text().splitlines(keepends=True)
    moved = []
    inside = False
    for index, line in enumerate(lines):
        if line.startswith("["):
            inside = line.startswith("[[dist.extra-artifacts]]")
        if not inside:
            continue
        for before, after in ((debian_version(old), debian_version(new)), (old, new)):
            if before in line:
                lines[index] = line.replace(before, after)
                line = lines[index]
                moved.append(line.strip())
    if not dry and moved:
        path.write_text("".join(lines))
    return moved


def bump_readme(root: pathlib.Path, old: str, new: str, dry: bool) -> list[str]:
    """The two install one-liners, and only the URL forms.

    Prose that mentions an old tag is history, not drift, so it is reported rather than rewritten: a tool
    that rewrites a sentence about a release that already happened is a tool that falsifies the record.
    """
    path = root / README
    if not path.exists():
        return []
    text = path.read_text()
    moved = []
    for shape in ("/releases/download/v%s", "/releases/tag/v%s"):
        before, after = shape % old, shape % new
        if before in text:
            moved.append("%d occurrence(s) of %s" % (text.count(before), before))
            text = text.replace(before, after)
    if not dry and moved:
        path.write_text(text)
    stragglers = [n for n, line in enumerate(text.splitlines(), 1)
                  if re.search(r"\bv%s\b" % re.escape(old), line)]
    if stragglers:
        print("   note: README line(s) %s still mention v%s in prose; check by eye"
              % (", ".join(str(n) for n in stragglers), old))
    return moved


def command_prepare(root: pathlib.Path, args: argparse.Namespace) -> int:
    new = args.version.lstrip("v")
    old = declared(root)
    if not SEMVER.match(new):
        print("P01 --version: %r is not X.Y.Z(-prerelease)" % new)
        return 1
    if not old:
        print("P01 %s: cannot read [workspace.package] version" % CARGO)
        return 1
    existing = subprocess.run(["git", "tag", "-l", "v%s" % new], cwd=str(root),
                              capture_output=True, text=True).stdout.strip()
    if existing and not args.force:
        print("refusing: v%s already exists here, and a release is cut once (say --force to mean it)" % new)
        return 1
    if old == new:
        print("the tree already carries %s: regenerating the derived files only" % new)

    print("release %s -> %s in %s%s" % (old, new, root, " (dry run: nothing is written)"
                                         if args.dry_run else ""))
    failed: list[str] = []

    def step(label: str, action) -> None:
        print("- %s" % label)
        try:
            action()
        except StepSkipped as skipped:
            print("   skipped: %s" % skipped)
            failed.append(label)

    class StepSkipped(Exception):
        pass

    def do_manifest() -> None:
        moved = bump_manifest(root, old, new, args.dry_run)
        print("   %s" % (", ".join(moved) if moved else "already at %s" % new))

    def do_dist_config() -> None:
        moved = bump_dist_config(root, old, new, args.dry_run)
        print("   %s" % ("; ".join(moved) if moved else "no artefact names to move"))

    def do_readme() -> None:
        moved = bump_readme(root, old, new, args.dry_run)
        print("   %s" % ("; ".join(moved) if moved else "no install URLs to move"))

    def do_lock() -> None:
        if args.dry_run:
            return
        if not have("cargo"):
            raise StepSkipped("no cargo on PATH; run `cargo update --workspace`")
        if run(["cargo", "update", "--workspace", "--offline"], root) != 0 and \
                run(["cargo", "update", "--workspace"], root) != 0:
            raise StepSkipped("cargo update failed; run it by hand")

    def do_changelog() -> None:
        seen, _ = changelog_has(root, new)
        if seen:
            print("   `## [%s]` already cut" % new)
            return
        if args.dry_run:
            return
        argv = [sys.executable, "tools/gen_changelog.py", "cut", "--version", new]
        if args.date:
            argv += ["--date", args.date]
        if args.force:
            argv += ["--force"]
        if run(argv, root) != 0:
            raise StepSkipped("gen_changelog.py cut refused (its audit is the gate: fix the "
                              "coverage, or pass --force)")

    def do_binary() -> None:
        if args.no_build:
            raise StepSkipped("--no-build; the man pages and the CLI page are stale until "
                              "`cargo build -p fathom-cli` runs")
        if not have("cargo"):
            raise StepSkipped("no cargo on PATH")
        if args.dry_run:
            return
        if run(["cargo", "build", "-p", "fathom-cli"], root) != 0:
            raise StepSkipped("cargo build failed")

    def do_shell_artefacts() -> None:
        if args.dry_run:
            return
        if run([sys.executable, "tools/gen_shell_artifacts.py", "write"], root) != 0:
            raise StepSkipped("gen_shell_artifacts.py write failed (does target/debug/fathom exist?)")

    def do_docs() -> None:
        if args.dry_run:
            return
        if run([sys.executable, "tools/gen_docs.py", "write", "--page", "cli"], root) != 0:
            raise StepSkipped("gen_docs.py write --page cli failed")

    def do_dist() -> None:
        if not have("dist"):
            raise StepSkipped("no `dist` on PATH. Until you run these two, `R10` and the generated "
                              "artefact table are stale:\n"
                              "     dist generate\n"
                              "     dist plan --output-format=json | python3 tools/check_release.py capture\n"
                              "     python3 tools/check_release.py write")
        if args.dry_run:
            return
        if run(["dist", "generate"], root) != 0:
            raise StepSkipped("`dist generate` failed")

    def do_capture() -> None:
        if not have("dist"):
            raise StepSkipped("needs `dist` (see above)")
        if args.dry_run:
            return
        plan = subprocess.run(["dist", "plan", "--output-format=json"], cwd=str(root),
                              capture_output=True, text=True)
        if plan.returncode != 0:
            raise StepSkipped("`dist plan` failed")
        capture = subprocess.run([sys.executable, "tools/check_release.py", "capture"], cwd=str(root),
                                 input=plan.stdout, text=True)
        if capture.returncode != 0:
            raise StepSkipped("check_release.py capture failed")

    def do_release_doc() -> None:
        if args.dry_run:
            return
        if run([sys.executable, "tools/check_release.py", "write"], root) != 0:
            raise StepSkipped("check_release.py write failed")

    def do_fmt() -> None:
        if args.dry_run or not have("cargo"):
            return
        run(["cargo", "fmt"], root)

    # The order is the one `docs/concepts/release.md` §3 step 3 records, and it is not arbitrary:
    # `gen_changelog.py cut` refuses a version the manifest does not carry (`C04`), so the manifest moves
    # first; `dist generate` reads the artefact names, so the config moves before it; the man pages and
    # the CLI reference come from a built binary; and the release page renders from the *re-captured* plan.
    step("manifest: [workspace.package] version and the path-dep requirements", do_manifest)
    step("dist-workspace.toml: the .deb/.rpm names", do_dist_config)
    step("README: the install one-liners", do_readme)
    step("Cargo.lock", do_lock)
    step("CHANGELOG.md: cut the section", do_changelog)
    step("dist generate (release.yml from dist-workspace.toml)", do_dist)
    step("cargo build -p fathom-cli (the artefacts come from this binary)", do_binary)
    step("packaging/completions and packaging/man", do_shell_artefacts)
    step("the CLI reference page", do_docs)
    step("re-measure the plan", do_capture)
    step("docs/concepts/release.md", do_release_doc)
    step("cargo fmt", do_fmt)

    if args.dry_run:
        print("\ndry run: nothing written. Re-run without --dry-run to do it.")
        return 0

    print("\nverdict on the tree as it now stands:")
    code = command_check(root, new)
    print("and the full release gate says:")
    gate = subprocess.run([sys.executable, "tools/check_release.py", "--tag", "v%s" % new], cwd=str(root))
    code = code or gate.returncode
    print("\n%s" % ("green: commit it. Pushing this to the default branch cuts v%s — publish.yml "
                    "dispatches release.yml, which creates the tag and the release together." % new
                    if code == 0 else
                    "not green: fix the lines above before pushing. Anything red here is red in CI, "
                    "and a tag pushed on top of it releases a broken tree."))
    if failed:
        print("\noutstanding (re-run this command once the tool is available):")
        for label in failed:
            print("  - %s" % label)
    return code


# ---------------------------------------------------------------------------
# proving the check can go red
# ---------------------------------------------------------------------------


def _fixture(tree: pathlib.Path) -> pathlib.Path:
    """A throwaway tree that carries a version in all seven places, with no `dist` and no binary."""
    version = "0.1.0-alpha.1"
    spelled = debian_version(version)
    (tree / CRATES / "fathom-cli").mkdir(parents=True, exist_ok=True)
    (tree / CRATES / "fathom-core").mkdir(parents=True, exist_ok=True)
    for crate in ("fathom-cli", "fathom-core"):
        (tree / CRATES / crate / "Cargo.toml").write_text(
            '[package]\nname = "%s"\nversion.workspace = true\nedition = "2024"\n' % crate)
    (tree / CARGO).write_text(
        '[workspace]\nresolver = "3"\nmembers = ["crates/*"]\n\n'
        '[workspace.package]\nversion = "%s"\n\n'
        '[workspace.dependencies]\n'
        'fathom-core = { path = "crates/fathom-core", version = "%s" }\n' % (version, version))
    (tree / LOCK).write_text(
        'version = 3\n\n[[package]]\nname = "fathom-cli"\nversion = "%s"\n\n'
        '[[package]]\nname = "fathom-core"\nversion = "%s"\n' % (version, version))
    (tree / DIST_CONFIG).write_text(
        '[dist]\ncargo-dist-version = "0.33.0"\n\n'
        '[[dist.extra-artifacts]]\nartifacts = [\n'
        '  "target/packaging/fathom_%s_amd64.deb",\n]\nbuild = ["packaging/linux-packages.sh"]\n'
        % spelled)
    (tree / README).write_text("    https://github.com/o/r/releases/download/v%s/x-installer.sh | sh\n"
                               % version)
    (tree / MAN).mkdir(parents=True, exist_ok=True)
    (tree / MAN / "fathom.1").write_text('.TH FATHOM 1  "fathom %s"\n' % version)
    (tree / CHANGELOG).write_text("## [Unreleased]\n\n## [%s] - 2026-10-18\n\n- everything so far\n"
                                  % version)
    (tree / PLAN).parent.mkdir(parents=True, exist_ok=True)
    (tree / PLAN).write_text(json.dumps({"announcement": {"tag": "v%s" % version, "is_prerelease": True},
                                         "app": {"version": version}}))
    return tree


def command_self_test(root: pathlib.Path) -> int:
    """Each mutation is one way a real release goes quietly wrong; `check` must name it."""
    import tempfile

    mutations = [
        ("P02", lambda t: (t / CARGO).write_text(
            (t / CARGO).read_text().replace('version = "0.1.0-alpha.1" }', 'version = "0.1.0" }')),
         "a path dep was left behind by the bump"),
        ("P03", lambda t: (t / LOCK).write_text(
            (t / LOCK).read_text().replace('name = "fathom-core"\nversion = "0.1.0-alpha.1"',
                                           'name = "fathom-core"\nversion = "0.1.0"')),
         "the lockfile was not refreshed"),
        ("P04", lambda t: (t / DIST_CONFIG).write_text(
            (t / DIST_CONFIG).read_text().replace("0.1.0~alpha.1", "0.0.9~alpha.9")),
         "the .deb is named for an old version"),
        ("P05", lambda t: (t / MAN / "fathom.1").write_text('.TH FATHOM 1  "fathom 0.0.9"\n'),
         "a man page still documents the old release"),
        ("P06", lambda t: (t / README).write_text(
            (t / README).read_text().replace("v0.1.0-alpha.1", "v0.0.9")),
         "README points at an old release"),
        ("P07", lambda t: (t / PLAN).write_text(json.dumps({"announcement": {"tag": "v0.0.9"}})),
         "the captured plan still describes the old release"),
        ("P08", lambda t: (t / CHANGELOG).write_text("## [Unreleased]\n"),
         "no section, so the release announces nothing"),
    ]
    failures = []
    with tempfile.TemporaryDirectory() as where:
        tree = _fixture(pathlib.Path(where))
        problems, _ = check(tree)
        if problems:
            failures.append("the pristine fixture should pass check(): %s"
                            % "; ".join(p.line() for p in problems))
        print("ok: a tree with the version in all seven places passes")

        for expected, mutate, *why in mutations:
            why = why[0] if why else expected
            with tempfile.TemporaryDirectory() as where:
                tree = _fixture(pathlib.Path(where))
                mutate(tree)
                found = [p.rule for p in check(tree)[0]]
                status = "caught" if expected in found else "MISSED"
                if expected not in found:
                    failures.append("%s not detected by: %s" % (expected, why))
                print("%s: %s (%s)" % (status, why, expected))

        # And the rewriting half: one command, and every place moves with it.
        with tempfile.TemporaryDirectory() as where:
            tree = _fixture(pathlib.Path(where))
            bump_manifest(tree, "0.1.0-alpha.1", "0.1.0-alpha.2", False)
            bump_dist_config(tree, "0.1.0-alpha.1", "0.1.0-alpha.2", False)
            bump_readme(tree, "0.1.0-alpha.1", "0.1.0-alpha.2", False)
            (tree / LOCK).write_text((tree / LOCK).read_text().replace("0.1.0-alpha.1", "0.1.0-alpha.2"))
            (tree / MAN / "fathom.1").write_text('.TH FATHOM 1  "fathom 0.1.0-alpha.2"\n')
            (tree / PLAN).write_text(json.dumps({"announcement": {"tag": "v0.1.0-alpha.2"}}))
            (tree / CHANGELOG).write_text("## [0.1.0-alpha.2] - 2026-11-01\n")
            problems, _ = check(tree)
            if problems:
                failures.append("bump left drift behind: %s" % "; ".join(p.line() for p in problems))
            else:
                print("ok: one version in, all seven moved")

        if failures:
            print("\nSELF-TEST FAILED:")
            for line in failures:
                print("  - %s" % line)
            return 1
    print("\nself-test: the version rules bite, and the bump clears them")
    return 0


# ---------------------------------------------------------------------------


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="release.py", description=__doc__.splitlines()[0])
    parser.add_argument("--root", default=str(ROOT), help="tree to work on (default: this repository)")
    sub = parser.add_subparsers(dest="command")

    cut = sub.add_parser("prepare", help="move the version everywhere it has to appear")
    cut.add_argument("--version", required=True, help="the new version, with or without the `v`")
    cut.add_argument("--date", default=None, help="changelog date for the cut (default: today)")
    cut.add_argument("--no-build", action="store_true",
                     help="skip the steps that need a built binary (man pages, CLI page)")
    cut.add_argument("--force", action="store_true",
                     help="cut despite an existing tag, or a changelog audit gap")
    cut.add_argument("--dry-run", action="store_true", help="print the plan, write nothing")

    chk = sub.add_parser("check", help="the seven derived places, offline")
    chk.add_argument("--version", default=None, help="check against this version (default: the manifest's)")

    sub.add_parser("self-test", help="prove the checks can go red, in a throwaway tree")

    args = parser.parse_args(argv)
    root = pathlib.Path(args.root).resolve()
    if args.command == "prepare":
        return command_prepare(root, args)
    if args.command == "check":
        return command_check(root, args.version)
    if args.command == "self-test":
        return command_self_test(root)
    parser.print_help()
    return 2


if __name__ == "__main__":
    sys.exit(main())
