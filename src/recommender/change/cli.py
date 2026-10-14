"""`fathom change …` — the verb surface of `e2.1`.

The Rust product exposes this from `fathom-cli` (verbs parse, build a context, dispatch,
return an exit code — ADR-002 §4.3, §7.3); the executable form in this repository is:

    python3 -m recommender.change explain <path> …        # receipts for one path
    python3 -m recommender.change episode --manifest m.json
    python3 -m recommender.change score --min 0.95        # the AC 1 gate, run in CI
    python3 -m recommender.change classes
    python3 -m recommender.change markers
    python3 -m recommender.change bench

The flags for finding `fathom.toml` are `e0.8`'s verbatim, and the taxonomy is built the same
way `recommender.taxonomy` builds it, so `fathom change explain` and `fathom taxonomy explain`
cannot disagree about which rules are in force. Reading *content* is this verb's own job:
`--repo DIR` reads each path out of that working tree (bounded by `MAX_BLOB_BYTES`), which is
the only filesystem access anywhere in the package — everything below `cli.py` is pure.

A manifest is the shape an episode already has in JSON, and the shape `e2.11` will hand to
consumers:

    {"id": "ep-1", "title": "…", "changes":
        [{"path": "src/a.py", "content": "…", "change": "modified"},
         {"path": "old/b.py", "change": "deleted"},
         {"path": "assets/x.png", "content_base64": "…"}]}
"""

from __future__ import annotations

import argparse
import base64
import json
import sys
from pathlib import Path

from recommender.config.cli import common_flags
from recommender.config.errors import ConfigError, EXIT_CODES
from recommender.config.layer import load as load_config
from recommender.taxonomy import Taxonomy

from . import explain as X
from .cache import BlobCache
from .corpus import counts, rows
from .engine import Change, ChangeClassifier, CLASSIFIER_VERSION, default_classifier
from .episode import Episode, classify_episode
from .eval import MIN_ACCURACY, report_text, score
from .markers import MARKERS, MARKERS_VERSION, digest as markers_digest

#: Reading a 400 MB committed blob to classify it would be the wrong trade; `e1.2`'s size
#: caps exist for the same reason. Past this the classifier sees "unavailable" and says so.
MAX_BLOB_BYTES = 4 * 1024 * 1024


def build_parser() -> argparse.ArgumentParser:
    common = common_flags()
    ap = argparse.ArgumentParser(
        prog="fathom change", parents=[common],
        description="Change classifier (e2.1 / reco-dsh): what every changed path in an "
                    "episode is, with a confidence and a reason.")
    sub = ap.add_subparsers(dest="command", required=True)

    p = sub.add_parser("explain", parents=[common],
                       help="classify paths and print the evidence for each")
    p.add_argument("path", nargs="+")
    p.add_argument("--content", action="store_true",
                   help="read each path's content from --repo (default: yes if --repo is set)")
    p.add_argument("--path-only", action="store_true",
                   help="path rules alone: pretend no content is available")
    p.add_argument("--quiet", "-q", action="store_true", help="one line per path")
    p.add_argument("--json", action="store_true")
    p.add_argument("--ui", action="store_true", help="print the badge/detail block a pane gets")

    p = sub.add_parser("episode", parents=[common], help="classify a whole episode")
    p.add_argument("--manifest", type=Path, default=None, help="JSON change set (see the verb help)")
    p.add_argument("--path", dest="paths", nargs="*", default=[],
                   help="paths to classify from --repo, when there is no manifest")
    p.add_argument("--show", type=int, default=0, help="also print the first N rows of the table")
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("score", parents=[common], help="accuracy on the labelled fixture set")
    p.add_argument("--min", type=float, default=MIN_ACCURACY)
    p.add_argument("--with-config", action="store_true",
                   help="score against this repository's resolved taxonomy instead of the "
                        "built-in table the labels are written against")
    p.add_argument("--verbose", "-v", action="store_true")
    p.add_argument("--json", action="store_true")

    sub.add_parser("classes", help="the twelve classes and what each one means")
    sub.add_parser("markers", parents=[common],
                   help="the content-marker table with its version and digest")

    p = sub.add_parser("bench", parents=[common], help="cost per file and cache hit rate")
    p.add_argument("--repeat", type=int, default=4)
    p.add_argument("--json", action="store_true")
    return ap


def _taxonomy(args) -> Taxonomy:
    env = {} if getattr(args, "no_env", False) else None
    config = load_config(getattr(args, "repo", None), repo_file=getattr(args, "repo_file", None),
                         user_file=getattr(args, "user_file", None), env=env,
                         require_repo=False, sets=getattr(args, "set", []),
                         extends=getattr(args, "extend", []),
                         offline=getattr(args, "offline", False))
    return Taxonomy.from_config(config), config


def _classifier(args) -> ChangeClassifier:
    taxonomy, config = _taxonomy(args)
    return ChangeClassifier(taxonomy, config=config)


def _read(repo, path: str) -> str | None:
    """One file's content from a working tree, or None. The package's only file read."""
    if repo is None:
        return None
    candidate = Path(repo) / path
    try:
        if not candidate.is_file():
            return None
        return candidate.read_bytes()[:MAX_BLOB_BYTES].decode("utf-8", "replace")
    except OSError:
        return None


