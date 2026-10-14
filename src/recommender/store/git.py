"""ADR-004 §10 — building and re-building an index, so that recovery is a test.

Two things live here:

* :func:`build_repo` — a deterministic git repository, built with `git fast-import`
  (fixed timestamps, fixed committers, byte-identical oids on every run). It is a
  smaller cousin of the `e0.10` fixture corpus (`reco-whw`), and it exists for the same
  reason: a recovery claim you cannot reproduce is a recovery claim you do not have.
* :func:`index_repo` — a miniature of the ingestion + analysis pipeline that writes
  through the §4 schema and the §5 cache, so that "delete `.fathom/` and rebuild"
  (§10.3) can be asserted on real rows rather than asserted in prose.

The analyzers here are deliberately tiny and *deterministic*: this module is a contract
fixture, not the analysis engine (`e2.x`, `e3.x`).
"""

from __future__ import annotations

import hashlib
import json
import os
import sqlite3
import subprocess
import time
from pathlib import Path

from . import cache as C
from .schema import GITIGNORE_STANZA, SqliteStore

AGENT = ("Fathom Agent", "agent@example.invalid")
HUMAN = ("Dana Reviewer", "dana@example.invalid")
EPOCH0 = 1_760_000_000  # fixed: wall clocks are never an input (ADR-001 §7.4)

#: The v1 config the analyzers declare keys against (§5.5). Changing a value here in a
#: test is how §6's config rows are exercised.
DEFAULT_CONFIG = {
    "noise": {"globs": ["**/dist/**", "**/*.snap"], "builtins_version": 1},
    "diff": {"context_lines": 3},
    "rename": {"threshold": 60},
    "episode": {"boundaries": ["hook", "trailer", "author"], "time_gap_minutes": 30},
    "layers": {"ui": ["src/ui"], "core": ["src/core"], "store": ["src/store"]},
    "deps": {"ecosystems": ["python", "node", "rust"]},
    "non_code": {"patterns": ["**/migrations/**", ".github/workflows/**"]},
    "test": {"globs": ["tests/**", "**/test_*.py"]},
    "graph": {"ignored_dirs": ["node_modules"]},
    "detectors": {"enabled": ["test_deletion", "untested_new_logic", "debug_leftover"]},
    "risk": {"weights": {"size": 0.4, "test_ratio": 0.3, "blast": 0.3}, "tau": 70},
    "blast": {"hops": 1},
    "ingest": {"size_cap_mib": 8},
}


# ---------------------------------------------------------------------------
# the fixture repository
# ---------------------------------------------------------------------------


def _files_for(i: int, n_files: int) -> dict[str, str]:
    """The tree content after commit `i`. Pure function of `i`: rebuilds are identical."""
    files: dict[str, str] = {}
    for f in range(n_files):
        mod = (i + f) % 7
        layer = ("core", "store", "ui")[f % 3]
        path = "src/%s/m%d.py" % (layer, mod)
        body = ["module m%d layer %s rev %d" % (mod, layer, i)]
        if i % 3 == 0:
            body.append("from src.core.m%d import thing" % ((mod + 1) % 7))
        if i % 5 == 0:
            body.append("print('debug leftover %d')" % i)
        files[path] = "\n".join(body) + "\n"
    if i % 4 == 0:
        files["tests/test_m%d.py" % (i % 7)] = "def test_%d():\n    assert %d\n" % (i, i % 2)
    if i % 11 == 0:
        files["package.json"] = json.dumps({"dependencies": {"left-pad": "1.%d.0" % i}},
                                           sort_keys=True) + "\n"
    if i % 13 == 0:
        files["src/store/migrations/%d_add_col.sql" % i] = "ALTER TABLE t ADD COLUMN c%d;\n" % i
    if i % 17 == 0:
        files["dist/bundle.js"] = "// generated\nvar x = %d;\n" % i
    return files


