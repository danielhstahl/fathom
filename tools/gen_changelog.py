#!/usr/bin/env python3
"""The changelog as a generated object: commits in, release notes out, and a gate on the gap.

`e7.7` (**reco-rny**) is the beta loop, and a beta loop needs two things a hand-written list of changes
does not have: a release body that cannot forget a commit, and a cadence that does not depend on whoever
wrote the code remembering to write prose. This file is both. It reads `git log`, turns commits into
Keep a Changelog sections, and refuses a release whose notes do not account for every commit in it.

    python3 tools/gen_changelog.py draft                    # what the next release would say
    python3 tools/gen_changelog.py draft --json > draft.json
    python3 tools/gen_changelog.py audit                     # what CI runs: does [Unreleased] cover it?
    python3 tools/gen_changelog.py audit --bless             # re-record the changelog's own rung claim
    python3 tools/gen_changelog.py cut --version 0.1.0-alpha.2   # rename [Unreleased], start a new one
    python3 tools/gen_changelog.py lint                      # are the commits shaped the way the tool reads?
    python3 tools/gen_changelog.py self-test                 # two real release cuts in a throwaway repo

**Why "generated" and not "regenerated".** This repository's `CHANGELOG.md` is prose with arguments in it:
each entry says what the ticket decided and what it cost, and several entries name the defect the work
found on the way. No commit subject contains that, so this tool never rewrites an entry — `cut` renames a
heading and opens a new one, `audit` reads, `draft` prints a draft a human edits before it becomes the
section. The generator is the *safety net under* the curated file, which is the only version of "generated
from commits" that does not quietly destroy the most-read document in the repository.

**Conventional commits, and the seven that are not.** The header grammar is the usual one —

    feat(tui)!: the attention queue folds long remedies       type(scope)!: subject
    BREAKING CHANGE: `fathom why` no longer prints the raw vector

with `fix`→Fixed, `feat`→Added, `refactor`/`docs`/`perf`→Changed (the type word stays in the bullet,
because "a refactor" and "a perf fix" are different things to a reviewer), `deprecated`→Deprecated,
`removed`→Removed, `security`→Security, `revert`→Changed with its target named, and
`build`/`ci`/`test`/`chore`/`style` kept out of the release notes unless `--include-internal`. `!` or a
`BREAKING CHANGE:` trailer moves the entry into **Breaking changes**, first. A commit may also say exactly
what the release should claim: `Changelog: <one line>` and `Changelog-Section: Fixed` trailers, which is
how a commit whose subject is an essay contributes one accurate bullet instead of a truncated one.

Six of this repository's last 74 commits use that grammar. Sixty-seven are a prose sentence with a bead ref
in them — *"Cold-index cost was quadratic in the cache, because a diagnostic query missed its index"* —
because that is the house style here, and a tool that demanded `fix(cache):` before it could describe such a
commit would be a tool that gets bypassed. Four carry no bead ref at all, three of them under a `chore:` or
a `plan:` header; the fourth is what the last rung of the ladder, and `C03`, are for. So the fallback is documented and mechanical: a commit is classified
from its trailers if it has them, otherwise from its bead refs, otherwise from a keyword ladder (§"the
fallback ladder" below) into Fixed or Changed, and `lint` says out loud which commits took the ladder.
The subjects stay prose; the classification stays auditable.

**The gate: coverage.** `audit` resolves the range a release covers — from the tag `v<version>` if the
clone has one, otherwise from the commit that added the previous `## [<version>]` heading (this clone has
no tags: it has no remote to push them to, which is `m0.1`/**reco-lnd**'s problem and is why the fallback
exists) — and then asks, for every commit in it, whether the release's section accounts for it: by bead
ref, by plan key (`e6.6`, `s4`, `m2`), by short sha, or by the `Changelog:` text. A commit nobody described
is `C03`. Known gaps are allowed to be listed in `.plan/changelog-coverage.json` with a reason and an
owner, and `C07` makes that list only ever shrink: a baseline entry that *is* now covered fails until it
is pruned, so the exemption file cannot quietly become the design. A **merge commit** is not a candidate
for `C03` at all — recognised by its parents, not by its subject — because a merge is the join rather than
the change and every commit it joins is audited on its own. That is not a theoretical case: the clone CI
audits is often the host's merge commit for the pull request, sitting on a `Merge <sha> into <sha>` no
human wrote and no `Changelog:` trailer will ever arrive for (**reco-d2bo**).

**The rung distribution, and what this file is allowed to quote about it.** `audit` prints which rung of the
fallback ladder produced each commit's bullet — trailer, header, bead, key, bare subject — because a
generator whose notes are mostly guesses is a generator that gets replaced by a human worse at reading
`git log`. That number counts commits, so it moves with every commit that lands, and a copy of it pasted
into prose is true for exactly one commit: the one that wrote it. `reco-0ip` is the test that enforced
exactly that, and that could only be made green by a commit predicting its own rung. So the number is not
prose any more: `audit --bless` writes it into a marked block in `[Unreleased]` **naming the range it
counted**, `audit` counts that range again on every run (`C06`) and says how far behind HEAD the block is,
and a claim inside a released section is frozen with the release it was cut from. Re-recording the reading
is a tool act, and the claim is about a tree rather than about now.

**Two consecutive releases, proven.** The beta criterion is that the changelog process is *followed*, and a
process claim needs a run rather than an intention, so `self-test` builds a throwaway repository, gives it
conventional and legacy commits, cuts `0.1.0-alpha.1` and `0.1.0-alpha.2` through the same `cut` code CI
runs on the real tree, and then breaks five things on purpose (a commit with no entry, a version that does
not match `Cargo.toml`, an undated heading, an exemption that no longer applies, a rung claim whose numbers
do not match the range it names) to show the gate bites.
On the real tree, `audit` is what makes the same claim about the one release that has been cut.

Exit: 0 clean, 1 a rule failed, 2 bad usage or unreadable inputs.
"""
from __future__ import annotations

import argparse
import contextlib
import dataclasses
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field

ROOT = pathlib.Path(__file__).resolve().parent.parent
CHANGELOG = ROOT / "CHANGELOG.md"
BASELINE = ROOT / ".plan" / "changelog-coverage.json"

#: The Keep a Changelog headings, in the order a reader expects them. `Breaking changes` is first because
#: it is the only one that changes what a reader has to do, not merely what they get.
SECTIONS = ["Breaking changes", "Added", "Changed", "Deprecated", "Removed", "Fixed", "Security"]

#: A conventional-commit type and where it lands. Anything absent here is not a release note (`Internal`),
#: which is what `chore:` and `ci:` are for.
TYPE_SECTION = {
    "feat": "Added",
    "fix": "Fixed",
    "perf": "Changed",
    "refactor": "Changed",
    "docs": "Changed",
    "deprecated": "Deprecated",
    "remove": "Removed",
    "removed": "Removed",
    "security": "Security",
    "revert": "Changed",
    "build": "Internal",
    "ci": "Internal",
    "test": "Internal",
    "chore": "Internal",
    "style": "Internal",
}
#: A type whose name carries information the section heading has thrown away. `perf` under **Changed** is
#: a different promise from `refactor` under **Changed**, so the word survives in the bullet.
MARKED_TYPES = {"perf", "docs", "revert", "refactor"}

HEADER = re.compile(r"^(?P<type>[a-z]+)(?:\((?P<scope>[^()]*)\))?(?P<bang>!)?:\s*(?P<subject>\S.*)$")
TRAILER = re.compile(r"^(?P<key>[A-Za-z][A-Za-z0-9-]*)\s*:\s*(?P<value>.+)$")
#: A bead reference. The width is a range and not a constant because the board's ids grew from
#: three characters to four (`reco-ghao`, `reco-cqwz`, `reco-p6zw`) when the three-character
#: space started colliding, and a fixed `{3}` followed by a "no more word characters" lookahead
#: refuses to match *any* four-character id — so every commit of the last fortnight looked like
#: a commit with no bead in it. That is what put 17 commits on the prose rung of the ladder and
#: failed the 10%-guess bar (`reco-ghao`); the ladder was not guessing, the pattern was blind.
#: Sub-issues (`reco-mkl.1`) need nothing here: the ref ends at the dot, which the lookahead
#: already allows, and names the parent bead.
BEAD_REF = re.compile(r"(?<![\w-])(reco-[a-z0-9]{3,4})(?![\w-])")
PLAN_KEY = re.compile(r"(?<![\w.-])([esdm]\d+(?:\.\d+)?|s\d{1,2})(?![\w.])")
SECTION_HEADING = re.compile(r"^##\s+\[([^\]]+)\](?:\s*-\s*(\d{4}-\d{2}-\d{2}))?\s*$", re.M)
#: The fallback ladder. Order matters: a subject that says both "perf" and "fix" is a fix that mattered.
FIX_WORDS = re.compile(r"\b(fix|fixes|fixed|bug|panic|crash|refus|refuses|stop|stops|prevents?|guard|guards|"
                       r"corrects?|typo|regression|leak|crash)\b", re.I)
