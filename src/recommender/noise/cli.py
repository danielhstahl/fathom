"""`fathom noise …` — the verb surface of `e2.2`, and the only IO in the package.

The Rust product exposes this from `fathom-cli` (verbs parse, build a context, dispatch,
return an exit code — ADR-002 §4.3, §7.3); the executable forms here are:

    python3 -m recommender.noise explain <path> …    # one file pair, with the receipts
    python3 -m recommender.noise episode --manifest m.json
    python3 -m recommender.noise policy              # what is folded, and who said so
    python3 -m recommender.noise languages           # the token model, per language
    python3 -m recommender.noise score --min-precision 0.95    # the CI gate
    python3 -m recommender.noise bench

A pair is *two revisions of one path*, and the verb needs both to say anything useful, so
it takes them three ways, in increasing order of fuss:

    python3 -m recommender.noise explain src/a.rs --repo after/ --old-repo before/
    python3 -m recommender.noise explain src/a.rs --before a.rs --after b.rs
    python3 -m recommender.noise episode --manifest episode.json

The manifest is the shape an episode already has in JSON, and the one `e2.11` will hand to
consumers:

    {"id": "ep-1", "title": "rustfmt the tree",
     "changes": [{"path": "src/a.rs", "change": "modified",
                  "before": "…", "after": "…",
                  "before_base64": "…", "after_base64": "…",
                  "added_lines": 12, "deleted_lines": 8, "history": 3}],
     "history": {"src/a.rs": 3}, "history_window": 8}

The flags for finding `fathom.toml` are `e0.8`'s verbatim, so `fathom noise policy` and
`fathom config explain` cannot disagree about which layer won. `--no-suppress` is the CLI
spelling of the session keystroke: it changes this process's policy and writes nothing.
"""

from __future__ import annotations

import argparse
import base64
import json
import sys
import time
from pathlib import Path

from recommender.config.cli import common_flags
from recommender.config.errors import ConfigError, EXIT_CODES
from recommender.config.layer import load as load_config

from . import explain as X
from . import render
from .corpus import EPISODES, GENERATED, LOCKS, PAIRS
from .episode import Change, Episode, suppress
from .eval import MIN_ACCURACY, MIN_EPISODES, MIN_PRECISION, MIN_RECALL, score
from .norm import LANGUAGES, NORM_VERSION
from .policy import Policy, Session
from .reformat import compare

#: Same cap `e2.1` puts on a blob it will read: a 400 MB committed file is not classified.
MAX_BLOB_BYTES = 4 * 1024 * 1024


def build_parser() -> argparse.ArgumentParser:
    common = common_flags()
    ap = argparse.ArgumentParser(
        prog="fathom noise", parents=[common],
        description="Noise suppression (e2.2 / reco-zqf): reformat, generated churn and "
                    "lockfile-only change, folded with the reason printed.")
    sub = ap.add_subparsers(dest="command", required=True)

    p = sub.add_parser("explain", parents=[common],
                       help="compare one file pair and print the claim with its evidence")
    p.add_argument("path", nargs="+")
    p.add_argument("--old-repo", type=Path, default=None,
                   help="the old revision's tree (default: --repo, i.e. compare a file "
                        "against itself, which is a useful no-op check)")
    p.add_argument("--before", type=Path, default=None, help="explicit old-revision file")
    p.add_argument("--after", type=Path, default=None, help="explicit new-revision file")
    p.add_argument("--change", default="modified",
                   choices=("modified", "added", "deleted", "renamed"))
    p.add_argument("--generated", action="store_true",
                   help="also run the generated-file detector over the pair")
    p.add_argument("--lock", action="store_true",
                   help="also summarise the pair as a lockfile diff")
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("episode", parents=[common], help="suppress one episode's noise")
    p.add_argument("--manifest", type=Path, default=None, help="JSON change set (see the help)")
    p.add_argument("--path", dest="paths", nargs="*", default=[],
                   help="paths to read from --repo / --old-repo instead of a manifest")
    p.add_argument("--old-repo", type=Path, default=None)
    p.add_argument("--expanded", action="store_true",
                   help="render the post-`b` state: everything visible, policy still printed")
    p.add_argument("--no-suppress", action="store_true",
                   help="the session keystroke: fold nothing for this run")
    p.add_argument("--policy", action="store_true", help="print the policy block too")
    p.add_argument("--cols", type=int, default=render.DEFAULT_COLS)
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("policy", parents=[common],
                       help="which rules are on, from which layer, with which key")
    p.add_argument("--no-suppress", action="store_true")
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("languages", help="the token model: languages, formatters, refusals")
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("score", parents=[common],
                       help="accuracy, fold precision and recall on the labelled corpus")
    p.add_argument("--min-accuracy", type=float, default=MIN_ACCURACY)
    p.add_argument("--min-precision", type=float, default=MIN_PRECISION)
    p.add_argument("--min-recall", type=float, default=MIN_RECALL)
    p.add_argument("--min-episodes", type=float, default=MIN_EPISODES)
    p.add_argument("--verbose", "-v", action="store_true")
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("bench", parents=[common],
                       help="cost per pair and per episode, so the AC's budget is a number")
    p.add_argument("--repeat", type=int, default=4)
    p.add_argument("--json", action="store_true")
    return ap