def build_repo(root: str | os.PathLike, n_commits: int, *, seed: int = 1,
               commits_per_session: int = 2, n_files: int = 6,
               branch: str = "main") -> dict:
    """Create a deterministic git repo at `root` and return `{oid: meta}`.

    `git fast-import` rather than `git commit`: a 1 000-commit fixture must be buildable
    inside the `make bench` budget (e0.11), and one subprocess per commit would spend
    the entire budget on process creation.
    """
    root = Path(root)
    root.mkdir(parents=True, exist_ok=True)
    # §12.2: the ignore rules are part of the fixture, because they are load-bearing. In a
    # repository without them, `.fathom/store.sqlite3` gets committed, the store's own bytes
    # become part of the tree hash, every cache key depends on the cache, and the identity
    # test (§10.2) becomes unfalsifiable. Tests do not get to skip that trap.
    (root / ".gitignore").write_text(
        "\n".join(l for l in GITIGNORE_STANZA.splitlines()
                  if l.strip() and not l.startswith("#")) + "\n",
        encoding="utf-8")
    if not (root / ".git").exists():
        subprocess.run(["git", "init", "-q", "-b", branch, str(root)], check=True,
                       env=_git_env(root), capture_output=True)
    fi = subprocess.Popen(
        ["git", "fast-import", "--done", "--quiet"], cwd=str(root), env=_git_env(root),
        stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
    assert fi.stdin
    metas: dict[str, dict] = {}
    mark = 0
    sha_mark: dict[str, int] = {}          # blob sha -> fast-import mark (dedupe by content)
    prev: dict[str, str] = {}
    stream = fi.stdin
    for i in range(n_commits):
        files = _files_for(i + seed, n_files)
        blobs: dict[str, str] = dict(prev)
        for path in sorted(set(blobs) - set(files)):
            del blobs[path]
        for path, content in sorted(files.items()):
            data = content.encode()
            h = hashlib.sha1(b"blob\x00" + str(len(data)).encode() + b"\n" + data).hexdigest()
            if h not in sha_mark:
                mark += 1
                stream.write("blob\nmark :%d\ndata %d\n%s" % (mark, len(data), data.decode()))
                stream.write("\n" if data.endswith(b"\n") else "\n\n")
                sha_mark[h] = mark
            blobs[path] = h
        session = "s%d" % (i // max(1, commits_per_session))
        turn = i % max(1, commits_per_session)
        msg = "work on batch %d\n\nAgent-Session: %s\nAgent-Turn: %d\n" % (i, session, turn)
        ts = EPOCH0 + i * 60
        mark += 1
        stream.write("commit refs/heads/%s\nmark :%d\ncommitter %s <%s> %d +0000\n"
                     "data %d\n%s" % (branch, mark, AGENT[0], AGENT[1], ts,
                                      len(msg.encode()), msg))
        for path, h in sorted(blobs.items()):
            stream.write("M 100644 :%d %s\n" % (sha_mark[h], path))
        metas["pending_%d" % i] = {"session": session, "turn": turn, "ts": ts,
                                   "files": sorted(files)}
    stream.write("done\n")
    stream.close()
    rc = fi.wait()
    if rc != 0:
        raise RuntimeError("git fast-import failed: %s" % fi.stderr.read()[:400])
    # Resolve oids after the fact: fast-import's marks are internal, the log is truth.
    log = _run(root, ["git", "log", "--reverse", "--format=%H\x1f%an\x1f%ae\x1f%at\x1f%T\x1f%B\x1e"])
    entries = [e for e in log.split("\x1e") if e.strip()]
    out: dict[str, dict] = {}
    for idx, entry in enumerate(entries):
        oid, an, ae, at, tree, body = [x.strip() for x in entry.split("\x1f")[:6]]
        out[oid] = {"index": idx, "ts": int(at), "tree": tree, "author": (an, ae),
                    "session": _trailer(body, "Agent-Session"),
                    "turn": _trailer(body, "Agent-Turn"),
                    "files": list(_diff_names(root, oid))}
    return out


_BLOB_MARKS: dict[str, str] = {}


def add_commit(root: str | os.PathLike, *, path: str, content: str,
               message: str = "warm commit\n\nAgent-Session: s99\nAgent-Turn: 0\n",
               author: tuple[str, str] = AGENT, ts: int | None = None) -> str:
    """Append one commit the slow, normal way — this is what a warm re-index observes."""
    root = Path(root)
    p = root / path
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(content)
    ts = ts or EPOCH0 + 10_000
    env = {**_git_env(root), "GIT_AUTHOR_DATE": "@%d +0000" % ts,
           "GIT_COMMITTER_DATE": "@%d +0000" % ts}
    subprocess.run(["git", "add", "-A"], cwd=root, env=env, check=True, capture_output=True)
    subprocess.run(["git", "-c", "commit.gpgsign=false", "commit", "-q", "--author=%s <%s>"
                    % author, "-m", message], cwd=root, env=env, check=True,
                   capture_output=True)
    return _run(root, ["git", "rev-parse", "HEAD"]).strip()


def dirty_working_tree(root: str | os.PathLike, path: str, content: str) -> None:
    """The provisional-episode input of e1.4: an edit that is not a commit."""
    p = Path(root) / path
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(content)


def _git_env(root: Path) -> dict[str, str]:
    return {
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "HOME": str(root / ".fathom-home"),
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_CONFIG_GLOBAL": str(root / ".fathom-home" / ".gitconfig"),
        "GIT_AUTHOR_NAME": AGENT[0], "GIT_AUTHOR_EMAIL": AGENT[1],
        "GIT_COMMITTER_NAME": AGENT[0], "GIT_COMMITTER_EMAIL": AGENT[1],
        "LC_ALL": "C",
    }


def _run(root: Path, argv: list[str]) -> str:
    return subprocess.run(argv, cwd=str(root), env=_git_env(root), capture_output=True,
                          text=True, check=True).stdout


def _trailer(body: str, key: str) -> str | None:
    for line in body.splitlines():
        if line.lower().startswith(key.lower() + ":"):
            return line.split(":", 1)[1].strip()
    return None


def diff_tree_raw(root: Path, revspec: str) -> dict[str, list[dict]]:
    """Per-file change records for a revspec, in **one** git subprocess.

    `-z` for the path encoding (unicode and tab-bearing paths survive quoting-free),
    `-M` for rename detection, `--numstat` in a second pass because combining the two
    in one parse makes the record boundaries depend on file count. Batching is not a
    micro-optimisation: measured in §13, one subprocess per commit is the largest single
    cost of a cold index, and it is a cost the store would otherwise pay forever.
    """
    root = Path(root)
    raw = subprocess.run(["git", "log", "-r", "-M", "-z", "--raw", "--format=%x01%H", revspec],
                         cwd=str(root), env=_git_env(root), capture_output=True, check=True)
    out: dict[str, list[dict]] = {}
    oid = None
    toks = raw.stdout.decode("utf-8", "replace").split("\0")
    i = 0
    while i < len(toks):
        tok = toks[i]
        i += 1
        if not tok:
            continue
        if tok.startswith("\x01"):
            oid = tok.strip()[1:]
            out.setdefault(oid, [])
            continue
        if not tok.startswith(":") or oid is None:
            continue
        body = tok[1:]
        fields = body.split(" ")
        if len(fields) < 5:
            continue
        old_sha, new_sha, status = fields[2], fields[3], fields[4]
        kind = {"A": "add", "M": "modify", "D": "delete", "R": "rename",
                "C": "copy", "T": "modify"}.get(status[0], "modify")
        old_path = toks[i] if kind in ("rename", "copy") else None
        if old_path is not None:
            i += 1
        new_path = toks[i]
        i += 1
        out[oid].append({
            "change_kind": kind,
            "is_binary": b"\0" in (new_path or "").encode(),
            "blob_old": None if old_sha.startswith("0" * 8) else old_sha,
            "blob_new": None if new_sha.startswith("0" * 8) else new_sha,
            "old_path": old_path,
            "new_path": new_path,
            "n_add": 0, "n_del": 0, "hunk_headers": "[]",
        })
    num = subprocess.run(["git", "log", "-r", "-M", "--numstat", "--format=%x01%H", revspec],
                         cwd=str(root), env=_git_env(root), capture_output=True, check=True)
    cur = None
    for line in num.stdout.decode("utf-8", "replace").splitlines():
        if line.startswith("\x01"):
            cur = line.strip()[1:]
            continue
        if not line.strip() or cur is None:
            continue
        parts = line.split("\t")
        if len(parts) < 3:
            continue
        adds, dels, path = parts[0], parts[1], parts[-1]
        for rec in out.get(cur, []):
            if rec["new_path"] == path or rec["old_path"] == path:
                rec["n_add"] = 0 if adds == "-" else int(adds)
                rec["n_del"] = 0 if dels == "-" else int(dels)
    return out


def _diff_names(root: Path, oid: str) -> list[str]:
    out = _run(root, ["git", "diff-tree", "-r", "-M", "--no-commit-id", "--name-only", oid])
    return [x for x in out.splitlines() if x]


def log_commits(root: Path, head: str) -> list[dict]:
    """Newest-first log with trailers, parents and tree."""
    fmt = "%H\x1f%P\x1f%an\x1f%ae\x1f%at\x1f%cn\x1f%ct\x1f%T\x1f%B\x1e"
    raw = _run(root, ["git", "log", "--format=" + fmt, head])
    commits = []
    for entry in raw.split("\x1e"):
        if not entry.strip():
            continue
        oid, parents, an, ae, at, cn, ct, tree, body = [x.strip() for x in
                                                        entry.split("\x1f")[:9]]
        commits.append({
            "oid": oid, "parents": [p for p in parents.split() if p],
            "author_name": an, "author_email": ae, "author_utc": int(at) * 1000,
            "committer_utc": int(ct) * 1000, "tree": tree,
            "message": body.strip(), "session": _trailer(body, "Agent-Session"),
            "turn": _trailer(body, "Agent-Turn"),
        })
    return list(reversed(commits))


# ---------------------------------------------------------------------------
# taxonomy (e0.9 stands in for this one function)
# ---------------------------------------------------------------------------


def classify_path(path: str) -> str:
    if path.startswith("dist/") or "/dist/" in path or path.endswith((".snap", ".min.js")):
        return "generated"
    if path.endswith(("package-lock.json", "Cargo.lock", "poetry.lock", "yarn.lock")):
        return "lockfile"
    if path.startswith("tests/") or path.startswith("test_") or "/test_" in path:
        return "test"
    if path.startswith(".github/") or path.startswith("src/store/migrations/"):
        return "migration"
    if path.endswith((".md", ".rst")):
        return "docs"
    if path.endswith((".json", ".toml", ".yaml", ".yml")):
        return "config"
    if path.endswith((".py", ".js", ".ts", ".rs", ".go")):
        return "source"
    return "other"


NOISE_CLASSES = {"generated": "generated", "lockfile": "lockfile"}


# ---------------------------------------------------------------------------
# the analyzers used by the fixture pipeline
# ---------------------------------------------------------------------------


def _graph_of(files: dict[str, str]) -> tuple[list[tuple], list[tuple]]:
    """A stand-in module graph: one node per file, one edge per `from src.x import`."""
    nodes = sorted(files)
    edges = []
    for path, content in sorted(files.items()):
        for line in content.splitlines():
            if line.startswith("from src."):
                target = line.split()[1]
                cand = target.replace(".", "/") + ".py"
                if cand in files and cand != path:
                    edges.append((path, cand))
    return [(n,) for n in nodes], sorted(set(edges))


def _delta_bytes(files: dict[str, str], prev: dict[str, str]) -> bytes:
    added = sorted(set(files) - set(prev))
    removed = sorted(set(prev) - set(files))
    changed = sorted(p for p in set(files) & set(prev) if files[p] != prev[p])
    return json.dumps({"added": added, "removed": removed, "changed": changed,
                       "graph_nodes": len(files), "edges": len(_graph_of(files)[1])},
                      sort_keys=True).encode()


# ---------------------------------------------------------------------------
# the pipeline
# ---------------------------------------------------------------------------


def index_repo(store: SqliteStore, repo: Path, *, config: dict | None = None,
               versions: dict[str, int] | None = None, head: str | None = None,
               incremental: bool = False, now_ms: int | None = None,
               budget_ops: int | None = None) -> dict:
    """Ingest + analyse `repo` into `store`, exercising §5, §6 and §7.

    With `incremental=True` only commits newer than the stored head are read: that is
    the warm path `e1.4` budgets, and its cost must not grow with the repository.

    `budget_ops` installs a §7.6 progress-handler budget: the index aborts itself
    instead of running to completion, which is how "degrade, never block" is a property
    rather than an intention.
    """
    config = config if config is not None else DEFAULT_CONFIG
    t0 = time.perf_counter()
    repo = Path(repo)
    head = head or _run(repo, ["git", "rev-parse", "HEAD"]).strip()
    epoch = store.epoch()
    now_ms = now_ms if now_ms is not None else int(time.time() * 1000)
    C.register_analyzers(store.conn)
    conn = store.conn
    interrupted = 0
    if budget_ops:
        trips = {"n": 0}

        def _guard() -> int:
            trips["n"] += 1
            return 1 if trips["n"] > budget_ops else 0
        conn.set_progress_handler(_guard, 200)
    try:
        conn.execute("BEGIN IMMEDIATE")
        prev_head = store.meta("head") or ""
        conn.execute("INSERT INTO index_run(kind,trigger,epoch,status,started_utc) "
                     "VALUES(?,?,?,?,?)",
                     ("warm" if incremental else "cold", "commit", epoch, "running", now_ms))
        # §9.2 recovery pass: a run left 'running' by a process that died is stamped by
        # the process that notices, because the dead one never can.
        conn.execute("UPDATE index_run SET status='crashed', finished_utc=? "
                     "WHERE status='running'", (now_ms,))
        run_id = conn.execute("SELECT last_insert_rowid()").fetchone()[0]
        t_git = time.perf_counter()
        seen = {r["oid"] for r in conn.execute("SELECT oid FROM git_commit")}
        revspec = ("%s..%s" % (prev_head, head)) if (incremental and prev_head) else head
        all_commits = log_commits(repo, revspec)
        todo = [c for c in all_commits if c["oid"] not in seen] if incremental else all_commits
        raw = diff_tree_raw(repo, revspec)
        git_ms = (time.perf_counter() - t_git) * 1000
        for c in todo:
            _write_commit(conn, c, raw.get(c["oid"], []), run_id, now_ms)
        eps = _segment(conn, todo, epoch)
        # §8.1 memory rule: only the trees the current episode needs are held — its own
        # tree and its parent's. A cold index must not grow with the repository, and
        # retaining every tree is how it would grow.
        files_by_tree: dict[str, dict[str, str]] = {}
        parent_trees: dict[str, str | None] = {}
        try:
            for ep_id, members in eps:
                tree = members[-1]["tree"]
                # An episode's delta is against its own first commit's parent tree, never
                # against "whatever the previous episode in this batch was". Deriving it
                # from the batch is the subtle bug §10.2's identity test exists to catch: a
                # cold build and a warm build then disagree about the same episode, and only
                # a full rebuild would reveal it — which is nobody's test.
                parents = members[0].get("parents") or []
                ptree = None
                if parents:
                    if parents[0] not in parent_trees:
                        parent_trees[parents[0]] = _tree_of(repo, parents[0])
                    ptree = parent_trees[parents[0]]
                for t in (tree, ptree):
                    if t and t not in files_by_tree:
                        t_read = time.perf_counter()
                        files_by_tree[t] = _tree_files(repo, t)
                        git_ms += (time.perf_counter() - t_read) * 1000
                files = files_by_tree.get(tree, {})
                prev_files = files_by_tree.get(ptree, {}) if ptree else {}
                keep = {tree, ptree}
                files_by_tree = {t: v for t, v in files_by_tree.items() if t in keep}
                parent_trees = {k: v for k, v in parent_trees.items() if v in keep}
                _run_analyzers(store, ep_id=ep_id, tree=tree, files=files,
                               prev_files=prev_files, config=config, versions=versions,
                               epoch=epoch, now_ms=now_ms,
                               counters=lambda d: _bump(conn, d))
        except sqlite3.OperationalError as exc:
            # §7.6: the progress-handler budget tripped. This is a *stated partial*, not
            # a failure: the run is recorded `aborted` and the store stays consistent.
            if "interrupt" not in str(exc).lower():
                raise
            interrupted = 1
        # §10.2: `lineage_seq` is a property of the *whole* lineage, not of the batch just
        # indexed. Assigning it per batch makes a warm re-index and a cold rebuild disagree,
        # which would break both the identity invariant and ADR-001 §5.3's L_N(e) window.
        conn.execute(
            "UPDATE episode SET lineage_seq = ("
            "  SELECT count(*) FROM episode e2 WHERE e2.lineage_ref = episode.lineage_ref"
            "   AND (e2.created_utc < episode.created_utc"
            "        OR (e2.created_utc = episode.created_utc AND e2.id < episode.id)))")
        conn.execute(
            "UPDATE index_run SET commits_seen=?, commits_added=?, episodes_new=?, "
            "cache_hits=?, cache_misses=?, analyzers_run=?, finished_utc=?, "
            "status=?, interrupted=? WHERE id=?",
            (len(all_commits), len(todo), len(eps), _counter(conn, "cache.hit"),
             _counter(conn, "cache.miss"), len(eps), now_ms,
             "aborted" if interrupted else "ok", interrupted, run_id))
        store.set_meta("head", head)
        conn.execute("COMMIT")
    except Exception:
        conn.execute("ROLLBACK")
        raise
    finally:
        if budget_ops:
            conn.set_progress_handler(None, 0)
    dt = time.perf_counter() - t0
    return {"run_id": run_id, "commits": len(todo), "episodes": len(eps),
            "wall_ms": round(dt * 1000, 1), "interrupted": interrupted,
            "git_ms": round(git_ms, 1), "store_ms": round(dt * 1000 - git_ms, 1),
            "hits": _counter(conn, "cache.hit"), "misses": _counter(conn, "cache.miss")}


def _bump(conn, counters: dict[str, int]) -> None:
    for name, delta in counters.items():
        conn.execute("INSERT INTO counter(name,value,updated_utc) VALUES(?,?,0) "
                     "ON CONFLICT(name) DO UPDATE SET value=value+excluded.value",
                     (name, delta))


def _counter(conn, name: str) -> int:
    row = conn.execute("SELECT value FROM counter WHERE name=?", (name,)).fetchone()
    return row["value"] if row else 0


def _write_commit(conn, c: dict, files: list[dict], run_id: int, now_ms: int) -> None:
    msg = c["message"]
    conn.execute(
        "INSERT OR IGNORE INTO git_commit(oid,tree_oid,parent_oids,author_name,author_email,"
        "author_utc,committer_utc,message_head,message_hash,trailers,n_files,n_add,n_del,"
        "n_binary,oversized,ingested_utc,run_id) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        (c["oid"], c["tree"], json.dumps(c["parents"]), c["author_name"], c["author_email"],
         c["author_utc"], c["committer_utc"], msg.splitlines()[0][:512] if msg else "",
         hashlib.sha256(msg.encode()).hexdigest(),
         json.dumps({k: v for k, v in (("agent_session", c.get("session")),
                                       ("agent_turn", c.get("turn"))) if v}),
         len(files), sum(f.get("n_add", 0) for f in files),
         sum(f.get("n_del", 0) for f in files),
         sum(1 for f in files if f.get("is_binary")), 0, now_ms, run_id))
    for f in files:
        path = f["new_path"] or f["old_path"] or ""
        conn.execute(
            "INSERT OR REPLACE INTO git_commit_file(commit_oid,path_hash,new_path,old_path,"
            "change_kind,mode_old,mode_new,is_binary,n_add,n_del,hunk_headers,blob_old,"
            "blob_new,path_class,taxonomy_version,noise_class) "
            "VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            (c["oid"], _h(path), path, f.get("old_path"), f["change_kind"], None, None,
             1 if f.get("is_binary") else 0, f.get("n_add", 0), f.get("n_del", 0), "[]",
             f.get("blob_old"), f.get("blob_new"), classify_path(path), 1,
             NOISE_CLASSES.get(classify_path(path), "informative")))