PERF_WORDS = re.compile(r"\b(perf|faster|slower|quadratic|budget|latency|cache|memory|cost)\b", re.I)
ADDED_WORDS = re.compile(r"\b(adds?|adds|new |introduces?|lands?|ships?|grows?|first)\b", re.I)
#: `Changelog:` / `Changelog-Section:` — a commit's own words about what the release should claim.
CHANGELOG_TRAILER = re.compile(r"^changelog\s*:\s*(?P<value>.+)$", re.I)
SECTION_TRAILER = re.compile(r"^changelog-section\s*:\s*(?P<value>.+)$", re.I)
BREAKING_TRAILER = re.compile(r"^breaking(?:-|\s)change\s*:\s*(?P<value>.+)$", re.I)
REVERT_BODY = re.compile(r"this reverts commit ([0-9a-f]{7,40})", re.I)
#: The trailers this tool reads. A named set, so "metadata" is a claim with a definition rather than a
#: colon in the right place.
KNOWN_TRAILER_KEYS = {"changelog", "changelog-section", "breaking change", "break-change",
                      "co-authored-by", "signed-off-by", "reviewed-by", "acked-by", "tested-by"}
#: Types that are not release notes: they change how the software is built or tested, not what it does.
INTERNAL_TYPES = {"build", "ci", "test", "chore", "style"}

#: Where a release's notes come from, per version: `cut` writes them and `audit` reads them.
UNRELEASED = "Unreleased"

#: The order the rung distribution prints in — the rungs that took the trouble first, the guesses last.
RUNG_ORDER = ["trailer", "header", "bead", "key", "subject", "none"]
#: The sentence `audit` prints, and the sentence a changelog quotes. One constant, so the two cannot drift.
RUNG_HEADLINE = "which rung of the fallback ladder produced each commit's bullet: "
#: What a claim over a range with nothing in it says, so a freshly cut `[Unreleased]` has something to be
#: true about instead of a blank line pretending to be a distribution.
RUNG_EMPTY = "no commits in the range, so there is nothing to count"
#: The changelog's own rung claim, as a generated block written by `audit --bless`. **The marker names the
#: range the numbers counted**, which is the whole point: a distribution counts commits, so a quoted number
#: that means "since the last release, up to right now" is true for exactly one commit — the one that wrote
#: it — and the next commit makes it a lie (**reco-0ip**). Naming the endpoints turns the claim into a claim
#: about a tree: re-checkable forever, and it ages into a named commit instead of into a red build.
RUNG_CLAIM = re.compile(
    r"(?P<indent>[ \t]*)<!--[ \t]*gen_changelog:rungs[ \t]+from[ \t]+(?P<from>[0-9a-f]{7,40}|whole-history)"
    r"[ \t]+to[ \t]+(?P<to>[0-9a-f]{7,40})[ \t]*-->[ \t]*\n"
    r"[ \t]*(?P<line>\S[^\n]*)\n"
    r"[ \t]*<!--[ \t]*/gen_changelog:rungs[ \t]*-->")
#: What a `from` says when the range starts at the beginning of the history (a repository with no release yet).
RUNG_WHOLE = "whole-history"
#: A claim *starts here*, at the beginning of a line (after its indent), which is how `--bless` writes it. A
#: marker mentioned mid-sentence — inside the code span of the entry that documents the block, say — is prose
#: about a claim, not a half-typed claim, so only line-initial ones have to parse.
RUNG_MARKER = re.compile(r"^[ \t]*<!--[ \t]*gen_changelog:rungs", re.M)


def _run(args: list[str], cwd: pathlib.Path) -> subprocess.CompletedProcess:
    """One place that shells out to git, so every caller reads the same repository it was given."""
    return subprocess.run(["git", *args], cwd=str(cwd), capture_output=True, text=True)


@dataclass(frozen=True)
class Problem:
    rule: str
    where: str
    message: str

    def line(self) -> str:
        return "  %-4s %s: %s" % (self.rule, self.where, self.message)


# ---------------------------------------------------------------------------
# reading commits
# ---------------------------------------------------------------------------


@dataclass
class Entry:
    """One bullet in the release notes, and the commit it came from."""

    section: str
    text: str
    marker: str = ""          # a type word that survives into the bullet: `perf`, `docs`, `revert`
    sha: str = ""
    short: str = ""
    refs: list[str] = field(default_factory=list)
    verbatim: bool = False    # came from a `Changelog:` trailer, so it is a human sentence
    legacy: bool = False      # classified by the fallback ladder rather than a conventional header

    def bullet(self) -> str:
        head = "**%s** — " % self.marker if self.marker else ""
        return "- %s%s" % (head, self.text)


