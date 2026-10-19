# Fathom review ledger

<!-- format: fathom-review/1 -->
<!--
     One block per human decision, appended oldest first. This file is the durable record:
     the index in .fathom/index.sqlite3 is a rebuildable mirror of it, and deleting the store
     loses no review.

     Do not edit, reorder or re-wrap a block that has been committed. The `digest:` line at the
     end of each block hashes the lines above it, so an edit is detectable and `fathom review
     verify` will name it. To correct a decision, append a new block saying so.

     `members:` are the commits the decision covered, `content:` is the digest of the file
     contents it covered, and `fathom review status` re-checks both against the current history.
-->

## sg-c0b942f5ea096692 | accepted | 2033-05-18T03:33:20Z | daniel <daniel@example.invalid>

- episode: ep-0d0ab15e5d63d45e397a
- decision: accepted
- actor: daniel <daniel@example.invalid>
- actor-kind: human
- signed: 2033-05-18T03:33:20Z
- fingerprint: 0d0ab15e5d63d45e397abd3e79332789e56cba42c5e5a2f593c0c26827d3cf73
- fallback: e35933d820174d706bfa21cf9803e84721e6b1aa75df1bfa960d4cc0a1c52554
- shape: 0908cfd5a8377531a1451fa6d8dbd8e7be083ce6cbe0571169d3df82ad78cf54
- content: 4a995d340b80afca79805f15bb623524dbf9b52e5b4d3083532f1e215b33ced4
- members: f9fc98bd
- paths: README.md@c0263bd18f6f
- scale: 1 commit(s), 1 file(s), +1 -0
- note: looked it over
- digest: sha256:cbbf53e6dddce557e618c7217b2c92c00029a8d1d524e66b8b1b5bc96bdaf2af

## sg-0e3a5a1aa5ab1857 | accepted | 2033-05-18T03:33:20Z | daniel <daniel@example.invalid>

- episode: ep-1adaccd586db858a2f0e
- decision: accepted
- actor: daniel <daniel@example.invalid>
- actor-kind: human
- signed: 2033-05-18T03:33:20Z
- fingerprint: 1adaccd586db858a2f0e4f70c8770049ec06756d88de1ce28b9cc0742b86d1e1
- fallback: 594b8002aa545b3a66c123714f0e240b14dd631e8c4562ab47a544de30106666
- shape: 48b04224964275b93bbc73fec3d36f8005b971f2074a8693dd4efaef1545df57
- content: 3384fb785d40243a9809b8fabb377bc63a996f9333c6ea3aaf86a4a33f4c51e1
- members: 5ff51737, 2e6dad08, 99bf6dd7
- paths: src/store.rs@b7564d28e314, tests/src/store.rs@09d4352e8bfc
- scale: 3 commit(s), 2 file(s), +3 -1
- note: looked it over
- digest: sha256:ce0636239ec4487052d886506eb7a71621179df834a7e0db24559b4134bec317
