"""One commit, one set of bytes — on every platform (**reco-d3p9**).

Two dependency-delta tests were red on the Windows runner and green on the Linux and macOS runners that ran
beside it. Nothing in the product differed. What differed is that the Git for Windows installer sets
`core.autocrlf=true`, and a repository that does not say otherwise lets that decide what a checkout
contains: the same commit arrived on Windows as a different tree, every LF in it rewritten to CRLF. One of
those tests compares a rendered table against a reviewed `expected.txt`, which arrived with a `\r` on
every line; the other looks for a lockfile whose bytes end `}\n` so it can truncate it, found none in a
CRLF checkout, and went on to assert a corruption it had never made.

The rules here are what keeps that class of "red there, green here" from coming back:

* the repository ships a `.gitattributes` whose first rule pins text to LF on checkout, so a clone on any
  platform contains the bytes the blob contains;
* every carriage return in a *stored* blob is on a path declared `-text`, which converts in neither
  direction — an attribute that instead *normalises* such a file would clean its worktree bytes to LF, find
  the stored blob is not LF, and make a clean clone look dirty;
* the paths whose bytes are the point (fuzz seeds, the CRLF diff fixture, cargo-dist's generated `main.wxs`,
  every committed binary) are declared `-text`, and the text a gate reads is declared text;
* and the claim the bug actually violated, checked end to end: a clone made the way a Windows runner makes
  one — `core.autocrlf=true` — yields LF fixtures, and `main.wxs` and `crlf.diff` byte for byte.

The product-side half of the promise lives in Rust: `crates/fathom-analyze/tests/deps_delta.rs`
(`a_checkout_with_windows_line_endings_reads_the_same_as_a_unix_one`) asserts that a case whose fixtures
carry CRLF projects and renders the same delta as the same case with LF, because a repository under review
is allowed to keep its lockfiles with CRLF — it is written on Windows.

Run: python3 -m pytest tests/test_line_endings_contract.py -q
"""

from __future__ import annotations

import pathlib
import shutil
import subprocess

import pytest

ROOT = pathlib.Path(__file__).resolve().parents[1]
ATTRIBUTES = ROOT / ".gitattributes"
DEPS_GOLDEN = pathlib.Path("crates/fathom-analyze/tests/golden/deps-delta")

needs_git = pytest.mark.skipif(
    shutil.which("git") is None or not (ROOT / ".git").exists(),
    reason="asks git what it thinks, and git is not available here",
)

#: Paths whose committed bytes are the fixture, or are not text at all: a converted one is a different
#: input, and the diff viewer's CRLF golden and cargo-dist's generated installer source both read their
#: own carriage returns as content. `test_every_carriage_return_in_a_stored_blob_is_declared` derives this
#: set from the index; this list is the *intent*, so that losing one of them to a re-normalisation is red
#: even on a day the index happens to agree.
BYTE_EXACT = [
    "crates/fathom-tui/tests/fixtures/crlf.diff",
    "crates/fathom-tui/tests/fixtures/empty.diff",
    "crates/fathom-cli/wix/main.wxs",
    "crates/fathom-cli/assets/fathom-demo.pack",
    "docs/screenshots/fathom.png",
    "crates/fathom-analyze/spike/s5/deltas/ep-03fcc089123e2e468204.json.gz",
    "fuzz/corpus/lockfile/go.sum__real",
    "fuzz/corpus/hooks/torn-line",
]

#: Text a gate compares rather than compiles. Each of these arrived with a `\r` on the Windows runner
#: before `.gitattributes` existed, and each comparison is a byte comparison.
MUST_BE_LF = [
    "Cargo.lock",
    "README.md",
    "crates/fathom-analyze/tests/golden/deps-delta/js-direct-add/expected.txt",
    "crates/fathom-analyze/tests/golden/deps-delta/js-direct-add/expected.json",
    "crates/fathom-analyze/tests/golden/deps-delta/js-direct-add/after/package-lock.json",
    "crates/fathom-analyze/tests/golden/deps-delta/cargo-lock-refresh/before/Cargo.lock",
]


