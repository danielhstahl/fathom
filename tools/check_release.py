#!/usr/bin/env python3
"""The release path, gated offline: one binary, one version, three operating systems.

`m0` (`reco-n1j`) is the alpha gate, and one of its four acceptance criteria is "alpha artefacts
installable on three OSes". Two of the three words in that sentence are about infrastructure a commit
cannot supply — a public remote to tag, runners to build on, people to install it. This file is the
third, and the one this repository *can* decide: the release path must be **correct and checked while
it is still cheap to fix**. `e0.7` (`reco-e77`) put `cargo-dist` in place and CI runs
`dist generate --check` over it, which proves the generated workflow and its config agree. That is not
the same as proving the *release* is sane, and it cannot see any of these six things:

* a **target set that quietly loses an OS** — four triples, and deleting one still generates a workflow;
* a **version that has been bumped but not released** — `cargo-dist` builds its release body out of
  `CHANGELOG.md`, so a version with no dated section announces nothing (the file says so itself);
* a **half-done version bump** — every path dependency carries a `version` requirement, and a `0.1.0`
  requirement does not resolve against a `0.1.0-alpha.1` crate, so a bump that moves one line and not
  the other eight is a broken build for whoever clones next;
* **docs that point somewhere else than the installer does** — the installers bake their download URL
  out of `package.repository` plus the package version (ADR-002 §10.3's warning: the repository URL is
  a placeholder until the public remote exists, and a stale value is a broken install, not a cosmetic
  bug), so a README that still quotes the old tag is a broken instruction nobody can fix by reading it;
* **evidence that has gone stale** — `benchmarks/reports/dist-release-plan.json` is what `dist plan`
  actually said, measured on a real tree with a real `dist` installed; if the config moves and the
  captured plan does not, the document is describing a pipeline that no longer exists;
* **an alpha that marks itself "latest"** — `force_latest` / `publish_prereleases` are the difference
  between an alpha a colleague opts into and an alpha that becomes a stranger's default install.

So this is the gate, and it needs no network, no `dist`, and no credentials:

    python3 tools/check_release.py                    # every rule; exit 1 on any failure
    python3 tools/check_release.py --tag v0.1.0-alpha.1   # pre-flight before tagging a release
    python3 tools/check_release.py write              # regenerate docs/concepts/release.md's blocks
    python3 tools/check_release.py capture < plan.json  # re-measure the dist plan into the report
    python3 tools/check_release.py --self-test        # prove the gate can go red, on mutated copies

Rule ids appear in every message: `R01`–`R04` the release config, `R05`–`R06` the version and the
artefact, `R07` CI wiring, `R08` the release notes, `R09` the install instructions, `R10` the captured
plan, `R11` the generated documents, `R12` prerelease discipline. `R13` is the `--tag` pre-flight: the
tag someone is about to push is the tag the package actually carries, and it has a changelog section.

The captured plan is deliberately a projection and not the raw `dist plan --output-format=json`
manifest: the raw manifest embeds the whole CHANGELOG in its announcement body (217 KB at the time of
writing), which is not a file anyone should review in a diff. `capture` keeps the fields the rules and
the documents quote, and the one-line refresh is documented at the top of
[docs/concepts/release.md](../docs/concepts/release.md).
"""
from __future__ import annotations

import argparse
import contextlib
import json
import pathlib
import re
import shutil
import sys
import tempfile
import tomllib
from dataclasses import dataclass

ROOT = pathlib.Path(__file__).resolve().parent.parent
PLAN_FILE = ROOT / "dist-workspace.toml"
WORKFLOW = ROOT / ".github" / "workflows" / "release.yml"
CI_WORKFLOW = ROOT / ".github" / "workflows" / "ci.yml"
README = ROOT / "README.md"
CHANGELOG = ROOT / "CHANGELOG.md"
CONCEPT = ROOT / "docs" / "concepts" / "release.md"
CAPTURED = ROOT / "benchmarks" / "reports" / "dist-release-plan.json"

#: The dist-enabled package and the one binary it ships (ADR-002 §4.3, §6.1).
BIN_CRATE = "fathom-cli"
BIN_NAME = "fathom"

#: The OS families the alpha gate counts. `three OSes` is Linux, macOS and Windows; both macOS
#: architectures are required because an x86_64-only Mac build is a broken install for every Apple
#: silicon machine — the failure mode is a user following the documented one-liner and getting a binary
#: their own machine cannot run.
OS_FAMILIES = {
    "linux": lambda t: "linux" in t,
    "macos-x86_64": lambda t: t == "x86_64-apple-darwin",
    "macos-aarch64": lambda t: t == "aarch64-apple-darwin",
    "windows": lambda t: "windows" in t,
}

#: The installer per OS family: `shell` covers Linux and macOS, `powershell` covers Windows.
NEEDED_INSTALLERS = {"shell": ("linux", "macos"), "powershell": ("windows",)}

SEMVER = re.compile(r"^(?P<major>0|[1-9]\d*)\.(?P<minor>0|[1-9]\d*)\.(?P<patch>0|[1-9]\d*)"
                    r"(?:-(?P<pre>[0-9A-Za-z.-]+))?(?:\+(?P<build>[0-9A-Za-z.-]+))?$")