@dataclass
class Commit:
    sha: str
    subject: str
    body: str
    date: str
    parents: list[str] = field(default_factory=list)

    def changelog_notes(self) -> list[str]:
        return self.trailers().get("changelog", [])

    @property
    def short(self) -> str:
        return self.sha[:9]

    @property
    def refs(self) -> list[str]:
        text = "%s\n%s" % (self.subject, self.body)
        return sorted(set(BEAD_REF.findall(text)) | set(PLAN_KEY.findall(text)))

    def trailers(self) -> dict[str, list[str]]:
        """The recognised trailers in this commit's body, whatever paragraph they sit in.

        Only named trailers count — `Changelog:`, `Changelog-Section:`, `BREAKING CHANGE:` and the usual
        sign-off family — so a prose paragraph whose lines happen to contain a colon is not mistaken for
        metadata. That restriction is also what lets a one-paragraph body that *is* a trailer work.
        """
        found: dict[str, list[str]] = {}
        for block in self.body.strip().split("\n\n"):
            lines = [line.strip() for line in block.splitlines() if line.strip()]
            if not lines:
                continue
            matched = [TRAILER.match(line) for line in lines]
            if not matched or not all(matched):
                continue
            keys = [m.group("key").lower() for m in matched]
            if not all(key in KNOWN_TRAILER_KEYS for key in keys):
                continue
            for key, m in zip(keys, matched):
                found.setdefault(key, []).append(m.group("value").strip())
        for line in self.body.splitlines():
            match = BREAKING_TRAILER.match(line.strip())
            if match and not found.get("breaking change"):
                found["breaking change"] = [match.group("value").strip()]
        return found

    def is_merge(self) -> bool:
        """A commit with two or more parents: `git merge`, a host's PR merge, a bot's sync.

        A merge is a join, not a change. Its own content is conflict resolution, and the change it
        carries is the commits it joins — every one of which is audited on its own, so a merge can never
        be the commit the notes forgot. It matters because of where the gate runs: a CI clone of a pull
        request is checked out at the host's merge commit, whose subject is a machine's
        `Merge <sha> into <sha>` — no bead ref, no conventional header, no `Changelog:` trailer anyone
        could write, and no human in sight to write one (**reco-d2bo**: the audit was green on every
        laptop, whose clones have no merge commits, and red on the runner, whose clone is one).

        A merge that carries a `Changelog:` trailer has something to say about itself and is taken at its
        word, exactly as a `chore:` commit with one is.
        """
        return len(self.parents) > 1

    def is_internal(self) -> bool:
        """A `ci:`/`chore:`/`test:` commit that says nothing about the release: exempt from coverage.

        Exempt is not invisible: `lint` still reads it, `--include-internal` still prints it, and a commit
        that carries a `Changelog:` trailer or a `BREAKING CHANGE:` has opted back in whatever its type.
        A merge commit is exempt the same way, for the same reason — see :meth:`is_merge`.
        """
        trailers = self.trailers()
        if trailers.get("changelog") or trailers.get("breaking change"):
            return False
        if self.is_merge():
            return True
        header = HEADER.match(self.subject)
        return bool(header) and header.group("type") in INTERNAL_TYPES

    def entries(self, include_internal: bool = False) -> list[Entry]:
        """The release notes this commit contributes; possibly none, possibly more than one."""
        trailers = self.trailers()
        breaking = trailers.get("breaking change", [])
        notes = trailers.get("changelog", [])
        override = (trailers.get("changelog-section") or [""])[0].strip()
        header = HEADER.match(self.subject)
        out: list[Entry] = []

        if header:
            kind = header.group("type")
            scope = (header.group("scope") or "").strip()
            subject = header.group("subject").strip()
            section = TYPE_SECTION.get(kind)
            if section is None:
                # An unknown type is not a licence to vanish: it is a lint problem and a Changed entry.
                section = "Changed"
                out.append(Entry(section, _scoped(scope, subject), marker=kind, sha=self.sha,
                                 short=self.short, refs=self.refs))
            else:
                marker = kind if kind in MARKED_TYPES else ""
                text = "\n".join(_scoped(scope, note) for note in notes) if notes else _scoped(scope, subject)
                out.append(Entry(section, text, marker=marker, sha=self.sha, short=self.short,
                                 refs=self.refs, verbatim=bool(notes)))
            if header.group("bang") or breaking:
                out[0] = dataclasses.replace(
                    out[0], section="Breaking changes",
                    text="\n".join(breaking) if breaking else out[0].text)
        else:
            # The fallback ladder, in the order the docstring states it.
            section = override if override in SECTIONS else ""
            if not section:
                if self.is_merge() and not notes and not breaking:
                    # No bullet for a join; `--include-internal` still prints it. See :meth:`is_merge`.
                    section = "Internal"
                elif self.refs and BEAD_REF.search(" ".join(self.refs)):
                    section = "Fixed" if FIX_WORDS.search(self.subject) else "Added"
                elif FIX_WORDS.search(self.subject):
                    section = "Fixed"
                elif PERF_WORDS.search(self.subject):
                    section = "Changed"
                elif ADDED_WORDS.search(self.subject):
                    section = "Added"
                else:
                    section = "Changed"
            text = "\n".join(notes) if notes else self.subject
            out.append(Entry(section, text, sha=self.sha, short=self.short, refs=self.refs,
                             verbatim=bool(notes), legacy=True))

        if breaking and out and out[0].section != "Breaking changes":
            out.insert(0, Entry("Breaking changes", "\n".join(breaking), sha=self.sha,
                                short=self.short, refs=self.refs))
        if not include_internal:
            out = [e for e in out if e.section != "Internal"]
        return out

    def rung(self) -> str:
        """Which rung of the fallback ladder produces this commit's bullet (C06's other half).

        `trailer` — a `Changelog:` line was written on purpose; `header` — a conventional subject;
        `bead` — no header, so the bead's title is the release note; `key` — a plan key (`e1.8`) with no
        bead ref to look up; `subject` — a prose subject and nothing else, the rung the notes are guessing
        at; `none` — an internal commit that gets no bullet at all.
        """
        if self.changelog_notes():
            return "trailer"
        if self.is_merge():
            return "none"
        header = HEADER.match(self.subject)
        if header:
            return "none" if self.is_internal() else "header"
        if any(BEAD_REF.search(r) for r in self.refs):
            return "bead"
        if any(PLAN_KEY.search(r) for r in self.refs):
            return "key"
        return "subject"

    def lint(self) -> str:
        """Why this commit is not conventional-shaped; empty when it is."""
        if self.is_merge() and not self.trailers().get("changelog"):
            return ""            # a machine wrote this subject; there is nothing here for a human to fix
        header = HEADER.match(self.subject)
        if header:
            if header.group("type") not in TYPE_SECTION:
                return "unknown type %r; expected one of %s" % (
                    header.group("type"), ", ".join(sorted(TYPE_SECTION)))
            if len(self.subject) > 100:
                return "header is %d characters; keep the subject under 100 and put the argument in the body" % len(self.subject)
            return ""
        if self.trailers().get("changelog"):
            return ""
        if self.refs:
            return "prose subject classified by the fallback ladder; add `Changelog:` to choose the wording"
        return "no conventional header, no bead ref and no `Changelog:` trailer: the release notes are guessing"


def _scoped(scope: str, text: str) -> str:
    return "**%s**: %s" % (scope, text) if scope else text


def is_shallow(root: pathlib.Path) -> bool:
    """Whether this clone was fetched with its history cut short."""
    return _run(["rev-parse", "--is-shallow-repository"], root).stdout.strip() == "true"


def has_commit(root: pathlib.Path, ref: str) -> bool:
    """Whether this clone actually holds the commit `ref` names.

    `rev-parse --verify` is not the test: handed a full 40-character SHA it answers success for
    an object the clone has never seen, which is how a depth-1 checkout walked past every
    resolver this file has and then died inside `git log` with an "Invalid revision range"
    traceback (`reco-ghao`). `cat-file -e` asks the object store instead.
    """
    if not ref:
        return True
    return _run(["cat-file", "-e", "%s^{commit}" % ref], root).returncode == 0


#: What to say when a range cannot be read, because the reason is almost never the range.
SHALLOW_HINT = ("this clone does not hold the commits that range names — a checkout that audits "
                "history has to fetch it, so a CI job needs `actions/checkout` with "
                "`fetch-depth: 0`")


def read_commits(root: pathlib.Path, since: str, until: str = "HEAD") -> list[Commit]:
    """`git log` over `since..until`, oldest first, with bodies kept intact.

    The parents come along because a merge is only recognisable by them: `%p` is the one field that
    says "this commit joins", and a subject like `Merge <sha> into <sha>` — the one a CI clone of a
    pull request is sitting on — says it only to a pattern written for that one phrasing.
    """
    args = ["log", "--reverse", "--date=short", "--format=%H\x1f%p\x1f%ad\x1f%s\x1f%b\x1e"]
    args.append("%s..%s" % (since, until) if since else until)
    result = _run(args, root)
    if result.returncode != 0:
        hint = " — %s" % SHALLOW_HINT if is_shallow(root) else ""
        raise SystemExit("git log failed: %s%s" % (result.stderr.strip(), hint))
    commits = []
    for record in result.stdout.split("\x1e"):
        if not record.strip():
            continue
        parts = record.strip("\n").split("\x1f")
        if len(parts) < 5:
            continue
        sha, parents, date, subject, body = parts[:5]
        commits.append(Commit(sha=sha.strip(), date=date.strip(), subject=subject.strip(),
                              body=body.rstrip(), parents=[p for p in parents.split() if p]))
    return commits


# ---------------------------------------------------------------------------
# reading the changelog
# ---------------------------------------------------------------------------


def section_spans(text: str) -> list[tuple[str, str | None, int, int]]:
    """Every `## [x]` heading as (version, date, body starts, body ends) offsets, in file order."""
    out: list[tuple[str, str | None, int, int]] = []
    matches = list(SECTION_HEADING.finditer(text))
    for index, match in enumerate(matches):
        end = matches[index + 1].start() if index + 1 < len(matches) else len(text)
        out.append((match.group(1).strip(), match.group(2), match.end(), end))
    return out


def sections(path: pathlib.Path = CHANGELOG) -> dict[str, dict]:
    """Every `## [x]` section of the changelog, in file order, with its body text."""
    text = path.read_text() if path.exists() else ""
    out: dict[str, dict] = {}
    for version, date, start, end in section_spans(text):
        body = text[start:end]
        if version in out:
            out[version] = {"duplicate": True, "date": date,
                            "body": out[version]["body"] + body}
            continue
        out[version] = {"date": date, "body": body, "duplicate": False}
    return out


def released_versions(path: pathlib.Path = CHANGELOG) -> list[str]:
    """The dated versions in the file, newest cut first (file order is the project's order)."""
    return [v for v, s in sections(path).items() if v != UNRELEASED]


def release_anchor(root: pathlib.Path, version: str | None, tag_first: bool = True) -> tuple[str, str]:
    """Where a release started: a rev to read commits from, and how that was decided.

    The tag is the truth when it exists. When it does not — and in a clone with no remote, pushed tags are
    exactly what is missing — the commit that *added the section* is the cut, because `cut` renames the
    heading in the same commit that bumps the version and that commit is in the history even when the tag
    object is not.
    """
    if version is None:
        return "", "the whole history"
    if tag_first:
        for candidate in ("v%s" % version, version):
            probe = _run(["rev-parse", "--verify", "--quiet", candidate], root)
            if probe.returncode == 0:
                return probe.stdout.strip(), "tag %s" % candidate
    found = _run(["log", "--format=%H", "-S", "## [%s]" % version, "--reverse", "--", "CHANGELOG.md"], root)
    for line in found.stdout.splitlines():
        if line.strip():
            return line.strip(), "the commit that added the `## [%s]` section" % version
    return "", "no tag and no section-commit for %s; nothing to measure from" % version


