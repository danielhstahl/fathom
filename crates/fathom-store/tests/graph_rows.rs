//! The graph tables themselves: the node dictionary, the delta chain, and the three schema findings
//! `s3` paid for (`reco-8s4`, migration 8).
//!
//! The analyzer's `tests/graph_lane.rs` drives these through the lane, which is how a caller uses
//! them. This file writes them directly, because two of the three findings are invisible from above:
//! a `dst = ''` that a later migration could "tidy", and a foreign key that only fails when the
//! manifest is linked by hand. The third — a tree written twice — is a store property before it is a
//! lane policy.

use fathom_store::graph::{
    ANCHOR_SLOT, CHUNK_KIND_ADJACENCY, CHUNK_KIND_MANIFEST, ChainPartial, DeltaOp, GraphBytes,
    NodeEntry, Op, SnapshotRow,
};
use fathom_store::spine::Store;

/// The instant every row here is stamped with.
const NOW: i64 = 2_000_000_000_000;

/// A module dictionary entry, spelled the short way.
fn module(key: &str, id: i64) -> NodeEntry {
    NodeEntry::module(key, Some(id), "l0".to_string(), false)
}

/// A delta op, spelled the short way: `(op, kind, src, dst)`.
fn op(add: bool, kind: &str, src: i64, dst: i64) -> DeltaOp {
    DeltaOp {
        op: if add { Op::Add } else { Op::Remove },
        kind: kind.to_string(),
        src: src.to_string(),
        dst: if kind == "module" || kind == "symbol" {
            String::new()
        } else {
            dst.to_string()
        },
        src_layer: "l0".to_string(),
        dst_layer: "l0".to_string(),
    }
}

#[test]
fn migration_eight_creates_the_graph_tables_and_leaves_them_empty() {
    let store = Store::open_in_memory().expect("open store");
    let counts = store.graph_row_counts().expect("counts");
    for table in [
        "graph_snapshot",
        "graph_delta",
        "graph_chunk",
        "graph_chunk_link",
        "graph_node",
    ] {
        assert_eq!(
            counts.get(table),
            Some(&0),
            "{table} should exist and be empty; counts: {counts:?}"
        );
    }
}

#[test]
fn the_dictionary_mints_once_never_moves_an_id_and_refreshes_its_tags() {
    // An id that moved would silently change what every stored chunk means, so "never moves" is the
    // property the whole delta chain rests on. The tags are the opposite: a re-intern is exactly the
    // moment the lane knows something new about a vertex — a `[layers]` edit, a module that grew a
    // macro scope — and a stale tag would be a layering answer that quietly went out of date.
    let mut store = Store::open_in_memory().expect("open store");
    let first = store
        .graph_intern(&[module("core/a.rs", 0), module("core/b.rs", 1)])
        .expect("intern");
    assert_eq!(first.get("core/a.rs"), Some(&0));
    assert_eq!(first.get("core/b.rs"), Some(&1));

    let again = store
        .graph_intern(&[NodeEntry::module(
            "core/a.rs",
            Some(7),
            "l2".to_string(),
            true,
        )])
        .expect("re-intern");
    assert_eq!(
        again.get("core/a.rs"),
        Some(&0),
        "the id the dictionary already issued wins over the one the caller proposed"
    );
    let nodes = store.graph_nodes().expect("nodes");
    let a = nodes
        .values()
        .find(|n| n.key == "core/a.rs")
        .expect("there");
    assert_eq!(a.layer, "l2", "the tag is refreshed");
    assert!(a.incomplete, "and so is the incompleteness flag");
    assert_eq!(
        nodes
            .values()
            .find(|n| n.key == "core/b.rs")
            .expect("there")
            .id,
        1,
        "and the untouched entry did not move"
    );
}

#[test]
fn a_node_op_is_stored_with_an_empty_object_because_the_primary_key_leaves_no_other_spelling() {
    // `s3` finding 1, at the row level: `graph_delta.dst` is in the primary key and not nullable, so
    // "this row adds a node" has to spell its missing object as the empty string. Asserted here so a
    // later migration that "normalised" it by repeating the id would fail rather than double the
    // meaning of every node row.
    let mut store = Store::open_in_memory().expect("open store");
    store
        .graph_write_snapshot(&SnapshotRow::anchor("tree0"))
        .expect("snapshot");
    store
        .graph_write_ops(
            "tree0",
            &[op(true, "module", 0, 0), op(true, "imports", 0, 1)],
        )
        .expect("ops");
    let ops = store.graph_ops("tree0").expect("ops");
    let node = ops.iter().find(|row| row.kind == "module").expect("there");
    assert_eq!(node.dst, "", "a unary row's object slot is empty");
    assert_eq!(
        ops.iter().find(|r| r.kind == "imports").expect("there").dst,
        "1"
    );

    // And the same row is found from either endpoint, which is "what did this commit do to X".
    assert_eq!(
        store
            .graph_ops_touching("tree0", 0)
            .expect("touching")
            .len(),
        2,
        "node 0 is the source of both rows"
    );
    assert_eq!(
        store
            .graph_ops_touching("tree0", 1)
            .expect("touching")
            .len(),
        1,
        "node 1 only appears as an object"
    );
}

