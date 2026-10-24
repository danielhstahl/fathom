#!/usr/bin/env python3
"""Write the miniature repository `fathom --demo` indexes, and pack it into the binary.

`--demo` is the first five minutes of the product (ticket `reco-2uo`): somebody on a clean machine types
one flag and sees a real review of a real-looking history — no configuration, no network, and no
repository of theirs touched. That needs a repository *shipped inside the binary*, and a repository is git
objects, so this tool builds one and packs it.

What the miniature contains is not arbitrary. It is the shortest history that puts a row in every view the
shell has: an episode each for a test deleted, a reformat, a rename, a dependency bump in two ecosystems,
a cycle introduced, a CI job added, a merge, and a large rewrite, authored by two people and one agent so
the queue ranks agent work against human work instead of showing one label. The whole story is the
`history()` list below: the pack contains nothing that is not named there, and each entry says which view
it exists to fill.

Why the `git` binary and not a hand-written object store: `fathom-fixtures` owns the byte-exact pure-Rust
writer, but ADR-002 §5.1 makes it dev-only and `fathom-cli` may not depend on it. What this tool needs is
not byte-identical *files* — it is a history git agrees with, and the honest way to get one is to ask git.
Determinism is therefore asserted where it matters and stated where it does not:

* a loose object's path *is* the sha1 of its content, so two runs that make the same commits make the same
  objects; the manifest's `digest` hashes each object's **decompressed** bytes, the refs and HEAD, the
  config this tool writes itself, and every worktree file;
* what that digest deliberately excludes: the zlib streams (a compression setting, not a fact), and
  `.git/index` (which carries stat data from the machine that built it, and is not packed at all —
  `--demo` indexes with `--committed-only`, so the worktree's mtime table is nobody's input).

    python3 tools/gen_demo_repo.py write    # build and pack over crates/fathom-cli/assets/
    python3 tools/gen_demo_repo.py check    # rebuild, and fail if the manifest moved
    python3 tools/gen_demo_repo.py show     # print the manifest without touching the tree
    python3 tools/gen_demo_repo.py unpack --out DIR   # write the committed pack out as a real repository

The pack format is `crates/fathom-cli/src/demo.rs`'s to read and this file's to keep in step: a magic
string, a version byte, a count, then one entry per file (path, exec flag, length, bytes) sorted by path.
Loose git objects are already deflate-compressed, so the pack is not compressed a second time — a second
deflate over zlib output saves bytes nobody misses and adds a decoder to a binary that would rather not
have one.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / "crates" / "fathom-cli" / "assets"
PACK = ASSETS / "fathom-demo.pack"
MANIFEST = ASSETS / "fathom-demo.json"

#: The pack's magic and version, shared with `crates/fathom-cli/src/demo.rs`.
MAGIC = b"FATHOMDEMO"
VERSION = 1

#: Every commit is stamped this instant, an hour apart, so the timeline is a day of work rather than the
#: minute this tool happened to run. A fixture that reads a clock is a fixture with a flake in it.
BASE = 1_745_971_200  # 2025-04-29T12:00:00+00:00
GAP = 3_600

DANA = ("Dana Stahl", "dana@example.com")
SAM = ("Sam Park", "sam@example.com")
#: The agent's commits are Sam's, with a claim trailer naming the agent: rung 3 of the attribution
#: cascade (`crates/fathom-ingest/src/attribute.rs`), which is what makes `agent · claude · 0.95` appear
#: in the card and the queue instead of a fifth human nobody hired.
CLAUDE = "Claude <noreply@anthropic.com>"

# ---------------------------------------------------------------------------
# The miniature's source. Real code with real differences between revisions: the analyzers key on blob
# content, so a dependency bump that does not change Cargo.lock byte-for-byte changes nothing, and a demo
# whose every pane is empty is a demonstration of nothing.
# ---------------------------------------------------------------------------


def cargo_toml(deps: int, serde: str) -> str:
    head = '[package]\nname = "signal"\nversion = "0.1.0"\nedition = "2021"\n'
    if deps == 1:
        return head
    table = f"[dependencies]\nserde = \"{serde}\"\n"
    if deps >= 3:
        table += "toml = \"0.8\"\n"
    return head + "\n" + table


def cargo_lock(with_serde: bool) -> str:
    serde = """
