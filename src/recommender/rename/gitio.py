"""The IO half of `e2.4`: read a window out of a real repository, then hand it to the pure engine.

Everything that touches `git` lives here so that :mod:`recommender.rename.pair` stays a pure function
of its inputs (ADR-002 §4.5), and so the port to `fathom-analyze::rename` has one file to translate.
Three sources, in increasing order of what they can tell a reviewer:

```
--old-repo DIR --repo DIR        two working trees: content only, so every pairing is a guess
--manifest FILE                  a serialised window (what e2.11's consumers will hand us)
--git-dir REPO --old-rev A --new-rev B   a real repository: git's own rename lines come along too
```

The last one is the only source that can produce a **fact**, and that is not an implementation
accident: a fact is defined here as "git says so at the fact bar", so a caller who cannot give us
git's answer cannot give us a fact either. The card prints which source a window came from, because
otherwise a tree-pair comparison and a git-diff comparison look identical on the page.

Blob reading is bounded: a blob over `max_blob_bytes` is passed through as empty with the reason on
the window, exactly like `e2.1`'s cap, so a committed 400 MB asset costs a line of text and not the
afternoon.
"""

from __future__ import annotations

import base64
import subprocess
from pathlib import Path

from . import model as M
from . import repo as R
from .policy import Policy

#: Same cap `e2.1` puts on a blob it will read.
MAX_BLOB_BYTES = 4 * 1024 * 1024


class WindowSourceError(Exception):
    """A window could not be read at all — as opposed to read and found to be empty."""


def git_rename_lines(repo: Path, base: str, head: str, *, percent: int,
                     copies: bool = True, copies_harder: bool = False) -> list[M.GitPair]:
    """`git diff -M<pct>%` between two revisions, as machine-readable lines.

    Asked at `percent`, which the policy sets *below* the fact bar on purpose: git's sub-fact-bar
    opinions are worth showing as guesses with git's own number on them, and refusing to ask for them
    would leave the guess tier with nothing but our similarity to stand on.
    """
    args = ["git", "diff", "-M%d%%" % percent, "--name-status"]
    if copies:
        args.append("-C%d%%" % percent)
        if copies_harder:
            args.append("--find-copies-harder")
    args += [base, head]
    proc = subprocess.run(args, cwd=repo, capture_output=True, text=True, check=True)
    out: list[M.GitPair] = []
    for line in proc.stdout.splitlines():
        if not line:
            continue
        parts = line.split("\t")
        if len(parts) != 3:
            continue
        status, src, dst = parts
        kind = M.KIND_RENAME if status.startswith("R") else M.KIND_COPY
        if not status.startswith(("R", "C")):
            continue
        out.append(M.GitPair(src=src, dst=dst, score=int(status[1:] or 0) / 100.0, kind=kind))
    return out


def _bulk_reader(repo: Path, base_tree: dict[str, str], head_tree: dict[str, str]):
    """A `reader(side, path)` that reads a whole tree in one `cat-file --batch` pass on first use.

    Attached only when the repository asked for the widened copy search, because that mode is the only
    caller: it has already accepted O(repo) work, and one batch read is the cheapest way to do that
    work. Attaching it unconditionally would let a stray lookup pay a whole-tree read unnoticed, which
    is the failure mode the cost criterion exists to prevent.
    """
    cache: dict[str, bytes] = {}

    def read(side: str, path: str) -> bytes | None:
        tree = base_tree if side == "base" else head_tree
        oid = tree.get(path)
        if oid is None:
            return None
        if not cache:
            cache.update(R.blobs(repo, sorted({*base_tree.values(), *head_tree.values()})))
        data = cache.get(oid, b"")
        if len(data) > MAX_BLOB_BYTES:
            return b""
        return data

    return read


