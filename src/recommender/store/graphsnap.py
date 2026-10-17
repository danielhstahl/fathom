"""The five snapshot strategies spike `s3` measures, all on ADR-004 §4.6's real DDL.

Every strategy writes through :class:`SqliteStore` — the same schema, the same §7 pragma
contract, the same `graph_snapshot` / `graph_delta` / `graph_chunk` / `graph_chunk_link` /
`blob` / `cache` tables production creates. That is the point of this file: ADR-004 D15
claims "the schema holds all three candidate strategies so `s3` chooses between
configuration settings instead of forcing a migration", and a claim of that shape is worth
nothing until somebody writes every candidate into it. Two places turn out not to hold (a
`graph_delta` row cannot name a *node* op without a dummy `dst`, and a materialised-tree
strategy wants the §4.5 `cache`+`blob` pair rather than a graph table); both are reported
as findings for `e3.2` rather than worked around silently.

The strategies:

``FULL``
    a full snapshot per revision with nothing shared — the naive reading of `e3.2`.
``CHAIN``
    delta rows per revision plus a cheap content-addressed anchor every N revisions:
    §4.6's delta chain, with N swept to answer §14.1's `graph.chain_full_every`.
``CHUNKED``
    content-defined chunking: one chunk per module adjacency, every revision linking all
    of its chunks — payloads shared by content, membership stored per revision.
``MERKLE``
    a full snapshot per revision with real structural sharing: a persistent trie whose
    path-copied nodes are the only new rows. The charitable reading of `e3.2`.
``PER_FILE``
    no per-revision graph at all: per-file structural facts keyed by blob (§4.5's analyzer
    cache) and a revision's graph rebuilt on demand.

The op row is `graph_delta`'s: ``(tree_oid, op, kind, src, dst, src_layer, dst_layer)``
with ``op ∈ add|remove`` and ``kind ∈ module|symbol|contains|imports|submodule``; node ops
carry ``dst = ''``.
"""

from __future__ import annotations

import hashlib
import sqlite3
import time
from dataclasses import dataclass, field
from pathlib import Path

from .schema import SqliteStore

#: Queries per latency probe; p50/p95 are taken over this many.
QUERIES = 240
#: Anchor periods swept. §14.1 asks for a value for `graph.chain_full_every` and
#: `graph.query.max_chain_walk`; the honest answer is a curve, so the curve is measured.
SWEEP = (8, 16, 32, 64, 128, 256)
#: BFS depth cap for the path query — §8.5's cap, applied to `e3.2`'s "path between A and B".
PATH_DEPTH = 6
#: §4.5's inline/objects boundary: at or under this a payload lives in `blob.data`, above it in
#: `.fathom/objects/`. The spike counts both, from the same place §8.4 would.
INLINE_LIMIT = 4_096
#: `kind` values that are vertices rather than edges.
NODE_KINDS = ("module", "symbol")
#: The empty trie node's content address.
EMPTY = "0" * 64

Row = tuple[str, str, str]          # (kind, src, dst)
Op = tuple[str, str, str, str]      # (op, kind, src, dst)


def pctl(values: list[float], q: float) -> float | None:
    if not values:
        return None
    xs = sorted(values)
    return round(xs[min(len(xs) - 1, int(len(xs) * q))], 3)


def layer_of(path: str) -> str:
    """`src/api3/m17.rs` -> `api`. Layers come from `fathom.toml` in production (`e3.6`);
    here the crate directory plays that role, which is the same function of the path."""
    parts = path.split("/")
    if len(parts) >= 2:
        name = parts[1]
        return "".join(c for c in name if not c.isdigit()) or name
    return "?"


def encode(rows: list[Row]) -> bytes:
    """A module's rows as text: what a chunk holds, and what a delta serialises to."""
    return "\n".join(f"{k}\t{s}\t{d}" for k, s, d in rows).encode()


def decode(payload: bytes) -> list[Row]:
    if not payload:
        return []
    out: list[Row] = []
    for line in payload.decode().splitlines():
        kind, src, dst = line.split("\t")
        out.append((kind, src, dst))
    return out


def ops_from_diff(old: list[Row], new: list[Row]) -> list[Op]:
    """The delta between two revisions of one module, as `graph_delta` rows."""
    old_set, new_set = set(old), set(new)
    ops: list[Op] = [("remove", k, s, d) for k, s, d in sorted(old_set - new_set)]
    ops += [("add", k, s, d) for k, s, d in sorted(new_set - old_set)]
    return ops


# ---------------------------------------------------------------------------
# ground truth, as recorded by the walk in `graphbench`
# ---------------------------------------------------------------------------

@dataclass
class Revision:
    index: int
    commit: str
    tree: str
    parent_tree: str
    changed: tuple[str, ...]             # module paths written by this commit
    removed: tuple[str, ...]             # module paths deleted or renamed away
    ops: tuple[Op, ...]                  # graph ops against the parent revision
    new_rows: tuple[tuple[str, bytes], ...]   # (module, encoded rows) for the changed ones
    files: tuple[tuple[str, str], ...] = ()    # whole (path, blob) list; probe revisions only


