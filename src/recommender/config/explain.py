"""Rendering: `fathom config explain`, the privacy matrix, and the generated docs.

The ticket's reason for existing is in this file: *"prints which layer won for each
effective value, because every later feature's behaviour must be traceable to a file."*
A value with no provenance is a value nobody can debug at 2am, and a diagnostic surface
that is not text-examinable is a surface that cannot be tested — so both renderers here
are pure functions returning strings, and `tests/test_config_contract.py` asserts on the
text rather than on an internal structure only this module could produce.

Three renderings:

* :func:`explain_text` — every key that is not the built-in default, with its full chain.
* :func:`explain_path_text` — `--path P`: the keys that decided something about that file,
  which patterns matched, which layer's rule matched, and which analyzers' cache keys those
  values feed.
* :func:`privacy_matrix` — the consent table `fathom doctor --privacy` (`e1.10`,
  `reco-*`) renders, produced here so the doctor and `config explain` can never disagree
  about what is on.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from pathlib import Path

from . import globs
from . import json_schema as JS
from . import layer as L
from . import schema as S
from . import sources as SRC

_LABEL_WIDTH = max(len(S.Layer.label(name)) for name in S.Layer.ORDER)

MARK = re.compile(r"<!-- BEGIN GENERATED: (\w+) -->\n(.*?)<!-- END GENERATED: \1 -->", re.S)


# ---------------------------------------------------------------------------
# explain: keys
# ---------------------------------------------------------------------------
def header_lines(config: L.Config) -> list[str]:
    locations = config.locations
    out = []
    if locations is not None:
        root = SRC.display(locations.repo_root) if locations.repo_root else "(no repository)"
        out.append("fathom config · %s" % root)
    out.append("size class: %s (%s) · budget rows: %s"
               % (config.size_class, config.size_class_reason, config.budgets_source))
    out.append("precedence: %s" % " > ".join(S.Layer.label(name) for name in S.Layer.ORDER))
    present = [s.name for s in config.sources if s.present]
    out.append("layers present: %s" % (", ".join(present) if present else "defaults only"))
    return out


def chain_block(decision: L.Decision, *, key_width: int, matched=None) -> str:
    """One key, then its chain: the chain is the answer, the value is the headline.

    The headline carries the *full* effective value; the chain truncates, because the
    point of a chain is to read six layers in one scan and `noise.globs` has no business
    being 200 characters wide in a table.
    """
    headline = _winner_text(decision)
    if matched is True:
        headline = "matches this path · " + headline
    elif matched is False:
        headline = "no match for this path · " + headline
    lines = ["%-*s  %s" % (key_width, decision.key, headline),
             "    value: %s" % decision.render_value()]
    for step in decision.chain():
        value = L.render_value(decision.key, step.value, secret=decision.secret)
        if step.action == "suppressed":
            value = "%s  SUPPRESSED" % value
        short = value if len(value) <= 34 else value[:33] + "\u2026"
        detail = step.source or ""
        if step.note:
            detail = "%s · %s" % (detail, step.note) if detail else step.note
        lines.append("    %-*s  %-*s  %-*s  %s"
                     % (_LABEL_WIDTH, S.Layer.label(step.layer), 34,
                        short or "\u2014", 12, step.action, detail))
    return "\n".join(lines)


def _winner_text(decision: L.Decision) -> str:
    if decision.winner == S.Layer.DEFAULTS:
        text = "(built-in default)"
    else:
        text = "won: %s · %s" % (S.Layer.label(decision.winner), decision.source)
    if decision.merge == "extend":
        text += "  [extend]"
    elif decision.merge == "deep":
        text += "  [entry-wise]"
    return text


def explain_text(config: L.Config, *, keys=None, all_keys: bool = False) -> str:
    """The `fathom config explain` body: changed keys first, chains for each."""
    if keys:
        decisions = _select(config, keys)
        if not decisions:
            return "no config key matches %s" % ", ".join(repr(k) for k in keys)
    elif all_keys:
        decisions = [config.decisions[k] for k in config.keys()]
    else:
        decisions = config.changed()

    out = header_lines(config) + [""]
    if not decisions:
        out.append("nothing overrides the built-in defaults: every effective value below is "
                   "the compiled-in answer for size class %s." % config.size_class)
        out.append("pass --all to print all %d keys anyway." % len(config.decisions))
        out += _warnings_block(config)
        return "\n".join(out)

    width = min(max(len(d.key) for d in decisions), 34)
    scope = ("%d key%s %s from the built-in defaults:"
             % (len(decisions), "" if len(decisions) == 1 else "s",
                "differs" if len(decisions) == 1 else "differ"))
    if all_keys:
        scope = "all %d keys:" % len(decisions)
    elif keys:
        scope = "%d requested key%s:" % (len(decisions), "" if len(decisions) == 1 else "s")
    out.append(scope)
    out.append("")
    for decision in decisions:
        out.append(chain_block(decision, key_width=width))
        out.append("")
    if not all_keys and not keys:
        unchanged = len(config.decisions) - len(decisions)
        out.append("(%d keys are at their built-in default; `--all` prints them)" % unchanged)
        out.append("")
    out += _warnings_block(config)
    return "\n".join(out).rstrip() + "\n"


def _select(config: L.Config, keys) -> list[L.Decision]:
    """Accept `risk.tau`, a section (`noise`) or a prefix (`risk.weights`)."""
    out = []
    for wanted in keys:
        for path in config.keys():
            if path == wanted or path.startswith(wanted.rstrip(".") + ".") \
                    or S.abstract(path).startswith(wanted.rstrip(".") + ".") \
                    or path.split(".")[0] == wanted.rstrip("."):
                decision = config.decisions[path]
                if decision not in out:
                    out.append(decision)
    return out


def _warnings_block(config: L.Config) -> list[str]:
    if not config.warnings:
        return []
    lines = ["%d warning%s:" % (len(config.warnings), "" if len(config.warnings) == 1 else "s")]
    for warning in config.warnings:
        lines.append("  " + warning.render())
    return lines


# ---------------------------------------------------------------------------
# explain --path
# ---------------------------------------------------------------------------
def explain_path_text(config: L.Config, path: str) -> str:
    """The resolved decision chain for one repository path (the ticket's AC).

    This answers "which configuration decided something about *this file*" and stops
    there: which taxonomy rule *wins* is `e0.9`'s job (`reco-5vz`), and pretending
    otherwise would put a second, subtly different path classifier in the tree — the exact
    duplication `e0.9` exists to prevent. What this can say, honestly and usefully: which
    patterns matched, from which layer each came, which layer rule the path falls into,
    and which analyzers' cache digests those values feed.
    """
    path = globs.normalise(path)
    out = header_lines(config) + ["", "path: %s" % path, ""]
    relevance = path_relevance(config, path)

    out.append("PATH-RELEVANT KEYS")
    width = max(len(item.key) for item in relevance) if relevance else 8
    for item in relevance:
        out.append("  %-*s  %s" % (width, item.key, item.detail))
        if item.patterns:
            for pattern, hit, source in item.patterns:
                out.append("  %-*s  %s %-14s %s" % (width, "", "[x]" if hit else "[ ]",
                                                    pattern, source))
    out.append("")

    out.append("DECISION CHAIN FOR THOSE KEYS")
    out.append("")
    ordered = sorted(relevance, key=lambda item: (not item.matched, item.key))
    key_width = min(max(len(item.key) for item in ordered), 34)
    for item in ordered:
        out.append(chain_block(item.decision, key_width=key_width,
                               matched=item.matched))
        out.append("")

    out.append("CACHE IMPACT (ADR-004 §5.5: only declared keys move a digest)")
    moved, untouched = [], []
    for analyzer in analyzer_ids():
        declared = declared_keys(analyzer)
        if not declared:
            continue
        if any(is_path_relevant(key) for key in declared):
            moved.append((analyzer, declared))
        else:
            untouched.append(analyzer)
    for analyzer, declared in moved:
        try:
            digest = config.digest(declared)
        except Exception:                               # noqa: BLE001 - render must not fail
            digest = "(unavailable)"
        out.append("  %-18s config_digest %s…  declared: %s"
                   % (analyzer, digest[:16], ", ".join(declared) or "—"))
    if not moved:
        out.append("  none: no analyzer declares a path-relevant key")
    if untouched:
        out.append("  not touched by this path: %s" % ", ".join(untouched))
    out.append("")
    out.append("note: which taxonomy rule *wins* for %s is e0.9's decision (reco-5vz); "
               "this is the config that feeds it." % path)
    return "\n".join(out).rstrip() + "\n"


@dataclass
class PathItem:
    """One path-relevant key, and what it said about the path."""

    key: str
    detail: str
    matched: bool
    decision: L.Decision
    patterns: list[tuple[str, bool, str]] = field(default_factory=list)


def path_relevance(config: L.Config, path: str) -> list[PathItem]:
    """Which config keys speak to `path`, with every pattern's verdict and provenance."""
    items: list[PathItem] = []
    for key_path in S.PATH_KEYS:
        if key_path.endswith(">"):
            continue                                   # open maps are handled below
        decision = _best_decision(config, key_path)
        if decision is None:
            continue
        if isinstance(decision.value, str):            # a paths.* value: does it name us?
            hit = globs.inside(path, str(decision.value))
            items.append(PathItem(key_path, "names this path" if hit
                                  else "does not name this path", hit, decision,
                                  [(str(decision.value), hit, decision.source)]
                                  if hit else []))
            continue
        patterns = list(decision.value or [])
        provenance = _pattern_provenance(config, S.abstract(key_path) if "<" in key_path
                                         else key_path)
        hits = [(pattern, globs.matches(pattern, path), provenance.get(pattern, decision.source))
                for pattern in patterns]
        matched = any(hit for _, hit, _ in hits)
        detail = ("%d of %d patterns match: %s"
                  % (sum(1 for _, h, _ in hits if h), len(patterns),
                     ", ".join(p for p, h, _ in hits if h)) if matched
                  else "no pattern matches (%d considered)" % len(patterns))
        items.append(PathItem(key_path, detail, matched, decision, hits))

    for name, source in _layers_for(config, path):
        decision = config.decision("layers.%s" % name)
        if decision is None:
            continue
        items.append(PathItem("layers.%s" % name, "the path falls in this layer", True,
                              decision, [(prefix, True, source)
                                         for prefix in (decision.value or [])]))
    return items


def _pattern_provenance(config: L.Config, abstract_key: str) -> dict[str, str]:
    """Which layer each individual glob in a merged list came from.

    The answer is recoverable because every layer's *own* list is kept in the chain, so a
    `!keep-me.txt` can be traced to the file that wrote it — which is the question people
    actually ask when a file is mysteriously noisy.
    """
    provenance: dict[str, str] = {}
    for path in config.keys():
        if S.abstract(path) != abstract_key:
            continue
        for step in config.decisions[path].chain():
            if step.action in ("suppressed", "same-as-below"):
                continue
            for pattern in (step.value if isinstance(step.value, list) else []):
                provenance.setdefault(pattern, step.source)
    return provenance


def is_path_relevant(concrete_path: str) -> bool:
    abstract = S.abstract(concrete_path)
    return abstract in S.PATH_KEYS or abstract.startswith(("layers.", "noise.", "test.",
                                                           "non_code.", "graph."))


def _best_decision(config: L.Config, key_path: str) -> L.Decision | None:
    for path in config.keys():
        if S.abstract(path) == key_path:
            return config.decisions[path]
    return config.decision(key_path)


def _layers_for(config: L.Config, path: str) -> list[tuple[str, str]]:
    hits = []
    for decision in config.changed() + [config.decisions[p] for p in config.keys()]:
        if not S.abstract(decision.key).startswith("layers."):
            continue
        name = decision.key.split(".")[-1]
        for prefix in decision.value or []:
            if globs.matches(prefix, path) or globs.inside(path, prefix):
                hits.append((name, decision.source))
                break
    seen, unique = set(), []
    for name, source in hits:
        if (name, source) not in seen:
            seen.add((name, source))
            unique.append((name, source))
    return unique


def declared_keys(analyzer_id: str) -> list[str]:
    """The config keys one analyzer declared in ADR-004 §5.1's manifest."""
    from recommender.store.cache import ANALYZER_INDEX

    analyzer = ANALYZER_INDEX.get(analyzer_id)
    return list(analyzer.config_keys) if analyzer else []


_ANALYZER_IDS: list[str] | None = None


def analyzer_ids() -> list[str]:
    """The ADR-004 §5.1 manifest ids, read lazily.

    Lazy on purpose: importing the store at module import time would make the *renderer*
    depend on the storage fixture, and the renderer's whole job is to take a resolved
    `Config` and print it. A caller (the CLI, a test) may pin the list with
    :func:`set_analyzer_ids`; nobody may pin an empty list to hide the section.
    """
    if _ANALYZER_IDS is not None:
        return _ANALYZER_IDS
    try:
        from recommender.store.cache import ANALYZERS

        return [analyzer.id for analyzer in ANALYZERS]
    except Exception:                                   # noqa: BLE001 - render must not fail
        return []


def set_analyzer_ids(ids) -> None:
    """Pin the analyzer list (the CLI does; tests may too). `None` restores the lazy path."""
    global _ANALYZER_IDS
    _ANALYZER_IDS = list(ids) if ids is not None else None


# ---------------------------------------------------------------------------
# the privacy matrix (`fathom doctor --privacy` renders this, e1.10)
# ---------------------------------------------------------------------------
#: The consent keys, and the compile-time feature each one gates (ADR-002 §6.3). Two
#: columns, because two gates is the whole argument: compiled-in *and* consented.
CONSENT_FEATURES = {
    "privacy.advisories": ("net-advisories", "OSV advisory lookup (e2.7)"),
    "privacy.llm": ("net-llm", "optional narrative polish (e4.7)"),
    "privacy.mcp_http_transport": ("net-http-transport", "MCP HTTP listener (e6.4)"),
}


def privacy_matrix(config: L.Config) -> list[dict]:
    rows = []
    for key, (feature, purpose) in sorted(CONSENT_FEATURES.items()):
        decision = config.decision(key)
        rows.append({
            "capability": purpose,
            "feature": feature,
            "consent_key": key,
            "consented": bool(config.get(key, False)),
            "set_by": S.Layer.label(decision.winner) if decision else "built-in default",
            "source": decision.source if decision else "",
            "user_scoped": S.scope_of(key) == "user",
        })
    return rows


def privacy_text(config: L.Config) -> str:
    lines = ["network consent (default off everywhere; a repo file can never grant it)", ""]
    width = max(len(r["capability"]) for r in privacy_matrix(config))
    for row in privacy_matrix(config):
        lines.append("  %-*s  %-18s  %-28s  %s"
                     % (width, row["capability"], row["feature"],
                        "ON" if row["consented"] else "off", row["consent_key"]))
        if row["consented"]:
            lines.append("  %-*s  set by %s · %s" % (width, "", row["set_by"], row["source"]))
    lines.append("")
    lines.append("telemetry: none, ever — there is no key for it, and a key for it would be "
                 "a defect (ADR-001 §8 non-goal 5).")
    return "\n".join(lines) + "\n"


# ---------------------------------------------------------------------------
# generated documentation blocks
# ---------------------------------------------------------------------------
def doc_blocks() -> dict[str, str]:
    """Markdown blocks for docs/concepts/config.md, generated from the key spec."""
    layers = ["| Layer | Rank | Source | What it is |", "|---|---|---|---|"]
    for index, name in enumerate(S.Layer.ORDER):
        layers.append("| `%s` | %d | %s | %s |" % (name, index + 1, _layer_source(name),
                                                   _layer_doc(name)))
    sections = ["| Section | Owner | What it owns |", "|---|---|---|"]
    for name, meta in S.SECTIONS.items():
        sections.append("| `%s` | %s | %s |" % (name, meta["owner"], meta["doc"]))

    keys = ["| Key | Type | Default | Merge | Scope | Units | Declared by | Ticket |",
            "|---|---|---|---|---|---|---|---|"]
    for key in S.KEY_LIST:
        default = key.fallback if key.is_budget() else key.default
        keys.append("| `%s` | `%s` | %s | `%s` | `%s` | %s | %s | `%s` |"
                    % (key.path, key.kind, _cell(default), key.merge, key.scope,
                       key.units or "—", key.analyzer or "—", key.ticket))

    budgets = ["| Config key | Budget family (`benchmarks/budgets.json`) | Fallback |",
               "|---|---|---|"]
    for key in S.KEY_LIST:
        if key.budget:
            budgets.append("| `%s` | `%s` | %s |" % (key.path, key.budget, key.fallback))

    aliases = ["| Config key | Budget id | Why two names |", "|---|---|---|"]
    for path, family in sorted(S.BUDGET_ALIASES.items()):
        aliases.append("| `%s` | `%s` | ADR-004 §5.1 declares the key, §11 publishes the "
                       "budget; one value |" % (path, family))

    return {
        "precedence": "\n".join(layers) + "\n",
        "sections": "\n".join(sections) + "\n",
        "keys": "\n".join(keys) + "\n",
        "budgets": "\n".join(budgets) + "\n",
        "aliases": "\n".join(aliases) + "\n",
    }


def _cell(value) -> str:
    if isinstance(value, list):
        return "`%s`" % (", ".join(str(v) for v in value) or "—")
    if isinstance(value, dict):
        return "`%s`" % (", ".join("%s=%s" % kv for kv in sorted(value.items())) or "—")
    if value is None:
        return "—"
    return "`%s`" % value


def _layer_source(name: str) -> str:
    return {
        S.Layer.FLAG: "`--set k=v`, `--extend k=v`, sugar flags",
        S.Layer.ENV: "`FATHOM_*` (`__` = path separator)",
        S.Layer.REPO: "`<repo>/fathom.toml`",
        S.Layer.USER: "`$FATHOM_CONFIG`, `$XDG_CONFIG_HOME/fathom/config.toml`, `~/.config/fathom/config.toml`",
        S.Layer.DEFAULTS: "compiled-in + the size-class budget row",
    }[name]


def _layer_doc(name: str) -> str:
    return {
        S.Layer.FLAG: "this invocation, and the only layer that is transient by design",
        S.Layer.ENV: "CI and shell sessions; wins over files so a job can pin a value",
        S.Layer.REPO: "committed, reviewed, and hostile by default (see §5)",
        S.Layer.USER: "the person's own preferences, outside the repository",
        S.Layer.DEFAULTS: "what ships in the binary; no `/etc` file exists (see §3.1)",
    }[name]


def render_doc_blocks(text: str) -> str:
    """Insert or replace every generated block in a markdown document.

    Idempotent, because CI's `--check` compares against what `--write` produces: a
    generator that grows a blank line each time it runs is a generator whose own output
    fails the gate. Runs of three or more newlines are collapsed for the same reason — the
    prose may use one blank line between blocks, and only one.
    """
    blocks = doc_blocks()
    for name, body in blocks.items():
        begin = "<!-- BEGIN GENERATED: %s -->" % name
        end = "<!-- END GENERATED: %s -->" % name
        if begin in text and end in text:
            text = MARK.sub(lambda m, b=body: "%s\n%s%s" % (begin, b, end)
                            if m.group(1) == name else m.group(0), text)
        else:
            text = text.rstrip("\n") + "\n\n%s\n%s%s\n" % (begin, body, end)
    return re.sub(r"\n{3,}", "\n\n", text).rstrip("\n") + "\n"


def check_doc(path) -> list[str]:
    """`fathom config doc --check`: names of blocks that have drifted from the model."""
    text = Path(path).read_text(encoding="utf-8")
    blocks = doc_blocks()
    present = {m.group(1): m.group(2) for m in MARK.finditer(text)}
    problems = []
    for name, body in blocks.items():
        if name not in present:
            problems.append("%s: generated block `%s` is missing" % (path, name))
        elif present[name].strip() != body.strip():
            problems.append("%s: generated block `%s` is stale — run `fathom config doc "
                            "--write`" % (path, name))
    return problems


def schema_drift(path) -> tuple[bool, str]:
    return JS.drift(path)