# ---------------------------------------------------------------------------
# helpers
# ---------------------------------------------------------------------------
def _config(args):
    env = {} if getattr(args, "no_env", False) else None
    return load_config(getattr(args, "repo", None), repo_file=getattr(args, "repo_file", None),
                       user_file=getattr(args, "user_file", None), env=env,
                       require_repo=False, sets=getattr(args, "set", []),
                       extends=getattr(args, "extend", []),
                       offline=getattr(args, "offline", False))


def _policy(args, config=None) -> Policy:
    config = config if config is not None else _config(args)
    session = Session(suppress=False, pressed="--no-suppress") \
        if getattr(args, "no_suppress", False) else Session()
    return Policy.from_config(config, session=session)


def _read(root: Path | None, path: str) -> str | None:
    """One file's text from a tree, or None. Part of the verb half on purpose."""
    if root is None:
        return None
    candidate = Path(root) / path
    try:
        if not candidate.is_file():
            return None
        data = candidate.read_bytes()[:MAX_BLOB_BYTES]
        return data.decode("utf-8", "replace")
    except OSError:
        return None


def _sides(args, path: str) -> tuple[str | None, str | None]:
    """(before, after) for one path, from whichever flags the caller used.

    `--before/--after` name two files and win outright; otherwise both revisions are read
    out of `--repo` (and `--old-repo` when given, which is how a rustfmt commit is compared
    without a second checkout: one tree before the run, one after).
    """
    if getattr(args, "before", None) or getattr(args, "after", None):
        return _read_file(args.before), _read_file(args.after)
    old_root = args.old_repo or args.repo
    if args.old_repo is None and args.repo is None:
        return None, None
    return _read(old_root, path), _read(args.repo, path)


def _read_file(path: Path | None) -> str | None:
    if path is None:
        return None
    try:
        return Path(path).read_text("utf-8", "replace")[:MAX_BLOB_BYTES]
    except OSError:
        return None


def _field(entry: dict, key: str) -> str | None:
    """A manifest field, inline or base64 (`before` / `before_base64`)."""
    if key in entry and entry[key] is not None:
        return str(entry[key])
    encoded = entry.get(key + "_base64")
    if encoded:
        return base64.b64decode(encoded).decode("utf-8", "replace")
    return None


def episode_from_manifest(payload: dict) -> Episode:
    files = []
    for entry in payload.get("changes", []):
        change_type = entry.get("change", entry.get("change_type", "modified"))
        files.append(Change(path=entry.get("path", ""),
                            old_path=entry.get("old_path", ""),
                            before=_field(entry, "before"), after=_field(entry, "after"),
                            change_type=change_type,
                            added_lines=int(entry.get("added_lines", 0) or 0),
                            deleted_lines=int(entry.get("deleted_lines", 0) or 0)))
    return Episode(id=payload.get("id", ""), files=tuple(files),
                   title=payload.get("title", ""))


def episode_from_paths(args) -> Episode:
    files = []
    for path in args.paths:
        files.append(Change(path=path, before=_read(args.old_repo or args.repo, path),
                            after=_read(args.repo, path)))
    return Episode(id="", files=tuple(files))


# ---------------------------------------------------------------------------
# the verbs
# ---------------------------------------------------------------------------
def cmd_explain(args) -> int:
    if args.json:
        payload = []
        for path in args.path:
            before, after = _sides(args, path)
            row = compare(path, before, after, change_type=args.change).as_dict()
            if args.generated:
                from .generated import inspect as inspect_generated
                row["generated"] = inspect_generated(path, before, after).as_dict()
            if args.lock:
                from .lockfile import summarize
                row["lock"] = summarize(path, before, after).as_dict()
            payload.append(row)
        print(json.dumps(payload if len(payload) > 1 else payload[0], indent=2))
        return 0
    for path in args.path:
        before, after = _sides(args, path)
        for line in X.explain_pair(path, before, after, change_type=args.change):
            print(line)
        if args.generated:
            print()
            for line in X.explain_generated(path, before, after):
                print(line)
        if args.lock:
            print()
            for line in X.explain_lock(path, before, after):
                print(line)
        print()
    return 0


