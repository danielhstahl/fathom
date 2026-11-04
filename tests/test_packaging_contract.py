"""`e7.4`'s packaging is a contract, and this is the half that can be checked without six runners.

The ticket's four acceptance criteria are all sentences about *artefacts a stranger installs*, and three
of the four need machines: a `.deb` installed on a clean VM, an `.msi` on Windows, a `brew install` on a
Mac. What a commit can do — and what this file, `tools/check_release.py` `R14`–`R17`, and two CI jobs do
— is make everything about them that is *a mechanism* fail loudly here instead of quietly on a release
page:

* the **completions and man pages are the CLI's own grammar**, printed by `fathom completions`, and the
  committed copies are re-diffed against the binary in CI. A flag that exists in `--help` and not in your
  shell is the failure this forbids;
* the **generator cannot eat the tree**: `packaging/` also holds a hand-written release script, and an
  early version of the generator pruned it. `write` prunes only what it renders, inside two directories;
* the **`.deb`/`.rpm` are checked by their own formats' tools** — the packager's reader and, in CI,
  `dpkg-deb`, `dpkg --install` and `rpm -qp` — and the packager's nine self-claims include the two that
  matter most (a control member with the wrong size, a package name that disagrees with `--version`);
* the **curl installer's promise is rehearsed locally**, mirror and all, and its four negative controls
  are part of the run: a rehearsal that cannot fail is not a test;
* the **secrets story is a table**, not a paragraph, so "which steps need a maintainer-only secret" stays
    a thing that can be checked rather than reworded away.

Run: python3 -m pytest tests/test_packaging_contract.py -q
(The two tests that need a built binary skip without one, like every other binary-backed contract here;
the installer rehearsal additionally needs `dist` and a local build — see `docs/concepts/release.md` §4.)
"""

from __future__ import annotations

import os
import subprocess
import sys
import tomllib
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))

import gen_shell_artifacts as G  # noqa: E402
import package_release as P  # noqa: E402

GENERATOR = ROOT / "tools" / "gen_shell_artifacts.py"
PACKAGER = ROOT / "tools" / "package_release.py"
REHEARSAL = ROOT / "tools" / "rehearse_installer.py"
CI = ROOT / ".github" / "workflows" / "ci.yml"
CONCEPT = ROOT / "docs" / "concepts" / "release.md"
CONFIG = ROOT / "dist-workspace.toml"
CARGO = ROOT / "Cargo.toml"
PACKAGE_DIR = ROOT / "packaging"
DISTRIB = ROOT / "target" / "distrib"


def run(*argv: str) -> subprocess.CompletedProcess:
    return subprocess.run([sys.executable, *argv], capture_output=True, text=True, cwd=ROOT)


def binary() -> Path:
    """The same resolution `gen_shell_artifacts.py` uses: `$FATHOM_BIN`, else the default-feature debug build.

    Not release-first like some of the other contract files: the committed artefacts were rendered by the
    build the generator itself defaults to, and a test that resolves a *different* binary than the gate it
    is testing is a test of two binaries.
    """
    candidate = Path(os.environ.get("FATHOM_BIN") or "target/debug/fathom")
    if not candidate.is_absolute():
        candidate = ROOT / candidate
    if not candidate.is_file():
        pytest.skip("no fathom binary built; set FATHOM_BIN or run cargo build --release")
    return candidate


def version() -> str:
    return str(tomllib.loads(CARGO.read_text())["workspace"]["package"]["version"])


# --- the shell artefacts -----------------------------------------------------


def test_the_committed_shell_artefacts_are_what_the_binary_prints():
    """The ticket's criterion, as a exit code: drift is impossible because drift is a red build."""
    binary()  # skip without a build; the tool resolves the same default the gate uses
    result = run(str(GENERATOR), "check")
    assert result.returncode == 0, result.stdout + result.stderr
    assert "shell artefacts OK" in result.stdout


def test_the_generator_owns_two_directories_and_not_the_whole_tree():
    """`write` prunes stale files, so its scope has to be narrower than `packaging/`.

    `packaging/linux-packages.sh` is the command `dist` runs to build the system packages, and it lives in
    the same directory as the generated output. An early version of the generator walked all of
    `packaging/`, found the script rendered by nothing, and deleted it — which is the worst possible shape
    for a generator: correct 100 % of the time about what it makes, and destructive about what it does not.
    """
    assert G.OWNED == ("packaging/completions", "packaging/man")
    listed = G.on_disk()
    assert listed, "the generator finds nothing to own, which means it will not notice a stale file"
    for rel in listed:
        assert rel.startswith(G.OWNED), "%s is outside what the generator may prune" % rel
    assert (PACKAGE_DIR / "linux-packages.sh").is_file(), "the release script the packages are built by"


def test_the_generator_can_go_red_four_ways():
    result = run(str(GENERATOR), "self-test")
    assert result.returncode == 0, result.stdout + result.stderr
    assert "self-test OK" in result.stdout


def test_every_man_page_names_the_version_it_was_cut_from():
    """A man page that does not say its version is a page from some release, which is no evidence."""
    binary()  # skip without a build
    pages = sorted((PACKAGE_DIR / "man" / "man1").glob("*.1"))
    assert len(pages) > 20, "one page per verb and subverb; %d is too few" % len(pages)
    for page in pages:
        text = page.read_text()
        # `clap_mangen` opens with two register lines before the title, so "is roff" is a search, not a
        # prefix test — and the title is what `man` prints in its header, so it has to be there at all.
        assert ".TH FATHOM" in text[:400], "%s has no .TH title line" % page.name
        assert version() in text, "%s does not name v%s" % (page.name, version())
        assert ".SH SYNOPSIS" in text, "%s has no synopsis" % page.name