def window_from_repo(repo: Path, old_rev: str, new_rev: str, *, window_id: str = "",
                     policy: Policy | None = None, granularity: str = "episode",
                     walk_policy: str = "any") -> M.Window:
    """Read one review window out of a git repository."""
    policy = policy or Policy.built_in()
    repo = Path(repo)
    base_tree = R.ls_tree(repo, old_rev)
    head_tree = R.ls_tree(repo, new_rev)
    if not base_tree and not head_tree:
        raise WindowSourceError("%s..%s has no trees to compare" % (old_rev[:12], new_rev[:12]))

    removed, added = R.changed(repo, old_rev, new_rev)
    modified = sorted(path for path in set(base_tree) & set(head_tree)
                      if base_tree[path] != head_tree[path])

    wanted = [(base_tree[p], "base " + p) for p in removed + modified] \
        + [(head_tree[p], "head " + p) for p in added + modified]
    blobs = R.blobs(repo, [oid for oid, _ in wanted])

    notes: list[str] = []

    def read(oid: str, label: str) -> bytes:
        data = blobs.get(oid, b"")
        if len(data) > MAX_BLOB_BYTES:
            notes.append("%s is %d bytes, over the %d MiB read cap: paired with no content"
                         % (label, len(data), MAX_BLOB_BYTES // (1024 * 1024)))
            return b""
        return data

    window = M.Window(
        id=window_id or "%s..%s" % (old_rev[:8], new_rev[:8]),
        base=old_rev, head=new_rev,
        base_paths=set(base_tree), head_paths=set(head_tree),
        removed={path: read(base_tree[path], path) for path in removed},
        added={path: read(head_tree[path], path) for path in added},
        modified={path: (read(base_tree[path], path), read(head_tree[path], path))
                  for path in modified},
        git_pairs=git_rename_lines(repo, old_rev, new_rev,
                                   percent=policy.git_candidate_bar(),
                                   copies_harder=policy.flag("copies_harder")),
        granularity=granularity, policy=walk_policy,
        reader=_bulk_reader(repo, base_tree, head_tree) if policy.flag("copies_harder") else None,
        note="; ".join(notes))
    return window


def window_from_dirs(old_dir: Path, new_dir: Path, *, window_id: str = "worktree",
                     granularity: str = "episode") -> M.Window:
    """Two working trees, compared by bytes. No git, so no facts — and the card has to say that.

    The honest degenerate case: a dirty working tree, a directory a harness left behind, a review of
    "what changed since I copied this folder". The content matcher still works, splits and folds are
    still found, and every row is a guess because there is no `git log --follow` to check one against.
    """
    old_dir, new_dir = Path(old_dir), Path(new_dir)
    if not old_dir.is_dir() or not new_dir.is_dir():
        raise WindowSourceError("both --old-repo and --repo must be directories")

    def walk(root: Path) -> dict[str, bytes]:
        out: dict[str, bytes] = {}
        for path in sorted(root.rglob("*")):
            if not path.is_file() or ".git" in path.parts:
                continue
            data = path.read_bytes()
            out[path.relative_to(root).as_posix()] = data[:MAX_BLOB_BYTES]
        return out

    before, after = walk(old_dir), walk(new_dir)
    removed = {p: b for p, b in before.items() if p not in after}
    added = {p: b for p, b in after.items() if p not in before}
    modified = {p: (before[p], after[p]) for p in sorted(set(before) & set(after))
                if before[p] != after[p]}
    return M.Window(id=window_id, base=str(old_dir), head=str(new_dir),
                    base_paths=set(before), head_paths=set(after),
                    removed=removed, added=added, modified=modified, git_pairs=[],
                    granularity=granularity,
                    note="no git rename evidence: paired on content alone, so nothing on this "
                         "card can be a fact")


def window_from_manifest(doc: dict) -> M.Window:
    """A window carried as data — the shape `e2.11`'s consumers hand the analyzers.

    ```json
    {"id": "ep-storm", "base": "a11", "head": "b22", "granularity": "episode",
     "removed": {"src/old.py": "…"}, "added": {"src/new.py": "…"},
     "modified": {"src/a.py": {"before": "…", "after": "…"}},
     "git_pairs": [{"src": "src/old.py", "dst": "src/new.py", "score": 1.0}],
     "base_paths": ["…"], "head_paths": ["…"]}
    ```

    Text fields may be given as `…_base64` for binary payloads. `base_paths`/`head_paths` default to
    the keys of what is present, which is enough for move/copy detection in a self-contained window
    but not for a copy whose source was untouched — that needs the caller to say what the tree held.
    """
    def decode(value) -> bytes:
        """A blob in a manifest: plain text, or `{"base64": …}` for anything that is not text."""
        if isinstance(value, bytes):
            return value
        if isinstance(value, dict):
            if "base64" in value:
                return base64.b64decode(value["base64"])
            return str(value.get("text", "")).encode()
        return str(value or "").encode()

    removed = {p: decode(b) for p, b in doc.get("removed", {}).items()}
    added = {p: decode(b) for p, b in doc.get("added", {}).items()}
    modified: dict[str, tuple[bytes, bytes]] = {}
    for path, sides in doc.get("modified", {}).items():
        if not isinstance(sides, dict):
            raise WindowSourceError("modified.%s must be {before, after}" % path)
        modified[path] = (decode(sides.get("before", "")), decode(sides.get("after", "")))
    base_paths = set(doc.get("base_paths") or (set(removed) | set(modified)))
    head_paths = set(doc.get("head_paths") or (set(added) | set(modified)))
    base_paths |= set(modified)
    head_paths |= set(modified)
    return M.Window(id=doc.get("id", "manifest"), base=str(doc.get("base", "base")),
                    head=str(doc.get("head", "head")),
                    base_paths=base_paths | set(removed), head_paths=head_paths | set(added),
                    removed=removed, added=added, modified=modified,
                    git_pairs=[M.GitPair(src=row["src"], dst=row["dst"],
                                         score=float(row.get("score", 0.0)),
                                         kind=row.get("kind", M.KIND_RENAME))
                               for row in doc.get("git_pairs", [])],
                    granularity=doc.get("granularity", "episode"),
                    policy=doc.get("policy", "any"), note=doc.get("note", ""))


def manifest_of(window: M.Window) -> dict:
    """The inverse: a window as data, for `explain --json` and for a bug report."""
    return {"id": window.id, "base": window.base, "head": window.head,
            "granularity": window.granularity, "policy": window.policy, "note": window.note,
            "base_paths": sorted(window.base_paths), "head_paths": sorted(window.head_paths),
            "removed": {p: {"base64": base64.b64encode(b).decode()}
                        for p, b in window.removed.items()},
            "added": {p: {"base64": base64.b64encode(b).decode()}
                      for p, b in window.added.items()},
            "modified": {p: {"before": {"base64": base64.b64encode(a).decode()},
                             "after": {"base64": base64.b64encode(b).decode()}}
                         for p, (a, b) in window.modified.items()},
            "git_pairs": [{"src": g.src, "dst": g.dst, "score": g.score, "kind": g.kind}
                          for g in window.git_pairs]}
