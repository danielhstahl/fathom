"""The version is one number, and this is the contract on the tool that moves it.

`Cargo.toml` declares the version; six other places have to *derive* it, and every one of them is a red
build, a misnamed package or a broken installer when it is left behind. `tools/release.py` is that
claim made executable: `P01`–`P08` re-check all seven offline, `prepare` moves them in the order the
generators require, and `self-test` proves each rule can go red (`e0.11`'s argument again: a gate nobody
has seen fail is decoration).

The other half is the trigger. `dispatch-releases = true` means a pushed tag does nothing, so
`.github/workflows/publish.yml` is what turns a version bump into a release: it fires after `ci` is green
on the default branch, refuses unless *this commit moved the version*, refuses if the tag already exists,
refuses if the changelog has no dated section for it, and then dispatches `release.yml` at that commit —
which creates the tag and the GitHub Release together, so neither can name a version the tree does not
carry.

This file asserts:

* `tools/release.py check` is **green on this tree**, with no `dist`, no build and no network;
* `tools/release.py self-test` **catches every mutation** it claims to — seven of them, one per place;
* the seven places are the seven the checker looks at, measured on the real tree rather than asserted;
* `ci.yml` **calls the tool** (a tool no workflow calls is a tool that rots), and
* `publish.yml` still refuses the three things it exists to refuse, and dispatches rather than publishes.

Run: python3 -m pytest tests/test_release_version_contract.py -q
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TOOL = ROOT / "tools" / "release.py"
PUBLISH = ROOT / ".github" / "workflows" / "publish.yml"
RELEASE = ROOT / ".github" / "workflows" / "release.yml"
CI = ROOT / ".github" / "workflows" / "ci.yml"

sys.path.insert(0, str(ROOT / "tools"))
import release as R  # noqa: E402


def run(*args: str) -> subprocess.CompletedProcess:
    return subprocess.run([sys.executable, str(TOOL), *args], capture_output=True, text=True,
                          cwd=ROOT, check=False)


# ---------------------------------------------------------------------------
# the checker, on this tree
# ---------------------------------------------------------------------------


def test_check_is_green_on_this_tree():
    result = run("check")
    assert result.returncode == 0, result.stdout + result.stderr


def test_self_test_proves_every_rule_can_go_red():
    result = run("self-test")
    assert result.returncode == 0, result.stdout + result.stderr
    assert "MISSED" not in result.stdout, result.stdout
    for rule in ("P02", "P03", "P04", "P05", "P06", "P07", "P08"):
        assert "(%s)" % rule in result.stdout, "self-test no longer exercises %s" % rule


def test_the_seven_places_are_the_ones_the_checker_reads():
    """The docstring promises seven; this measures it, so the promise cannot rot."""
    version = R.declared(ROOT)
    assert version, "no [workspace.package] version"
    counts = {
        "manifest": 1,
        "path deps": len(R.path_dep_requirements(ROOT)),
        "lock entries": len(R.locked_versions(ROOT)),
        "package names": len(R.packaged_names(ROOT)),
        "man pages": len(R.man_page_versions(ROOT)),
        "readme": int(R.readme_urls_current(ROOT, version)),
        "captured plan": int(bool(R.plan_version(ROOT))),
        "changelog": int(R.changelog_has(ROOT, version)[0]),
    }
    for what, count in counts.items():
        assert count >= 1, "nothing to check for %s" % what
    # Every derived place agrees: that is the whole claim, restated as data.
    assert set(R.path_dep_requirements(ROOT)) == {version}
    assert set(R.locked_versions(ROOT).values()) == {version}
    assert R.plan_version(ROOT) == version
    assert set(R.man_page_versions(ROOT).values()) == {version}


def test_the_package_version_spelling_is_the_packager_s():
    """`0.1.0-alpha.1` is not a legal Debian version; the `~` form is what both package managers read."""
    assert R.debian_version("0.1.0-alpha.1") == "0.1.0~alpha.1"
    assert R.debian_version("1.2.3") == "1.2.3"
    assert all(R.debian_version("0.1.0-alpha.1") in name for name in R.packaged_names(ROOT))


def _argv(tree: Path, *args: str) -> list[str]:
    """Always name the tree: `--root` defaults to the repository this tool lives in, not the cwd."""
    return [sys.executable, str(TOOL), "--root", str(tree), *args]


def test_prepare_refuses_a_version_that_is_already_cut(tmp_path):
    """`prepare` is idempotent in the safe direction: an existing tag stops it (a cut happens once)."""
    tree = R._fixture(tmp_path)
    git = ["git", "-c", "user.email=t@example.invalid", "-c", "user.name=t"]
    subprocess.run(git + ["init", "-q"], cwd=tree, check=True)
    subprocess.run(git + ["commit", "-q", "--allow-empty", "-m", "x"], cwd=tree, check=True)
    subprocess.run(git + ["tag", "v0.1.0-alpha.1"], cwd=tree, check=True)
    result = subprocess.run(_argv(tree, "prepare", "--version", "0.1.0-alpha.1"),
                            capture_output=True, text=True, cwd=tree, check=False)
    assert result.returncode == 1
    assert "a release is cut once" in result.stdout


def test_prepare_refuses_a_version_it_cannot_parse(tmp_path):
    tree = R._fixture(tmp_path)
    result = subprocess.run(_argv(tree, "prepare", "--version", "v0.1"),
                            capture_output=True, text=True, cwd=tree, check=False)
    assert result.returncode == 1
    assert "P01" in result.stdout


def test_prepare_dry_run_writes_nothing(tmp_path):
    tree = R._fixture(tmp_path)
    before = {p: p.read_bytes() for p in sorted(tree.rglob("*")) if p.is_file()}
    result = subprocess.run(_argv(tree, "prepare", "--version", "0.1.0-alpha.2", "--dry-run"),
                            capture_output=True, text=True, cwd=tree, check=False)
    assert result.returncode == 0, result.stdout + result.stderr
    after = {p: p.read_bytes() for p in sorted(tree.rglob("*")) if p.is_file()}
    assert before == after, "a dry run wrote something"
    assert "dry run: nothing written" in result.stdout


def test_prepare_moves_the_manifest_and_the_requirements_together(tmp_path):
    """`R05`'s half-done bump: a path dep left at the old version is a build failure for the next clone."""
    tree = R._fixture(tmp_path)
    R.bump_manifest(tree, "0.1.0-alpha.1", "0.1.0-alpha.2", False)
    assert R.declared(tree) == "0.1.0-alpha.2"
    assert set(R.path_dep_requirements(tree)) == {"0.1.0-alpha.2"}
    # Comments and layout survive: this is text surgery, not a TOML round-trip that eats them.
    assert "[workspace.dependencies]" in (tree / R.CARGO).read_text()


