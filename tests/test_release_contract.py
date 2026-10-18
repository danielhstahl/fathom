"""The release path is a contract, and this file is the half that can be checked without a network.

`m0` (`reco-n1j`) is the alpha gate, and "alpha artefacts installable on three OSes" is the acceptance
criterion that a commit cannot satisfy on its own: it needs a public remote, four runners and three
humans. What a commit *can* do is make the release path correct and checked now, so that the tag is a
formality rather than an expedition. `tools/check_release.py` is that gate (twelve rules plus a tag
pre-flight); this file is the contract on the gate:

* it is **green** on this tree, right now, with no `dist`, no credentials and no network;
* it **can go red** — twelve mutations, each one a way a real release pipeline quietly rots, and every
  one detected (`e0.11`'s argument: a gate nobody has seen fail is decoration);
* the **version is one number in five places** — `[workspace.package]`, the seven path-dependency
  requirements, `Cargo.lock`, the changelog section that *is* the release body, and the captured plan —
  because a half-moved version is a build failure for whoever clones next;
* the **three operating systems are actually covered** by the target set, and both Apple silicon and
  Intel Macs are in it, which is the difference between "three OSes" and "three OSes your colleagues
  can install";
* the **tag pre-flight** refuses a tag the package does not carry, which is otherwise something
  `cargo-dist` only says at publish time;
* the **generated tables** in `docs/concepts/release.md` are the captured plan and not somebody's
  memory of it, and the capture is a real `dist plan` projection (four targets, four runner labels,
  the installer's environment knobs) rather than a hand-typed table;
* the **adoption ledger** — the no-telemetry way `m0`'s fourth criterion gets counted — exists, is
  empty, and says it is empty.

Run: python3 -m pytest tests/test_release_contract.py -q
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))

import check_release as R  # noqa: E402

TOOL = ROOT / "tools" / "check_release.py"
PLAN_TOML = ROOT / "dist-workspace.toml"
CARGO_TOML = ROOT / "Cargo.toml"
CARGO_LOCK = ROOT / "Cargo.lock"
README = ROOT / "README.md"
CHANGELOG = ROOT / "CHANGELOG.md"
CONCEPT = ROOT / "docs" / "concepts" / "release.md"
CAPTURED = ROOT / "benchmarks" / "reports" / "dist-release-plan.json"
CI = ROOT / ".github" / "workflows" / "ci.yml"


def run(*args: str) -> subprocess.CompletedProcess:
    return subprocess.run([sys.executable, str(TOOL), *args], capture_output=True, text=True,
                          cwd=ROOT, check=False)


def dist_config() -> dict:
    return tomllib.loads(PLAN_TOML.read_text())["dist"]


def workspace_version() -> str:
    return str(tomllib.loads(CARGO_TOML.read_text())["workspace"]["package"]["version"])


# --- the gate itself ---------------------------------------------------------


def test_the_release_gate_is_green_on_this_tree():
    result = run()
    assert result.returncode == 0, result.stdout + result.stderr
    assert "release path OK" in result.stdout


def test_the_gate_can_go_red_twelve_ways():
    """Every rule that is worth writing is worth watching fire on a deliberate break."""
    result = run("--self-test")
    assert result.returncode == 0, result.stdout + result.stderr
    assert "self-test OK" in result.stdout
    assert "FAIL" not in result.stdout


def test_the_self_test_covers_every_rule_that_can_break_silently():
    """The rules with a self-test case are named, so a rule added without a case is visible."""
    covered = {"R02", "R03", "R04", "R05", "R06", "R07", "R08", "R09", "R10", "R11", "R12"}
    text = TOOL.read_text()
    for rule in sorted(covered):
        assert '"%s"' % rule in text, "%s has no self-test case" % rule
    assert "R01" in text and "R13" in text, "R01 and R13 exist even without a mutation case"


def test_the_tag_preflight_refuses_a_tag_the_package_does_not_carry():
    good = run("--tag", "v%s" % workspace_version())
    assert good.returncode == 0, good.stdout + good.stderr
    assert "ready v%s" % workspace_version() in good.stdout

    bad = run("--tag", "v9.9.9")
    assert bad.returncode == 1
    assert "R13" in bad.stdout


# --- one version, in five places --------------------------------------------


def test_one_version_in_the_workspace_and_every_path_dep():
    version = workspace_version()
    assert R.SEMVER.match(version), "the version must be X.Y.Z(-prerelease)"
    deps = tomllib.loads(CARGO_TOML.read_text())["workspace"]["dependencies"]
    moved = [name for name, spec in deps.items() if isinstance(spec, dict) and "path" in spec]
    assert len(moved) == 7, "ADR-002 §3: seven library crates, each a path dep"
    for name in moved:
        assert deps[name]["version"] == version, "%s did not move with the workspace" % name


def test_no_crate_restates_the_version():
    for manifest in sorted((ROOT / "crates").glob("*/Cargo.toml")):
        package = tomllib.loads(manifest.read_text()).get("package", {})
        assert not isinstance(package.get("version"), str), \
            "%s declares its own version" % manifest


def test_the_lockfile_agrees_with_the_manifest():
    """A lockfile left behind by a version bump is a `cargo build` that edits the tree on first run."""
    version = workspace_version()
    text = CARGO_LOCK.read_text()
    members = re.findall(r'name = "(fathom-[a-z]+)"\nversion = "([^"]+)"', text)
    assert len(members) == 8, "eight crates, per ADR-002 §3: %s" % members
    for name, locked in members:
        assert locked == version, "%s is locked at %s, workspace is %s" % (name, locked, version)


def test_the_changelog_has_a_dated_section_for_the_version():
    """The section *is* the release body: no section means an announcement that says nothing."""
    sections = R.changelog_sections(ROOT)
    version = workspace_version()
    assert version in sections, "CHANGELOG.md has no `## [%s]` section" % version
    assert re.match(r"^\d{4}-\d{2}-\d{2}$", sections[version] or ""), "the section needs its date"
    assert "## [Unreleased]" in CHANGELOG.read_text(), "and a fresh page for what comes next"


def test_the_alpha_is_a_prerelease_so_it_cannot_become_latest():
    version = workspace_version()
    match = R.SEMVER.match(version)
    assert match and match.group("pre"), "the alpha tag is a prerelease, so GitHub marks it one too"
    captured = json.loads(CAPTURED.read_text())
    assert captured["announcement"]["is_prerelease"] is True
    assert captured["announcement"]["force_latest"] is False
    assert captured["announcement"]["publish_prereleases"] is False


# --- three operating systems ------------------------------------------------


def test_the_target_set_covers_three_operating_systems():
    targets = [str(t) for t in dist_config()["targets"]]
    assert any("linux" in t for t in targets)
    assert any("windows" in t for t in targets)
    assert "x86_64-apple-darwin" in targets, "Intel Macs exist; an x86_64-only Mac build is a broken install"
    assert "aarch64-apple-darwin" in targets
    assert dist_config()["pr-run-mode"] == "upload", "e0.7's criterion: every PR builds every target"


def test_both_installers_exist_and_the_plan_names_the_runners():
    assert set(dist_config()["installers"]) == {"shell", "powershell"}
    captured = json.loads(CAPTURED.read_text())
    runners = {t["triple"]: t["runner"] for t in captured["targets"]}
    assert len(runners) == 4
    assert all(runners.values()), "every target needs a runner, or nothing builds it"
    for target in captured["targets"]:
        assert target["archive"].endswith((".tar.xz", ".zip"))
        assert target["checksum"] == target["archive"] + ".sha256"
        assert "fathom" in target["contents"]


def test_the_installer_can_be_tested_without_publishing_anything():
    """`FATHOM_CLI_DOWNLOAD_URL` is what §4 of the release document runs with; if it disappeared from
    the plan, the documented rehearsal would silently stop being possible."""
    captured = json.loads(CAPTURED.read_text())
    names = {row["name"] for row in captured["installer_env"]}
    assert {"FATHOM_CLI_DOWNLOAD_URL", "FATHOM_CLI_INSTALL_DIR", "FATHOM_CLI_NO_MODIFY_PATH"} <= names
    assert "FATHOM_CLI_DOWNLOAD_URL" in CONCEPT.read_text()


# --- the documents ----------------------------------------------------------


def test_readme_quotes_the_url_the_installers_bake():
    """Two sources for one URL is how a documented install line goes stale: the installer bakes
    `<package.repository>/releases/download/v<version>`, and the README must say the same thing."""
    url = R.download_path(tomllib.loads(PLAN_TOML.read_text()), ROOT, workspace_version())
    assert url in README.read_text(), url
    assert "fathom-cli-installer.sh" in README.read_text()
    assert "fathom-cli-installer.ps1" in README.read_text()


def test_the_release_document_carries_the_generated_blocks():
    text = CONCEPT.read_text()
    names = {m.group(1) for m in R.MARK.finditer(text)}
    assert names == {"targets", "artefacts", "installer_env", "gates"}
    # The blocks are not empty shells: each one rendered at least one row from the captured plan.
    for match in R.MARK.finditer(text):
        body = match.group(2)
        assert body.count("\n") >= 3, "generated block %r is empty" % match.group(1)


def test_the_gate_is_wired_into_ci():
    text = CI.read_text()
    assert "check_release.py" in text
    assert "cargo-dist/action" in text, "the other half: `dist generate --check`"


def test_the_adoption_ledger_exists_and_is_honestly_empty():
    """`m0`'s fourth criterion is counted by a ledger, not by telemetry (ADR-001 §8, `e6.8`). An empty
    table that says it is empty is the truth; a paragraph claiming adoption would be a lie."""
    text = CONCEPT.read_text()
    assert "fathom doctor --json" in text
    assert "| Week (ISO) |" in text
    assert "no rows yet" in text


def test_the_captured_plan_is_a_projection_and_not_a_raw_manifest():
    """The raw `dist plan` manifest embeds the whole changelog in its announcement body (~217 KB); the
    committed file is the projection `capture` writes, and it must stay small enough to review. The
    milestone's gate verdicts are deliberately **not** in it — they are an editorial claim that belongs
    in a diff (`GATES` in the tool), not something a re-capture is allowed to rewrite."""
    captured = json.loads(CAPTURED.read_text())
    assert set(captured) >= {"cargo_dist_version", "announcement", "app", "hosting", "targets",
                            "global_artifacts", "installer_env"}
    assert "gates" not in captured, "gate verdicts live in the tool, not in the measurement"
    assert "announcement_github_body" not in captured
    assert CAPTURED.stat().st_size < 20_000


def test_the_release_document_is_linked_from_the_readme():
    assert "docs/concepts/release.md" in README.read_text()


def test_check_docs_still_passes_with_the_new_document():
    """The new doc cites plan keys and beads ids; `check_docs.py` is what makes those citations real."""
    result = subprocess.run([sys.executable, str(ROOT / "tools" / "check_docs.py")],
                            capture_output=True, text=True, cwd=ROOT, check=False)
    assert result.returncode == 0, result.stdout + result.stderr
