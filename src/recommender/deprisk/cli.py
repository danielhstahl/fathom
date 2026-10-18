"""`fathom deprisk …` — the verb surface of `reco-yxi`, and the only IO in the package.

The Rust product exposes this from `fathom-cli` (verbs parse, build a context, dispatch, return an
exit code — ADR-002 §4.3, §7.3); the executable forms here are:

```
python3 -m recommender.deprisk card --manifest episode.json
python3 -m recommender.deprisk card --fixture advisory_hot
python3 -m recommender.deprisk provenance --fixture intent_recorded
python3 -m recommender.deprisk advisories --fixture advisory_hot --cache .fathom/advisories.json
python3 -m recommender.deprisk advisories --refresh          # needs consent, or exits 8
python3 -m recommender.deprisk license --fixture copyleft_in_library
python3 -m recommender.deprisk findings --fixture licence_drift
python3 -m recommender.deprisk consent
python3 -m recommender.deprisk policy
python3 -m recommender.deprisk definition
python3 -m recommender.deprisk score
```

Three input routes, in increasing order of fuss.

`--fixture NAME` runs a hand-authored case out of `corpus.py`, including its cache fixture — the
route every documented number here was produced by, and the one the docs print.

`--manifest FILE` (or `-` for stdin) carries one episode as JSON, the shape `e2.11` hands to
consumers: `delta.changes` is `e2.6`'s projection, `episode.intent` is whatever intent capture
produced, and `introductions` are earlier episodes' recorded answers.

```json
{"episode": {"id": "ep-41", "title": "add refresh-token rotation",
             "intent": {"text": "add refresh-token rotation", "source": "agent-intent-trailer"}},
 "delta": {"changes": [{"kind": "added",
                        "after": {"name": "jwt", "version": "2.1.0", "ecosystem": "rust",
                                  "licence": "MIT", "attribution": "direct",
                                  "scope": "crates/fathom-cli/Cargo.toml",
                                  "asked_by": ["fathom-cli"]}}]},
 "asked": ["old-crypto 1.2.0"]}
```

`--cache FILE` reads the advisory cache from a path (default: `deps.advisories.cache`, resolved
against `--repo`). A missing file is an answer with a partial in it, not an error.

**The one network call.** `advisories --refresh` is the only verb that can reach the network, and it
needs all three of `deps.advisories.enabled`, `deps.advisories.mode = "refresh"` and user-scoped
`privacy.advisories`; without them it prints which gate refused and exits 8
(`NETWORK_CONSENT_REQUIRED`). No other verb takes the flag, and the render verbs ignore it — that is
what "advisory lookup never blocks episode rendering" costs, and it is cheap.
"""

from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path

from recommender.config.cli import common_flags
from recommender.config.errors import ConfigError, ConfigInvalid, EXIT_CODES
from recommender.config.layer import load as load_config

from . import corpus as CORPUS
from . import explain as X
from . import osv as OSV
from . import render
from .delta import measure
from .model import Input, parse_input
from .policy import Policy, describe_rules, matrix_rows
from .provenance import ANSWER_NONE

#: A cache file bigger than this is not a cache any more; it is refused rather than mmap'd.
MAX_CACHE_BYTES = 8 * 1024 * 1024


# ---------------------------------------------------------------------------
# config, policy, input
# ---------------------------------------------------------------------------
def _config(args):
    env = {} if getattr(args, "no_env", False) else None
    return load_config(getattr(args, "repo", None), repo_file=getattr(args, "repo_file", None),
                       user_file=getattr(args, "user_file", None), env=env,
                       require_repo=False, sets=getattr(args, "set", []),
                       extends=getattr(args, "extend", []),
                       offline=getattr(args, "offline", False))