# ---------------------------------------------------------------------------
# the materialised graph a query runs against
# ---------------------------------------------------------------------------

@dataclass
class Graph:
    """The in-memory module graph the TUI keeps for the revision in focus.

    `out` and `inc` are both keyed by (edge kind, endpoint) on purpose: `e3.2` budgets
    fan-in (`query.graph_dependents.ms`) and fan-out equally, and a structure that indexes
    only the source side makes `dependents(X)` a scan. That is one of the things this spike
    measures, so it is a property of the data structure rather than a comment.
    """
    nodes: dict[str, str] = field(default_factory=dict)
    out: dict[tuple[str, str], set[str]] = field(default_factory=dict)
    inc: dict[tuple[str, str], set[str]] = field(default_factory=dict)

    @classmethod
    def from_rows(cls, rows: list[Row]) -> "Graph":
        g = cls()
        for kind, src, dst in rows:
            g.apply_one("add", kind, src, dst)
        return g

    def apply(self, ops) -> None:
        for op, kind, src, dst in ops:
            self.apply_one(op, kind, src, dst)

    def apply_one(self, op: str, kind: str, src: str, dst: str) -> None:
        add = op == "add"
        if kind in NODE_KINDS:
            if add:
                self.nodes[src] = kind
            else:
                self.nodes.pop(src, None)
            return
        if add:
            self.out.setdefault((kind, src), set()).add(dst)
            self.inc.setdefault((kind, dst), set()).add(src)
        else:
            self.out.get((kind, src), set()).discard(dst)
            self.inc.get((kind, dst), set()).discard(src)

    # ---- the queries `e3.2` exposes, i.e. the ones ADR-006's architecture panes issue
    def dependencies(self, module: str) -> list[str]:
        return sorted(self.out.get(("imports", module), ()))

    def dependents(self, module: str) -> list[str]:
        return sorted(self.inc.get(("imports", module), ()))

    def path(self, a: str, b: str, depth: int = PATH_DEPTH) -> list[str] | None:
        if a == b:
            return [a]
        seen = {a}
        parents: dict[str, str | None] = {a: None}
        frontier = [a]
        for _ in range(depth):
            nxt: list[str] = []
            for node in frontier:
                for dst in self.out.get(("imports", node), ()):
                    if dst in seen:
                        continue
                    seen.add(dst)
                    parents[dst] = node
                    if dst == b:
                        out = [dst]
                        cur: str | None = parents[dst]
                        while cur is not None:
                            out.append(cur)
                            cur = parents[cur]
                        return out[::-1]
                    nxt.append(dst)
            frontier = nxt
            if not frontier:
                return None
        return None

    def layer_subgraph(self, layer: str) -> tuple[int, int]:
        kept = {n for n in self.nodes if layer_of(n) == layer}
        edges = 0
        for (kind, src), dsts in self.out.items():
            if kind in ("imports", "submodule") and src in kept:
                edges += sum(1 for d in dsts if d in kept)
        return len(kept), edges

    def edge_set(self) -> set[Row]:
        out: set[Row] = set()
        for (kind, src), dsts in self.out.items():
            out |= {(kind, src, d) for d in dsts}
        return out


# ---------------------------------------------------------------------------
# shared store plumbing
# ---------------------------------------------------------------------------