MARK = re.compile(r"<!-- BEGIN GENERATED: (\w+) -->\n(.*?)<!-- END GENERATED: \1 -->", re.S)
#: `## [0.1.0-alpha.1] - 2026-10-18`, with or without the date cargo-dist does not read but humans do.
CHANGELOG_SECTION = re.compile(r"^##\s+\[([^\]]+)\](?:\s*-\s*(\d{4}-\d{2}-\d{2}))?", re.M)


def _esc(text: str) -> str:
    """A cell is not a sentence: a `|` inside one would start a new column (`check_workspace`'s rule)."""
    return str(text).replace("|", "\\|")


@dataclass(frozen=True)
class Problem:
    rule: str
    where: str
    message: str

    def line(self) -> str:
        return "  %-4s %s: %s" % (self.rule, self.where, self.message)


# ---------------------------------------------------------------------------
# reading the tree
# ---------------------------------------------------------------------------


def load_dist_config(root: pathlib.Path) -> tuple[dict, list[Problem]]:
    """`dist-workspace.toml`, with the rules that only its shape can answer."""
    problems: list[Problem] = []
    path = root / "dist-workspace.toml"
    if not path.exists():
        return {}, [Problem("R01", "dist-workspace.toml", "missing; this is the release skeleton")]
    try:
        config = tomllib.loads(path.read_text())
    except tomllib.TOMLDecodeError as exc:
        return {}, [Problem("R01", "dist-workspace.toml", "does not parse: %s" % exc)]
    dist = config.get("dist", {})
    if not dist:
        problems.append(Problem("R01", "dist-workspace.toml", "no [dist] table, so `dist` sees no config"))

    # R01 — the plan-time knobs have to be pinned, not defaulted. A floating dist version is a release
    # pipeline whose behaviour changes when nobody is looking, and `dist plan` is what CI runs per PR.
    if not dist.get("cargo-dist-version"):
        problems.append(Problem("R01", "dist-workspace.toml", "cargo-dist-version is not pinned"))
    if str(dist.get("ci", "")).lower() not in ("github", "github,gitea"):
        problems.append(Problem("R01", "dist-workspace.toml", "ci must name a backend this repo hosts on"))
    if "cargo:." not in [str(m) for m in config.get("workspace", {}).get("members", [])]:
        problems.append(Problem("R01", "dist-workspace.toml",
                                "[workspace] members must include \"cargo:.\" (this Cargo workspace)"))

    targets = [str(t) for t in dist.get("targets", [])]
    if not targets:
        problems.append(Problem("R01", "dist-workspace.toml", "no targets: nothing would be built"))

    # R02 — the three operating systems, as data. Adding a target is free; losing one is silent.
    for family, matches in OS_FAMILIES.items():
        if not any(matches(t) for t in targets):
            problems.append(Problem("R02", "dist-workspace.toml",
                                    "no target covers %s, so no artefact exists for it (targets: %s)"
                                    % (family, ", ".join(targets) or "none")))

    # R03 — every OS family gets an installer its users actually have.
    installers = {str(i) for i in dist.get("installers", [])}
    for installer, families in NEEDED_INSTALLERS.items():
        if installer not in installers:
            problems.append(Problem("R03", "dist-workspace.toml",
                                    "installers is missing %r, which is how %s gets installed"
                                    % (installer, "/".join(families))))

    # R04 — e0.7's acceptance criterion: every PR builds and uploads the artefacts, so the release path
    # is exercised on the way to a release rather than discovered at tag time.
    if str(dist.get("pr-run-mode", "")) != "upload":
        problems.append(Problem("R04", "dist-workspace.toml",
                                "pr-run-mode must be \"upload\" (e0.7: a per-PR build of every target)"))
    if dist.get("source-tarball") is not True:
        problems.append(Problem("R04", "dist-workspace.toml",
                                "source-tarball must be on: it is the artefact a distro builds from"))
    if dist.get("dispatch-releases") is not True:
        problems.append(Problem("R04", "dist-workspace.toml",
                                "dispatch-releases must be on, so a tag is a deliberate act rather "
                                "than a push to main"))
    return config, problems