def _policy(args, config=None) -> Policy:
    active = config if config is not None else _config(args)
    policy = Policy.from_config(active)
    if getattr(args, "offline", False):
        policy = policy.offline()
    problems = policy.problems()
    if problems:
        raise ConfigInvalid("; ".join(problems),
                            hint="`fathom deprisk policy` prints every value with the layer it "
                                 "came from, which is usually the line that needs changing")
    return policy


def _read_text(path: str, *, limit: int = MAX_CACHE_BYTES) -> str:
    if path == "-":
        return sys.stdin.read()
    target = Path(path)
    size = target.stat().st_size
    if size > limit:
        raise ValueError("%s is %d bytes, over the %d byte limit this verb will read"
                         % (path, size, limit))
    return target.read_text("utf-8")


def _input(args) -> Input:
    """The episode under review, from a fixture or from a manifest."""
    if getattr(args, "fixture", None):
        return CORPUS.fixture(args.fixture)
    manifest = getattr(args, "manifest", None)
    if not manifest:
        raise ValueError("say which episode: --fixture NAME or --manifest FILE (or - for stdin)")
    return parse_input(json.loads(_read_text(manifest)))


def _now(args) -> int:
    """The clock, as a value handed to the analysis. `--now` makes a card reproducible."""
    value = getattr(args, "now", None)
    return int(value) if value else int(time.time())


def _cache_path(args, policy: Policy) -> Path:
    explicit = getattr(args, "cache", None)
    if explicit:
        return Path(explicit)
    declared = policy.text("adv_cache") or ".fathom/advisories.json"
    path = Path(declared)
    if path.is_absolute():
        return path
    repo = getattr(args, "repo", None)
    return (Path(repo) if repo else Path.cwd()) / path


def _read_cache(args, policy: Policy, *, now: int) -> OSV.Cache:
    """The advisory cache as a value. No file, unreadable file, `offline`: each its own answer."""
    path = _cache_path(args, policy)
    if policy.text("adv_mode") == "offline":
        return OSV.empty_cache(path=str(path), now=now,
                               error="`deps.advisories.mode` is `offline`, so the cache was not "
                                     "read")
    try:
        text = path.read_text("utf-8")
    except FileNotFoundError:
        return OSV.empty_cache(path=str(path), now=now)
    except OSError as exc:
        return OSV.Cache(entries={}, path=str(path), read_at=now, exists=True, error=str(exc))
    cache = OSV.deserialize(text, path=str(path), now=now)
    if cache.error:
        return cache
    return cache


def _measure(args, policy: Policy | None = None):
    config = None
    active_policy = policy
    if active_policy is None:
        config = _config(args)
        active_policy = _policy(args, config)
    episode = _input(args)
    now = _now(args)
    cache = _read_cache(args, active_policy, now=now)
    view = measure(episode, active_policy, cache=cache, now=now)
    return view, active_policy, cache, config


# ---------------------------------------------------------------------------
# verbs
# ---------------------------------------------------------------------------
def cmd_card(args) -> int:
    """The whole card: who asked, what is known about it, and what policy makes of it."""
    view, policy, _cache, _config_ = _measure(args)
    if args.json:
        print(json.dumps(view.as_dict(), indent=2, sort_keys=True))
        return EXIT_CODES["OK"]
    lines = render.render(view, cols=args.cols, show_all=args.all)
    for line in lines:
        print(line)
    audit = render.audit(view, lines)
    for problem in audit.problems:
        print("audit: %s" % problem, file=sys.stderr)
    if args.check and not audit.ok:
        return EXIT_CODES["ERROR"]
    return EXIT_CODES["OK"]


def cmd_summary(args) -> int:
    """The one sentence, on its own — what a list view or a commit annotation would print."""
    view, _policy_, _cache, _config_ = _measure(args)
    summary = view.summary()
    if args.json:
        print(json.dumps(summary, indent=2, sort_keys=True))
        return EXIT_CODES["OK"]
    print("%s · %s" % (view.episode.episode.id or "episode", summary["sentence"]))
    if args.verbose:
        print("       %s" % summary["freshness"])
        print("       confidence %.2f · %d stated partial(s)"
              % (summary["confidence"], len(summary["partials"])))
    return EXIT_CODES["OK"]