def git(*args: str, cwd: pathlib.Path | None = None) -> str:
    """git, asked directly, from the repository under test."""
    result = subprocess.run(
        ["git", *args], cwd=cwd or ROOT, capture_output=True, text=True, check=True
    )
    return result.stdout


@needs_git
def eol_table(cwd: pathlib.Path | None = None) -> dict[str, tuple[str, str, str]]:
    """Every tracked path, as `git ls-files --eol` reads it: `(index eol, worktree eol, attributes)`."""
    table: dict[str, tuple[str, str, str]] = {}
    for line in git("ls-files", "--eol", cwd=cwd or ROOT).splitlines():
        fields, _, path = line.partition("\t")
        parts = fields.split()
        if len(parts) >= 3:
            table[path] = (parts[0], parts[1], " ".join(parts[2:]))
    return table


# --------------------------------------------------------------------------- the policy file
# ---------------------------------------------------------------------------


def test_the_repository_ships_a_gitattributes():
    """Without it, `core.autocrlf` is the policy — and the installer sets it to `true` on Windows."""
    assert ATTRIBUTES.is_file(), (
        ".gitattributes is gone, so every platform's `core.autocrlf` decides what a checkout of this "
        "commit contains; that is what made two dependency-delta tests red on Windows (`reco-d3p9`)"
    )


def test_the_first_rule_pins_text_to_lf_on_checkout():
    """`text=auto eol=lf`: store LF, check out LF, and let git decide what is text at all."""
    rules = [
        line.split("#", 1)[0].strip()
        for line in ATTRIBUTES.read_text(encoding="utf-8").splitlines()
        if line.split("#", 1)[0].strip()
    ]
    star = [rule for rule in rules if rule.split() and rule.split()[0] == "*"]
    assert star, f"`*` has no rule in .gitattributes: {rules[:3]}"
    assert "eol=lf" in star[-1], (
        f"the `*` rule is `{star[-1]}`; without `eol=lf` a checkout inherits `core.eol`, which on "
        "Windows is the CRLF that broke the goldens"
    )


# --------------------------------------------------------------------------- what git resolves
# ---------------------------------------------------------------------------


@needs_git
def test_the_byte_exact_paths_convert_in_neither_direction():
    """`-text` on every path whose bytes are the fixture.

    `-text` and `text eol=crlf` look interchangeable and are not. `main.wxs` and `crlf.diff` carry carriage
    returns *in the stored blob* — `crlf.diff` deliberately mixes lines that have one with lines that do
    not, which no `eol=` value can express. A normalising attribute would clean their worktree bytes to
    something the blob is not, and every clean clone would read as dirty.
    """
    table = eol_table()
    missing = [path for path in BYTE_EXACT if path not in table]
    assert not missing, "these paths are not tracked any more: " + ", ".join(missing)
    converted = [
        f"{path} ({table[path][2]})"
        for path in BYTE_EXACT
        if "-text" not in table[path][2]
    ]
    assert not converted, "no longer declared `-text`, so a checkout rewrites them: " + "; ".join(converted)


@needs_git
def test_the_text_a_gate_reads_checks_out_with_lf():
    """The other side of the same rule: the fixtures and goldens are LF *in the working tree*, everywhere."""
    table = eol_table()
    wrong = [
        f"{path} -> index {table[path][0]}, worktree {table[path][1]}, {table[path][2]}"
        for path in MUST_BE_LF
        if table[path][0] != "i/lf" or "eol=lf" not in table[path][2]
    ]
    assert not wrong, "not pinned to LF: " + "; ".join(wrong)


