"""Spike `s4` (`reco-gj6`): how much can rename and copy pairing be trusted?

Nothing else in Epic 2 depends this directly on a *guess*. `e2.3` tells a reviewer which parts of the
tree are in motion, and "the router moved" and "the router was deleted and something else appeared"
are different stories about the same commit — one of them is a lie the reviewer can act on. `e2.4`
(`reco-a0v`) proposes to pair paths with git's rename detection plus a content-similarity matcher and
to give each pairing a confidence; `e0.12` refused to let that 90 % bar stand as a guess written before
anybody measured (`reco-tx0.1`).

This package is that measurement:

* :mod:`recommender.rename.content` — deterministic file bodies, so a fixture's *similarity* is an
  authored quantity rather than an accident of the generator;
* :mod:`recommender.rename.repo` — a real `git` repository writer (branches, merges, rebases) and the
  window semantics the product uses to ask "what changed between these two revisions";
* :mod:`recommender.rename.specs` — the fixture set: twelve authored histories, each with the cases
  `s4` names — rename-with-rewrite, split, merge, copy, a move that crosses a merge, a move introduced
  by a rebase — and the decoys that make a matcher confident about the wrong thing;
* `truth/<scenario>.json` — the hand-authored pair expectations, written by a person and never
  computed by a detector (the rule `fathom-fixtures` exists to enforce);
* :mod:`recommender.rename.detect` — the three candidates: git's own detection at several thresholds,
  a shingle/minhash content matcher, and a hybrid that uses git as a floor;
* :mod:`recommender.rename.score` — precision, recall, and the asymmetry the ticket asks for: a wrong
  pairing is a stated falsehood, a missing pairing is only noise;
* :mod:`recommender.rename.spike` — the driver; run it with
  ``PYTHONPATH=src python3 -m recommender.rename.spike --report benchmarks/reports/s4-rename-detection.json``
  and re-measure with ``--check``.
"""