class Store:
    """One store file per strategy: §7 pragmas, §4.5 blobs, byte accounting that labels its basis."""

    #: The per-revision cost claim, stated *before* measuring and printed beside the number.
    scaling = ""
    #: True when the strategy must be offered every revision in order (chains, roots).
    sequential = True
    #: True when a query can be answered from SQL without materialising (FULL, CHAIN).
    supports_sql = False

    def __init__(self, root: Path, name: str):
        self.root = root
        self.name = name
        self.store = SqliteStore.create(root, name=name)
        self.conn: sqlite3.Connection = self.store.conn
        self.objects = root / "objects"
        self.objects.mkdir(exist_ok=True)
        self.label = type(self).__name__
        self.rows_written = 0
        self.revisions_written = 0
        self.written: list[int] = []
        self.duplicate_trees = 0
        #: bytes of *new* blob payload written (shared payloads are written once, so this is
        #: the honest marginal storage cost of whatever wrote them).
        self.payload_bytes = 0
        #: strategy-specific extra numbers, merged into the summary.
        self.extra: dict = {}
        self.record_ms: list[float] = []
        self.materialize_ms: list[float] = []
        self.warm_step_ms: list[float] = []
        self.query_ms: dict[str, list[float]] = {}
        self.sql_ms: dict[str, list[float]] = {}
        self.notes: list[str] = []
        self._known_blobs: set[str] = set()
        self.checkpoint()
        self.base_bytes = self.total_bytes()

    # -- bytes (§8.4: an estimate passed off as a measurement is how a budget becomes fiction)
    def checkpoint(self) -> None:
        """Fold the WAL into the file. The WAL is transient by §7.4, so it is *not* counted on
        either side of a measurement — counting it once and not the other way is how a
        strategy measures negative bytes, which is how this line came to exist."""
        self.conn.execute("PRAGMA wal_checkpoint(TRUNCATE)")

    def total_bytes(self) -> int:
        pages = self.conn.execute("PRAGMA page_count").fetchone()[0]
        size = self.conn.execute("PRAGMA page_size").fetchone()[0]
        objects = sum(f.stat().st_size for f in self.objects.rglob("*") if f.is_file())
        return pages * size + objects

    def per_table_bytes(self) -> dict[str, int]:
        try:
            rows = self.conn.execute(
                "SELECT name, sum(pgsize) FROM dbstat GROUP BY name ORDER BY 2 DESC").fetchall()
        except sqlite3.OperationalError:
            return {}
        return {r[0]: int(r[1] or 0) for r in rows if r[0]}

    def usage_basis(self) -> str:
        try:
            self.conn.execute("SELECT 1 FROM dbstat LIMIT 1").fetchone()
            return "dbstat"
        except sqlite3.OperationalError:
            return "file size + objects dir (dbstat unavailable)"

    # -- the §4.5 blob pair: content-addressed, inline <= 4 KiB, `objects/` above it
    def put_blob(self, kind: str, payload: bytes) -> str:
        sha = hashlib.sha256(payload).hexdigest()
        if sha in self._known_blobs:
            return sha
        if len(payload) <= INLINE_LIMIT:
            self.conn.execute(
                "INSERT OR IGNORE INTO blob(sha256,kind,bytes,storage,data,created_utc) "
                "VALUES(?,?,?,?,?,?)", (sha, kind, len(payload), "inline", payload, 0))
        else:
            bucket = self.objects / sha[:2]
            bucket.mkdir(exist_ok=True)
            path = bucket / sha[2:]
            if not path.exists():
                path.write_bytes(payload)
            self.conn.execute(
                "INSERT OR IGNORE INTO blob(sha256,kind,bytes,storage,data,created_utc) "
                "VALUES(?,?,?,?,?,?)", (sha, kind, len(payload), "file", None, 0))
        self._known_blobs.add(sha)
        self.payload_bytes += len(payload)
        self.rows_written += 1
        return sha

    def get_blobs(self, shas: list[str]) -> dict[str, bytes]:
        return {sha: data for sha, _kind, data in self.get_nodes(shas)}

    def get_nodes(self, shas: list[str]) -> list[tuple[str, str, bytes]]:
        """(sha, graph_chunk.kind, payload) for a batch of content addresses, `objects/` read back."""
        out: list[tuple[str, str, bytes]] = []
        for start in range(0, len(shas), 900):
            part = shas[start:start + 900]
            marks = ",".join("?" * len(part))
            for sha, kind, storage, data in self.conn.execute(
                    f"SELECT b.sha256, g.kind, b.storage, b.data FROM blob b "
                    f"JOIN graph_chunk g ON g.sha256 = b.sha256 WHERE b.sha256 IN ({marks})",
                    part):
                payload = bytes(data) if storage == "inline" else \
                    (self.objects / sha[:2] / sha[2:]).read_bytes()
                out.append((sha, kind, payload))
        return out

    # -- the §4.6 rows
    def snapshot_row(self, tree: str, base: str | None, kind: str, depth: int,
                     nodes: int, edges: int, nbytes: int, cost_ms: float) -> None:
        """One `graph_snapshot` row per revision. `INSERT`, not `INSERT OR REPLACE`, on
        purpose: two commits that reach the same tree would otherwise rewrite the first
        one's `base_tree` and quietly break the chain walk, which §D16's whole rule is about.
        """
        try:
            self.conn.execute(
                "INSERT INTO graph_snapshot(tree_oid,base_tree,kind,depth,node_count,"
                "edge_count,bytes,analyzer_version,built_utc,cost_ms) "
                "VALUES(?,?,?,?,?,?,?,?,?,?)",
                (tree, base, kind, depth, nodes, edges, nbytes, 1, 0, max(0, int(cost_ms))))
            self.rows_written += 1
        except sqlite3.IntegrityError:
            self.duplicate_trees += 1

    def delta_rows(self, tree: str, ops) -> None:
        for op, kind, src, dst in ops:
            self.conn.execute(
                "INSERT OR IGNORE INTO graph_delta(tree_oid,op,kind,src,dst,src_layer,dst_layer) "
                "VALUES(?,?,?,?,?,?,?)",
                (tree, op, kind, src, dst, layer_of(src), layer_of(dst) if dst else ""))
            self.rows_written += 1

    def read_delta_ops(self, tree: str) -> list[Op]:
        return [(r[0], r[1], r[2], r[3]) for r in self.conn.execute(
            "SELECT op, kind, src, dst FROM graph_delta WHERE tree_oid=?", (tree,))]

    def read_delta_rows(self, tree: str) -> list[Row]:
        return [(r[0], r[1], r[2]) for r in self.conn.execute(
            "SELECT kind, src, dst FROM graph_delta WHERE tree_oid=? AND op='add'", (tree,))]

    def chunk_row(self, sha: str, kind: str, nbytes: int) -> None:
        self.conn.execute("INSERT OR IGNORE INTO graph_chunk(sha256,kind,bytes) VALUES(?,?,?)",
                          (sha, kind, nbytes))
        self.rows_written += 1

    def link_row(self, tree: str, slot: int, sha: str) -> None:
        self.conn.execute("INSERT OR IGNORE INTO graph_chunk_link(tree_oid,slot,chunk_sha) "
                          "VALUES(?,?,?)", (tree, slot, sha))
        self.rows_written += 1

    # -- the interface each strategy implements
    def record(self, rev: Revision, live: dict[str, list[Row]]) -> None:
        raise NotImplementedError

    def materialize(self, rev: Revision, files: list[tuple[str, str]]) -> Graph:
        raise NotImplementedError

    def warm_step(self, current: Graph, rev: Revision) -> Graph:
        """The interactive case: the cursor moved by one commit and a graph is already in hand.
        A strategy that stores no deltas must reload the snapshot — that *is* its warm cost."""
        t0 = time.perf_counter()
        graph = self.materialize(rev, [])
        self.warm_step_ms.append((time.perf_counter() - t0) * 1000)
        return graph

    def sql_query(self, name: str, rev: Revision, arg):
        """A query answered from SQL without materialising; only some strategies can."""
        return None

    def finish(self) -> None:
        try:
            self.conn.execute("COMMIT")
        except sqlite3.OperationalError:
            pass
        self.checkpoint()

    def measured(self) -> int:
        self.checkpoint()
        return self.total_bytes() - self.base_bytes

    def summary(self, total_commits: int) -> dict:
        written = self.revisions_written or 1
        bytes_measured = self.measured()
        per_rev = bytes_measured / written
        return {
            "strategy": self.label,
            "scaling_claim": self.scaling,
            "revisions_written": self.revisions_written,
            "revisions_in_workload": total_commits,
            "rows_written": self.rows_written,
            "rows_per_revision": round(self.rows_written / written, 1),
            "bytes_measured": bytes_measured,
            "bytes_per_revision": round(per_rev),
            "bytes_projected_at_10k": round(per_rev * total_commits),
            "extrapolated": self.revisions_written != total_commits,
            "record_ms_p50": pctl(self.record_ms, 0.50),
            "record_ms_p95": pctl(self.record_ms, 0.95),
            "assemble_ms_p50": pctl(getattr(self, "assemble_ms", []), 0.50),
            "record_ms_total_s": round(sum(self.record_ms) / 1000, 2),
            "materialize_ms_p50": pctl(self.materialize_ms, 0.50),
            "materialize_ms_p95": pctl(self.materialize_ms, 0.95),
            "warm_step_ms_p50": pctl(self.warm_step_ms, 0.50),
            "warm_step_ms_p95": pctl(self.warm_step_ms, 0.95),
            "query_ms": {k: {"p50": pctl(v, 0.50), "p95": pctl(v, 0.95), "n": len(v)}
                         for k, v in sorted(self.query_ms.items())},
            "direct_sql_ms": {k: {"p50": pctl(v, 0.50), "p95": pctl(v, 0.95), "n": len(v)}
                              for k, v in sorted(self.sql_ms.items())},
            "extra": self.extra,
            "usage_basis": self.usage_basis(),
            "per_table_bytes": dict(list(self.per_table_bytes().items())[:10]),
            "notes": self.notes,
        }