[[package]]
name = "serde"
version = "1.0.190"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "7761121fc46e22defff54d2e3a0dbf23e3d6f53a62b4d348bc7bdee92ecc01f0"
"""
    return 'version = 3\n\n[[package]]\nname = "signal"\nversion = "0.1.0"\n' + (serde if with_serde else "")


def npm_json(bump: bool) -> str:
    express = "4.18.2" if bump else "4.17.3"
    return (
        '{\n  "name": "signal-dashboard",\n  "version": "0.1.0",\n'
        f'  "dependencies": {{ "express": "{express}", "zod": "3.22.4" }}\n}}\n'
    )


def npm_lock(bump: bool) -> str:
    express = "4.18.2" if bump else "4.17.3"
    return (
        '{\n  "name": "signal-dashboard",\n  "lockfileVersion": 3,\n  "packages": {\n'
        f'    "node_modules/express": {{ "version": "{express}" }},\n'
        '    "node_modules/zod": { "version": "3.22.4" }\n  }\n}\n'
    )


CORE = """\
//! One observation, and the store that keeps them.

/// One observation.
pub struct Sample {
    pub at: i64,
    pub value: f64,
}

/// The in-memory store, one segment per flush.
#[derive(Default)]
pub struct Store {
    segments: Vec<Vec<Sample>>,
}

impl Store {
    pub fn append(&mut self, samples: &[Sample]) {
        match self.segments.last_mut() {
            Some(segment) => segment.extend_from_slice(samples),
            None => self.segments.push(samples.to_vec()),
        }
    }

