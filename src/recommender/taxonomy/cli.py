"""`fathom explain <path>` and `fathom taxonomy …` — the verb surface of `e0.9`.

The Rust product exposes `fathom explain <path>` from `fathom-cli` (verbs parse, build a
context, dispatch, return an exit code — ADR-002 §4.3, §7.3). In this repository the
executable form of the same surface is:

    python3 -m recommender.taxonomy explain <path> [<path> …]
    python3 -m recommender.taxonomy classes
    python3 -m recommender.taxonomy rules --class generated
    python3 -m recommender.taxonomy share <path> …
    python3 -m recommender.taxonomy bench

It reuses the config verb's discovery flags verbatim (`--repo`, `--set`, `--extend`,
`-c`, `--no-env`): a taxonomy question is a config question two steps downstream, and a
second set of flags for finding `fathom.toml` would be a second opinion about where the
configuration lives — the thing `e0.8` exists to prevent.

The engine never reads a file (ADR-002 §4.5: only the CLI half does). This module is
where `fathom.toml` is loaded and handed to `Taxonomy.from_config` as a value, which is
also why the verb prints the *resolved* table digest: two runs that disagree about a path
can be shown to disagree about a file.
"""

from __future__ import annotations

import argparse
import json
import sys
import time

from recommender.config.cli import common_flags
from recommender.config.errors import ConfigError, EXIT_CODES
from recommender.config.layer import load as load_config

from . import classes as C
from . import explain as X
from .builtins import BUILTINS_VERSION
from .engine import Taxonomy, default_taxonomy


def build_parser() -> argparse.ArgumentParser:
    common = common_flags()
    ap = argparse.ArgumentParser(
        prog="fathom taxonomy", parents=[common],
        description="Path taxonomy and noise policy (e0.9 / reco-5vz): what a path is, "
                    "how noisy it is, and which rule decided.")
    sub = ap.add_subparsers(dest="command", required=True)

    p = sub.add_parser("explain", parents=[common],
                       help="which rule matched a path, which rule it beat, and why")
    p.add_argument("path", nargs="+", help="one or more repository-relative paths")
    p.add_argument("--quiet", "-q", action="store_true",
                   help="one line per path (class, noise, rule, layer)")
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("rules", parents=[common], help="print the effective rule table")
    p.add_argument("--class", dest="cls", default=None, choices=list(C.PATH_CLASSES))
    p.add_argument("--layer", default=None)
    p.add_argument("--pattern", default="", help="substring filter on the patterns")

    sub.add_parser("classes", parents=[common],
                   help="the class and noise vocabularies, with their meanings")

    p = sub.add_parser("share", parents=[common],
                       help="class and noise breakdown of a change set")
    p.add_argument("path", nargs="+")
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("bench", parents=[common],
                       help="measure lookups per second on this machine")
    p.add_argument("--repeat", type=int, default=200)
    p.add_argument("--json", action="store_true")
    return ap


def _taxonomy(args) -> Taxonomy:
    """Resolve the configuration, then build the taxonomy from it. No other IO happens."""
    env = {} if getattr(args, "no_env", False) else None
    config = load_config(getattr(args, "repo", None), repo_file=getattr(args, "repo_file", None),
                         user_file=getattr(args, "user_file", None), env=env,
                         require_repo=False, sets=getattr(args, "set", []),
                         extends=getattr(args, "extend", []),
                         offline=getattr(args, "offline", False))
    return Taxonomy.from_config(config)


def main(argv=None) -> int:
    args = build_parser().parse_args(argv)
    try:
        return _dispatch(args)
    except ConfigError as exc:
        print("fathom taxonomy: %s" % exc.render(), file=sys.stderr)
        return exc.exit_code
    except FileNotFoundError as exc:                       # pragma: no cover
        print("fathom taxonomy: %s" % exc, file=sys.stderr)
        return EXIT_CODES["ERROR"]