def resolve_previous(root: pathlib.Path, upto: str | None = None, path: pathlib.Path = CHANGELOG) -> str | None:
    versions = released_versions(path)
    if not versions:
        return None
    if upto is None:
        return versions[0]
    if upto in versions:
        return versions[versions.index(upto) + 1] if versions.index(upto) + 1 < len(versions) else None
    return upto


# ---------------------------------------------------------------------------
# drafting
# ---------------------------------------------------------------------------


def draft(commits: list[Commit], include_internal: bool = False) -> dict[str, list[Entry]]:
    """Group every entry from every commit into the sections, in the order a reader wants them."""
    grouped: dict[str, list[Entry]] = {}
    for commit in commits:
        for entry in commit.entries(include_internal=include_internal):
            grouped.setdefault(entry.section, []).append(entry)
    return {name: grouped[name] for name in SECTIONS if name in grouped}


def render(version: str, grouped: dict[str, list[Entry]], date: str = "",
           heading: str = "## [%s]%s") -> str:
    """The markdown for one release section, in Keep a Changelog shape."""
    lines = [heading % (version, " - %s" % date if date else "")]
    for name in SECTIONS:
        entries = grouped.get(name)
        if not entries:
            continue
        lines.append("")
        lines.append("### %s" % name)
        lines.append("")
        for entry in entries:
            suffix = "" if entry.verbatim else "  `%s`" % entry.short
            lines.append(entry.bullet() + suffix)
    return "\n".join(lines).rstrip() + "\n"


# ---------------------------------------------------------------------------
# coverage: the gate
# ---------------------------------------------------------------------------


def load_baseline(path: pathlib.Path = BASELINE) -> dict:
    if not path.exists():
        return {"entries": []}
    try:
        data = json.loads(path.read_text())
    except json.JSONDecodeError as exc:
        raise SystemExit("%s does not parse: %s" % (path, exc))
    data.setdefault("entries", [])
    return data


def covered(commit: Commit, body: str) -> str:
    """How a commit is accounted for in a section; empty when it is not accounted for at all."""
    for ref in commit.refs:
        if ref in body:
            return ref
    if commit.short and commit.short in body:
        return "sha"
    for entry in commit.entries(include_internal=True):
        probe = re.sub(r"[^a-z0-9 ]", "", entry.text.lower())
        words = [w for w in probe.split() if len(w) > 3][:6]
        if len(words) >= 3 and all(w in body.lower() for w in words):
            return "prose"
    return ""


def audit(root: pathlib.Path, version: str | None, from_ref: str = "", to_ref: str = "HEAD",
          baseline_path: pathlib.Path = BASELINE, section_path: pathlib.Path = CHANGELOG) -> tuple[
          list[Problem], list[str], dict]:
    """C03 and C07: every commit in the range is described, and the exemption list only shrinks."""
    problems: list[Problem] = []
    notes: list[str] = []
    body_sections = sections(section_path)
    target = version or UNRELEASED
    section = body_sections.get(target)
    if section is None:
        return ([Problem("C01", str(section_path.relative_to(root)) if section_path.is_relative_to(root)
                         else str(section_path), "no `## [%s]` section to audit" % target)], [], {})
    if from_ref:
        since = from_ref
    else:
        previous = resolve_previous(root, version, section_path)
        since = release_anchor(root, previous)[0] if previous else ""
    commits = read_commits(root, since, to_ref)
    text = section["body"]
    baseline = load_baseline(baseline_path)
    exempt = {str(e.get("ref")): e for e in baseline["entries"]}

    gaps: list[dict] = []
    internal = 0
    merges = 0
    rungs = count_rungs(commits)
    for commit in commits:
        if commit.is_merge():
            merges += 1
        if commit.is_internal():
            internal += 1
            continue
        how = covered(commit, text)
        if how:
            continue
        refs = commit.refs
        waived = next((r for r in refs if r in exempt), None)
        if waived or commit.short in exempt:
            continue
        gaps.append({"sha": commit.short, "subject": commit.subject, "refs": refs,
                     "lint": commit.lint()})
    if gaps:
        for gap in gaps:
            problems.append(Problem("C03", gap["sha"],
                                    "%s is not accounted for in `## [%s]` — %s%s"
                                    % (", ".join(gap["refs"]) or "a commit with no bead ref", target,
                                       gap["subject"][:78],
                                       " (%s)" % gap["lint"] if gap["lint"] else "")))
        notes.append("%d of %d commits in %s..%s are not accounted for in `## [%s]`"
                     % (len(gaps), len(commits), since or "*", to_ref, target))
    else:
        notes.append("every one of %d commits in %s..%s is accounted for in `## [%s]`"
                     % (len(commits), since or "*", to_ref, target))

    # C07 — an exemption that has become covered, or whose commit is gone, is a lie with a name on it.
    for entry in baseline["entries"]:
        ref = str(entry.get("ref", ""))
        if not ref:
            problems.append(Problem("C07", str(baseline_path.relative_to(root) if baseline_path.is_relative_to(root) else baseline_path),
                                    "an exemption names nothing: every entry needs a `ref`, a `reason` and an `owner`"))
            continue
        missing = [k for k in ("reason", "owner") if not str(entry.get(k, "")).strip()]
        if missing:
            problems.append(Problem("C07", ref, "exemption is missing %s; an unnamed exemption is not an exemption"
                                    % " and ".join(missing)))
        if ref in text:
            problems.append(Problem("C07", ref, "is now described in `## [%s]` — delete the exemption, "
                                    "which is the only direction this list is allowed to move" % target))
    notes.append("%s: %d commits read, %d not release notes (%d of them merge commits), %d gaps, "
                 "%d exemptions on file"
                 % (target, len(commits), internal, merges, len(gaps), len(baseline["entries"])))
    if merges:
        notes.append("%d merge commit(s) in the range: a merge is the join, not the change, and every "
                     "commit it joins is audited on its own — which is why a merge is never a coverage "
                     "gap, and why the runner (checked out at the host's merge commit) is not red for it"
                     % merges)
    # The honest half of the fallback ladder, printed every time: which rung each commit's bullet came from.
    # A generator whose notes are mostly guesses is a generator that will be replaced by a human who is worse
    # at reading `git log`, so the guess rate is a number the gate states rather than a detail of the code.
    notes.append(RUNG_HEADLINE + rung_line(rungs))
    # C06 read back: anything this file quotes about its own ladder has to be true for the range it names.
    claim_problems, claim_notes = check_rung_claims(root, section_path)
    problems.extend(claim_problems)
    notes.extend(claim_notes)
    return problems, notes, {"range": "%s..%s" % (since, to_ref), "commits": len(commits),
                             "internal": internal, "merges": merges, "gaps": gaps, "rungs": rungs,
                             "rung_line": rung_line(rungs),
                             "exemptions": len(baseline["entries"]), "section": target}


# ---------------------------------------------------------------------------
# the rung distribution, and the claim a changelog makes about it
# ---------------------------------------------------------------------------


def count_rungs(commits: list[Commit]) -> dict[str, int]:
    """How many commits each rung of the fallback ladder produced."""
    rungs: dict[str, int] = {}
    for commit in commits:
        rung = commit.rung()
        rungs[rung] = rungs.get(rung, 0) + 1
    return rungs


def rung_line(rungs: dict[str, int]) -> str:
    """A rung distribution as one line: `header 1, bead 70, key 5, subject 1`."""
    return ", ".join("%s %d" % (r, rungs[r]) for r in RUNG_ORDER if r in rungs)


def rung_claim_text(from_ref: str, to_ref: str, rungs: dict[str, int], commits: int,
                    indent: str = "") -> str:
    """The generated block itself — the only place a rung distribution is allowed to be written by a tool."""
    line = RUNG_HEADLINE + rung_line(rungs) if commits else RUNG_EMPTY
    return ("%s<!-- gen_changelog:rungs from %s to %s -->\n"
            "%s%s\n"
            "%s<!-- /gen_changelog:rungs -->" % (indent, from_ref or RUNG_WHOLE, to_ref, indent, line, indent))


def rung_claims(path: pathlib.Path = CHANGELOG) -> list[dict]:
    """Every rung claim in a changelog, with the section holding it and the span it occupies."""
    text = path.read_text() if path.exists() else ""
    spans = section_spans(text)
    out: list[dict] = []
    for match in RUNG_CLAIM.finditer(text):
        section = next((v for v, _date, start, end in spans if start <= match.start() < end), "")
        out.append({"from": match.group("from"), "to": match.group("to"),
                    "line": match.group("line").strip(), "section": section,
                    "indent": match.group("indent"), "span": match.span()})
    return out