@needs_git
def test_every_carriage_return_in_a_stored_blob_is_declared():
    """A new file with CRLF in it has to be *thought about*, not committed on a Windows laptop.

    `git ls-files --eol` names every tracked path whose stored content is not plain LF. Each one is
    required to carry `-text`, so the way to satisfy this test after committing a CRLF file is to say which
    of the two things it is: a fixture whose bytes are the point (declare it here and in
    `.gitattributes`), or an accident of somebody's editor (`git add --renormalize` it).
    """
    offenders = [
        f"{path} ({index}, {attrs})"
        for path, (index, _worktree, attrs) in eol_table().items()
        if index not in ("i/lf", "i/-text") and "-text" not in attrs
    ]
    assert not offenders, "stored with a carriage return and not declared `-text`: " + "; ".join(offenders)


@needs_git
def test_the_eol_reader_is_not_reading_nothing():
    """A rule that silently sees no paths is a rule that passes everything, including the bug."""
    table = eol_table()
    assert len(table) > 500, f"`git ls-files --eol` yielded {len(table)} paths; the parse stopped early"
    assert any("-text" in attrs for _path, (_i, _w, attrs) in table.items()), (
        "no path in the repository resolves to `-text`, which means the attribute rules are not read"
    )
    assert any(
        "eol=lf" in attrs for _path, (_i, _w, attrs) in table.items()
    ), "no path resolves to `eol=lf`, so the pin the whole file is about is not being applied"
    assert all(table.get(path) for path in BYTE_EXACT + MUST_BE_LF), (
        "a probe path is not in the index; every rule below that reads `table[path]` is now vacuous"
    )


def crlf_files(root: pathlib.Path, under: pathlib.Path) -> list[str]:
    """Every file under `root / under` whose bytes carry a carriage return, path-relative to `root`."""
    found = []
    for path in (root / under).rglob("*"):
        if path.is_file() and b"\r" in path.read_bytes():
            found.append(str(path.relative_to(root)))
    return sorted(found)


# --------------------------------------------------------------------------- the end-to-end claim
# ---------------------------------------------------------------------------


@needs_git
def test_a_clone_made_the_windows_way_contains_the_same_bytes(tmp_path):
    """The bug, reproduced by construction and refused by the policy.

    `core.autocrlf=true` is what the Git for Windows installer leaves behind, and so what a Windows runner
    clones with. This makes such a clone of this repository and asks the two questions the failing tests
    asked: are the dependency-delta fixtures and goldens LF, and are the paths whose CR is load-bearing
    still exactly what they are in the object store.
    """
    clone = tmp_path / "windows-clone"
    subprocess.run(
        ["git", "-c", "core.autocrlf=true", "clone", "-q", "--no-hardlinks", "--depth", "1",
         str(ROOT), str(clone)],
        capture_output=True, text=True, check=True,
    )
    assert (clone / ".gitattributes").is_file(), (
        "the clone has no .gitattributes, so it is not a clone of a commit with the policy in it"
    )

    assert not crlf_files(clone, DEPS_GOLDEN), (
        "a `core.autocrlf` clone stored CR in " + ", ".join(crlf_files(clone, DEPS_GOLDEN)[:5])
        + " — the dependency goldens compare rendered text byte for byte"
    )

    # The control: the same walk, over the same clone, after this test itself wrote a carriage return into
    # one golden. A check that has never gone red is a check nobody can trust.
    witness = clone / DEPS_GOLDEN / "js-direct-add" / "expected.txt"
    witness.write_bytes(witness.read_bytes().replace(b"\n", b"\r\n"))
    assert crlf_files(clone, DEPS_GOLDEN) == [str(witness.relative_to(clone))], (
        "the walk did not see a CR that was just written, so the assertion above proves nothing"
    )

    for path in BYTE_EXACT:
        stored = subprocess.run(
            ["git", "show", f"HEAD:{path}"], cwd=ROOT, capture_output=True, check=True
        ).stdout
        checked_out = (clone / path).read_bytes()
        assert checked_out == stored, f"{path} is not its stored bytes after a Windows-style checkout"