def _dispatch(args) -> int:
    if args.command == "classes":
        sys.stdout.write(X.classes_text())
        return 0

    taxonomy = default_taxonomy() if args.command == "bench" else _taxonomy(args)

    if args.command == "explain":
        verdicts = [taxonomy.classify(p) for p in args.path]
        if args.json:
            print(json.dumps({"taxonomy_version": taxonomy.version,
                              "table_digest": taxonomy.digest,
                              "decisions": [v.as_dict() for v in verdicts],
                              "warnings": [w.render() for w in taxonomy.warnings]},
                             indent=2, sort_keys=True))
        else:
            sys.stdout.write(X.explain_text(taxonomy, verdicts, quiet=args.quiet))
        return 0

    if args.command == "rules":
        sys.stdout.write(X.rules_text(taxonomy, cls=args.cls, pattern=args.pattern,
                                      layer=args.layer))
        return 0

    if args.command == "share":
        verdicts = [taxonomy.classify(p) for p in args.path]
        payload = {"paths": len(verdicts),
                   "noise_share": round(taxonomy.noise_share(args.path), 4),
                   "classes": taxonomy.summary(args.path),
                   "noise": taxonomy.noise_summary(args.path)}
        if args.json:
            print(json.dumps(payload, indent=2, sort_keys=True))
            return 0
        print(X.SUMMARY_HEADER)
        for verdict in verdicts:
            print(X.summary_line(verdict))
        print("\nnoise share: %.3f over %d path%s"
              % (payload["noise_share"], len(verdicts), "" if len(verdicts) == 1 else "s"))
        print("classes: %s" % ", ".join("%s=%d" % kv for kv in payload["classes"].items()))
        print("noise:   %s" % ", ".join("%s=%d" % kv for kv in payload["noise"].items()))
        return 0

    if args.command == "bench":
        return _bench(args, taxonomy)

    return EXIT_CODES["USAGE"]


def _bench(args, taxonomy: Taxonomy) -> int:
    """Measure the cost of the lookup path, on the paths the corpus actually contains.

    Printed as a rate and a per-lookup microsecond figure because the acceptance criterion
    is "cheap enough to call per changed file": what a reviewer can check is whether the
    number is smaller than the per-file budget it has to live inside.
    """
    from .corpus import paths as corpus_paths

    paths = corpus_paths()
    reps = max(1, args.repeat)
    # The cold figure has to defeat the memo, and it can only do that with paths that are
    # genuinely new, so every repeat is wrapped in a unique directory. The bucket mix is
    # unchanged (one extra literal segment), which is what makes the number comparable to
    # the warm one.
    distinct = ["run%03d/%s" % (rep, p) for rep in range(reps) for p in paths]
    taxonomy.clear_memo()
    t0 = time.perf_counter()
    for path in distinct:
        taxonomy.classify(path)
    cold = time.perf_counter() - t0
    taxonomy.classify_many(paths)
    work = paths * reps
    t0 = time.perf_counter()
    for path in work:
        taxonomy.classify(path)
    warm = time.perf_counter() - t0
    t0 = time.perf_counter()
    candidates = sum(taxonomy.probed(p) for p in paths)
    probe_us = (time.perf_counter() - t0) / len(paths) * 1e6

    stats = taxonomy.stats()
    out = {"distinct_paths": len(paths), "lookups": len(distinct), "rules": stats["rules"],
           "index": stats["buckets"],
           "cold_us_per_lookup": round(cold / len(distinct) * 1e6, 2),
           "memo_us_per_lookup": round(warm / len(work) * 1e6, 2),
           "probe_us_per_lookup": round(probe_us, 2),
           "index_probes_per_lookup": round(candidates / len(paths), 2),
           "builtins_version": BUILTINS_VERSION}
    if args.json:
        print(json.dumps(out, indent=2, sort_keys=True))
        return 0
    print("fathom taxonomy bench · %d rules, %d patterns, %d distinct paths, %d lookups"
          % (out["rules"], out["index"]["entries"], out["distinct_paths"], out["lookups"]))
    print("  cold (memo defeated)     %7.2f µs/lookup   → 1 000 changed files in %.0f ms"
          % (out["cold_us_per_lookup"], out["cold_us_per_lookup"]))
    print("  memo hit                 %7.2f µs/lookup" % out["memo_us_per_lookup"])
    print("  index probes             %7.2f per lookup, of %d patterns in the table"
          % (out["index_probes_per_lookup"], out["index"]["entries"]))
    print("  buckets                  %s" % out["index"])
    return 0


if __name__ == "__main__":                                   # pragma: no cover
    sys.exit(main())