def _segment(conn, commits: list[dict], epoch: int) -> list[tuple[str, list[dict]]]:
    """Group commits into episodes by the `Agent-Session`/`Agent-Turn` trailer (ADR-003).

    Episode id is content-addressed over the ordered member oids; the *fingerprint* is
    content-addressed over the resulting trees, which is what survives a rewrite and
    therefore what the ledger links to (§10.3).
    """
    groups: list[list[dict]] = []
    for c in commits:
        if groups and c.get("session") and groups[-1][0].get("session") == c.get("session"):
            groups[-1].append(c)
        else:
            groups.append([c])
    out = []
    for n, g in enumerate(groups):
        oids = [c["oid"] for c in g]
        ep_id = "ep_" + hashlib.sha256(_frame(oids)).hexdigest()[:24]
        fp = "fp_" + hashlib.sha256(_frame([c["tree"] for c in g])).hexdigest()[:24]
        # Upsert, never REPLACE: `episode` has ON DELETE CASCADE children (delta, finding,
        # evidence, risk, episode_doc), and REPLACE is a DELETE plus an INSERT — so a re-index
        # that REPlaceD an episode silently destroyed its derived rows while the §5 cache went
        # on reporting the episode as fresh. Measured, caught by §10.2's identity test, and the
        # reason §4.0's rule is "upsert, or delete deliberately".
        conn.execute(
            "INSERT INTO episode(id,kind,fingerprint,lineage_ref,lineage_seq,"
            "created_utc,created_source,created_confidence,session_id,turn_id,boundary_rule,"
            "boundary_confidence,author_kind,author_confidence,intent,noise_share,epoch) "
            "VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?) "
            "ON CONFLICT(id) DO UPDATE SET fingerprint=excluded.fingerprint,"
            "kind=excluded.kind,intent=excluded.intent,epoch=excluded.epoch",
            (ep_id, "committed", fp, "refs/heads/main", n, g[0]["author_utc"], "commit", 0.7,
             g[0].get("session"), "%s:%s" % (g[0].get("session"), g[0].get("turn")),
             "trailer", 0.8, "agent", 0.9,
             json.dumps({"session": g[0].get("session")}), 0.0, epoch))
        for i, c in enumerate(g):
            conn.execute("INSERT OR REPLACE INTO episode_member(episode_id,ordinal,commit_oid) "
                         "VALUES(?,?,?)", (ep_id, i, c["oid"]))
        out.append((ep_id, g))
    return out