def test_the_documented_install_paths_are_the_paths_the_packages_use():
    """§1.3's table and `package_release.py`'s relocation map have to be one statement, not two."""
    doc = CONCEPT.read_text()
    for mapping in P.SHELL_DIRS.values():
        for target in mapping.values():
            assert target.split("/")[-1] in doc, "%s appears in the packager but not in the document" % target
    # zsh is the one directory that differs by distribution, and a document that names only one of the two
    # is a document that is wrong on the other half of the Linux users it is addressed to.
    assert "vendor-completions" in doc and "site-functions" in doc


# --- the Linux system packages ----------------------------------------------


def test_the_packagers_claims_all_hold():
    """Nine claims, including the two that are invisible until a user hits them: a control member whose
    declared size is wrong, and a file name that disagrees with the binary's `--version`."""
    result = run(str(PACKAGER), "--self-test")
    assert result.returncode == 0, result.stdout + result.stderr
    assert "self-test OK" in result.stdout


def test_the_package_names_carry_the_package_spelling_of_the_version():
    """`0.1.0-alpha.1` is `0.1.0~alpha.1` to `dpkg`, and the file name is where it reads it."""
    up = P.package_version(version())
    assert "-" not in up and "~" in up, "a `-` is not legal in an upstream version"
    config = tomllib.loads(CONFIG.read_text())
    listed = [str(path) for extra in config["dist"]["extra-artifacts"] for path in extra["artifacts"]]
    assert listed, "no [[dist.extra-artifacts]]: nothing builds the system packages"
    for path in listed:
        name = path.rsplit("/", 1)[-1]
        assert up in name, "%s will be misnamed by the next version bump" % name
        assert path.startswith("target/"), "%s is generated and must not be committed" % path
    assert len(set(listed)) == len(listed), "the artefact list has a duplicate: a double upload"


def test_only_the_glibc_triples_get_a_package():
    """The musl build is the static one; a `.deb` of it would declare dependencies it does not have."""
    assert set(P.TARGETS) == {"x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"}
    config = tomllib.loads(CONFIG.read_text())
    for triple in P.TARGETS:
        assert triple in config["dist"]["targets"], "%s is packaged but not built" % triple
    assert not any("musl" in triple for triple in P.TARGETS)


def test_the_package_builder_script_is_what_dist_is_told_to_run():
    """`dist` runs this file in its global phase; a stale path there fails the release, not CI."""
    config = tomllib.loads(CONFIG.read_text())
    for extra in config["dist"]["extra-artifacts"]:
        build = extra.get("build") or []
        assert build and (ROOT / build[0]).is_file(), "%r is not a script in the tree" % build
        assert "rpmbuild" in (ROOT / build[0]).read_text(), "the global runner has no rpmbuild; say so"


# --- the installer ----------------------------------------------------------


def test_the_installer_rehearsal_and_its_negative_controls():
    """Needs the local artefacts (see release.md §4): `dist build --artifacts=global` first."""
    if not (DISTRIB / "fathom-cli-installer.sh").is_file():
        pytest.skip("no installer in target/distrib; run `python3 tools/rehearse_installer.py` once")
    for extra in ([], ["--self-test"]):
        result = run(str(REHEARSAL), "--skip-build", *extra)
        assert result.returncode == 0, result.stdout + result.stderr
        assert "OK" in result.stdout


# --- the wiring and the documentation ---------------------------------------


def test_ci_actually_runs_these_checks():
    """A tool no workflow calls is a tool that rots. Each claim here is a step in `.github/workflows/ci.yml`."""
    ci = CI.read_text()
    for needle in ["tools/gen_shell_artifacts.py check",
                   "tools/package_release.py --self-test",
                   "python3 tools/package_release.py\n",
                   "tools/rehearse_installer.py --no-extra-artifacts",
                   "tools/rehearse_installer.py --skip-build --self-test",
                   "dpkg --install",
                   "rpm -qip"]:
        assert needle in ci, "no CI step runs %r" % needle


def test_the_archive_the_packages_come_from_is_this_runner_s_own():
    """`dist build --artifacts=local` with no `--target` means "all six triples, on this one machine".

    That is what `dist` does with a bare `--artifacts=local`: the triples come from `dist-workspace.toml`,
    and the first macOS one stops the job with `Cross-compilation from x86_64-unknown-linux-gnu to
    aarch64-apple-darwin is not supported` — which is how the packaging job died while asking for the
    archive for its *own* triple (`reco-hzr`). The step asks the compiler which machine it is on, so the
    step cannot outlive the runner label it was written against.
    """
    lines = [line.strip() for line in CI.read_text().splitlines()
             if "dist build" in line and ("artifacts=local" in line or "artifacts=all" in line)]
    assert lines, "the packaging job no longer builds a local archive; where do the .debs come from?"
    for line in lines:
        assert "--target" in line, f"`{line}` asks `dist` for every triple in `dist-workspace.toml`"


def test_the_release_document_tells_the_packaging_story():
    doc = CONCEPT.read_text()
    for section in ["### 1.3 What is inside an archive", "## 8 The secrets"]:
        assert section in doc, "release.md lost %r" % section
    for secret in ["HOMEBREW_TAP_TOKEN", "APPLE_CERTIFICATE", "APPLE_ID", "SSLDOTCOM_USERNAME"]:
        assert secret in doc, "the secrets table no longer names %s" % secret
    assert "not wired" in doc, "the table must say what is not wired, not only what is"


def test_the_cli_verb_that_prints_the_grammar_exists():
    """The whole "drift is impossible" argument rests on the binary being the generator."""
    out = subprocess.run([str(binary()), "completions", "--help"], capture_output=True, text=True)
    assert out.returncode == 0
    assert all(shell in out.stdout for shell in ("bash", "zsh", "fish", "man"))