def main(argv=None) -> int:
    args = build_parser().parse_args(argv)
    try:
        return _dispatch(args)
    except ConfigError as exc:
        print("fathom change: %s" % exc.render(), file=sys.stderr)
        return exc.exit_code
    except (OSError, ValueError) as exc:                       # pragma: no cover
        print("fathom change: %s" % exc, file=sys.stderr)
        return EXIT_CODES["ERROR"]


def _dispatch(args) -> int:
    if args.command == "classes":
        sys.stdout.write(X.vocabulary_text())
        return 0

    if args.command == "score":
        # The labelled set is written against the built-in table — the same rule `e0.9`'s
        # corpus follows — so a local `fathom.toml` can never move the accuracy number.
        # `--with-config` scores the resolved table on purpose, to show what a repository's
        # own assertions do to the answers; it is a diagnostic, not the gate.
        classifier = _classifier(args) if args.with_config else default_classifier()
        report = score(classifier)
        if args.json:
            print(json.dumps(report.as_dict(), indent=2, sort_keys=True))
        else:
            sys.stdout.write(report_text(report, minimum=args.min, verbose=args.verbose))
        return 0 if report.accuracy >= args.min else EXIT_CODES["ERROR"]

    if args.command == "markers":
        return _markers(args)

    classifier = _classifier(args)

    if args.command == "explain":
        repo = getattr(args, "repo", None)
        use_content = not args.path_only
        decisions = []
        for path in args.path:
            content = _read(repo, path) if (use_content and repo) else None
            decisions.append(classifier.classify(Change(path=path, content=content)))
        if args.json:
            print(json.dumps({"classifier_version": CLASSIFIER_VERSION,
                              "taxonomy_version": classifier.taxonomy.version,
                              "table_digest": classifier.taxonomy.digest,
                              "markers_version": MARKERS_VERSION,
                              "decisions": [d.as_dict() for d in decisions],
                              "cache": classifier.cache.stats()},
                             indent=2, sort_keys=True))
        elif args.ui:
            for decision in decisions:
                sys.stdout.write(X.ui_block_text(decision))
        elif args.quiet:
            sys.stdout.write(X.table_text(decisions))
        else:
            sys.stdout.write(X.explain_text(decisions))
        return 0

    if args.command == "episode":
        episode = _episode(args)
        result = classify_episode(episode, classifier)
        if args.json:
            print(json.dumps(result.as_dict(), indent=2, sort_keys=True))
        else:
            sys.stdout.write(X.episode_text(result, show=args.show))
        return 0

    if args.command == "bench":
        return _bench(args, classifier)

    return EXIT_CODES["USAGE"]


def _episode(args) -> Episode:
    repo = getattr(args, "repo", None)
    if args.manifest:
        payload = json.loads(Path(args.manifest).read_text())
        changes = []
        for item in payload.get("changes", []):
            content = item.get("content")
            if content is None and item.get("content_base64"):
                content = base64.b64decode(item["content_base64"])
            if content is None and repo:
                content = _read(repo, item.get("path", ""))
            changes.append(Change(path=item.get("path", ""), content=content,
                                  change_type=item.get("change", "modified"),
                                  old_path=item.get("from", "")))
        return Episode(id=payload.get("id", "episode"), title=payload.get("title", ""),
                       changes=tuple(changes))
    changes = tuple(Change(path=path, content=_read(repo, path)) for path in args.paths)
    return Episode(id="working-tree", changes=changes)


def _markers(args) -> int:
    print("content-marker table v%s · digest %s · %d markers · head window %s bytes / %s lines"
          % (MARKERS_VERSION, markers_digest(), len(MARKERS), 4096, 40))
    print("")
    for marker in MARKERS:
        print("%-16s %-10s %.2f %-9s %s" % (marker.id, marker.cls, marker.weight,
                                             "decisive" if marker.decisive() else "suggestive",
                                             marker.note))
    return 0


def _bench(args, classifier: ChangeClassifier) -> int:
    """Cost per changed file, cold and cached, on the fixture corpus."""
    import time

    fixtures = [(path, content) for _shape, path, _cls, content in rows()]
    reps = max(1, args.repeat)
    cold = ChangeClassifier(classifier.taxonomy, cache=BlobCache(), enable_cache=False)
    start = time.perf_counter()
    for _ in range(reps):
        for path, content in fixtures:
            cold.classify_path(path, content)
    cold_us = (time.perf_counter() - start) / (reps * len(fixtures)) * 1e6

    start = time.perf_counter()
    for _ in range(reps):
        for path, content in fixtures:
            classifier.classify_path(path, content)
    warm_us = (time.perf_counter() - start) / (reps * len(fixtures)) * 1e6

    out = {"paths": len(fixtures), "lookups": reps * len(fixtures),
           "cold_us_per_file": round(cold_us, 2), "cached_us_per_file": round(warm_us, 2),
           "cache": classifier.cache.stats(),
           "corpus_classes": sum(counts().values()),
           "classifier_version": CLASSIFIER_VERSION, "markers_version": MARKERS_VERSION}
    if args.json:
        print(json.dumps(out, indent=2, sort_keys=True))
        return 0
    print("fathom change bench · %d fixture paths, %d lookups" % (out["paths"], out["lookups"]))
    print("  classify (cold)     %7.2f µs/file  → 1 000 changed files in %.0f ms"
          % (out["cold_us_per_file"], out["cold_us_per_file"]))
    print("  classify (cached)   %7.2f µs/file" % out["cached_us_per_file"])
    print("  cache               %s" % out["cache"])
    return 0


if __name__ == "__main__":                                   # pragma: no cover
    sys.exit(main())