def load_versions(root: pathlib.Path) -> tuple[str, list[Problem], list[str]]:
    """The one version, and the places it is repeated (ADR-002 §4.3: one binary, one version)."""
    problems: list[Problem] = []
    notes: list[str] = []
    cargo = root / "Cargo.toml"
    if not cargo.exists():
        return "", [Problem("R05", "Cargo.toml", "missing")], notes
    data = tomllib.loads(cargo.read_text())
    version = str(data.get("workspace", {}).get("package", {}).get("version", ""))
    if not SEMVER.match(version):
        problems.append(Problem("R05", "Cargo.toml",
                                "[workspace.package] version %r is not X.Y.Z(-prerelease)" % version))
        return version, problems, notes

    # R05a — no crate restates the version. A crate with its own version is a second release train that
    # cargo-dist would try to announce separately, and the tag would name one of them.
    for manifest in sorted((root / "crates").glob("*/Cargo.toml")):
        package = tomllib.loads(manifest.read_text()).get("package", {})
        declared = package.get("version")
        if isinstance(declared, str):
            problems.append(Problem("R05", manifest.relative_to(root).as_posix(),
                                    "declares version = %r; use version.workspace = true" % declared))

    # R05b — the path-dep requirements move with the version. `0.1.0` does not match `0.1.0-alpha.1`:
    # cargo treats a prerelease as outside the requirement unless the requirement says so.
    workspace_deps = data.get("workspace", {}).get("dependencies", {})
    for name, spec in sorted(workspace_deps.items()):
        if not isinstance(spec, dict) or "path" not in spec:
            continue
        required = str(spec.get("version", ""))
        if required != version:
            problems.append(Problem("R05", "Cargo.toml",
                                    "path dep %s requires %r but the workspace is %r; a half-done bump "
                                    "is a build failure for the next clone" % (name, required, version)))

    # R06 — one dist-enabled package, and it is the binary. `publish = false` hides a crate from a tool
    # whose whole job is publishing, hence the explicit `dist = true` in the CLI crate (ADR-002 §6.1).
    enabled: list[str] = []
    for manifest in sorted((root / "crates").glob("*/Cargo.toml")):
        data = tomllib.loads(manifest.read_text())
        if data.get("package", {}).get("metadata", {}).get("dist", {}).get("dist") is True:
            enabled.append(str(data["package"]["name"]))
    if enabled != [BIN_CRATE]:
        problems.append(Problem("R06", "crates", "exactly %s may set [package.metadata.dist] dist = true, "
                                "found: %s" % (BIN_CRATE, ", ".join(enabled) or "none")))
    cli = root / "crates" / BIN_CRATE / "Cargo.toml"
    if cli.exists():
        bins = tomllib.loads(cli.read_text()).get("bin", [])
        names = [str(b.get("name", "")) for b in bins]
        if names != [BIN_NAME]:
            problems.append(Problem("R06", "crates/%s/Cargo.toml" % BIN_CRATE,
                                    "must declare exactly one [[bin]] named %s (ADR-002 §4.3)" % BIN_NAME))
    notes.append("%s %s (feature set = the shipped product, ADR-002 §6.1)" % (BIN_CRATE, version))
    return version, problems, notes


def load_workflows(root: pathlib.Path, dist_config: dict) -> list[Problem]:
    problems: list[Problem] = []
    want = str(dist_config.get("dist", {}).get("cargo-dist-version", ""))
    workflow = root / ".github" / "workflows" / "release.yml"
    if not workflow.exists():
        return [Problem("R07", ".github/workflows/release.yml", "missing; run `dist generate`")]
    text = workflow.read_text()
    # R07 — the generated workflow and the config must name the same dist. `dist generate --check`
    # already proves it in CI, and this rule proves it on a laptop that has never installed dist: the
    # two failure modes differ, because `generate --check` compares content, and this compares the pin
    # that decides *which* content is correct.
    pinned = re.search(r"cargo-dist/releases/download/v([0-9][^/]*)/", text)
    if not pinned:
        problems.append(Problem("R07", "release.yml", "no cargo-dist version pin found"))
    elif want and pinned.group(1) != want:
        problems.append(Problem("R07", "release.yml", "installs cargo-dist v%s but dist-workspace.toml "
                                "pins %s; regenerate the workflow" % (pinned.group(1), want)))
    # R07b — the job that regenerates must be wired up, or the drift this rule fears is only deferred.
    ci = root / ".github" / "workflows" / "ci.yml"
    text_ci = ci.read_text() if ci.exists() else ""
    if "cargo-dist/action" not in text_ci or "--check" not in text_ci:
        problems.append(Problem("R07", "ci.yml", "no `dist generate --check` job, so release.yml may be "
                                "hand-edited without anyone noticing"))
    if "check_release.py" not in text_ci:
        problems.append(Problem("R07", "ci.yml", "this gate is not wired into CI"))
    return problems


def changelog_sections(root: pathlib.Path) -> dict[str, str | None]:
    text = (root / "CHANGELOG.md").read_text() if (root / "CHANGELOG.md").exists() else ""
    return {m.group(1): m.group(2) for m in CHANGELOG_SECTION.finditer(text)}


def load_changelog(root: pathlib.Path, version: str) -> tuple[list[Problem], list[str]]:
    problems: list[Problem] = []
    notes: list[str] = []
    sections = changelog_sections(root)
    if not sections:
        return [Problem("R08", "CHANGELOG.md", "no `## [version]` sections at all")], notes
    date = sections.get(version)
    if date is None and version not in sections:
        problems.append(Problem("R08", "CHANGELOG.md",
                                "version %r has no `## [%s] - YYYY-MM-DD` section, and the release body "
                                "is built from this file: the announcement would say nothing"
                                % (version, version)))
    elif date is None:
        problems.append(Problem("R08", "CHANGELOG.md",
                                "`## [%s]` has no release date; cargo-dist does not read the date, "
                                "whoever reads it next does" % version))
    return problems, notes