def cmd_provenance(args) -> int:
    """The 'who asked for this' rows, one per new direct dependency."""
    view, _policy_, _cache, _config_ = _measure(args)
    if args.json:
        print(json.dumps([answer.as_dict() for answer in view.answers], indent=2, sort_keys=True))
        return EXIT_CODES["OK"]
    if not view.answers:
        print("no new direct dependency in this episode: nothing to answer")
        return EXIT_CODES["OK"]
    answers = view.answers if args.all else view.answers[:view.policy.integer("prov_max_rows")]
    for answer in answers:
        print("%s (%s, %s)" % (answer.package.text,
                               answer.package.ecosystem or "unknown ecosystem",
                               answer.package.attribution))
        for row in answer.rows():
            print("  " + row)
        print()
    hidden = len(view.answers) - len(answers)
    if hidden:
        print("… %d more (--all)" % hidden)
    if any(answer.state == ANSWER_NONE for answer in view.answers):
        print("rows above that say `not recorded` are the answer: nothing named the episode.")
    return EXIT_CODES["OK"]


def cmd_license(args) -> int:
    """The licence rows: the verdict, the class, the layer and the edge path for each package."""
    view, policy, _cache, _config_ = _measure(args)
    if args.json:
        print(json.dumps({"rows": [row.as_dict() for row in view.rows],
                          "new_in_set": [{"id": one, "brought_by": who}
                                         for one, who in view.new_licences],
                          "matrix": policy.as_dict()["licence_matrix"]},
                         indent=2, sort_keys=True))
        return EXIT_CODES["OK"]
    rows = [row for row in view.rows if not args.violations or row.violation]
    shown = rows if args.all else rows[:policy.integer("lic_max_rows")]
    for row in shown:
        for line in row.rows(max_evidence=None if args.all else policy.integer("card_max_evidence")):
            print(line)
    if len(rows) > len(shown):
        print("… %d more (--all)" % (len(rows) - len(shown)))
    if view.new_licences:
        print("newly in the shipped licence set:")
        for licence_id, who in view.new_licences:
            print("  %s — brought by %s" % (licence_id, who))
    print("matrix: " + " · ".join(item.strip() for item in matrix_rows(policy)))
    return EXIT_CODES["OK"]


def cmd_findings(args) -> int:
    """Every finding, one block each, in severity order."""
    view, policy, _cache, _config_ = _measure(args)
    findings = view.findings
    if args.severity:
        findings = [finding for finding in findings if finding.severity == args.severity]
    if args.kind:
        findings = [finding for finding in findings if finding.kind == args.kind]
    if args.json:
        print(json.dumps([finding.as_dict() for finding in findings], indent=2, sort_keys=True))
        return EXIT_CODES["OK"]
    if not findings:
        print("no findings")
    for finding in findings:
        for line in finding.block(indent=0,
                                  max_evidence=None if args.all
                                  else policy.integer("card_max_evidence")):
            print(line)
        print()
    counts = view.counts()
    print("%d finding(s)%s · suppressed by their own switch: %d"
          % (len(findings), "" if not args.severity and not args.kind else " (filtered)",
             counts["suppressed"]))
    return EXIT_CODES["OK"]