# ---------------------------------------------------------------------------
# 1. FULL — a full snapshot per revision, nothing shared
# ---------------------------------------------------------------------------

class FullSnapshot(Store):
    """Every revision writes every node and every edge: the naive reading of `e3.2`.

    `graph_delta` with `op='add'` for the whole graph is how §4.6's schema spells "full
    snapshot", so the byte count is production's byte count and not a prototype's.
    """

    scaling = "O(nodes + edges) per revision"
    supports_sql = True
    #: O(graph) per revision, so the driver samples it (see `graphbench`'s `full_samples`).
    sequential = False

    def __init__(self, root: Path, name: str):
        super().__init__(root, name)
        self.label = "FULL: a full snapshot per revision, nothing shared"
        self.assemble_ms: list[float] = []

    def record(self, rev: Revision, live: dict[str, list[Row]]) -> None:
        t0 = time.perf_counter()
        rows: list[Row] = []
        for module_rows in live.values():
            rows.extend(module_rows)
        nodes = sorted({r for r in rows if r[0] in NODE_KINDS})
        edges = sorted({r for r in rows if r[0] not in NODE_KINDS})
        assemble_ms = (time.perf_counter() - t0) * 1000
        t1 = time.perf_counter()
        self.conn.execute("BEGIN IMMEDIATE")
        self.snapshot_row(rev.tree, rev.parent_tree, "full", 0, len(nodes), len(edges), 0, 0.0)
        self.delta_rows(rev.tree, [("add", k, s, d) for k, s, d in nodes + edges])
        self.conn.execute("COMMIT")
        self.assemble_ms.append(assemble_ms)
        self.revisions_written += 1
        self.written.append(rev.index)
        self.record_ms.append((time.perf_counter() - t1) * 1000)

    def materialize(self, rev: Revision, files: list[tuple[str, str]] = ()) -> Graph:
        t0 = time.perf_counter()
        g = Graph.from_rows(self.read_delta_rows(rev.tree))
        self.materialize_ms.append((time.perf_counter() - t0) * 1000)
        return g

    def sql_query(self, name: str, rev: Revision, arg):
        """Answer from SQL instead of loading 46 000 rows into the UI process on every cursor
        move — which is what a full-snapshot store would have to do, so both are measured."""
        t0 = time.perf_counter()
        if name == "dependencies":
            rows = self.conn.execute(
                "SELECT dst FROM graph_delta WHERE tree_oid=? AND kind='imports' AND src=?",
                (rev.tree, arg)).fetchall()
            out: object = sorted(r[0] for r in rows)
        elif name == "dependents":
            rows = self.conn.execute(
                "SELECT src FROM graph_delta WHERE tree_oid=? AND kind='imports' AND dst=?",
                (rev.tree, arg)).fetchall()
            out = sorted(r[0] for r in rows)
        elif name == "layer":
            rows = self.conn.execute(
                "SELECT src, dst FROM graph_delta WHERE tree_oid=? AND kind IN "
                "('imports','submodule') AND src_layer=?", (rev.tree, arg)).fetchall()
            out = len(rows)
        elif name == "delta_adjacent":
            rows = self.conn.execute(
                "SELECT tree_oid, kind, src, dst FROM graph_delta WHERE tree_oid IN (?,?) "
                "AND op='add' ORDER BY tree_oid", (arg[0], rev.tree)).fetchall()
            out = len(rows)
        else:
            out = None
        self.sql_ms.setdefault(name, []).append((time.perf_counter() - t0) * 1000)
        return out