def download_path(dist_config: dict, root: pathlib.Path, version: str) -> str:
    """What the installers bake: `<repository>/releases/download/v<version>`.

    Measured, not remembered — `dist build --artifacts=global` writes the installer with this URL, and
    `captured` keeps the same value out of the plan. The placeholder `package.repository` (ADR-002 §10.3)
    is why this is derived from the manifest rather than typed into a rule.
    """
    captured = load_captured(root)
    if captured:
        hosting = captured.get("hosting", {})
        base = str(hosting.get("artifact_base_url", "")).rstrip("/")
        suffix = str(hosting.get("artifact_download_path") or
                     "/%s/%s/releases/download/v%s" % (hosting.get("owner", ""), hosting.get("repo", ""),
                                                       version))
        return base + suffix
    cli = root / "crates" / BIN_CRATE / "Cargo.toml"
    repository = ""
    if cli.exists():
        repository = str(tomllib.loads(cli.read_text()).get("package", {}).get("repository", ""))
    return "%s/releases/download/v%s" % (repository.rstrip("/"), version)


def load_readme(root: pathlib.Path, version: str, dist_config: dict) -> tuple[list[Problem], list[str]]:
    problems: list[Problem] = []
    notes: list[str] = []
    path = root / "README.md"
    if not path.exists():
        return [Problem("R09", "README.md", "missing")], notes
    text = path.read_text()
    path_from_config = download_path(dist_config, root, version)
    # R09 — the instructions a user follows must be the URL the installer would have used. Two sources
    # for one URL is exactly the drift this rule exists for; the tag is the part that goes stale first.
    if path_from_config not in text:
        problems.append(Problem("R09", "README.md",
                                "does not quote the release path the installers bake: %s"
                                % path_from_config))
    for spelling in (("%s-installer.sh" % BIN_CRATE, "install.sh"),
                     ("%s-installer.ps1" % BIN_CRATE, "install.ps1")):
        if not any(word in text for word in spelling):
            problems.append(Problem("R09", "README.md",
                                    "says nothing about the %s, which is the other OS's way in"
                                    % spelling[1]))
    if "fathom doctor" not in text:
        problems.append(Problem("R09", "README.md",
                                "does not point at `fathom doctor --json`, the report an alpha user "
                                "can send without sending their repository"))
    return problems, notes


def load_captured(root: pathlib.Path) -> dict:
    path = root / "benchmarks" / "reports" / "dist-release-plan.json"
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text())
    except json.JSONDecodeError:
        return {}


def check_captured(root: pathlib.Path, version: str, dist_config: dict) -> tuple[list[Problem], list[str]]:
    """R10 — the captured plan must still describe this config, and R12 prerelease discipline."""
    problems: list[Problem] = []
    notes: list[str] = []
    captured_path = root / "benchmarks" / "reports" / "dist-release-plan.json"
    captured = load_captured(root)
    if not captured:
        return [Problem("R10", captured_path.relative_to(root).as_posix(),
                        "missing; refresh with `dist plan --output-format=json | "
                        "python3 tools/check_release.py capture`")], notes
    dist = dist_config.get("dist", {})
    if captured.get("cargo_dist_version") != str(dist.get("cargo-dist-version", "")):
        problems.append(Problem("R10", "dist-release-plan.json",
                                "captured cargo-dist %s, config pins %s"
                                % (captured.get("cargo_dist_version"), dist.get("cargo-dist-version"))))
    app = captured.get("app", {})
    if app.get("version") != version:
        problems.append(Problem("R10", "dist-release-plan.json",
                                "captured app version %r, the workspace is %r; re-capture after a bump"
                                % (app.get("version"), version)))
    if app.get("package") != BIN_CRATE:
        problems.append(Problem("R10", "dist-release-plan.json",
                                "captured package %r, not %s" % (app.get("package"), BIN_CRATE)))
    want_targets = [str(t) for t in dist.get("targets", [])]
    got_targets = [str(t.get("triple", "")) for t in captured.get("targets", [])]
    if sorted(got_targets) != sorted(want_targets):
        problems.append(Problem("R10", "dist-release-plan.json",
                                "captured targets %s, config says %s; the documents are describing a "
                                "pipeline that no longer exists"
                                % (", ".join(got_targets) or "none", ", ".join(want_targets) or "none")))
    got_installers = {str(i) for i in captured.get("installers", [])}
    if got_installers != {str(i) for i in dist.get("installers", [])}:
        problems.append(Problem("R10", "dist-release-plan.json", "captured installers %s, config says %s"
                                % (sorted(got_installers), sorted(str(i) for i in dist.get("installers", [])))))

    # R12 — an alpha must not become somebody's default install. A prerelease tag is what makes GitHub
    # mark the release a prerelease; `force_latest` or `publish_prereleases` would undo it silently, and
    # `install.sh` resolves "latest" for anyone who does not name a version.
    announcement = captured.get("announcement", {})
    prerelease = bool(SEMVER.match(version) and SEMVER.match(version).group("pre"))
    if prerelease and (announcement.get("force_latest") or announcement.get("publish_prereleases")):
        problems.append(Problem("R12", "dist-release-plan.json",
                                "%s is a prerelease, but force_latest/publish_prereleases is on: the "
                                "alpha would be published as a default install" % version))
    if prerelease and not announcement.get("is_prerelease", False):
        problems.append(Problem("R12", "dist-release-plan.json",
                                "%s looks like a prerelease but the announcement tag %r is not marked "
                                "one" % (version, announcement.get("tag"))))
    if announcement.get("tag") != "v%s" % version:
        problems.append(Problem("R10", "dist-release-plan.json",
                                "announcement tag %r does not name the workspace version v%s; "
                                "cargo-dist refuses a tag the package does not carry"
                                % (announcement.get("tag"), version)))
    notes.append("tag %s (prerelease=%s), %d targets, %d global artefacts, pr-run-mode=%s"
                 % (announcement.get("tag"), announcement.get("is_prerelease"), len(got_targets),
                    len(captured.get("global_artifacts", [])), captured.get("pr_run_mode")))
    return problems, notes