def cmd_episode(args) -> int:
    if args.manifest:
        payload = json.loads(Path(args.manifest).read_text("utf-8"))
        episode = episode_from_manifest(payload)
        history = payload.get("history", {})
        window = int(payload.get("history_window", 8) or 8)
    else:
        if not args.paths:
            print("episode: pass --manifest or at least one --path", file=sys.stderr)
            return EXIT_CODES["USAGE"]
        episode = episode_from_paths(args)
        history, window = {}, 8
    view = suppress(episode, _policy(args), history=history, history_window=window)
    if args.json:
        print(json.dumps(view.as_dict(), indent=2))
        return 0
    for line in render.render(view, expanded=args.expanded, cols=args.cols):
        print(line)
    if args.policy:
        print()
        for line in render.policy_block(view, cols=args.cols):
            print(line)
    problems = render.audit(render.render(view, expanded=args.expanded, cols=args.cols),
                            view=view).problems
    for problem in problems:
        print("audit: %s" % problem, file=sys.stderr)
    return EXIT_CODES["ERROR"] if problems else 0


def cmd_policy(args) -> int:
    policy = _policy(args)
    if args.json:
        print(json.dumps(policy.as_dict(), indent=2))
        return 0
    for line in X.explain_policy(policy):
        print(line)
    return 0


def cmd_languages(args) -> int:
    if args.json:
        print(json.dumps({lang.id: {"title": lang.title, "tokenizer": lang.tokenizer,
                                    "extensions": list(lang.extensions),
                                    "formatter": lang.formatter, "notes": lang.notes}
                          for lang in LANGUAGES.values()}, indent=2))
        return 0
    print("token model v%d — %d languages; anything else gets the line-level claim only"
          % (NORM_VERSION, len(LANGUAGES)))
    for lang in sorted(LANGUAGES.values(), key=lambda item: item.id):
        print("  %-12s %-24s %-9s %-42s %s"
              % (lang.id, lang.title, lang.tokenizer, ", ".join(lang.extensions),
                 lang.formatter))
        if lang.notes:
            print("  %-12s %s" % ("", lang.notes))
    return 0


def cmd_score(args) -> int:
    report = score(policy=_policy(args))
    problems = report.gates(accuracy=args.min_accuracy, precision=args.min_precision,
                            recall=args.min_recall, episodes=args.min_episodes)
    if args.json:
        print(json.dumps({"report": report.as_dict(), "problems": problems,
                          "bars": {"accuracy": args.min_accuracy,
                                   "precision": args.min_precision,
                                   "recall": args.min_recall,
                                   "episodes": args.min_episodes}}, indent=2))
    else:
        print(report.text(verbose=args.verbose))
        if problems:
            print("FAIL")
            for problem in problems:
                print("  %s" % problem)
    return EXIT_CODES["ERROR"] if problems else 0


def cmd_bench(args) -> int:
    pairs = PAIRS
    repeat = max(1, args.repeat)
    started = time.perf_counter()
    for _ in range(repeat):
        for case in pairs:
            compare(case.path, case.before, case.after)
    pair_ms = (time.perf_counter() - started) * 1000 / (repeat * max(1, len(pairs)))

    started = time.perf_counter()
    for _ in range(repeat):
        for case in EPISODES:
            files = tuple(Change(path=path, before=before, after=after)
                          for path, (before, after) in case.files.items())
            suppress(Episode(id=case.name, files=files))
    episode_ms = (time.perf_counter() - started) * 1000 / (repeat * max(1, len(EPISODES)))

    started = time.perf_counter()
    for _ in range(repeat):
        for case in LOCKS:
            from .lockfile import summarize
            summarize(case.path, case.before, case.after)
    lock_ms = (time.perf_counter() - started) * 1000 / (repeat * max(1, len(LOCKS)))

    started = time.perf_counter()
    for _ in range(repeat):
        for case in GENERATED:
            from .generated import inspect as inspect_generated
            inspect_generated(case.path, case.before, case.after,
                              path_class=case.path_class, band=case.band,
                              generator_hint=case.hint)
    gen_ms = (time.perf_counter() - started) * 1000 / (repeat * max(1, len(GENERATED)))

    if args.json:
        print(json.dumps({"per_pair_ms": round(pair_ms, 3), "per_episode_ms": round(episode_ms, 3),
                          "per_lockfile_ms": round(lock_ms, 3),
                          "per_generated_ms": round(gen_ms, 3),
                          "corpus": {"pairs": len(PAIRS), "episodes": len(EPISODES),
                                     "locks": len(LOCKS), "generated": len(GENERATED)},
                          "repeat": repeat}, indent=2))
        return 0
    print("noise bench (repeat %d): %.3f ms per pair, %.3f ms per episode, "
          "%.3f ms per lockfile, %.3f ms per generated file"
          % (repeat, pair_ms, episode_ms, lock_ms, gen_ms))
    return 0


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        handlers = {"explain": cmd_explain, "episode": cmd_episode, "policy": cmd_policy,
                    "languages": cmd_languages, "score": cmd_score, "bench": cmd_bench}
        return handlers[args.command](args)
    except ConfigError as exc:
        print("error: %s" % exc, file=sys.stderr)
        return exc.exit_code
    except json.JSONDecodeError as exc:
        print("error: the manifest is not JSON (%s)" % exc, file=sys.stderr)
        return EXIT_CODES["USAGE"]


if __name__ == "__main__":                                  # pragma: no cover
    sys.exit(main())