def cmd_advisories(args) -> int:
    """The advisory half: what was checked, how fresh it is, and what a refresh would do."""
    view, policy, cache, _config_ = _measure(args)
    if args.refresh:
        return _refresh(args, policy, cache, view)
    if args.plan:
        if args.json:
            print(json.dumps(view.plan.as_dict(), indent=2, sort_keys=True))
        else:
            for line in X.plan_text(view.plan):
                print(line)
        return EXIT_CODES["OK"]
    if args.json:
        print(json.dumps({"freshness": view.freshness.as_dict(),
                          "cache": {"path": view.cache_path, "exists": view.cache_exists,
                                    "entries": cache.size},
                          "network": policy.network.as_dict(),
                          "rows": [row.as_dict(view.now) for row in view.advisory_rows]},
                         indent=2, sort_keys=True))
        return EXIT_CODES["OK"]
    print("cache: %s (%s)" % (view.cache_path,
                              "read: %d entr%s" % (cache.size, "y" if cache.size == 1 else "ies")
                              if cache.exists else "no file"))
    if cache.error:
        print("  %s" % cache.error)
    print("freshness: %s — %s" % (view.freshness.state, view.freshness.words))
    print("network: %s" % policy.network.reason)
    if not view.advisory_rows:
        print("nothing queryable in this delta")
        return EXIT_CODES["OK"]
    for row in view.advisory_rows:
        if not row.checked:
            print(" .  not checked: %s %s (%s) — %s"
                  % (row.lookup.package, row.lookup.version, row.lookup.osv_ecosystem,
                     row.reason))
            continue
        age = OSV.age_text(max(0, view.now - (row.entry.fetched_at if row.entry else view.now)))
        if not row.advisories:
            status = "no advisories" if (row.entry and row.entry.status != OSV.STATUS_ERROR) \
                else "the last query failed (%s)" % (row.entry.error or row.entry.http_status)
            print(" .  %s %s (%s) — %s, asked %s ago"
                  % (row.lookup.package, row.lookup.version, row.lookup.osv_ecosystem, status,
                     age))
            continue
        for advisory in row.advisories:
            print("!! %s %s (%s) — %s %s: %s"
                  % (row.lookup.package, row.lookup.version, row.lookup.osv_ecosystem,
                     advisory.id, advisory.severity, advisory.summary or "(no summary)"))
            if advisory.has_fix:
                print("    fix: %s (installed %s)" % (advisory.fixed_in, row.lookup.version))
            print("    database: %s · cached %s ago" % (advisory.database or "OSV", age))
    return EXIT_CODES["OK"]


def cmd_consent(args) -> int:
    """The three gates, each stated, and the verdict — the no-network claim, printed."""
    policy = _policy(args)
    network = policy.network
    if args.json:
        print(json.dumps(network.as_dict(), indent=2, sort_keys=True))
        return EXIT_CODES["OK"]
    print("network consent for advisory lookups")
    for line in network.grants:
        print("  %s" % line)
    print("  endpoint: %s (fixed in code: a config key here would let a clone redirect the "
          "fetch)" % network.endpoint)
    print("  verdict: %s" % network.reason)
    print("  default config makes %d network call(s)" % (1 if network.may_fetch else 0))
    return EXIT_CODES["OK"]


def cmd_cache(args) -> int:
    """Inspect the cache, or populate it offline from a file of recorded OSV responses."""
    policy = _policy(args)
    now = _now(args)
    path = _cache_path(args, policy)
    if args.import_from:
        return _import_cache(args, policy, path, now)
    cache = _read_cache(args, policy, now=now)
    if args.json:
        print(json.dumps(cache.as_dict(now), indent=2, sort_keys=True))
        return EXIT_CODES["OK"]
    print("cache: %s" % path)
    if not cache.exists:
        print("  no file: advisories will be reported as not checked")
        return EXIT_CODES["OK"]
    if cache.error:
        print("  %s" % cache.error)
    print("  %d entr%s, %d advisories"
          % (cache.size, "y" if cache.size == 1 else "ies",
             sum(len(entry.advisories) for entry in cache.entries.values())))
    if cache.size:
        print("  oldest %s, newest %s"
              % (OSV.age_text(max(0, now - cache.oldest())),
                 OSV.age_text(max(0, now - cache.newest()))))
    for key, entry in sorted(cache.entries.items()):
        print("  %-44s %s  %s" % (key, OSV.age_text(max(0, now - entry.fetched_at)) + " old",
                                  ", ".join(entry.texts) or "no advisories"))
    return EXIT_CODES["OK"]