# ---------------------------------------------------------------------------
# the generated documents
# ---------------------------------------------------------------------------


def render_blocks(captured: dict) -> dict[str, str]:
    """The tables `docs/concepts/release.md` renders, from the captured plan and nothing else."""
    targets = captured.get("targets", [])
    rows = ["| Target triple | Runner (GitHub-hosted) | Artefact it builds | Built by |",
            "|---|---|---|---|"]
    for target in targets:
        rows.append("| `%s` | `%s` | `%s` | `%s` |"
                    % (target.get("triple"), target.get("runner"), target.get("archive"),
                       target.get("dist_args")))

    artifacts = ["| Artefact | Made | What it is |", "|---|---|---|"]
    for row in captured.get("global_artifacts", []):
        artifacts.append("| `%s` | once per release | %s |" % (row.get("name"), _esc(row.get("what"))))
    for target in targets:
        contents = [c for c in target.get("contents", []) if c != "fathom"]
        artifacts.append("| `%s` | per target | `fathom`%s, with `%s` beside it |"
                         % (target.get("archive"),
                            (" plus " + ", ".join("`%s`" % c for c in contents)) if contents else "",
                            target.get("checksum")))

    env = ["| Variable | Effect |", "|---|---|"]
    for row in captured.get("installer_env", []):
        env.append("| `%s` | %s |" % (row.get("name"), _esc(row.get("what"))))

    # The gate verdicts come from the constant below, not from the captured file: they are an editorial
    # claim about a milestone, not a measurement, and they belong in a diff rather than a re-capture.
    status = ["| Gate | What proves it | State |", "|---|---|---|"]
    for row in GATES:
        status.append("| %s | %s | %s |" % (_esc(row.get("gate")), _esc(row.get("evidence")),
                                     _esc(row.get("state"))))
    return {
        "targets": "\n".join(rows),
        "artefacts": "\n".join(artifacts),
        "installer_env": "\n".join(env),
        "gates": "\n".join(status),
    }


def apply_blocks(text: str, blocks: dict[str, str]) -> tuple[str, list[Problem]]:
    """Replace every generated block in one pass; a block is named once per document."""
    problems: list[Problem] = []
    found = [m.group(1) for m in MARK.finditer(text)]

    def replace(match):
        name = match.group(1)
        body = blocks.get(name)
        if body is None:
            return match.group(0)
        return "<!-- BEGIN GENERATED: %s -->\n%s\n<!-- END GENERATED: %s -->" % (name, body, name)

    text = MARK.sub(replace, text)
    for name in sorted(set(blocks) - set(found)):
        problems.append(Problem("R11", "docs/concepts/release.md",
                                "has no GENERATED block %r for the release plan to fill" % name))
    for name in sorted(set(found) - set(blocks)):
        problems.append(Problem("R11", "docs/concepts/release.md",
                                "renders GENERATED block %r that this generator does not produce" % name))
    duplicates = sorted({n for n in found if found.count(n) > 1})
    for name in duplicates:
        problems.append(Problem("R11", "docs/concepts/release.md",
                                "renders GENERATED block %r twice; one block, one place" % name))
    return text, problems


def command_write(root: pathlib.Path) -> int:
    captured = load_captured(root)
    if not captured:
        print("  R10   no captured plan to render from; capture one first")
        return 1
    concept = root / "docs" / "concepts" / "release.md"
    if not concept.exists():
        print("  R11   docs/concepts/release.md is missing")
        return 1
    text, problems = apply_blocks(concept.read_text(), render_blocks(captured))
    if problems:
        for problem in problems:
            print(problem.line())
        return 1
    concept.write_text(text)
    print("release.md regenerated from the captured plan (%d blocks)" % len(render_blocks(captured)))
    return 0


# ---------------------------------------------------------------------------
# capture
# ---------------------------------------------------------------------------

ARCHIVE_WHAT = {
    "source.tar.gz": "the source tree a distro or an auditor builds from, with its own `.sha256`",
    "fathom-cli-installer.sh": "the `curl … | sh` script for Linux and macOS; picks the target from the machine",
    "fathom-cli-installer.ps1": "the `irm … | iex` script for Windows, so a Windows user is not told to use bash",
    "sha256.sum": "one digest per artefact, so a download can be checked without trusting the transport",
}

INSTALLER_ENV_WHAT = {
    "download_url_env_var": "point the installer at a mirror instead of a public release (used in this "
                            "repository's own end-to-end installer test)",
    "install_dir_env_var": "install somewhere other than `~/.local/bin`",
    "no_modify_path_env_var": "leave `PATH` (and the shell rc files) alone",
    "unmanaged_dir_env_var": "the unmanaged-install directory",
    "disable_update_env_var": "skip the self-update path entirely",
    "print_verbose_env_var": "more output when the install is going wrong",
    "print_quiet_env_var": "less output",
    "github_token_env_var": "a token for a private release, so an alpha can be gated without an object store",
    "github_base_url_env_var": "GitHub Enterprise instead of github.com",
    "ghe_base_url_env_var": "GitHub Enterprise base URL, second spelling",
}