# ---------------------------------------------------------------------------
# 2. CHAIN — delta rows every revision, a shared anchor every N
# ---------------------------------------------------------------------------

class Chain(Store):
    """§4.6's delta chain: `graph.chain_full_every = N`, `graph.query.max_chain_walk = N`.

    The anchor is deliberately *not* a row-per-node snapshot: it is one manifest blob (the
    content address of every live module's chunk) plus only the chunks that are new. That
    is the difference between "an anchor costs O(graph) rows" and "an anchor costs one row
    and O(graph) bytes of shared payload", and it is the largest lever this spike found —
    large enough that `FULL` above is re-measured with the anchor trick as `CHAIN` at
    `chain_full_every=1` in the sweep, so the two effects (anchor period, and anchor shape)
    are separated rather than confounded.

    Reconstruction walks `graph_snapshot.base_tree` back to the anchor and applies the
    deltas forward, which is the path production would take and the reason N costs on both
    sides: bigger N means fewer anchor bytes and longer walks.
    """

    scaling = "O(changed) delta rows per revision + a shared anchor every N"
    supports_sql = True

    def __init__(self, root: Path, name: str, every: int = 32):
        super().__init__(root, name)
        self.every = every

        self.label = f"CHAIN(N={every}): delta rows every revision, anchor every {every}"
        self.depth = 0
        self.anchors: set[str] = set()
        self.manifest_bytes = 0
        self.anchor_payload_bytes = 0

    def _write_anchor(self, rev: Revision, live: dict[str, list[Row]]) -> None:
        before_payload = self.payload_bytes
        entries: list[str] = []
        for module, rows in sorted(live.items()):
            payload = encode(rows)
            sha = self.put_blob("graph_chunk", payload)
            self.chunk_row(sha, "adjacency", len(payload))
            entries.append(f"{module}\t{sha}")
        manifest = "\n".join(entries).encode()
        manifest_sha = self.put_blob("graph_manifest", manifest)
        self.chunk_row(manifest_sha, "manifest", len(manifest))
        self.link_row(rev.tree, 0, manifest_sha)
        self.anchors.add(rev.tree)
        self.manifest_bytes += len(manifest)
        self.anchor_payload_bytes += self.payload_bytes - before_payload

    def record(self, rev: Revision, live: dict[str, list[Row]]) -> None:
        t0 = time.perf_counter()
        self.conn.execute("BEGIN IMMEDIATE")
        if rev.index % self.every == 0:
            self.snapshot_row(rev.tree, rev.parent_tree, "full", 0, len(live), 0, 0, 0.0)
            self._write_anchor(rev, live)
            self.depth = 0
        else:
            self.depth += 1
            self.snapshot_row(rev.tree, rev.parent_tree, "delta", self.depth, 0,
                              len(rev.ops), 0, 0.0)
            self.delta_rows(rev.tree, rev.ops)
        self.conn.execute("COMMIT")
        self.revisions_written += 1
        self.written.append(rev.index)
        self.record_ms.append((time.perf_counter() - t0) * 1000)

    def _walk_back(self, rev: Revision) -> tuple[str, list[str]]:
        """(anchor tree, delta trees oldest-first), following `base_tree` under §8.5's cap."""
        trees: list[str] = []
        tree, steps = rev.tree, 0
        while tree not in self.anchors and steps <= self.every + 2:
            row = self.conn.execute(
                "SELECT base_tree FROM graph_snapshot WHERE tree_oid=?", (tree,)).fetchone()
            if row is None or row[0] is None:
                break
            trees.append(tree)
            tree = row[0]
            steps += 1
        return tree, list(reversed(trees))

    def _read_anchor(self, tree: str) -> Graph:
        link = self.conn.execute(
            "SELECT chunk_sha FROM graph_chunk_link WHERE tree_oid=? AND slot=0", (tree,)).fetchone()
        manifest = self.get_blobs([link[0]]).get(link[0], b"") if link else b""
        shas = [line.split("\t")[1] for line in manifest.decode().splitlines() if line]
        payloads = self.get_blobs(shas)
        rows: list[Row] = []
        for sha in shas:
            rows.extend(decode(payloads.get(sha, b"")))
        return Graph.from_rows(rows)

    def materialize(self, rev: Revision, files: list[tuple[str, str]] = ()) -> Graph:
        t0 = time.perf_counter()
        anchor, trees = self._walk_back(rev)
        g = self._read_anchor(anchor)
        for tree in trees:
            g.apply(self.read_delta_ops(tree))
        self.materialize_ms.append((time.perf_counter() - t0) * 1000)
        return g

    def warm_step(self, current: Graph, rev: Revision) -> Graph:
        """Crossing an anchor costs an anchor read; anywhere else it costs one delta."""
        if rev.tree in self.anchors:
            return self.materialize(rev)
        t0 = time.perf_counter()
        current.apply(self.read_delta_ops(rev.tree))
        self.warm_step_ms.append((time.perf_counter() - t0) * 1000)
        return current

    def finish(self) -> None:
        super().finish()
        self.extra = {
            "anchors_written": len(self.anchors),
            "anchor_manifest_bytes": self.manifest_bytes,
            "anchor_new_payload_bytes": self.anchor_payload_bytes,
            "bytes_per_revision_without_manifest": round(
                (self.measured() - self.manifest_bytes) / max(1, self.revisions_written)),
        }

    def sql_query(self, name: str, rev: Revision, arg):
        if name != "delta_adjacent":
            return None
        t0 = time.perf_counter()
        rows = self.conn.execute(
            "SELECT op, kind, src, dst FROM graph_delta WHERE tree_oid=?", (rev.tree,)).fetchall()
        self.sql_ms.setdefault("delta_adjacent", []).append((time.perf_counter() - t0) * 1000)
        return len(rows)