def _import_cache(args, policy: Policy, path: Path, now: int) -> int:
    """Fold recorded OSV responses into the cache without touching the network.

    This is the offline half of the stale-cache story: a machine with no network at all (CI, an
    air-gapped build, a plane) can still be given a current cache, and `--import` is the honest verb
    for it. The input is either a cache file, or a JSON object mapping `"ecosystem|name|version"`
    to an OSV query response.
    """
    payload = json.loads(_read_text(args.import_from))
    cache = _read_cache(args, policy, now=now)
    results: dict[str, tuple[str, tuple, str]] = {}
    if isinstance(payload, dict) and "entries" in payload:
        loaded = OSV.deserialize(json.dumps(payload), path=str(path), now=now)
        for key, entry in loaded.entries.items():
            results[key] = (entry.status, entry.advisories, entry.error)
    elif isinstance(payload, dict):
        for key, response in payload.items():
            version = str(key).split("|")[2] if len(str(key).split("|")) > 2 else ""
            advisories = OSV.parse_response(response, version)
            results[str(key)] = (OSV.STATUS_OK if advisories else OSV.STATUS_NONE, advisories, "")
    else:
        raise ValueError("--import-from wants a cache file or a map of query responses")
    merged = OSV.merge(cache, results, now=now)
    text = OSV.serialize(merged, now=now)
    if args.dry_run:
        print("would write %d entries to %s (%d bytes)"
              % (merged.size, path, len(text)))
        return EXIT_CODES["OK"]
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, "utf-8")
    print("wrote %d entries to %s" % (merged.size, path))
    return EXIT_CODES["OK"]


def _refresh(args, policy: Policy, cache: OSV.Cache, view) -> int:
    """The only code path in the package that can reach the network, and its three gates."""
    network = policy.network
    if not network.may_fetch:
        print("refused: no fetch was made", file=sys.stderr)
        for line in network.grants:
            print("  %s" % line, file=sys.stderr)
        print("  %s" % network.reason, file=sys.stderr)
        print("  nothing above the cache changed; the answer printed from cache stands",
              file=sys.stderr)
        return EXIT_CODES["NETWORK_CONSENT_REQUIRED"]
    if not view.plan.lookups:
        print("nothing to fetch: no queryable package in this delta")
        return EXIT_CODES["OK"]
    results: dict[str, tuple[str, tuple, str]] = {}
    failures = 0
    for lookup in view.plan.lookups:
        try:
            payload = _osv_query(network.endpoint, lookup, network.timeout_ms)
            advisories = OSV.parse_response(payload, lookup.version)
            results[lookup.key] = (OSV.STATUS_OK if advisories else OSV.STATUS_NONE,
                                   advisories, "")
        except (OSError, ValueError) as exc:
            failures += 1
            results[lookup.key] = (OSV.STATUS_ERROR, (), "%s" % exc)
    merged = OSV.merge(cache, results, now=_now(args))
    if not args.dry_run:
        _cache_path(args, policy).parent.mkdir(parents=True, exist_ok=True)
        _cache_path(args, policy).write_text(OSV.serialize(merged, now=_now(args)), "utf-8")
    print("fetched %d package(s) from %s (%d failed), cache now %d entries"
          % (len(results), network.endpoint, failures, merged.size))
    if failures:
        print("failed queries are stored as failures, not as clean answers")
    return EXIT_CODES["OK"] if not failures else EXIT_CODES["ERROR"]