def command_capture(root: pathlib.Path, stream) -> int:
    """Project a raw `dist plan --output-format=json` manifest into the committed report."""
    try:
        manifest = json.load(stream)
    except json.JSONDecodeError as exc:
        print("R10 input is not the JSON `dist plan --output-format=json` prints: %s" % exc)
        return 1
    if "releases" not in manifest:
        print("R10 input does not look like a dist plan manifest (no `releases`)")
        return 1
    release = manifest["releases"][0]
    hosting = release.get("hosting", {}).get("github", {})
    matrix = manifest.get("ci", {}).get("github", {}).get("artifacts_matrix", {}).get("include", [])
    by_target = {}
    for row in matrix:
        for target in row.get("targets", []):
            by_target[target] = row
    # `dist` reports the artefact list as a name-keyed object; older shapes used a list. Either way the
    # per-target archive is the one artifact whose `target_triples` has exactly one member.
    artifact_rows = manifest.get("artifacts", [])
    if isinstance(artifact_rows, dict):
        artifact_rows = list(artifact_rows.values())
    archives = {}
    for artifact in artifact_rows:
        if not isinstance(artifact, dict) or artifact.get("kind") == "checksum":
            continue
        triples = artifact.get("target_triples", [])
        if len(triples) == 1:
            archives[triples[0]] = artifact
    targets = []
    for triple in sorted(by_target):
        archive = archives.get(triple, {})
        name = archive.get("name", "")
        targets.append({
            "triple": triple,
            "runner": by_target[triple].get("runner"),
            "dist_args": by_target[triple].get("dist_args"),
            "archive": name,
            "checksum": archive.get("checksum"),
            "contents": sorted({a.get("name") for a in archive.get("assets", []) if a.get("name")}),
        })
    global_artifacts = []
    for name in release.get("artifacts", []):
        if name.endswith(".sha256") or any(name == t["archive"] for t in targets):
            continue
        global_artifacts.append({"name": name, "what": ARCHIVE_WHAT.get(name, "")})
    env = [{"name": name.upper(), "what": INSTALLER_ENV_WHAT.get(key, "")}
           for key, name in sorted(release.get("env", {}).items())
           if key in INSTALLER_ENV_WHAT]
    projection = {
        "what": "What `cargo dist plan` actually said about this repository's release path, measured on "
                "a real tree. Refresh: `dist plan --output-format=json | python3 tools/check_release.py "
                "capture`. Checked by `python3 tools/check_release.py` (R10), rendered into "
                "docs/concepts/release.md by `... write`.",
        "cargo_dist_version": manifest.get("dist_version"),
        "announcement": {
            "tag": manifest.get("announcement_tag"),
            "is_prerelease": manifest.get("announcement_is_prerelease"),
            "force_latest": manifest.get("force_latest"),
            "publish_prereleases": manifest.get("publish_prereleases"),
        },
        "app": {
            "package": release.get("app_name"),
            "binary": "fathom",
            "version": release.get("app_version"),
        },
        "hosting": {
            "owner": hosting.get("owner"),
            "repo": hosting.get("repo"),
            "artifact_base_url": hosting.get("artifact_base_url"),
            "artifact_download_path": hosting.get("artifact_download_path"),
        },
        "pr_run_mode": manifest.get("ci", {}).get("github", {}).get("pr_run_mode"),
        "installers": sorted({str(i) for i in tomllib.loads((root / "dist-workspace.toml").read_text())
                             .get("dist", {}).get("installers", [])}),
        "installer_env": env,
        "targets": targets,
        "global_artifacts": global_artifacts,
    }
    path = root / "benchmarks" / "reports" / "dist-release-plan.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(projection, indent=2, sort_keys=True) + "\n")
    print("captured %s: %d targets, tag %s"
          % (path.relative_to(root).as_posix(), len(targets), projection["announcement"]["tag"]))
    return 0


#: The alpha gate's own four criteria, kept as data so the document and the checker quote the same
#: verdict. `bd show reco-n1j` is the authority; this is the in-repo mirror of it, and `--tag` prints it.
GATES = [
    {
        "gate": "Epic 0 and Epic 1 closed, no open P1 defects",
        "evidence": "`bd list --status open -l defect` is empty; `reco-w74` and `reco-quk` are closed",
        "state": "met by code",
    },
    {
        "gate": "Spike recommendations reflected in the plan (`e0.12`)",
        "evidence": "`.plan/reconciliation-e0.12.md`, and `tools/gen_segment_tables.py check` in CI",
        "state": "met by code",
    },
    {
        "gate": "Alpha artefacts installable on three OSes",
        "evidence": "the pipeline is gated and measured (`tools/check_release.py`, "
                    "`benchmarks/reports/dist-release-plan.json`); the tag, the runners and the three "
                    "installs still have to happen — `m0.1`, and runbook §3",
        "state": "ready, blocked on a public remote",
    },
    {
        "gate": "Three people outside the core team using it weekly",
        "evidence": "nothing in-repo, and nothing can be until the alpha is installable — `m0.2`, and "
                    "the ledger in runbook §5",
        "state": "not started, blocked on `m0.1`",
    },
]