def _frame(parts) -> bytes:
    out = bytearray()
    for p in parts:
        b = p.encode()
        out += len(b).to_bytes(4, "big") + b
    return bytes(out)


def _h(s: str) -> str:
    return hashlib.sha256(s.encode()).hexdigest()[:32]


def _tree_of(repo: Path, rev: str) -> str | None:
    """The tree object a commit points at, or None if it cannot be resolved (§9.4)."""
    try:
        return _run(repo, ["git", "rev-parse", "%s^{tree}" % rev]).strip() or None
    except Exception:
        return None


def _tree_files(repo: Path, tree: str) -> dict[str, str]:
    """(path -> content) for a tree, via one `git ls-tree` + one `git cat-file --batch`.

    Two subprocesses for the whole tree, not one per blob: the batch protocol is the
    only way a cold index stays inside the budget it is meant to measure (e1.2).
    """
    raw = _run(repo, ["git", "ls-tree", "-r", "--format=%(objectname) %(path)", tree])
    files: dict[str, str] = {}
    want: dict[str, list[str]] = {}
    for line in raw.splitlines():
        if not line.strip():
            continue
        sha, path = line.split(" ", 1)
        want.setdefault(sha, []).append(path)
    if want:
        p = subprocess.run(["git", "cat-file", "--batch"], cwd=str(repo), env=_git_env(repo),
                           input="".join(s + "\n" for s in want).encode(), capture_output=True)
        buf = p.stdout
        i = 0
        for sha in want:
            nl = buf.index(b"\n", i)
            header = buf[i:nl].decode(errors="replace").split()
            if len(header) < 3 or header[1] != "blob":
                i = nl + 1
                continue
            n = int(header[2])
            data = buf[nl + 1: nl + 1 + n]
            for path in want[sha]:
                files[path] = data.decode("utf-8", "replace")
            i = nl + 1 + n + 1
    return files