def quoted_rung_line(line: str) -> str:
    """The distribution inside a quoted line, without the sentence around it."""
    quoted = line.strip().strip("`").strip()
    return quoted[len(RUNG_HEADLINE):].strip() if quoted.startswith(RUNG_HEADLINE) else quoted


def check_rung_claims(root: pathlib.Path = ROOT, path: pathlib.Path = CHANGELOG) -> tuple[list[Problem],
                                                                                          list[str]]:
    """C06, read back: every rung claim in the changelog is true for the range it names.

    The distribution is a diagnostic about a *range of commits*, and a claim printed into prose can only
    name one range by accident — the range as of the commit that wrote it. That is `reco-0ip`: the test that
    compared the quote against the live distribution could only be satisfied by a commit that predicted its
    own rung, so the suite was red for everybody, always, and a permanent red is a red nobody reads. So the
    claim names its endpoints and this counts that range again: the number rots on schedule, into a named
    tree rather than into a failure, and `audit --bless` is the act that re-records it.
    """
    problems: list[Problem] = []
    notes: list[str] = []
    if not path.exists():
        return problems, notes
    text = path.read_text()
    claims = rung_claims(path)
    stray = len(RUNG_MARKER.findall(text)) - len(claims)
    if stray:
        problems.append(Problem("C06", path.name,
                                "%d claim(s) start but do not parse: a block is "
                                "`<!-- gen_changelog:rungs from <sha> to <sha> -->`, then the audit's line, "
                                "then `<!-- /gen_changelog:rungs -->` — or run `audit --bless`"
                                % abs(stray)))
    for claim in claims:
        where = "%s%s" % (("`[%s]` " % claim["section"]) if claim["section"] else "", claim["to"][:12])
        start = "" if claim["from"] == RUNG_WHOLE else claim["from"]
        unresolved = [ref for ref in (start, claim["to"])
                      if ref and not has_commit(root, ref)]
        if unresolved:
            problems.append(Problem("C06", where,
                                    "names %s, which this clone cannot resolve: a claim outlives the branch "
                                    "it was written on, but not the objects it counts%s"
                                    % (", ".join(r[:12] for r in unresolved),
                                       "; " + SHALLOW_HINT if is_shallow(root) else "")))
            continue
        commits = read_commits(root, start, claim["to"])
        expected = rung_line(count_rungs(commits)) if commits else RUNG_EMPTY
        quoted = quoted_rung_line(claim["line"])
        if quoted != expected:
            problems.append(Problem("C06", where,
                                    "quotes `%s` for `%s..%s`, which counts `%s`; the numbers are generated, "
                                    "not hand-edited — run `python3 tools/gen_changelog.py audit --bless`"
                                    % (quoted, claim["from"][:12], claim["to"][:12], expected)))
            continue
        behind = len(read_commits(root, claim["to"], "HEAD"))
        notes.append("the rung claim in `[%s]` is true for the range it names: %d commit(s) in `%s..%s`, "
                     "%d behind HEAD — %s"
                     % (claim["section"] or "?", len(commits), claim["from"][:12], claim["to"][:12],
                        behind, expected))
    if not claims:
        notes.append("no rung claim in %s; `audit --bless` writes one, so that the distribution is a fact "
                     "about a named tree rather than a sentence about now" % path.name)
    return problems, notes


def bless_rungs(root: pathlib.Path = ROOT, path: pathlib.Path = CHANGELOG, since: str = "",
                until: str = "HEAD", dry_run: bool = False) -> tuple[list[Problem], list[str], dict]:
    """Re-record the changelog's own rung claim for the range `audit` is measuring.

    Claims inside a released section are left alone: a released section describes the tree it was cut from,
    and rewriting its numbers is revising history rather than refreshing a reading. Claims in `[Unreleased]`
    are the live reading, and a claim that has never been written is written at the end of that section.
    The point of the tool act is that a human never has to guess a number which includes the commit they are
    about to make — which is the trap `reco-0ip` is about.
    """
    problems: list[Problem] = []
    notes: list[str] = []
    text = path.read_text()
    spans = section_spans(text)
    unreleased = next(((start, end) for v, _date, start, end in spans if v == UNRELEASED), None)
    if unreleased is None:
        return ([Problem("C01", path.name, "no `## [%s]` section; there is no live range to bless" % UNRELEASED)],
                notes, {})
    previous = resolve_previous(root, None, path)
    start = since or release_anchor(root, previous)[0]
    end = until
    to_sha = _run(["rev-parse", "--verify", "--quiet", end], root).stdout.strip() if end else ""
    if end and not to_sha:
        raise SystemExit("%s is not a commit this clone can resolve" % end)
    commits = read_commits(root, start, end)
    rungs = count_rungs(commits)

    claims = rung_claims(path)
    live = [c for c in claims if c["section"] == UNRELEASED]
    frozen = len(claims) - len(live)
    pieces: list[tuple[int, int, str]] = []
    if live:
        for claim in live:
            pieces.append((claim["span"][0], claim["span"][1],
                           rung_claim_text(start, to_sha, rungs, len(commits), claim["indent"])))
    else:
        pieces.append((unreleased[1], unreleased[1],
                       "\n\n" + rung_claim_text(start, to_sha, rungs, len(commits)) + "\n"))
    new_text = text
    for begin, finish, replacement in sorted(pieces, key=lambda p: p[0], reverse=True):
        if begin == finish:  # an insertion point, not a span
            new_text = new_text[:begin].rstrip("\n") + replacement + new_text[begin:].lstrip("\n")
        else:
            new_text = new_text[:begin] + replacement + new_text[finish:]
    if not dry_run:
        path.write_text(new_text)
    notes.append("%s the rung claim for `%s..%s` (%d commit(s)): %s%s"
                 % ("rewrote" if live else "wrote", (start or RUNG_WHOLE)[:12], to_sha[:12], len(commits),
                    rung_line(rungs) if commits else RUNG_EMPTY,
                    " (dry run: nothing written)" if dry_run else ""))
    if frozen:
        notes.append("%d claim(s) inside released sections left exactly as they are — a cut release "
                     "describes the tree it was cut from" % frozen)
    # What was written is true by construction — it was just counted from the range it names. Whether the
    # *rest* of the file is honest is `audit`'s question, and `audit --bless` runs it afterwards.
    return problems, notes, {"range": "%s..%s" % (start or RUNG_WHOLE, to_sha), "commits": len(commits),
                             "rungs": rungs, "rewrote": len(live), "frozen": frozen}


# ---------------------------------------------------------------------------
# the rules that are not coverage
# ---------------------------------------------------------------------------


def check_changelog(path: pathlib.Path = CHANGELOG, root: pathlib.Path = ROOT) -> tuple[list[Problem], list[str]]:
    """C01/C02: the file's shape — one `[Unreleased]`, first, everything else dated and not repeated."""
    problems: list[Problem] = []
    notes: list[str] = []
    if not path.exists():
        return [Problem("C01", "CHANGELOG.md", "missing")], notes
    parsed = sections(path)
    if UNRELEASED not in parsed:
        problems.append(Problem("C01", "CHANGELOG.md",
                                "no `## [%s]` section; work has nowhere to land before a cut" % UNRELEASED))
    elif parsed[UNRELEASED]["duplicate"]:
        problems.append(Problem("C01", "CHANGELOG.md", "two `## [%s]` sections; only the first is read" % UNRELEASED))
    order = [v for v in parsed]
    if order and order[0] != UNRELEASED and UNRELEASED in order:
        problems.append(Problem("C01", "CHANGELOG.md", "`## [%s]` is at position %d; it has to be first"
                                % (UNRELEASED, order.index(UNRELEASED) + 1)))
    dated = 0
    for version, section in parsed.items():
        if version == UNRELEASED:
            continue
        if section["duplicate"]:
            problems.append(Problem("C02", "CHANGELOG.md", "`## [%s]` appears twice" % version))
        if not section["date"]:
            problems.append(Problem("C02", "CHANGELOG.md",
                                    "`## [%s]` has no date; cargo-dist builds the release body from this "
                                    "file and `R08` already fails the cut — this is the same rule, earlier" % version))
        else:
            dated += 1
        if not re.match(r"^\d+\.\d+\.\d+", version):
            problems.append(Problem("C02", "CHANGELOG.md", "`## [%s]` is not a version" % version))
    notes.append("%d dated release section(s), %d unreleased" % (dated, 1 if UNRELEASED in parsed else 0))
    return problems, notes