# ---------------------------------------------------------------------------
# 3. CHUNKED — content-defined chunking, membership stored per revision
# ---------------------------------------------------------------------------

class Chunked(Store):
    """§4.6's `graph_chunk` + `graph_chunk_link`: one chunk per module adjacency.

    Chunk payloads are shared by content — that is where the bytes go. The *link* rows are
    not shared, one per live module per revision, which is where the rows come from and the
    finding this strategy exists to surface.
    """

    scaling = "O(changed) new chunks + O(modules) link rows per revision"
    sequential = False

    def __init__(self, root: Path, name: str):
        super().__init__(root, name)
        self.label = "CHUNKED: content-defined chunks; every revision links every chunk"

    def record(self, rev: Revision, live: dict[str, list[Row]]) -> None:
        t0 = time.perf_counter()
        self.conn.execute("BEGIN IMMEDIATE")
        self.snapshot_row(rev.tree, rev.parent_tree, "chunked", 0, len(live), 0, 0, 0.0)
        for slot, (module, rows) in enumerate(sorted(live.items())):
            payload = encode(rows)
            sha = self.put_blob("graph_chunk", payload)
            self.chunk_row(sha, "adjacency", len(payload))
            self.link_row(rev.tree, slot, sha)
        self.conn.execute("COMMIT")
        self.revisions_written += 1
        self.written.append(rev.index)
        self.record_ms.append((time.perf_counter() - t0) * 1000)

    def materialize(self, rev: Revision, files: list[tuple[str, str]] = ()) -> Graph:
        t0 = time.perf_counter()
        shas = [r[0] for r in self.conn.execute(
            "SELECT chunk_sha FROM graph_chunk_link WHERE tree_oid=? ORDER BY slot", (rev.tree,))]
        payloads = self.get_blobs(shas)
        rows: list[Row] = []
        for sha in shas:
            rows.extend(decode(payloads.get(sha, b"")))
        self.materialize_ms.append((time.perf_counter() - t0) * 1000)
        return Graph.from_rows(rows)