def _run_analyzers(store: SqliteStore, *, ep_id: str, tree: str,
                   files: dict[str, str], prev_files: dict[str, str], config: dict,
                   versions, epoch: int, now_ms: int, counters) -> None:
    """Run the fixture's analyzer DAG for one episode, through the §5 cache."""
    conn = store.conn
    v = dict(versions or {})
    scopes = {
        "taxonomy.path": [("blob", hashlib.sha256(c.encode()).hexdigest()[:40])
                          for _p, c in sorted(files.items())],
        "structure.delta": [("tree", tree)],
        "graph.build": [("tree", tree)],
        "deps.resolve": [("tree", tree)],
        "non_code.surface": [("tree", tree)],
        "test.surface": [("tree", tree)],
        "findings.detect": [("episode", ep_id)],
        "risk.score": [("episode", ep_id)],
        "delta.compose": [("episode", ep_id)],
    }
    for aid in ("taxonomy.path", "structure.delta", "graph.build", "deps.resolve",
                "non_code.surface", "test.surface", "findings.detect", "risk.score",
                "delta.compose"):
        analyzer = C.ANALYZER_INDEX[aid]
        for kind, ih in scopes.get(aid, []):
            res = C.resolve(conn, analyzer_id=aid, input_hash=ih, epoch=epoch,
                            input_kind=kind, versions=v, config=config, now_ms=now_ms)
            counters(res.counters)
            if res.hit:
                continue
            payload = _compute(aid, files=files, prev_files=prev_files, ep_id=ep_id)
            sha = _put_blob(conn, store.path.parent, "delta" if aid == "delta.compose"
                            else "graph_chunk", payload, now_ms)
            C.put(conn, key=res.key, result_sha=sha, epoch=epoch, cost_ms=analyzer.est_cost_ms,
                  analyzer=analyzer, input_hash=ih, result_bytes=len(payload), now_ms=now_ms)
            if aid == "delta.compose":
                conn.execute("INSERT OR REPLACE INTO delta(episode_id,schema_version,blob_sha,"
                             "built_utc) VALUES(?,?,?,?)", (ep_id, 1, sha, now_ms))
                conn.execute("INSERT OR REPLACE INTO blob_ref(owner_kind,owner_id,blob_sha) "
                             "VALUES('delta',?,?)", (ep_id, sha))
        if aid == "findings.detect":
            _write_findings(conn, ep_id, files, epoch, now_ms)
        if aid == "risk.score":
            _write_risk(conn, ep_id, files, config, epoch, now_ms)