def check_shape(root: pathlib.Path, since: str, until: str, strict: bool = False) -> tuple[list[Problem], list[str]]:
    """C05: are the commits shaped the way this tool reads them?"""
    problems: list[Problem] = []
    notes: list[str] = []
    commits = read_commits(root, since, until)
    noisy = 0
    for commit in commits:
        why = commit.lint()
        if not why:
            continue
        noisy += 1
        problems.append(Problem("C05", commit.short, "%s — %s" % (commit.subject[:64], why)))
    level = "error" if strict else "warning"
    notes.append("%d of %d commits in %s..%s are conventional-shaped (%s)"
                 % (len(commits) - noisy, len(commits), since or "*", until, level))
    if strict:
        return problems, notes
    return [], notes


def package_version(root: pathlib.Path = ROOT) -> str:
    """The version the workspace carries, read as text: this must not need a cargo build."""
    text = (root / "Cargo.toml").read_text()
    match = re.search(r'^\[workspace\.package\][^[]*?^\s*version\s*=\s*"([^"]+)"', text, re.S | re.M)
    return match.group(1) if match else ""


# ---------------------------------------------------------------------------
# cutting a release
# ---------------------------------------------------------------------------


def cut(root: pathlib.Path, version: str, date: str, out: pathlib.Path | None = None,
        force: bool = False, dry_run: bool = False, include_internal: bool = False,
        baseline: pathlib.Path | None = None) -> tuple[
        list[Problem], list[str], str]:
    """Rename `[Unreleased]` to the version and open a new `[Unreleased]`; print the release body."""
    problems: list[Problem] = []
    notes: list[str] = []
    path = out or CHANGELOG
    text = path.read_text()
    if not re.match(r"^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$", version):
        problems.append(Problem("C02", version, "not a version this tool will write; expected 1.2.3 or 1.2.3-rc.1"))
        return problems, notes, ""
    parsed = sections(path)
    if version in parsed:
        problems.append(Problem("C02", version, "`## [%s]` already exists; a release is cut once" % version))
        return problems, notes, ""
    carry = package_version(root)
    if carry and version != carry and not force:
        problems.append(Problem("C04", "Cargo.toml",
                                "the workspace carries %s and you asked to cut %s; bump the version first "
                                "(or say --force to cut a changelog for a version the tree does not carry)"
                                % (carry, version)))
        return problems, notes, ""
    if UNRELEASED not in parsed:
        problems.append(Problem("C01", path.name, "no `## [%s]` section to cut" % UNRELEASED))
        return problems, notes, ""

    previous = resolve_previous(root, None, path)
    since = release_anchor(root, previous)[0]
    commits = read_commits(root, since, "HEAD")
    gaps_problems, gap_notes, data = audit(root, None, from_ref=since,
                                           baseline_path=baseline or BASELINE, section_path=path)
    if gaps_problems and not force:
        problems.extend(gaps_problems)
        notes.append("nothing was written; %d commit(s) in `%s` are not accounted for (audit ran first)"
                     % (len(gaps_problems), UNRELEASED))
        notes.extend(gap_notes)
        return problems, notes, ""

    grouped = draft(commits, include_internal=include_internal)
    # The cut is a rename, not a rewrite: the curated prose travels with the heading, and the generator's
    # draft is printed beside it as the release body — never pasted into the file over somebody's prose.
    match = SECTION_HEADING.search(text)
    head, tail = text[:match.start()], text[match.end():]
    new_text = head + "## [%s]\n\n" % UNRELEASED + "## [%s] - %s\n" % (version, date) + tail
    # One blank line between the preamble and `[Unreleased]`, and never two in a row.
    new_text = re.sub(r"\n{3,}", "\n\n", new_text)
    if not dry_run:
        path.write_text(new_text)
    notes.append("renamed `## [%s]` to `## [%s] - %s` and started a new `## [%s]`%s"
                 % (UNRELEASED, version, date, UNRELEASED, " (dry run: nothing written)" if dry_run else ""))
    announcement = render(version, grouped, date)
    notes.append("%d commits since %s; %d of them generated a bullet (%d section(s))"
                 % (len(commits), previous or "the beginning",
                    len([e for rows in grouped.values() for e in rows]), len(grouped)))
    return problems, notes, announcement


# ---------------------------------------------------------------------------
# the self-test: two consecutive releases, in a repository that is not this one
# ---------------------------------------------------------------------------