    pub fn len(&self) -> usize {
        self.segments.iter().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
"""

CORE_NOTIFY = CORE.replace(
    "    pub fn is_empty(&self) -> bool {",
    """    /// Tell whoever is listening that a segment moved.
    pub fn notify(&self, moved: i64) {
        crate::api::publish(moved);
    }

    pub fn is_empty(&self) -> bool {""",
)

CORE_RAMP = CORE_NOTIFY.replace(
    "/// The in-memory store, one segment per flush.",
    """/// A ramp, for the dashboard's first frame.
pub fn ramp(n: i64) -> Vec<Sample> {
    (0..n)
        .map(|i| Sample {
            at: i,
            value: i as f64,
        })
        .collect()
}

/// The in-memory store, one segment per flush.""",
)

CORE_CYCLE = CORE_RAMP.replace(
    "    pub fn len(&self) -> usize {",
    """    /// The blast radius's favourite call: the store asks the API, the API asks the store.
    pub fn summary(&self) -> String {
        crate::api::summary(self)
    }

    pub fn len(&self) -> usize {""",
)

PARSE = """\
//! Parse one `name=value` pair per line.

/// One pair per line; a line without `=` is dropped.
pub fn parse(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let (name, value) = line.split_once('=')?;
            Some((name.trim().to_owned(), value.trim().to_owned()))
        })
        .collect()
}
"""

# Whitespace only, which is the point: the noise classifier's whole job is to call this episode a reformat
# rather than a change, and it can only do that if the bytes really are the same shape.
PARSE_REFORMATTED = PARSE.replace(
    "pub fn parse(text: &str) -> Vec<(String, String)> {",
    "pub fn parse(text: &str) -> Vec<(String, String)>   {",
)

API = """\
//! Answer a range query.

use crate::core::Store;

/// The rows in `[from, to]`.
pub fn query(store: &Store, from: i64, to: i64) -> usize {
    store.len().min((to - from).max(0) as usize)
}
"""

API_PUBLISH = API.replace(
    "use crate::core::Store;",
    """use crate::core::{Sample, Store};

/// Tell the dashboard something moved.
pub fn publish(moved: i64) {
    let _ = moved;
}""",
).replace(
    "/// The rows in `[from, to]`.",
    """/// Fold a window into one figure.
pub fn summary(store: &Store) -> String {
    let _ = store.len();
    let _ = std::mem::size_of::<Sample>();
    "ok".to_owned()
}

/// The rows in `[from, to]`.""",
)

RENDER = """\
//! Draw one sample column per bucket.

/// The sparkline for `values`.
pub fn sparkline(values: &[f64]) -> String {
    let blocks = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    values
        .iter()
        .map(|v| blocks[((v.abs() * 8.0) as usize) % blocks.len()])
        .collect()
}
"""

RENDER_REWRITE = """\
//! Draw one sample column per bucket, with the axis the dashboard needs.

/// The sparkline for `values`, clipped to `width` cells.
pub fn sparkline(values: &[f64], width: usize) -> String {
    let blocks = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let step = values.len().max(1).div_ceil(width.max(1));
    (0..width)
        .map(|bucket| {
            let window = &values[bucket * step..((bucket + 1) * step).min(values.len())];
            let mean = if window.is_empty() {
                0.0
            } else {
                window.iter().sum::<f64>() / window.len() as f64
            };
            blocks[((mean.abs() * 8.0) as usize) % blocks.len()]
        })
        .collect()
}

/// The axis under a sparkline: the first and last value in the window that was drawn.
pub fn axis(values: &[f64]) -> String {
    match (values.first(), values.last()) {
        (Some(first), Some(last)) => format!("{first:.1} … {last:.1}"),
        _ => String::new(),
    }
}
"""

QUEUE = """\
//! A job the scheduler can run.

/// One queued job.
pub struct Job {
    pub id: u32,
}

/// The queue.
#[derive(Default)]
pub struct Queue {
    jobs: Vec<Job>,
}

impl Queue {
    pub fn push(&mut self, job: Job) {
        self.jobs.push(job);
    }

    pub fn pop(&mut self) -> Option<Job> {
        self.jobs.pop()
    }
}
"""

SCHEDULER = """\
//! The scheduler that drains the queue.

use crate::queue::Job;
use crate::queue::Queue;

/// Drain at most `window` jobs.
pub fn drain(queue: &mut Queue, window: u32) -> Vec<Job> {
    (0..window).filter_map(|_| queue.pop()).collect()
}
"""

FETCH = """\
//! Pull a page for the dashboard.

/// The page's body, or why there is not one.
pub fn pull(path: &str) -> Result<String, String> {
    if path.is_empty() {
        return Err("an empty path is not a page".to_owned());
    }
    Ok(String::new())
}
"""

TESTS_CORE = """\
use signal::core::{Sample, Store};

#[test]
fn append_adds_every_sample() {
    let mut store = Store::default();
    store.append(&[Sample { at: 1, value: 1.0 }]);
    store.append(&[Sample { at: 2, value: 2.0 }]);
    assert_eq!(store.len(), 2);
}

#[test]
fn empty_is_empty() {
    assert!(Store::default().is_empty());
}
"""

TESTS_PARSE = """\
use signal::parse::parse;

#[test]
fn a_pair_is_a_pair() {
    assert_eq!(parse("at=1")[0], ("at".to_owned(), "1".to_owned()));
}

#[test]
fn a_line_without_an_equals_is_dropped() {
    assert!(parse("at").is_empty());
}
"""

#: The lib.rs the demo ends with; each module joins the list in the commit that adds it.
LIB_BASE = "//! signal: one observation at a time.\n\npub mod core;\npub mod parse;\n"

CI = """\
name: ci
on: [push]
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo test
"""

CI_DEPLOY = """\
name: ci
on: [push]
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo test
  deploy:
    runs-on: ubuntu-latest
    needs: build
    steps:
      - uses: actions/checkout@v4
      - run: cargo build --release
"""

README = "# signal\n\nOne observation at a time, and the store that keeps them.\n"

OPERATIONS = "## Operations\n\nThe dashboard reads `signal` through the API surface; the store is the source of truth.\n"


class Step:
    """One commit: a subject, who signs it, the files it writes, and optionally a branch to merge.

    `writes` maps a repository-relative path to its new content, or `None` to delete it. A rename is a
    delete and an add in one step, which is what git then chooses to call a rename.
    """

    def __init__(self, subject, writes, who=DANA, trailers=(), body=None, branch=None, merge=None):
        self.subject = subject
        self.writes = writes
        self.who = who
        self.trailers = list(trailers)
        self.body = body
        self.branch = branch
        self.merge = merge


def history() -> list[Step]:
    """The miniature's whole history, oldest first. Reading this list *is* reading the demo."""
    return [
        Step(
            "add the crate: one observation and the store",
            {
                "Cargo.toml": cargo_toml(1, "none"),
                "Cargo.lock": cargo_lock(False),
                ".gitignore": "target/\n.fathom/\n",
                "README.md": README,
                "src/lib.rs": LIB_BASE,
                "src/core.rs": CORE,
                "tests/core.rs": TESTS_CORE,
            },
        ),
        Step(
            "parse: read one name=value pair per line",
            {"src/parse.rs": PARSE, "tests/parse.rs": TESTS_PARSE},
        ),
        Step(
            "downsample a window to at most N buckets",
            {"src/core.rs": CORE_RAMP},
            who=SAM,
            trailers=[("Co-authored-by", CLAUDE)],
            body="The dashboard reads one figure per bucket, so the bucket is where the mean belongs.",
        ),
        Step(
            "notify: tell the dashboard a segment moved",
            {"src/core.rs": CORE_NOTIFY, "src/api.rs": API_PUBLISH},
        ),
        Step(
            "bump serde to 1.0.190 for the flatten fix",
            {"Cargo.toml": cargo_toml(2, "1.0.190"), "Cargo.lock": cargo_lock(True)},
        ),
        # The rename the folder-structure delta exists to see: one file, one directory, no content change.
        Step(
            "move core.rs into its own module",
            {"src/core.rs": None, "src/core/mod.rs": CORE_NOTIFY},
        ),
        Step(
            "tests: fold the empty-store case into the append test",
            {"tests/core.rs": TESTS_CORE.replace(
                "\n#[test]\nfn empty_is_empty() {\n    assert!(Store::default().is_empty());\n}\n", "")},
            body="The empty case is the zero row of the append assertion, not a test of its own.",
        ),
        Step("rustfmt the parse module", {"src/parse.rs": PARSE_REFORMATTED}, who=SAM),
        Step(
            "dashboard: pin express and zod",
            {
                "dashboard/package.json": npm_json(False),
                "dashboard/package-lock.json": npm_lock(False),
            },
            who=SAM,
        ),
        Step("query: answer a range query against the store", {"src/api.rs": API}),
        Step(
            "queue: a job the scheduler can run",
            {"src/queue.rs": QUEUE, "src/lib.rs": LIB_BASE + "pub mod queue;\n"},
        ),
        Step(
            "scheduler: drain a window of the queue",
            {"src/scheduler.rs": SCHEDULER, "src/lib.rs": LIB_BASE + "pub mod queue;\npub mod scheduler;\n"},
        ),
        Step(
            "generated with claude: the sparkline renderer",
            {"src/render.rs": RENDER, "src/lib.rs": LIB_BASE + "pub mod render;\n"},
            trailers=[("Generated-by", CLAUDE)],
        ),
        Step(
            "ci: deploy the release build after the tests",
            {".github/workflows/ci.yml": CI_DEPLOY},
        ),
        Step("docs: how the dashboard reads signal", {"docs/operations.md": OPERATIONS}, who=SAM),
        Step(
            "bump express to 4.18.2 for the path-name fix",
            {
                "dashboard/package.json": npm_json(True),
                "dashboard/package-lock.json": npm_lock(True),
            },
            who=SAM,
        ),
        Step(
            "summary: fold a window into one figure",
            {"src/api.rs": API_PUBLISH, "src/core/mod.rs": CORE_CYCLE},
            who=SAM,
            trailers=[("Co-authored-by", CLAUDE)],
            body="This puts a cycle through the API surface on purpose: the graph view has to show one.",
        ),
        Step("render: bucket the sparkline by width instead of by row",
             {"src/render.rs": RENDER_REWRITE},
             body="The dashboard was drawing 4 000 columns for a 90-cell window. Buckets come from the width now, and the axis labels say which window was drawn."),
        Step("add toml to the dependency surface",
             {"Cargo.toml": cargo_toml(3, "1.0.190")}),
        Step(
            "fetch: pull a page for the dashboard",
            {
                "src/fetch.rs": FETCH,
                "src/lib.rs": LIB_BASE + "pub mod fetch;\npub mod queue;\npub mod scheduler;\npub mod render;\n",
            },
            who=SAM,
            branch="fetch",
        ),
        # The merge closes the side branch, so the timeline has a merge episode and the lineage has two
        # parents rather than a straight line — which is the shape the spine's merge policy is for.
        Step("merge branch 'fetch'", {}, merge="fetch"),
    ]


# ---------------------------------------------------------------------------
# Building it
# ---------------------------------------------------------------------------


def git(cwd: Path, *args: str, env: dict[str, str] | None = None) -> str:
    """One `git` call with every identity, date and locale the object bytes could depend on pinned."""
    full = {
        "GIT_CONFIG_GLOBAL": os.devnull,
        "GIT_CONFIG_SYSTEM": os.devnull,
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_PAGER": "cat",
        "GIT_TERMINAL_PROMPT": "0",
        "LC_ALL": "C",
        "TZ": "UTC",
        "HOME": str(cwd),
        "PATH": os.environ.get("PATH", ""),
    }
    if env:
        full.update(env)
    out = subprocess.run(
        ["git", *args], cwd=cwd, env=full, capture_output=True, text=True, check=False
    )
    if out.returncode != 0:
        raise SystemExit(f"git {' '.join(args)}: {out.stderr.strip() or out.stdout.strip()}")
    return out.stdout


def stamp(index: int) -> dict[str, str]:
    """The author and committer dates for the `index`th commit, in git's exact `@<secs> +0000` form."""
    when = f"@{BASE + index * GAP} +0000"
    return {"GIT_AUTHOR_DATE": when, "GIT_COMMITTER_DATE": when}


#: Under `.git`: the cloning machine's own business, not part of the history. `.git/index` is here because
#: it carries stat data from the machine that wrote it; `--demo` indexes `--committed-only`, so nothing
#: reads it.
SKIP_FIRST = {"logs", "hooks", "objects/pack", "info", "worktrees"}
SKIP_NAMES = {"COMMIT_EDITMSG", "description", "ORIG_HEAD", "FETCH_HEAD", "index", "ssequel"}

CONFIG = """\
[core]
\trepositoryformatversion = 0
\tfilemode = false
\tbare = false
\tlogallrefupdates = true
"""


def build(root: Path) -> None:
    """Materialise the miniature into `root`: every commit in `history()`, in order."""
    root.mkdir(parents=True, exist_ok=True)
    git(root, "init", "-q", "--initial-branch=main")
    for index, step in enumerate(history()):
        if step.branch:
            git(root, "checkout", "-q", "-b", step.branch, env=stamp(index))
        for path, content in step.writes.items():
            target = root / path
            if content is None:
                target.unlink(missing_ok=True)
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(content, encoding="utf-8")
        if step.writes:
            git(root, "add", "-A", env=stamp(index))
        if step.merge:
            git(
                root,
                "-c",
                f"user.name={step.who[0]}",
                "-c",
                f"user.email={step.who[1]}",
                "merge",
                "--no-ff",
                "-q",
                "-m",
                step.subject,
                step.merge,
                env=stamp(index),
            )
            continue
        message = step.subject
        if step.body:
            message += "\n\n" + step.body
        for key, value in step.trailers:
            message += f"\n\n{key}: {value}"
        git(
            root,
            "-c",
            f"user.name={step.who[0]}",
            "-c",
            f"user.email={step.who[1]}",
            "commit",
            "-q",
            "-m",
            message,
            env=stamp(index),
        )
        if step.branch:
            git(root, "checkout", "-q", "main")
    # The config is written by this tool rather than left as `git init`'s, for the same reason the dates
    # are pinned: it is part of the pack, and two gits should not disagree about what was packed.
    (root / ".git" / "config").write_text(CONFIG, encoding="utf-8")


def entries(root: Path) -> list[tuple[str, int, bytes]]:
    """Every file the pack carries: the tracked worktree, plus the object store that proves it."""
    out: list[tuple[str, int, bytes]] = []
    # The mode comes from the tree, not from the filesystem: `core.fileMode = false` means the working
    # copy's exec bit says nothing about what was committed.
    listed = git(root, "ls-tree", "-r", "--format=%(objectmode)\t%(path)", "HEAD").splitlines()
    for line in sorted(listed):
        mode, path = line.split("\t", 1)
        out.append((path, int(mode, 8), (root / path).read_bytes()))
    for path in sorted((root / ".git").rglob("*")):
        if not path.is_file():
            continue
        relative = path.relative_to(root / ".git")
        if relative.parts[0] in SKIP_FIRST or relative.name in SKIP_NAMES:
            continue
        out.append((f".git/{relative.as_posix()}", 0o644, path.read_bytes()))
    return out


def digest_of(entries: list[tuple[str, int, bytes]]) -> str:
    """A digest over the *meaning* of the pack, so two gits and three platforms agree on one number."""
    digest = hashlib.sha256()
    for path, mode, payload in sorted(entries):
        parts = path.split("/")
        if len(parts) == 4 and parts[0] == ".git" and parts[1] == "objects":
            try:
                payload = zlib.decompress(payload)
            except zlib.error:
                pass
        digest.update(f"{path}\t{mode:o}\t{hashlib.sha256(payload).hexdigest()}\n".encode())
    return digest.hexdigest()


def pack(entries: list[tuple[str, int, bytes]]) -> bytes:
    out = bytearray(MAGIC)
    out.append(VERSION)
    out += len(entries).to_bytes(4, "little")
    for path, mode, payload in entries:
        raw = path.encode("utf-8")
        out += len(raw).to_bytes(2, "little") + raw
        out += bytes([1 if mode & 0o100 else 0])
        out += len(payload).to_bytes(8, "little") + payload
    return bytes(out)


def manifest_for(packed: bytes, root: Path, count: int) -> dict:
    """The facts about one built pack: what it is, and what a test can assert without unpacking it."""
    return {
        "contract": "fathom.demo/1",
        "pack": PACK.name,
        "version": VERSION,
        "bytes": len(packed),
        "entries": count,
        "head": git(root, "rev-parse", "HEAD").strip(),
        "commits": int(git(root, "rev-list", "--count", "HEAD").strip()),
        "digest": digest_of(entries(root)),
        "subjects": [step.subject for step in history()],
    }


def build_all() -> tuple[bytes, dict]:
    work = Path(tempfile.mkdtemp(prefix="fathom-demo-"))
    try:
        root = work / "signal"
        build(root)
        built = entries(root)
        return pack(built), manifest_for(pack(built), root, len(built))
    finally:
        shutil.rmtree(work, ignore_errors=True)


def read_pack(packed: bytes) -> list[tuple[str, int, bytes]]:
    """The inverse of [`pack`]: the entries one pack carries, in the order it stored them.

    No decompression, no git, no network — the same `crates/fathom-cli/src/demo.rs` reader does at run
    time, in the language the rest of the tooling is written in. The pack is a length-prefixed format on
    purpose: a version byte and a count, so a reader can refuse what it cannot read instead of
    half-reading it.
    """
    if packed[: len(MAGIC)] != MAGIC:
        raise ValueError("not a fathom demo pack")
    version = packed[len(MAGIC)]
    if version != VERSION:
        raise ValueError(f"pack version {version}, this reader knows {VERSION}")
    offset = len(MAGIC) + 1
    count = int.from_bytes(packed[offset : offset + 4], "little")
    offset += 4
    out: list[tuple[str, int, bytes]] = []
    for index in range(count):
        name_len = int.from_bytes(packed[offset : offset + 2], "little")
        offset += 2
        path = packed[offset : offset + name_len].decode("utf-8")
        offset += name_len
        exec_bit = packed[offset]
        offset += 1
        length = int.from_bytes(packed[offset : offset + 8], "little")
        offset += 8
        payload = packed[offset : offset + length]
        offset += length
        out.append((path, 0o755 if exec_bit else 0o644, payload))
    if offset != len(packed):
        raise ValueError(f"{len(packed) - offset} trailing bytes after {count} entries")
    return out


def unpack(out: Path, packed: bytes) -> list[tuple[str, int, bytes]]:
    """Write a pack into `out` as the repository it was built from.

    The directory *is* a git repository when this returns: the pack carries the loose objects, the refs,
    HEAD and the config the generator wrote (`fathom-fixtures`' byte-exact writer is dev-only per
    ADR-002 §5.1, so git itself is the only author of this history), which is what makes a published demo
    repository have the history the binary ships rather than an imitation of it.
    """
    entries = read_pack(packed)
    for path, mode, payload in entries:
        target = out / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(payload)
        target.chmod(mode)
    # The index is deliberately not in the pack — it carries stat data from the machine that built it
    # (this file's own docstring), and `fathom --demo` indexes with `--committed-only`, so the binary has
    # never needed it. A *published* repository does: without an index `git status` reports every tracked
    # file as a staged deletion, which is not the first impression a demo wants. `read-tree` fills the
    # index from HEAD and touches nothing else.
    git(out, "read-tree", "HEAD")
    return entries


def main() -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    parser.add_argument("action", choices=["write", "check", "show", "unpack"])
    parser.add_argument(
        "--out",
        type=Path,
        help="unpack: the directory to write the miniature into (created; it must not already be a repository)",
    )
    args = parser.parse_args()

    if args.action == "unpack":
        # Deliberately the *committed* pack, not a fresh build: the thing a demo repository publishes is
        # the history the binary unpacks on a stranger's machine, and a rebuild would let the two drift
        # apart in silence. `check` above is what keeps this pack honest.
        if not PACK.is_file():
            print("the demo pack is missing; run `gen_demo_repo.py write`", file=sys.stderr)
            return 1
        if args.out is None:
            print("unpack needs --out", file=sys.stderr)
            return 2
        if (args.out / ".git").exists():
            print(f"{args.out} is already a repository; refusing to overwrite it", file=sys.stderr)
            return 1
        args.out.mkdir(parents=True, exist_ok=True)
        try:
            written = unpack(args.out, PACK.read_bytes())
        except ValueError as err:
            print(f"{PACK.name}: {err}", file=sys.stderr)
            return 1
        commits = git(args.out, "rev-list", "--count", "HEAD").strip()
        print(
            f"unpacked {len(written)} entries into {args.out} — {commits} commits at "
            f"{git(args.out, 'rev-parse', '--short', 'HEAD').strip()}"
        )
        return 0

    packed, facts = build_all()
    if args.action == "show":
        print(json.dumps(facts, indent=2))
        return 0
    if args.action == "write":
        ASSETS.mkdir(parents=True, exist_ok=True)
        PACK.write_bytes(packed)
        MANIFEST.write_text(json.dumps(facts, indent=2) + "\n", encoding="utf-8")
        print(
            f"wrote {PACK.relative_to(ROOT)} ({facts['bytes']:,} bytes, "
            f"{facts['commits']} commits, {facts['entries']} entries)"
        )
        return 0

    # `check`: the manifest is the contract. The pack's own bytes are allowed to differ — a loose object's
    # zlib stream is a compression setting, and demanding it match would make CI a git-version lottery.
    if not PACK.is_file() or not MANIFEST.is_file():
        print("the demo pack is missing; run `gen_demo_repo.py write`", file=sys.stderr)
        return 1
    stored = json.loads(MANIFEST.read_text(encoding="utf-8"))
    moved = [key for key in ("digest", "head", "commits", "subjects") if stored.get(key) != facts[key]]
    if moved:
        print(
            "the demo pack has drifted from the generator: " + ", ".join(moved)
            + "\nrun `python3 tools/gen_demo_repo.py write` and commit the result.",
            file=sys.stderr,
        )
        return 1
    if PACK.read_bytes() != packed:
        print(
            "note: the packed bytes differ from a fresh build (different git or zlib); the manifest "
            "agrees, which is the contract",
            file=sys.stderr,
        )
    print(
        f"{PACK.relative_to(ROOT)}: {stored['commits']} commits, {stored['entries']} entries, "
        f"digest {stored['digest'][:12]} — as generated"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