def _compute(aid: str, *, files: dict, prev_files: dict, ep_id: str) -> bytes:
    if aid == "structure.delta":
        return _delta_bytes(files, prev_files)
    if aid in ("graph.build",):
        nodes, edges = _graph_of(files)
        return json.dumps({"nodes": [n[0] for n in nodes], "edges": edges},
                          sort_keys=True).encode()
    if aid == "deps.resolve":
        deps = {}
        for p, c in files.items():
            if p.endswith("package.json"):
                deps = json.loads(c).get("dependencies", {})
        return json.dumps({"deps": deps}, sort_keys=True).encode()
    if aid == "test.surface":
        t = sum(1 for p in files if classify_path(p) == "test")
        s = sum(1 for p in files if classify_path(p) in ("source",))
        return json.dumps({"tests": t, "sources": s}, sort_keys=True).encode()
    if aid == "non_code.surface":
        return json.dumps({"non_code": sorted(p for p in files
                                              if classify_path(p) in ("migration", "config"))},
                          sort_keys=True).encode()
    if aid == "taxonomy.path":
        return b'{"taxonomy_version":1}'
    if aid == "risk.score":
        return b'{"scoring":"fixture"}'
    return _delta_bytes(files, prev_files)


def _write_findings(conn, ep_id: str, files: dict, epoch: int, now_ms: int) -> None:
    tests = [p for p in files if classify_path(p) == "test"]
    src = [p for p in files if classify_path(p) == "source"]
    cands = []
    if not tests and src:
        cands.append(("test_deletion", "high", "production change with no test surface"))
    if any("debug leftover" in c for c in files.values()):
        cands.append(("debug_leftover", "low", "print/debug statement added"))
    for det, sev, msg in cands:
        eid = "ev_" + _h(ep_id + det)
        fid = "f_" + _h(ep_id + det + "1")
        conn.execute("INSERT OR REPLACE INTO evidence(id,episode_id,kind,locator,grade,verified)"
                     " VALUES(?,?,?,?,'observed',1)",
                     (eid, ep_id, "range", json.dumps({"path": src[0] if src else "n/a"})))
        conn.execute(
            "INSERT OR REPLACE INTO finding(id,episode_id,detector_id,detector_version,"
            "severity,confidence,message,evidence_ids,jump,suppression_key,epoch,built_utc) "
            "VALUES(?,?,?,1,?,0.9,?,?,?,?,?,?)",
            (fid, ep_id, det, sev, msg, json.dumps([eid]),
             "diff:" + (src[0] if src else ""), "s:" + det, epoch, now_ms))