#[test]
fn writing_a_tree_twice_is_counted_and_the_first_row_survives_intact() {
    // `s3` finding 3. A merge commit and the commit it merges reach one tree; `INSERT OR REPLACE`
    // would rewrite `base_tree` and leave the chain unreachable from itself, which is a corrupt store
    // that answers every query normally. So the write returns "already there" and changes nothing.
    let mut store = Store::open_in_memory().expect("open store");
    let mut first = SnapshotRow::delta("tree1", "tree0", 1);
    first.node_count = 10;
    first.built_utc = NOW;
    assert!(
        store.graph_write_snapshot(&first).expect("write"),
        "the first write is a write"
    );
    let mut second = SnapshotRow::delta("tree1", "tree9", 7);
    second.node_count = 99;
    second.built_utc = NOW + 100;
    assert!(
        !store.graph_write_snapshot(&second).expect("duplicate"),
        "the second is reported as a duplicate"
    );
    let kept = store
        .graph_snapshot("tree1")
        .expect("read")
        .expect("still there");
    assert_eq!(
        kept.base_tree.as_deref(),
        Some("tree0"),
        "the chain kept its parent"
    );
    assert_eq!(kept.node_count, 10, "and its own counts");
    assert_eq!(kept.built_utc, NOW, "and when it was written");
}

#[test]
fn an_anchor_manifest_cannot_be_linked_without_a_chunk_row_of_its_own() {
    // `s3` finding 2, as the failure that taught it: `graph_chunk_link`'s foreign key reaches
    // `graph_chunk`, not `blob`, so a manifest that is only a blob cannot be linked into an anchor.
    let mut store = Store::open_in_memory().expect("open store");
    store
        .graph_write_snapshot(&SnapshotRow::anchor("tree0"))
        .expect("snapshot");
    let orphan = "0".repeat(64);
    let refused = store.graph_link_chunk("tree0", ANCHOR_SLOT, &orphan);
    assert!(
        refused.is_err(),
        "linking a chunk nobody registered must be loud, not a manifest that reads as empty"
    );
    store
        .graph_write_chunk(&orphan, CHUNK_KIND_MANIFEST, 12)
        .expect("chunk row");
    store
        .graph_link_chunk("tree0", ANCHOR_SLOT, &orphan)
        .expect("link now resolves");
    assert_eq!(
        store
            .graph_chunk_links("tree0")
            .expect("links")
            .into_iter()
            .map(|(slot, _)| slot)
            .collect::<Vec<_>>(),
        vec![ANCHOR_SLOT],
        "and exactly one manifest is linked into an anchor"
    );
}

#[test]
fn a_chunk_written_twice_is_one_row_because_content_is_the_address() {
    let mut store = Store::open_in_memory().expect("open store");
    let sha = "a".repeat(64);
    store
        .graph_write_chunk(&sha, CHUNK_KIND_ADJACENCY, 40)
        .expect("first");
    store
        .graph_write_chunk(&sha, CHUNK_KIND_ADJACENCY, 40)
        .expect("second");
    let bytes = store.graph_payload_bytes().expect("bytes");
    assert_eq!(
        bytes.chunk_rows, 1,
        "the sharing rate is the whole reason an anchor is cheap to repeat"
    );
}

#[test]
fn a_chain_that_reaches_no_anchor_inside_the_cap_refuses_instead_of_guessing() {
    let mut store = Store::open_in_memory().expect("open store");
    for index in 0_usize..4 {
        let oid = format!("tree{index}");
        let base = format!("tree{}", index.saturating_sub(1));
        let row = if index == 0 {
            SnapshotRow::anchor(&oid)
        } else {
            SnapshotRow::delta(&oid, &base, i64::try_from(index).unwrap())
        };
        store.graph_write_snapshot(&row).expect("snapshot");
    }
    let walked = store.graph_chain_walk("tree3", 8).expect("walk");
    assert_eq!(walked.anchor, "tree0");
    assert_eq!(walked.deltas, vec!["tree1", "tree2", "tree3"]);

    let Err(err) = store.graph_chain_walk("tree3", 2) else {
        panic!("a cap of two steps cannot reach the only anchor, and must say so");
    };
    let partial = match err {
        fathom_store::graph::GraphWalkError::Partial(partial) => partial,
        other => panic!("expected the stated partial, got {other:?}"),
    };
    assert_eq!(partial.code(), "graph.chain.too_long");
    assert!(
        partial.sentence().contains("anchor is missing"),
        "{}",
        partial.sentence()
    );

    // A dangling parent is the other shape of the same refusal.
    store
        .graph_write_snapshot(&SnapshotRow::delta("treeX", "treeGone", 1))
        .expect("snapshot");
    let Err(err) = store.graph_chain_walk("treeX", 8) else {
        panic!("a base_tree that names nothing is a lost anchor");
    };
    let lost = match err {
        fathom_store::graph::GraphWalkError::Partial(ChainPartial::AnchorLost {
            tree_oid, ..
        }) => tree_oid,
        other => panic!("expected the anchor-lost shape, got {other:?}"),
    };
    assert_eq!(lost, "treeGone", "named as the store saw it");
}

#[test]
fn the_byte_account_counts_rows_chunks_manifests_and_dictionary_separately() {
    let mut store = Store::open_in_memory().expect("open store");
    store
        .graph_intern(&[module("core/a.rs", 0)])
        .expect("intern");
    store
        .graph_write_snapshot(&SnapshotRow::anchor("tree0"))
        .expect("snapshot");
    store
        .graph_write_ops("tree0", &[op(true, "module", 0, 0)])
        .expect("ops");
    let bytes: GraphBytes = store.graph_payload_bytes().expect("bytes");
    assert_eq!(bytes.delta_rows, 1);
    assert_eq!(bytes.dictionary_rows, 1);
    assert!(
        bytes.delta_payload >= 12,
        "row text plus a fixed per-row allowance: {bytes:?}"
    );
    assert_eq!(bytes.chunk_rows, 0, "nothing chunked here");
    assert!(
        bytes.sentence().contains("counted as payload"),
        "and the basis travels with the number: {}",
        bytes.sentence()
    );
}