def self_test() -> int:
    """Build a throwaway repository, cut two releases through `cut`, then prove the gate bites."""
    work = pathlib.Path(tempfile.mkdtemp(prefix="gen-changelog-self-test-"))
    repo = work / "repo"
    repo.mkdir(parents=True)
    env = {"GIT_AUTHOR_NAME": "self test", "GIT_AUTHOR_EMAIL": "self@test.invalid",
           "GIT_COMMITTER_NAME": "self test", "GIT_COMMITTER_EMAIL": "self@test.invalid"}

    def git(*args: str) -> str:
        proc = subprocess.run(["git", *args], cwd=str(repo), capture_output=True, text=True,
                              env={**os.environ, **env})
        if proc.returncode != 0:
            raise SystemExit("git %s failed: %s" % (" ".join(args), proc.stderr.strip()))
        return proc.stdout.strip()

    def commit(message: str, touch: str = "") -> str:
        (repo / (touch or "notes")).write_text("%s\n" % message)
        git("add", "-A")
        git("commit", "-m", message)
        return git("rev-parse", "HEAD")

    failures: list[str] = []

    def expect(condition: bool, what: str) -> None:
        print("  %s %s" % ("ok  " if condition else "FAIL", what))
        if not condition:
            failures.append(what)

    try:
        git("init", "-q", "-b", "main")
        # A changelog in the shape the real one has: a preamble, then [Unreleased].
        (repo / "CHANGELOG.md").write_text(
            "# Changelog\n\nRules of the road.\n\n## [Unreleased]\n\n### Added\n\n"
            "- The thing (`reco-aaa`)\n\n- A fix that came later (`reco-bbb`)\n")
        (repo / "Cargo.toml").write_text('[workspace.package]\nname = "x"\nversion = "0.1.0-alpha.1"\n')
        commit("chore: the repository exists", touch="notes")
        commit("feat(cli)!: the review queue ranks episodes (reco-aaa)\n\n"
               "BREAKING CHANGE: `fathom queue` is gone; `fathom review status` replaces it.")
        commit("ci: the release job runs the audit", touch="ci")
        commit("fix(tui): the queue stops flickering on a rebase (reco-bbb)\n\n"
               "Changelog: The attention queue no longer flickers when the branch moves.",
               touch="fix")
        legacy = commit("Cold-index cost was quadratic in the cache, because a diagnostic query missed "
                        "its index (reco-ccc)", touch="legacy")

        # ---- release 1: alpha.1 -------------------------------------------------------------------
        # First, the gate before the cut: `reco-ccc` landed and nobody described it, and that is exactly
        # what `C03` exists to refuse. The `chore:` and `ci:` commits are exempt without being described,
        # because they are not release notes — which is the other half of the same rule.
        problems, notes, _ = audit(repo, None, baseline_path=repo / "none.json",
                                   section_path=repo / "CHANGELOG.md")
        expect(any(p.rule == "C03" and "reco-ccc" in p.message for p in problems),
               "C03 refuses to cut while a described-nowhere commit is in the range")
        expect(len([p for p in problems if p.rule == "C03"]) == 1,
               "and only that one: %s" % [p.message for p in problems if p.rule == "C03"])

        problems, notes, announcement = cut(repo, "0.1.0-alpha.1", "2026-10-18",
                                            out=repo / "CHANGELOG.md", baseline=repo / "none.json")
        expect(bool(problems) and all(p.rule == "C03" for p in problems),
               "the cut itself refuses for the same reason")
        # The human writes the missing line, and the cut goes through.
        (repo / "CHANGELOG.md").write_text((repo / "CHANGELOG.md").read_text().replace(
            "## [Unreleased]\n", "## [Unreleased]\n\n- Cold-index cost was quadratic in the cache (`reco-ccc`)\n"))
        git("add", "-A")
        git("commit", "-m", "docs: the changelog line for reco-ccc")
        problems, notes, announcement = cut(repo, "0.1.0-alpha.1", "2026-10-18",
                                            out=repo / "CHANGELOG.md", baseline=repo / "none.json")
        expect(not problems, "the first cut is clean once the notes cover it: %s"
               % [p.line() for p in problems])
        text = (repo / "CHANGELOG.md").read_text()
        expect("## [0.1.0-alpha.1] - 2026-10-18" in text, "the heading moved to the version, dated")
        expect("## [Unreleased]" in text, "a fresh [Unreleased] was opened")
        expect("### Breaking changes" in announcement and "`fathom queue` is gone" in announcement,
               "the breaking change is its own section in the release body the announcement prints")
        expect("### Fixed" in announcement and "flickers when the branch moves" in announcement,
               "and the `Changelog:` trailer chose that bullet's words")
        expect("flickers" not in text,
               "while the file keeps the curated prose rather than the generated bullet")
        expect("The thing (`reco-aaa`)" in text, "the curated prose travelled with the heading")
        expect("ci:" not in text and "repository exists" not in text,
               "`ci:` and `chore:` are not release notes")
        git("add", "-A")
        git("commit", "-m", "release: 0.1.0-alpha.1")
        git("tag", "v0.1.0-alpha.1")
        alpha1 = git("rev-parse", "v0.1.0-alpha.1")

        # ---- between the releases -----------------------------------------------------------------
        (repo / "Cargo.toml").write_text('[workspace.package]\nname = "x"\nversion = "0.1.0-alpha.2"\n')
        commit("fix(store): the index opens read-only when it is held (reco-ddd)", touch="d")
        (repo / "CHANGELOG.md").write_text(
            (repo / "CHANGELOG.md").read_text().replace(
                "## [Unreleased]\n", "## [Unreleased]\n\n### Fixed\n\n- The index opens read-only when it "
                "is held (`reco-ddd`)\n"))
        git("add", "-A")
        git("commit", "-m", "docs: the changelog entry for reco-ddd")

        # ---- C06: the changelog's own rung claim, and the tool act that re-records it ---------------
        # A rung distribution counts commits, so a copy of it in prose is true for exactly one commit — the
        # one that wrote it — and `reco-0ip` is the test that demanded exactly that impossible thing. The
        # claim names the range it counted, so it can be checked forever; and it is generated, so a human
        # never has to guess a number that includes the commit they are about to make. Both halves bite.
        head_now = git("rev-parse", "HEAD")
        hand_written = rung_claim_text(alpha1, head_now, {"bead": 9}, 9)
        (repo / "CHANGELOG.md").write_text((repo / "CHANGELOG.md").read_text().replace(
            "## [Unreleased]\n", "## [Unreleased]\n\n" + hand_written + "\n", 1))
        problems, _ = check_rung_claims(repo, repo / "CHANGELOG.md")
        expect(any(p.rule == "C06" and "bead 9" in p.message for p in problems),
               "C06 refuses a hand-written rung claim whose numbers are not the range's: %s"
               % [p.message for p in problems])
        expect(not any(p.rule != "C06" for p in problems),
               "and C06 is the only complaint: %s" % [p.line() for p in problems])
        problems, notes, data = bless_rungs(repo, repo / "CHANGELOG.md")
        expect(not problems and data["rewrote"] == 1,
               "`audit --bless` re-records it for the live range: %s" % [p.line() for p in problems])
        blessed = (repo / "CHANGELOG.md").read_text()
        problems, notes = check_rung_claims(repo, repo / "CHANGELOG.md")
        expect(not problems, "and the re-recorded claim is true for the range it names: %s"
               % [p.line() for p in problems])
        expect(rung_line(count_rungs(read_commits(repo, alpha1, head_now))) in blessed,
               "and what it wrote is the distribution of that range and not of HEAD: %s" % rung_line(data["rungs"]))
        bless_rungs(repo, repo / "CHANGELOG.md")
        expect((repo / "CHANGELOG.md").read_text() == blessed,
               "and blessing twice writes the same bytes — the block is generated, not appended to")

        # ---- release 2, and the negative controls --------------------------------------------------
        problems, notes, announcement2 = cut(repo, "0.1.0-alpha.2", "2026-10-25",
                                             out=repo / "CHANGELOG.md", baseline=repo / "none.json")
        expect(not problems, "the second cut is clean: %s" % [p.line() for p in problems])
        text = (repo / "CHANGELOG.md").read_text()
        expect("## [0.1.0-alpha.2] - 2026-10-25" in text, "the second heading is cut and dated")
        expect(text.index("0.1.0-alpha.2") < text.index("0.1.0-alpha.1"),
               "the newest release is the first one a reader meets")
        problems, notes, _ = audit(repo, None, from_ref="HEAD", baseline_path=repo / "none.json",
                                   section_path=repo / "CHANGELOG.md")
        expect(not problems, "a freshly opened [Unreleased] has nothing to complain about")

        stash = (repo / "CHANGELOG.md").read_text()

        def control(name: str, mutate, check) -> None:
            """Mutate the file, run the check on the mutated tree, put the file back, report."""
            (repo / "CHANGELOG.md").write_text(mutate(stash))
            try:
                check()
            finally:
                (repo / "CHANGELOG.md").write_text(stash)

        # C03: a section that dropped a commit. `[Unreleased]` is made to say something that covers nothing.
        def c03() -> None:
            problems, _, _ = audit(repo, None, from_ref="HEAD~1", baseline_path=repo / "none.json",
                                   section_path=repo / "CHANGELOG.md")
            expect(any(p.rule == "C03" for p in problems),
                   "C03 fails a release whose notes dropped a commit")

        control("C03", lambda t: t.replace("## [Unreleased]", "## [Unreleased]\n\n- nothing to see here"), c03)

        # C04: a cut whose version is not the one the tree carries.
        problems, _, _ = cut(repo, "9.9.9", "2026-11-01", out=repo / "CHANGELOG.md")
        expect(any(p.rule == "C04" for p in problems), "C04 refuses to cut a version the tree does not carry")

        # C02: a release heading without its date.
        def c02() -> None:
            problems, _ = check_changelog(repo / "CHANGELOG.md")
            expect(any(p.rule == "C02" for p in problems), "C02 fails an undated release section")

        control("C02", lambda t: t.replace("## [0.1.0-alpha.1] - 2026-10-18", "## [0.1.0-alpha.1]"), c02)

        # C07: an exemption that is no longer needed must be pruned.
        (repo / "exempt.json").write_text(json.dumps({"entries": [
            {"ref": "reco-ddd", "reason": "landed without a note", "owner": "self-test"}]}))
        problems, _, _ = audit(repo, "0.1.0-alpha.2", baseline_path=repo / "exempt.json",
                               section_path=repo / "CHANGELOG.md")
        expect(any(p.rule == "C07" for p in problems),
               "C07 refuses an exemption for something the notes now cover")
        (repo / "exempt.json").write_text(json.dumps({"entries": [{"ref": "reco-ddd"}]}))
        problems, _, _ = audit(repo, "0.1.0-alpha.2", baseline_path=repo / "exempt.json",
                               section_path=repo / "CHANGELOG.md")
        expect(any(p.rule == "C07" for p in problems), "C07 refuses an exemption with no reason or owner")

        # C06: a rung claim whose numbers are not the range's. The claim survived the cut — the block is
        # inside `## [0.1.0-alpha.2]` now, frozen with the release — and being frozen is not being wrong.
        frozen_line = rung_line(count_rungs(read_commits(repo, alpha1, head_now)))

        def c06() -> None:
            problems, _ = check_rung_claims(repo, repo / "CHANGELOG.md")
            expect(any(p.rule == "C06" for p in problems),
                   "C06 fails a rung claim whose numbers do not match the range it names, even inside a "
                   "released section")

        control("C06", lambda t: t.replace(frozen_line, "trailer 99"), c06)

        # And the rule about *describing* the marker: the file that owns the block has to spell it out to
        # explain it, and documentation of a claim is not a second, unparsable claim.
        def c06documented() -> None:
            problems, _ = check_rung_claims(repo, repo / "CHANGELOG.md")
            expect(not any(p.rule == "C06" for p in problems),
                   "a marker written inside a code span is documentation about the block, not a fragment: %s"
                   % [p.line() for p in problems])

        control("C06 (documented)", lambda t: t + "\nSpelled out: `<!-- gen_changelog:rungs from <sha> to "
                "<sha> -->`, then the audit's line, then `<!-- /gen_changelog:rungs -->`.\n", c06documented)
        expect("## [Unreleased]" in stash and frozen_line in stash,
               "the claim travelled with the heading it was under when it was blessed")
        _, bless_notes, bless_data = bless_rungs(repo, repo / "CHANGELOG.md", dry_run=True)
        expect(bless_data["rewrote"] == 0 and bless_data["frozen"] == 1,
               "and bless leaves a released section's claim alone, live range or no live range: %s / %s"
               % (bless_data, bless_notes))
        expect((repo / "CHANGELOG.md").read_text() == stash, "and a dry run writes nothing")

        # The commit that carries a claim is already past the tree the claim names, and that is the normal
        # state rather than a red: the tree it counted has not moved. A `chore:` commit, because
        # re-recording a reading is not a release note.
        git("add", "-A")
        git("commit", "-m", "chore: re-record the changelog's rung claim")
        problems, notes = check_rung_claims(repo, repo / "CHANGELOG.md")
        expect(not problems and any("1 behind HEAD" in n for n in notes),
               "a claim stays true when a newer commit lands, and says how far behind it is: %s" % notes)

        # The fallback ladder, and what it refuses to guess about.
        ladder = Commit(sha=legacy, subject="Cold-index cost was quadratic in the cache, because a "
                        "diagnostic query missed its index (reco-ccc)", body="", date="2026-10-20")
        entries = ladder.entries()
        expect(entries and entries[0].legacy, "the fallback ladder classifies a prose subject")
        expect("fallback ladder" in ladder.lint(), "and lint says out loud that it took the ladder")
        silent = Commit(sha="f" * 40, subject="Some change with nothing to go on", body="", date="")
        expect("no conventional header" in silent.lint(), "and a commit with no ref and no trailer is named")

        expect(bool(announcement2.strip()), "the second release has a body to publish")
    finally:
        shutil.rmtree(work, ignore_errors=True)

    if failures:
        print("\nself-test failed: %d check(s)" % len(failures))
        for name in failures:
            print("  - %s" % name)
        return 1
    print("\nself-test passed: two releases cut, five negative controls caught, the ladder and its "
          "refusals both confirmed")
    return 0