def _osv_query(endpoint: str, lookup, timeout_ms: int) -> dict:
    """One OSV query. Endpoint fixed in code, timeout from config, nothing else configurable."""
    import urllib.error
    import urllib.request

    from .policy import OSV_ENDPOINTS

    if not any(str(endpoint).startswith(allowed) for allowed in OSV_ENDPOINTS):
        raise ValueError("refusing to query %r: only %s are allowed endpoints"
                         % (endpoint, ", ".join(OSV_ENDPOINTS)))
    body = json.dumps({"package": {"name": lookup.package, "ecosystem": lookup.osv_ecosystem},
                       "version": lookup.version}).encode("utf-8")
    request = urllib.request.Request(
        "%s/v1/query" % str(endpoint).rstrip("/"), data=body,
        headers={"content-type": "application/json",
                 "user-agent": "fathom/0.1 (dependency-risk cache; e2.7)"})
    with urllib.request.urlopen(request, timeout=max(0.2, timeout_ms / 1000.0)) as response:
        return json.loads(response.read().decode("utf-8"))


def cmd_policy(args) -> int:
    """Every switch, knob, matrix cell and gate, with the layer that won each one."""
    policy = _policy(args)
    if args.digest_only:
        print(policy.config_digest or "(no config resolved: built-in default digest is empty)")
        return EXIT_CODES["OK"]
    if args.json:
        print(json.dumps(policy.as_dict(), indent=2, sort_keys=True))
        return EXIT_CODES["OK"]
    for line in policy.describe():
        print(line)
    return EXIT_CODES["OK"]


def cmd_definition(args) -> int:
    """The rules as sentences, with the numbers currently in force."""
    policy = _policy(args)
    for line in X.definition_text(policy):
        print(line)
    return EXIT_CODES["OK"]


def cmd_rules(args) -> int:
    """The finding table."""
    for line in describe_rules():
        print(line)
    print()
    for line in matrix_rows(_policy(args)):
        print(line)
    return EXIT_CODES["OK"]


def cmd_matrix(args) -> int:
    policy = _policy(args)
    for line in matrix_rows(policy):
        print(line)
    return EXIT_CODES["OK"]


def cmd_explain(args) -> int:
    """One part of the definition, chosen by topic; `read` is how to read the card."""
    policy = _policy(args)
    topic = getattr(args, "topic", "read") or "read"
    if topic == "definition":
        lines = X.definition_text(policy)
    elif topic == "rules":
        lines = X.rules_text()
    elif topic == "matrix":
        lines = X.matrix_text(policy)
    elif topic == "consent":
        lines = X.consent_text(policy)
    else:
        lines = X.how_to_read(policy)
    for line in lines:
        print(line)
    return EXIT_CODES["OK"]


def cmd_score(args) -> int:
    """The CI gate: the corpus, scored against its own bars."""
    from .eval import (
        MIN_ADVISORY_RECALL, MIN_FRESHNESS, MIN_PROVENANCE, MIN_LICENCE_PRECISION,
        MIN_LICENCE_RECALL, MIN_RECONCILE, score)
    report = score()
    print(report.text(verbose=args.verbose))
    failures = report.gates(provenance=MIN_PROVENANCE, licence_precision=MIN_LICENCE_PRECISION,
                            licence_recall=MIN_LICENCE_RECALL,
                            advisory_recall=MIN_ADVISORY_RECALL, freshness=MIN_FRESHNESS,
                            reconcile=MIN_RECONCILE,
                            min_cases=args.min_cases)
    for line in failures:
        print("gate failed: %s" % line, file=sys.stderr)
    return EXIT_CODES["ERROR"] if failures else EXIT_CODES["OK"]


def cmd_property(args) -> int:
    """Reconciliation on generated episodes: one line each, non-zero if any row fails."""
    policy = _policy(args)
    failures = 0
    for seed in range(args.start, args.start + args.seeds):
        for name, episode in CORPUS.fuzz_cases(seed, 8):
            cache = CORPUS.cache_for("fresh", now=CORPUS.NOW) if seed % 3 else None
            view = measure(episode, policy, cache=cache, now=CORPUS.NOW)
            problems = view.verify()
            failures += bool(problems)
            counts = view.counts()
            print("seed %5d %-11s changes=%-3d prov=%-3d lic=%-3d adv=%-3d findings=%-3d %s"
                  % (seed, name, counts["changes"], counts["provenance_rows"],
                     counts["licence_rows"], counts["advisories"], counts["findings"],
                     "reconciles" if not problems else "FAILS: " + "; ".join(problems)))
    total = args.seeds * 8
    print("%d/%d generated episode(s) reconcile" % (total - failures, total))
    return EXIT_CODES["ERROR"] if failures else EXIT_CODES["OK"]