# ---------------------------------------------------------------------------
# the gate
# ---------------------------------------------------------------------------


def check_all(root: pathlib.Path) -> tuple[list[Problem], list[str]]:
    problems: list[Problem] = []
    notes: list[str] = []
    config, problems = load_dist_config(root)
    version, version_problems, version_notes = load_versions(root)
    problems += version_problems
    notes += version_notes
    problems += load_workflows(root, config)
    changelog_problems, changelog_notes = load_changelog(root, version)
    problems += changelog_problems
    notes += changelog_notes
    readme_problems, readme_notes = load_readme(root, version, config)
    problems += readme_problems
    notes += readme_notes
    captured_problems, captured_notes = check_captured(root, version, config)
    problems += captured_problems
    notes += captured_notes

    concept = root / "docs" / "concepts" / "release.md"
    if not concept.exists():
        problems.append(Problem("R11", "docs/concepts/release.md", "missing: the runbook is the artefact"))
    else:
        captured = load_captured(root)
        if captured:
            _, block_problems = apply_blocks(concept.read_text(), render_blocks(captured))
            problems += block_problems
        if "--tag" not in (root / "README.md").read_text() and "check_release.py --tag" not in concept.read_text():
            problems.append(Problem("R11", "docs/concepts/release.md",
                                    "does not tell anyone that `check_release.py --tag` is the pre-flight"))
    return problems, notes


def command_check(root: pathlib.Path, tag: str | None) -> int:
    problems, notes = check_all(root)
    if tag:
        # R13 — the tag a human is about to push must be the tag the config would announce. cargo-dist
        # errors out at publish time on a mismatch, which is the worst possible moment to learn it.
        captured = load_captured(root)
        version = str(tomllib.loads((root / "Cargo.toml").read_text())
                      .get("workspace", {}).get("package", {}).get("version", ""))
        want = captured.get("announcement", {}).get("tag") if captured else "v%s" % version
        if tag != want:
            problems.append(Problem("R13", tag, "the plan announces %s; cargo-dist refuses a tag whose "
                                    "version the package does not carry" % want))
        sections = changelog_sections(root)
        stripped = tag[1:] if tag.startswith("v") else tag
        if stripped not in sections:
            problems.append(Problem("R13", tag, "CHANGELOG.md has no `## [%s]` section to announce" % stripped))
    for note in notes:
        print("  note  %s" % note)
    if problems:
        for problem in problems:
            print(problem.line())
        print("\n%s problem(s)" % len(problems))
        return 1
    if tag:
        captured = load_captured(root)
        print("  ready %s: %d targets, %d global artefacts, %d per-target archives"
              % (tag, len(captured.get("targets", [])), len(captured.get("global_artifacts", [])),
                 len(captured.get("targets", []))))
    print("release path OK: one binary (%s) at %s, three OS families, every rule green"
          % (BIN_NAME, _version_of(root)))
    return 0
    return 0


def command_self_test(root: pathlib.Path) -> int:
    """Prove the gate can go red. A gate nobody has seen fail is decoration (`e0.11`'s argument, reused).

    Each case copies the handful of files the gate reads into a throwaway tree, breaks one thing, and
    demands the named rule fires. Nothing here mutates the real tree.
    """
    cases = [
        ("R02", lambda r: _drop_target(r, "aarch64-apple-darwin"), "an Apple silicon machine gets no artefact"),
        ("R02", lambda r: _drop_target(r, "x86_64-unknown-linux-gnu"), "Linux gets no artefact"),
        ("R03", lambda r: _drop_installer(r, "powershell"), "Windows users are told to run a shell script"),
        ("R04", lambda r: _set_dist(r, "pr-run-mode", "plan"), "PRs stop building the artefacts"),
        ("R05", lambda r: _set_version(r, "9.9.9"), "the version moved and the path deps did not"),
        ("R06", lambda r: _dist_enable(r, "fathom-store"), "a second crate became releasable"),
        ("R07", lambda r: _pin_workflow(r, "0.1.0"), "the workflow installs a different dist than the config"),
        ("R08", lambda r: _undate_changelog(r), "the release section lost its date"),
        ("R09", lambda r: _stale_readme(r), "README still quotes an old tag"),
        ("R10", lambda r: _stale_capture(r), "the config moved and the captured plan did not"),
        ("R11", lambda r: _drop_block(r, "targets"), "the document dropped its generated block"),
        ("R12", lambda r: _force_latest(r), "a prerelease would publish itself as latest"),
    ]
    failures = 0
    with tempfile_tree() as base:
        for want_rule, mutate, why in cases:
            tree = base / ("case-%02d" % failures)
            _copy_inputs(root, tree)
            mutate(tree)
            problems, _ = check_all(tree)
            fired = [p for p in problems if p.rule == want_rule]
            if fired:
                print("  ok    %s fires when %s" % (want_rule, why))
            else:
                print("  FAIL  %s did not fire when %s (got: %s)"
                      % (want_rule, why, ", ".join(sorted({p.rule for p in problems})) or "nothing"))
                failures += 1
    if failures:
        print("\n%d self-test case(s) failed" % failures)
        return 1
    print("self-test OK: %d ways to be red, all of them detected" % len(cases))
    return 0