# ---------------------------------------------------------------------------
# 4. MERKLE — a full snapshot per revision with real structural sharing
# ---------------------------------------------------------------------------

class MerkleShared(Store):
    """The charitable reading of `e3.2`: a full snapshot, structurally shared.

    A persistent binary trie over slot ids. A slot is minted per module path ever seen, so
    adding or deleting a module costs O(log n) new nodes instead of shifting every leaf
    after it — which is what a real implementation (HAMT or B-tree keyed by path) gets and
    what a naive persistent array would not. Leaves are the same content-defined chunk
    `CHUNKED` uses; a branch holds its two children's content addresses.

    Two costs the phrase "structural sharing" does not mention, both measured here:
    the payload of a branch is 129 bytes of hex (the Merkle tax, on top of row overhead),
    and a fan-in query has to walk the whole tree because chunks are keyed by *source*, so
    `dependents(X)` costs a materialisation unless a reverse index is stored as well. The
    reverse index is reported as a projection (2x the write cost), not written twice.
    """

    scaling = "O(changed x log n) path-copied rows per revision"

    def __init__(self, root: Path, name: str, depth: int = 12):
        super().__init__(root, name)
        self.label = "MERKLE: full snapshot per revision, persistent trie, path-copied"
        self.depth = depth
        self.capacity = 1 << depth
        self.slots: dict[str, int] = {}
        self.next_slot = 0
        self.root_sha: str = EMPTY
        self.children: dict[str, tuple[str, str]] = {}
        self.notes.append("fan-in needs a full trie walk: chunks are keyed by source module")
        self.notes.append("a reverse (dst-keyed) index would double the write cost")

    def _slot(self, module: str) -> int:
        slot = self.slots.get(module)
        if slot is None:
            slot = self.next_slot
            self.next_slot += 1
            if self.next_slot > self.capacity:
                raise RuntimeError("trie full: raise MerkleShared(depth)")
            self.slots[module] = slot
        return slot

    def _put_branch(self, left: str, right: str) -> str:
        if left == EMPTY and right == EMPTY:
            return EMPTY
        payload = f"{left},{right}".encode()
        sha = hashlib.sha256(payload).hexdigest()
        if sha not in self._known_blobs:
            self.put_blob("merkle_branch", payload)
            self.chunk_row(sha, "branch", len(payload))
            self.children[sha] = (left, right)
        else:
            self.children.setdefault(sha, (left, right))
        return sha

    def _put_leaf(self, payload: bytes | None) -> str:
        if payload is None:
            return EMPTY
        sha = hashlib.sha256(payload).hexdigest()
        if sha not in self._known_blobs:
            self.put_blob("graph_chunk", payload)
            self.chunk_row(sha, "adjacency", len(payload))
        return sha

    def _update(self, sha: str, level: int, index: int, payload: bytes | None) -> str:
        if level == 0:
            return self._put_leaf(payload)
        half = 1 << (level - 1)
        left, right = self.children.get(sha, (EMPTY, EMPTY))
        if index < half:
            left = self._update(left, level - 1, index, payload)
        else:
            right = self._update(right, level - 1, index - half, payload)
        return self._put_branch(left, right)

    def record(self, rev: Revision, live: dict[str, list[Row]]) -> None:
        t0 = time.perf_counter()
        self.conn.execute("BEGIN IMMEDIATE")
        root = self.root_sha
        for module in rev.removed:
            if module in self.slots:
                root = self._update(root, self.depth, self.slots[module], None)
        if rev.index == 0:
            for module, rows in live.items():
                root = self._update(root, self.depth, self._slot(module), encode(rows))
        else:
            for module, payload in rev.new_rows:
                root = self._update(root, self.depth, self._slot(module), payload)
        self.snapshot_row(rev.tree, rev.parent_tree, "chunked", 0, len(live), 0, 0, 0.0)
        if root != self.root_sha and root != EMPTY:
            self.link_row(rev.tree, 0, root)
            self.root_sha = root
        self.conn.execute("COMMIT")
        self.revisions_written += 1
        self.written.append(rev.index)
        self.record_ms.append((time.perf_counter() - t0) * 1000)

    def materialize(self, rev: Revision, files: list[tuple[str, str]] = ()) -> Graph:
        """Level-order walk, one batched query per trie level, then decode the leaves."""
        t0 = time.perf_counter()
        link = self.conn.execute(
            "SELECT chunk_sha FROM graph_chunk_link WHERE tree_oid=? AND slot=0",
            (rev.tree,)).fetchone()
        rows: list[Row] = []
        frontier = [link[0]] if link and link[0] != EMPTY else []
        while frontier:
            nxt: list[str] = []
            for _sha, kind, payload in self.get_nodes(frontier):
                if kind == "branch":
                    left, right = payload.decode().split(",")
                    if left != EMPTY:
                        nxt.append(left)
                    if right != EMPTY:
                        nxt.append(right)
                else:
                    rows.extend(decode(payload))
            frontier = nxt
        self.materialize_ms.append((time.perf_counter() - t0) * 1000)
        return Graph.from_rows(rows)


