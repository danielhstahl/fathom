Polished by halogen-qwen3.8-flash-next; every figure below is cited to the delta document.

Indexing became incremental, keyed on a tree-hash analysis cache, for e1.4 [c1]. One commit, 45 files, +50 752 -56 [m3].

**What changed** - 50 808 lines changed, 50 722 of them reviewable with 115 discounted [f3 f4 f20]. Forty-five paths modified, no moves recorded [f6 f10].

**Where** - one golden file under crates/fathom-ingest/tests/golden is 1f +36 949 -0 by itself; the ingest source took 10f +3 562 -14 and its tests 9f +3 081 -25, with 13 directories not listed [f7 f8].

**What it can break** - risk spine-1 62 = surfaces 36 + size 16 + breadth 10, with the size term at its cap [f18 f20]. 82 % of the churn is test-side: 14 files, +41 851 -25 [f14 f16]. The delta itself warns that source grew by 8 704 while 25 test lines were removed and nothing in these rows re-establishes that cover [f17]. Cargo.lock is the one operator surface [f13 f19].

**Read first** - Cargo.lock, the file the surfaces term names [f19].

**Not answered** - the shape sentence [g2], dependencies [g3], operator findings [g4], case names [g5], blast radius [g6] and ranking beyond spine-1 [g7].