# ---------------------------------------------------------------------------
# the acceptance criteria, as data
# ---------------------------------------------------------------------------


def criteria(root: pathlib.Path = ROOT) -> list[dict]:
    """The two criteria this tool exists to serve, with the verdict computed rather than asserted."""
    problems, notes, data = audit(root, None)
    versions = released_versions()
    out = [{
        "gate": "changelog-covers-every-commit",
        "what": "every commit since the last release appears in the release notes, or is on the exemption "
                "list with a reason and an owner",
        "command": "python3 tools/gen_changelog.py audit",
        "state": "met" if not problems else "%d gap(s) open" % len(problems),
    }, {
        "gate": "changelog-process-followed-two-releases",
        "what": "the same `cut` code produced two consecutive releases, each dated, each with a body "
                "cargo-dist can publish; proven on a throwaway repository because this tree has one "
                "release and no remote to publish a second one from",
        "command": "python3 tools/gen_changelog.py self-test",
        "state": "proven twice in self-test; %d release(s) cut on this tree so far" % len(versions),
    }]
    return out


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="gen_changelog.py", description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="mode")

    def common(sp):
        sp.add_argument("--from", dest="since", default="", help="commit to start at (default: last release)")
        sp.add_argument("--to", dest="until", default="HEAD", help="commit to stop at")
        sp.add_argument("--changelog", default="", help="the file to read (default: CHANGELOG.md)")
        sp.add_argument("--json", action="store_true", help="machine-readable output")
        sp.add_argument("--include-internal", action="store_true",
                        help="keep build/ci/test/chore commits in the notes")
        return sp

    draft_p = common(sub.add_parser("draft", help="print the section the commits in a range generate"))
    draft_p.add_argument("--version", default=UNRELEASED, help="the heading to print (default: Unreleased)")
    draft_p.add_argument("--date", default="", help="a date for the heading")
    draft_p.add_argument("--out", default="", help="write it here instead of stdout")

    audit_p = common(sub.add_parser("audit", help="fail unless every commit is accounted for (CI)"))
    audit_p.add_argument("--version", default="", help="audit a released section instead of [Unreleased]")
    audit_p.add_argument("--baseline", default=str(BASELINE), help="the exemption file")
    audit_p.add_argument("--bless", action="store_true",
                         help="re-record the changelog's own rung claim for the range being audited, then "
                              "audit as usual (the claim is generated, never hand-edited: `C06`)")

    cut_p = sub.add_parser("cut", help="rename [Unreleased] to a version and start a new one")
    cut_p.add_argument("--version", required=True)
    cut_p.add_argument("--date", default="", help="default: today")
    cut_p.add_argument("--out", default="", help="the file to rewrite (default: CHANGELOG.md)")
    cut_p.add_argument("--force", action="store_true", help="cut despite coverage gaps or a version mismatch")
    cut_p.add_argument("--dry-run", action="store_true")
    cut_p.add_argument("--json", action="store_true")
    cut_p.add_argument("--include-internal", action="store_true")

    lint_p = common(sub.add_parser("lint", help="report the commits this tool cannot read as conventional"))
    lint_p.add_argument("--strict", action="store_true", help="make the warnings a failure")

    sub.add_parser("self-test", help="cut two releases in a throwaway repository, and prove the gate bites")
    sub.add_parser("criteria", help="print this ticket's two changelog criteria as data")

    args = parser.parse_args(argv)
    if not args.mode:
        parser.print_help()
        return 2

    path = pathlib.Path(args.changelog) if getattr(args, "changelog", "") else CHANGELOG
    if args.mode == "self-test":
        return self_test()
    if args.mode == "criteria":
        print(json.dumps(criteria(ROOT), indent=2))
        return 0

    try:
        if args.mode == "draft":
            since = args.since or release_anchor(ROOT, resolve_previous(ROOT, None, path))[0]
            commits = read_commits(ROOT, since, args.until)
            grouped = draft(commits, include_internal=args.include_internal)
            if args.json:
                payload = {"version": args.version, "range": "%s..%s" % (since, args.until),
                           "sections": {k: [dataclasses.asdict(e) for e in v] for k, v in grouped.items()}}
                text = json.dumps(payload, indent=2)
            else:
                text = render(args.version, grouped, args.date)
            if args.out:
                pathlib.Path(args.out).write_text(text)
                print("wrote %s (%d section(s), %d bullet(s))"
                      % (args.out, len(grouped), sum(len(v) for v in grouped.values())))
            else:
                print(text)
            return 0

        if args.mode == "audit":
            if args.bless:
                bless_problems, bless_notes, _ = bless_rungs(ROOT, path, since=args.since, until=args.until)
                for problem in bless_problems:
                    print(problem.line())
                for note in bless_notes:
                    print("  ok  %s" % note)
                if bless_problems:
                    return 1
            problems, notes, data = audit(ROOT, args.version or None, from_ref=args.since,
                                          to_ref=args.until,
                                          baseline_path=pathlib.Path(args.baseline), section_path=path)
            data["path"] = str(path.relative_to(ROOT)) if path.is_relative_to(ROOT) else str(path)
            if args.json:
                data["problems"] = [p.__dict__ for p in problems]
                print(json.dumps(data, indent=2))
            else:
                for problem in problems:
                    print(problem.line())
                for note in notes:
                    print("  ok  %s" % note)
            return 1 if problems else 0

        if args.mode == "cut":
            from datetime import date as _date
            day = args.date or _date.today().isoformat()
            problems, notes, announcement = cut(
                ROOT, args.version, day, out=pathlib.Path(args.out) if args.out else None,
                force=args.force, dry_run=args.dry_run, include_internal=args.include_internal)
            for problem in problems:
                print(problem.line())
            for note in notes:
                print("  ok  %s" % note)
            if problems:
                return 1
            if args.json:
                print(json.dumps({"version": args.version, "date": day, "body": announcement}, indent=2))
            else:
                print("\n--- release body (%s) ---\n%s" % (args.version, announcement))
                if not args.dry_run:
                    print("next: python3 tools/check_release.py --tag v%s, then git tag v%s && git push --tags"
                          % (args.version, args.version))
            return 0

        if args.mode == "lint":
            since = args.since or release_anchor(ROOT, resolve_previous(ROOT, None, path))[0]
            problems, notes = check_shape(ROOT, since, args.until, strict=args.strict)
            if args.json:
                print(json.dumps({"problems": [p.__dict__ for p in problems], "notes": notes}, indent=2))
            else:
                for problem in problems:
                    print("%s %s: %s" % ("::warning::" if not args.strict else "error", problem.where,
                                          problem.message))
                for note in notes:
                    print("  ok  %s" % note)
            return 1 if (problems and args.strict) else 0
    except SystemExit as exc:
        print(str(exc), file=sys.stderr)
        return 2
    return 2


if __name__ == "__main__":
    sys.exit(main())