# --- self-test plumbing ------------------------------------------------------


@contextlib.contextmanager
def tempfile_tree():
    path = pathlib.Path(tempfile.mkdtemp(prefix="check-release-selftest-"))
    try:
        yield path
    finally:
        shutil.rmtree(path, ignore_errors=True)


INPUTS = ["dist-workspace.toml", "Cargo.toml", "CHANGELOG.md", "README.md",
          ".github/workflows/release.yml", ".github/workflows/ci.yml",
          "docs/concepts/release.md", "benchmarks/reports/dist-release-plan.json"]


def _copy_inputs(root: pathlib.Path, tree: pathlib.Path) -> None:
    for rel in INPUTS:
        source = root / rel
        target = tree / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)
    # Only the manifests: the gate reads crate *shape* (version, features, the bin), never their sources,
    # and copying 8 TOML files instead of 8 crates is what keeps this self-test runnable in a test suite.
    for manifest in sorted((root / "crates").glob("*/Cargo.toml")):
        target = tree / manifest.relative_to(root)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(manifest, target)


def _rewrite_dist(tree: pathlib.Path, edit) -> None:
    path = tree / "dist-workspace.toml"
    text = path.read_text()
    path.write_text(edit(text))


def _drop_target(tree: pathlib.Path, triple: str) -> None:
    _rewrite_dist(tree, lambda t: t.replace('  "%s",\n' % triple, ""))


def _drop_installer(tree: pathlib.Path, installer: str) -> None:
    _rewrite_dist(tree, lambda t: t.replace('["%s", "powershell"]' % installer, '["shell"]')
                  .replace('["shell", "%s"]' % installer, '["shell"]'))


def _set_dist(tree: pathlib.Path, key: str, value: str) -> None:
    _rewrite_dist(tree, lambda t: re.sub(r"^%s = .*$" % re.escape(key), '%s = "%s"' % (key, value),
                                         t, flags=re.M))


def _set_version(tree: pathlib.Path, version: str) -> None:
    path = tree / "Cargo.toml"
    text = path.read_text()
    path.write_text(re.sub(r'^version = "0\.1\.0[^"]*"$', 'version = "%s"' % version, text, count=1,
                           flags=re.M))


def _dist_enable(tree: pathlib.Path, crate: str) -> None:
    path = tree / "crates" / crate / "Cargo.toml"
    path.write_text(path.read_text() + "\n[package.metadata.dist]\ndist = true\n")


def _pin_workflow(tree: pathlib.Path, version: str) -> None:
    path = tree / ".github" / "workflows" / "release.yml"
    path.write_text(path.read_text().replace("/cargo-dist/releases/download/v",
                                             "/cargo-dist/releases/download/v%s-x" % version, 1))


def _undate_changelog(tree: pathlib.Path) -> None:
    path = tree / "CHANGELOG.md"
    version = _version_of(tree)
    path.write_text(path.read_text().replace("## [%s] - " % version, "## [%s] " % version, 1))


def _stale_readme(tree: pathlib.Path) -> None:
    """Every occurrence, because that is what a stale tag looks like: the docs were cut from one release
    and the manifest moved on. Replacing one of two would leave a still-correct line behind."""
    path = tree / "README.md"
    path.write_text(path.read_text().replace("/releases/download/v", "/releases/download/v9-"))


def _stale_capture(tree: pathlib.Path) -> None:
    path = tree / "benchmarks" / "reports" / "dist-release-plan.json"
    data = json.loads(path.read_text())
    data["targets"] = data["targets"][:-1]
    path.write_text(json.dumps(data, indent=2, sort_keys=True))


def _force_latest(tree: pathlib.Path) -> None:
    path = tree / "benchmarks" / "reports" / "dist-release-plan.json"
    data = json.loads(path.read_text())
    data["announcement"]["force_latest"] = True
    path.write_text(json.dumps(data, indent=2, sort_keys=True))


def _drop_block(tree: pathlib.Path, name: str) -> None:
    path = tree / "docs" / "concepts" / "release.md"
    text = path.read_text()
    path.write_text(MARK.sub(lambda m: "" if m.group(1) == name else m.group(0), text, count=1))


def _version_of(tree: pathlib.Path) -> str:
    return str(tomllib.loads((tree / "Cargo.toml").read_text())
               .get("workspace", {}).get("package", {}).get("version", ""))


# ---------------------------------------------------------------------------


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="check_release.py", description=__doc__.splitlines()[0])
    parser.add_argument("command", nargs="?", default="check",
                        choices=["check", "write", "capture"], help="what to do (default: check)")
    parser.add_argument("--tag", help="pre-flight for a specific release tag before pushing it")
    parser.add_argument("--self-test", action="store_true", help="prove the gate can go red")
    parser.add_argument("--root", default=str(ROOT), help="tree to check (default: this repository)")
    args = parser.parse_args(argv)
    root = pathlib.Path(args.root).resolve()
    if args.self_test:
        return command_self_test(root)
    if args.command == "write":
        return command_write(root)
    if args.command == "capture":
        return command_capture(root, sys.stdin)
    return command_check(root, args.tag)


if __name__ == "__main__":
    sys.exit(main())