def _write_risk(conn, ep_id: str, files: dict, config: dict, epoch: int, now_ms: int) -> None:
    w = config["risk"]["weights"]
    tau = config["risk"]["tau"]
    n_find = conn.execute("SELECT count(*) n FROM finding WHERE episode_id=?",
                          (ep_id,)).fetchone()["n"]
    size = min(30, len(files) * 3)
    test_ratio = 0 if any(classify_path(p) == "test" for p in files) else 25
    raw = size * w["size"] + test_ratio * w["test_ratio"] + n_find * 12 * w["blast"]
    score = int(max(0, min(100, raw)))
    conn.execute("INSERT OR REPLACE INTO risk(episode_id,score,reasons,weights_version,"
                 "flagged,epoch,built_utc) VALUES(?,?,?,?,?,?,?)",
                 (ep_id, score, json.dumps([{"feature": "size", "contribution": size},
                                            {"feature": "test_ratio", "contribution": test_ratio},
                                            {"feature": "findings", "contribution": n_find * 12}]),
                  "fixture-v1", 1 if score >= tau else 0, epoch, now_ms))


def _put_blob(conn, fathom_dir: Path, kind: str, payload: bytes, now_ms: int) -> str:
    sha = hashlib.sha256(payload).hexdigest()
    inline = len(payload) <= 4096
    if not inline:
        d = Path(fathom_dir) / "objects" / sha[:2]
        d.mkdir(parents=True, exist_ok=True)
        (d / sha[2:]).write_bytes(payload)
    conn.execute("INSERT OR IGNORE INTO blob(sha256,kind,bytes,storage,data,created_utc) "
                 "VALUES(?,?,?,?,?,?)",
                 (sha, kind, len(payload), "inline" if inline else "file",
                  payload if inline else None, now_ms))
    return sha


# ---------------------------------------------------------------------------
# §10.3 recovery: the ledger is the thing that must come back
# ---------------------------------------------------------------------------

LEDGER_TEMPLATE = """# Fathom review ledger

## {id}

- episode_id: {episode_id}
- fingerprint: {fingerprint}
- decision: accepted
- actor: {actor}
- signed: {signed}
- scope: members={members}
- note: {note}
"""