def cmd_demo(args) -> int:
    """The card the docs print, audited."""
    episode = CORPUS.fixture(args.fixture)
    active = _policy(args).with_layers(CORPUS.FIXTURE_LAYERS)
    active = active.with_knobs("demo", lic_library_layers=CORPUS.FIXTURE_LIBRARY_LAYERS)
    now = _now(args) or CORPUS.NOW
    cache = CORPUS.cache_of(args.fixture, now=now or CORPUS.NOW)
    view = measure(episode, active, cache=cache, now=(now or CORPUS.NOW))
    if args.json:
        print(json.dumps(view.as_dict(), indent=2, sort_keys=True))
        return EXIT_CODES["OK"]
    lines = render.render(view, cols=args.cols, show_all=True)
    for line in lines:
        print(line)
    audit = render.audit(view, lines)
    for problem in audit.problems:
        print("audit: %s" % problem, file=sys.stderr)
    return EXIT_CODES["ERROR"] if audit.problems else EXIT_CODES["OK"]


# ---------------------------------------------------------------------------
# the parser
# ---------------------------------------------------------------------------
def build_parser() -> argparse.ArgumentParser:
    common = common_flags()
    ap = argparse.ArgumentParser(
        prog="fathom deprisk", parents=[common],
        description="Dependency risk: who asked for this, and should I be worried (e2.7 / "
                    "reco-yxi). Provenance, cached advisories and licence policy.")
    sub = ap.add_subparsers(dest="command", required=True)

    def inputs(parser):
        parser.add_argument("--fixture", choices=sorted(CORPUS.INPUTS),
                            help="run a hand-authored fixture instead of a manifest")
        parser.add_argument("--manifest", metavar="FILE",
                            help="the episode as JSON, from a file or - for stdin")
        parser.add_argument("--cache", metavar="FILE",
                            help="advisory cache path (default: deps.advisories.cache)")
        parser.add_argument("--now", type=int, default=None,
                            help="evaluate ages against this epoch second (default: the clock)")
        parser.add_argument("--all", action="store_true", help="do not cap rows")
        parser.add_argument("--json", action="store_true")
        parser.add_argument("--cols", type=int, default=render.DEFAULT_COLS)

    p = sub.add_parser("card", parents=[common], help="the dependency-risk card")
    inputs(p)
    p.add_argument("--check", action="store_true",
                   help="exit non-zero if the render audit finds a lost fact")
    p.set_defaults(handler=cmd_card)

    p = sub.add_parser("summary", parents=[common], help="the one sentence")
    inputs(p)
    p.add_argument("-v", "--verbose", action="store_true")
    p.set_defaults(handler=cmd_summary)

    p = sub.add_parser("provenance", parents=[common],
                       help="who asked for each new direct dependency")
    inputs(p)
    p.set_defaults(handler=cmd_provenance)

    p = sub.add_parser("license", parents=[common],
                       help="licence verdicts, drift and the shipped licence set")
    inputs(p)
    p.add_argument("--violations", action="store_true", help="only the rows policy refused")
    p.set_defaults(handler=cmd_license)

    p = sub.add_parser("findings", parents=[common], help="every finding, one block each")
    inputs(p)
    p.add_argument("--severity", choices=("critical", "high", "notable", "note"))
    p.add_argument("--kind", help="only this finding kind")
    p.set_defaults(handler=cmd_findings)

    p = sub.add_parser("advisories", parents=[common],
                       help="advisory state: cache, freshness, coverage, or a refresh")
    inputs(p)
    p.add_argument("--refresh", action="store_true",
                   help="query OSV now (needs deps.advisories.mode = \"refresh\" and "
                        "privacy.advisories consent; otherwise exits 8)")
    p.add_argument("--plan", action="store_true", help="print what a refresh would do, and stop")
    p.add_argument("--dry-run", action="store_true", help="do not write the cache")
    p.set_defaults(handler=cmd_advisories)

    p = sub.add_parser("consent", parents=[common],
                       help="print the three network gates and the verdict")
    p.add_argument("--json", action="store_true")
    p.set_defaults(handler=cmd_consent)

    p = sub.add_parser("cache", parents=[common], help="inspect or populate the advisory cache")
    p.add_argument("--cache", metavar="FILE", help="cache path")
    p.add_argument("--now", type=int, default=None)
    p.add_argument("--import-from", metavar="FILE", dest="import_from",
                   help="fold recorded OSV responses into the cache without any network access")
    p.add_argument("--dry-run", action="store_true")
    p.add_argument("--json", action="store_true")
    p.set_defaults(handler=cmd_cache)

    p = sub.add_parser("policy", parents=[common],
                       help="every switch, knob, matrix cell and gate")
    p.add_argument("--digest-only", action="store_true", dest="digest_only")
    p.add_argument("--json", action="store_true")
    p.set_defaults(handler=cmd_policy)

    p = sub.add_parser("definition", parents=[common], help="the rules as sentences")
    p.add_argument("--json", action="store_true")
    p.set_defaults(handler=cmd_definition)

    p = sub.add_parser("rules", parents=[common], help="the finding table")
    p.set_defaults(handler=cmd_rules)

    p = sub.add_parser("matrix", parents=[common], help="the licence matrix, as configured")
    p.set_defaults(handler=cmd_matrix)

    p = sub.add_parser("explain", parents=[common], help="how to read the card")
    p.add_argument("topic", nargs="?", default="read",
                   choices=("read", "definition", "rules", "matrix", "consent"),
                   help="which part to print (default: how to read the card)")
    p.set_defaults(handler=cmd_explain)

    p = sub.add_parser("score", parents=[common], help="score the corpus (the CI gate)")
    p.add_argument("--min-cases", type=float, default=28.0, dest="min_cases")
    p.add_argument("-v", "--verbose", action="store_true")
    p.set_defaults(handler=cmd_score)

    p = sub.add_parser("property", parents=[common],
                       help="reconciliation on generated episodes")
    p.add_argument("--seeds", type=int, default=25)
    p.add_argument("--start", type=int, default=0)
    p.set_defaults(handler=cmd_property)

    p = sub.add_parser("demo", parents=[common], help="the docs' card, audited")
    p.add_argument("--fixture", choices=sorted(CORPUS.INPUTS), default="advisory_hot")
    p.add_argument("--now", type=int, default=None)
    p.add_argument("--cols", type=int, default=render.DEFAULT_COLS)
    p.add_argument("--json", action="store_true")
    p.set_defaults(handler=cmd_demo)
    return ap


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        handler = getattr(args, "handler", None)
        if handler is None:
            return EXIT_CODES["USAGE"]
        return handler(args)
    except ConfigError as exc:
        print("error: %s" % exc, file=sys.stderr)
        return exc.exit_code
    except json.JSONDecodeError as exc:
        print("error: the manifest is not JSON (%s)" % exc, file=sys.stderr)
        return EXIT_CODES["USAGE"]
    except ValueError as exc:
        print("error: %s" % exc, file=sys.stderr)
        return EXIT_CODES["USAGE"]
    except OSError as exc:
        print("error: %s" % exc, file=sys.stderr)
        return EXIT_CODES["USAGE"]


if __name__ == "__main__":                                  # pragma: no cover
    sys.exit(main())