# ---------------------------------------------------------------------------
# 5. PER_FILE — no per-revision graph; recompute from the analyzer cache
# ---------------------------------------------------------------------------

class PerFileRecompute(Store):
    """On-demand recomputation: §4.5's `cache`+`blob` pair keyed by *blob*, nothing per revision.

    The substrate is what `e1.4`'s incremental cache already holds: one row per
    (analyzer, blob) with the structural facts that blob implies. A revision's graph is
    then a tree read plus one cache lookup per file, which is what this strategy is judged
    on: the cold case is O(files) and the interactive case (cursor moved one commit) is
    O(changed files), which is why they are measured separately.
    """

    scaling = "O(1) per revision; O(files) only when a revision is asked about"

    def __init__(self, root: Path, name: str):
        super().__init__(root, name)
        self.label = "PER_FILE: per-file rows keyed by blob, revision graph rebuilt on demand"
        self.seen: set[str] = set()
        self.substrate_ms = 0.0
        self.conn.execute(
            "INSERT OR IGNORE INTO analyzer(id,version,schema_version,scope_kind,config_keys,"
            "depends_on,source_paths) VALUES(?,?,?,?,?,?,?)",
            ("structure.per_file", 1, 1, "blob", "[]", "[]", '["crates/fathom-analyze/**"]'))

    def record(self, rev: Revision, live: dict[str, list[Row]]) -> None:
        # Nothing per revision. What is written is per distinct blob, which is the analyzer
        # cache and is charged in `ensure_substrate` — it is still graph-lane storage.
        self.revisions_written += 1
        self.written.append(rev.index)

    def ensure_substrate(self, structures: dict[str, list[Row]]) -> float:
        """One `cache` + `blob` pair per distinct blob's structural facts. Returns ms."""
        t0 = time.perf_counter()
        self.conn.execute("BEGIN IMMEDIATE")
        for blob_oid, rows in structures.items():
            if blob_oid in self.seen:
                continue
            payload = encode(rows)
            sha = self.put_blob("structure_per_file", payload)
            key = "ck1:" + hashlib.sha256(f"structure.per_file\t{blob_oid}".encode()).hexdigest()
            self.conn.execute(
                "INSERT OR IGNORE INTO cache(cache_key,analyzer_id,input_kind,input_hash,"
                "analyzer_version,dep_digest,config_digest,result_sha,result_bytes,error,"
                "built_utc,last_used_utc,hits,cost_ms,epoch) "
                "VALUES(?,?,'blob',?,1,'d','c',?,?,NULL,0,0,0,0,1)",
                (key, "structure.per_file", blob_oid, sha, len(payload)))
            self.rows_written += 1
            self.seen.add(blob_oid)
        self.conn.execute("COMMIT")
        self.substrate_ms = (time.perf_counter() - t0) * 1000
        self.notes.append(f"substrate: {len(self.seen)} blob-scoped cache rows in "
                          f"{round(self.substrate_ms)} ms "
                          f"({self.substrate_ms / max(1, len(structures)):.3f} ms each)")
        return self.substrate_ms

    def materialize(self, rev: Revision, files: list[tuple[str, str]]) -> Graph:
        """`files` is the revision's whole (path, blob oid) list: what `git ls-tree -r` gives."""
        t0 = time.perf_counter()
        oids = [oid for _path, oid in files]
        rows: list[Row] = []
        for start in range(0, len(oids), 900):
            part = oids[start:start + 900]
            marks = ",".join("?" * len(part))
            for payload, in self.conn.execute(
                    f"SELECT b.data FROM cache c JOIN blob b ON b.sha256=c.result_sha "
                    f"WHERE c.analyzer_id='structure.per_file' AND c.input_hash IN ({marks})",
                    part):
                rows.extend(decode(bytes(payload)))
        self.materialize_ms.append((time.perf_counter() - t0) * 1000)
        return Graph.from_rows(rows)

    def warm_step(self, current: Graph, rev: Revision) -> Graph:
        """The interactive case for this policy: apply the one commit's ops, no tree read."""
        t0 = time.perf_counter()
        current.apply(rev.ops)
        self.warm_step_ms.append((time.perf_counter() - t0) * 1000)
        return current