def write_ledger_entry(repo: Path, *, episode_id: str, fingerprint: str, members: list[str],
                       actor: str = "dana@example.invalid", note: str = "checked blast radius",
                       signed: int = EPOCH0 * 1000) -> Path:
    """Append a review to `.fathom/reviews/ledger.md` (the class-M artifact, e4.6)."""
    d = Path(repo) / ".fathom" / "reviews"
    d.mkdir(parents=True, exist_ok=True)
    lid = "rev_" + _h(episode_id + actor)[:16]
    text = LEDGER_TEMPLATE.format(id=lid, episode_id=episode_id, fingerprint=fingerprint,
                                  actor=actor, signed=signed, members=",".join(m[:8] for m in members),
                                  note=note)
    path = d / "ledger.md"
    # R8 / §7.4: the human act is durable *before* anything indexes it. The file is flushed
    # and fsynced here, and only the caller's later `mirror_ledger` puts a row in the store —
    # so a crash between the two loses an index entry (rebuildable, class R) and never a
    # sign-off. A mirror that fsynced the row instead would be a mirror that can lie.
    with path.open("a", encoding="utf-8") as fh:
        fh.write(text)
        fh.flush()
        os.fsync(fh.fileno())
    return path


def mirror_ledger(store: SqliteStore, repo: Path, *, now_ms: int | None = None) -> dict:
    """Parse the class-M markdown into the mirror tables, then relink (§10.3).

    This is the direction that makes recovery legal: the file is the truth, the row is
    an index. Deleting the store and re-mirroring loses nothing.
    """
    now_ms = now_ms if now_ms is not None else int(time.time() * 1000)
    repo = Path(repo)
    d = repo / ".fathom" / "reviews"
    n = linked = stale = orphan = 0
    if not d.exists():
        return {"entries": 0, "linked": 0, "stale": 0, "orphan": 0}
    conn = store.conn
    conn.execute("BEGIN IMMEDIATE")
    try:
        conn.execute("DELETE FROM ledger_entry")     # mirrors are rebuilt, never merged
        for path in sorted(d.glob("*.md")):
            text = path.read_text()
            for block in text.split("## ")[1:]:
                fields = {}
                for line in block.splitlines()[1:]:
                    if line.startswith("- ") and ":" in line:
                        k, v = line[2:].split(":", 1)
                        fields[k.strip()] = v.strip()
                if "decision" not in fields:
                    continue
                n += 1
                fp = fields.get("fingerprint", "")
                row = conn.execute("SELECT id, fingerprint FROM episode WHERE id=? OR "
                                   "fingerprint=? LIMIT 1",
                                   (fields.get("episode_id"), fp)).fetchone()
                state, method, reason = "fresh", "none", None
                if row is None:
                    state, method, orphan = "orphan", "none", orphan + 1
                elif row["fingerprint"] != fp:
                    state, method, stale, reason = "stale", "fingerprint", stale + 1, "content changed after sign-off"
                else:
                    linked += 1
                    method = "exact" if row["id"] == fields.get("episode_id") else "fingerprint"
                conn.execute(
                    "INSERT OR REPLACE INTO ledger_entry(id,file,anchor,episode_id,fingerprint,"
                    "decision,reason_code,actor,actor_kind,signed_utc,scope,content_sha,"
                    "link_method,link_score,state,stale_reason,mirrored_utc) "
                    "VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
                    (block.splitlines()[0].strip(), str(path.relative_to(repo)), 1,
                     row["id"] if row else fields.get("episode_id"), fp,
                     fields["decision"], None, fields.get("actor", "unknown"), "human",
                     int(fields.get("signed", 0)), fields.get("scope", ""),
                     hashlib.sha256(block.encode()).hexdigest(), method,
                     1.0 if state == "fresh" else 0.5, state, reason, now_ms))
        conn.execute("COMMIT")
    except Exception:
        conn.execute("ROLLBACK")
        raise
    return {"entries": n, "linked": linked, "stale": stale, "orphan": orphan}


def check_must_survive_tracked(repo: Path) -> list[str]:
    """§9.6: a class-M file git does not *hold* is a review that will be lost.

    "Held" means tracked **and** committed: an untracked ledger is invisible to a clone,
    and a modified-uncommitted one is invisible to `git clean -fdx`'s victim only slightly
    more slowly. Both are warnings, because both end in a lost sign-off.
    """
    repo = Path(repo)
    reviews = repo / ".fathom" / "reviews"
    if not reviews.exists():
        return []
    problems = []
    for f in sorted(reviews.glob("*")):
        rel = str(f.relative_to(repo))
        if subprocess.run(["git", "ls-files", "--error-unmatch", rel], cwd=str(repo),
                          env=_git_env(repo), capture_output=True).returncode != 0:
            problems.append("untracked must-survive file: %s" % rel)
            continue
        status = subprocess.run(["git", "status", "--porcelain", "--", rel], cwd=str(repo),
                                env=_git_env(repo), capture_output=True, text=True).stdout
        if status.strip():
            problems.append("uncommitted must-survive file: %s" % rel)
    return problems