def test_prepare_leaves_prose_alone(tmp_path):
    """A sentence about a release that already happened is history; the tool reports it, never rewrites it."""
    tree = R._fixture(tmp_path)
    (tree / R.README).write_text("Released as v0.1.0-alpha.1 in October, from "
                                "https://github.com/o/r/releases/download/v0.1.0-alpha.1/x.sh\n")
    moved = R.bump_readme(tree, "0.1.0-alpha.1", "0.1.0-alpha.2", False)
    text = (tree / R.README).read_text()
    assert moved, "the install URL should have moved"
    assert "Released as v0.1.0-alpha.1 in October" in text, "prose was rewritten"
    assert "releases/download/v0.1.0-alpha.2" in text


# ---------------------------------------------------------------------------
# the trigger
# ---------------------------------------------------------------------------


def test_publish_fires_after_ci_on_the_default_branch_only():
    text = PUBLISH.read_text(encoding="utf-8")
    assert "workflow_run:" in text and "workflows: [ci]" in text, \
        "publish must run after the gates, not beside them"
    assert "github.event.workflow_run.conclusion == 'success'" in text
    assert "github.event.repository.default_branch" in text, \
        "a release must not be cut from a branch that is not the one users are told about"


def test_publish_refuses_the_three_things_it_exists_to_refuse():
    text = PUBLISH.read_text(encoding="utf-8")
    assert "did not change the version line" in text, "the version-move guard is gone"
    assert "already exists; a release is cut once" in text, "the already-cut guard is gone"
    assert re.search(r"CHANGELOG\.md has no dated", text), "the changelog guard is gone"


def test_publish_dispatches_instead_of_publishing():
    """`release.yml` stays the single publisher: one workflow owns the tag and the release, together."""
    text = PUBLISH.read_text(encoding="utf-8")
    steps = "\n".join(line for line in text.splitlines() if not line.lstrip().startswith("#"))
    assert "gh workflow run release.yml" in steps, "publish must dispatch, not publish"
    assert "gh release create" not in steps, "two workflows creating a release is how they disagree"
    assert "actions: write" in steps.split("jobs:")[0]
    assert "gh release create" in RELEASE.read_text(encoding="utf-8"), \
        "release.yml is the one that creates the release; publish.yml assumes it"


def test_the_release_tool_is_wired_into_ci():
    """A tool no workflow calls is a tool that rots."""
    ci = CI.read_text(encoding="utf-8")
    assert "python3 tools/release.py check" in ci
    assert "python3 tools/release.py self-test" in ci
