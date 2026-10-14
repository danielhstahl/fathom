"""The layering engine: precedence, merge strategy, provenance, and the digest.

One function, :func:`load`, decides what the configuration is; everything else in the
package either renders that decision or consumes it. That is the point of ADR-002 §4.5's
"exactly one process should decide what the config is" — a second resolver would be a
second opinion, and two opinions about a config is what `config explain` exists to end.

Precedence is the ticket's, unchanged:

    flag > env > repo fathom.toml > user config > built-in defaults

and three merge strategies decide how a layer combines with the ones underneath it:

`replace`
    the higher layer's value wins outright. Scalars, and lists whose **order is the
    meaning** (`episode.boundaries`).
`extend`
    list concatenation, lower layers first, de-duplicated keeping first occurrence. This
    is why a project can add `"**/generated/**"` to `noise.globs` without silently
    deleting the globs you personally rely on. Negations (`!keep-me.txt`) survive the
    concat and keep their position, because with globs position *is* semantics.
`deep`
    entry-wise, for open maps (`layers.<name>`, `risk.weights.<name>`,
    `keys.overrides.<mode>.<key>`). Free, in a sense: provenance is tracked per *concrete*
    path, so `layers.ui` from the repo file and `layers.store` from the defaults are two
    decisions with two sources and neither clobbers the other.

Every concrete path gets a :class:`Decision` with a full :class:`Step` chain, including
the steps that *lost* and the ones that were **suppressed** (a repo file trying to set a
user-scoped key). Losing steps are kept because "which layer won" is only half of
`explain`; "what did it beat, and why" is the half that ends a debugging session.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path

from . import globs
from . import json_schema as JS
from . import schema as S
from . import sources as SRC
from .errors import ConfigInvalid, Warning

_MISSING = object()


@dataclass
class Step:
    """One layer's attempt at one key."""

    layer: str
    source: str
    action: str                  # default | added | replaced | extended | same-as-below | suppressed
    value: object = None
    note: str = ""
    key_hint: str = ""

    def as_dict(self, *, secret: bool = False) -> dict:
        out = {"layer": self.layer, "source": self.source, "action": self.action}
        if self.note:
            out["note"] = self.note
        if self.action == "suppressed":
            out["value"] = "<suppressed>"
        else:
            out["value"] = redact(self.key_hint or "", self.value, secret=secret)
        return out


@dataclass
class Decision:
    """The resolved answer for one concrete key path, plus how we got there."""

    key: str                          # concrete path, e.g. `risk.weights.size`
    key_spec: str = ""                # declared path, e.g. `risk.weights.<weight>`
    value: object = None
    winner: str = S.Layer.DEFAULTS
    source: str = ""
    merge: str = "replace"
    steps: list[Step] = field(default_factory=list)
    secret: bool = False
    declared_by: str = ""            # analyzer that declares this key, ADR-004 §5.1
    ticket: str = ""

    @property
    def from_defaults(self) -> bool:
        return self.winner == S.Layer.DEFAULTS

    def render_value(self) -> str:
        return render_value(self.key, self.value, secret=self.secret)

    def chain(self) -> list[Step]:
        """Steps in precedence order, lowest first: how the value grew."""
        return sorted(self.steps, key=lambda s: -S.Layer.rank(s.layer))

    def as_dict(self) -> dict:
        out = {
            "key": self.key,
            "value": redact(self.key, self.value, secret=self.secret),
            "winner": self.winner,
            "winner_label": S.Layer.label(self.winner),
            "source": self.source,
            "merge": self.merge,
            "declared_by": self.declared_by,
            "ticket": self.ticket,
            "chain": [step.as_dict(secret=self.secret) for step in self.chain()],
        }
        if self.key_spec and self.key_spec != self.key:
            out["key_spec"] = self.key_spec
        return out


def render_value(path: str, value, *, secret: bool = False) -> str:
    """One-line rendering for tables and terminals, with secrets never disclosed."""
    value = redact(path, value, secret=secret)
    if isinstance(value, list):
        return ", ".join(str(v) for v in value) if value else "(empty)"
    if isinstance(value, dict):
        return ", ".join("%s=%s" % (k, v) for k, v in sorted(value.items())) or "(empty)"
    if isinstance(value, bool):
        return "true" if value else "false"
    return str(value)


def redact(path: str, value, *, secret: bool = False):
    """Replace secret values with a fingerprint.

    The fingerprint is not decoration: `explain` must remain useful for "did this actually
    change between the two runs", and a bare `***` cannot answer that. ADR-005 §6.4 does
    the same trick with `tool_input_sha256` for the same reason.
    """
    if not (secret or S.is_secret(path)):
        return value
    if isinstance(value, str):
        if not value:
            return "<empty>"
        import hashlib

        return "<redacted sha256:%s>" % hashlib.sha256(value.encode("utf-8")).hexdigest()[:8]
    if isinstance(value, dict):
        return {k: redact("%s.%s" % (path, k), v) for k, v in value.items()}
    if isinstance(value, list):
        return [redact(path, v) for v in value]
    return "<redacted>"


# ---------------------------------------------------------------------------
# the resolved config
# ---------------------------------------------------------------------------
@dataclass
class Config:
    values: dict
    decisions: dict[str, Decision]
    warnings: list[Warning] = field(default_factory=list)
    sources: list[SRC.Source] = field(default_factory=list)
    locations: SRC.Locations | None = None
    size_class: str = "small"
    size_class_reason: str = ""
    budgets_source: str = ""
    schema_version: int = S.CONFIG_VERSION

    # -- access ------------------------------------------------------------
    def get(self, path: str, default=_MISSING):
        cur: object = self.values
        for seg in path.split("."):
            if not isinstance(cur, dict) or seg not in cur:
                if default is _MISSING:
                    raise KeyError(path)
                return default
            cur = cur[seg]
        return cur

    def keys(self) -> list[str]:
        return sorted(self.decisions)

    def changed(self) -> list[Decision]:
        """Decisions where a layer above the defaults won — what `explain` prints first."""
        return [self.decisions[k] for k in sorted(self.decisions)
                if not self.decisions[k].from_defaults]

    def decision(self, path: str) -> Decision | None:
        return self.decisions.get(path)

    def table(self, name: str) -> dict:
        """A concrete open map, e.g. `table("layers") -> {"ui": [...], …}`."""
        value = self.get(name.rsplit(".", 1)[0] if "<" in name else name, {})
        return value if isinstance(value, dict) else {}

    # -- digests (ADR-004 §5.5) -------------------------------------------
    def digest(self, keys) -> str:
        """`config_digest` over *only* the declared keys, using the store's own function.

        Importing the digest from `recommender.store.cache` rather than reimplementing it
        is deliberate: ADR-004 §5.5 is normative about framing, the `absent` sentinel and
        the `prefix.` subtree rule, and a second implementation would be a second opinion
        about a cache key — the worst possible place for one.
        """
        from recommender.store.cache import derive_config_digest

        return derive_config_digest(self.values, list(keys))

    def digest_for(self, analyzer_id: str) -> str:
        """`config_digest` for exactly the keys one analyzer declared (ADR-004 §5.5)."""
        from recommender.store.cache import ANALYZER_INDEX

        analyzer = ANALYZER_INDEX.get(analyzer_id)
        if analyzer is None:
            raise KeyError(analyzer_id)
        return self.digest(analyzer.config_keys)

    # -- serialisation -----------------------------------------------------
    def as_dict(self, *, all_keys: bool = False) -> dict:
        decisions = [self.decisions[k].as_dict() for k in sorted(self.decisions)
                     if all_keys or not self.decisions[k].from_defaults]
        return {
            "config_version": self.schema_version,
            "precedence": " > ".join(S.Layer.ORDER),
            "size_class": self.size_class,
            "size_class_reason": self.size_class_reason,
            "budgets": self.budgets_source,
            "locations": self.locations.as_dict() if self.locations else None,
            "layers_present": [s.as_dict() for s in self.sources],
            "decisions": decisions,
            "warnings": [w.as_dict() for w in self.warnings],
        }


# ---------------------------------------------------------------------------
# loading
# ---------------------------------------------------------------------------
def load(cwd=None, *, repo_file=None, user_file=None, env=None, require_repo: bool = True,
         sets=(), extends=(), aliases=None, offline: bool = False,
         budgets=None, budgets_path=None, size_class: str | None = None,
         probe=None, schema: dict | None = None) -> Config:
    """Resolve every layer into one typed root config.

    `size_class` (the argument) is an escape hatch for callers that already know the
    class — `fathom index` knows, `config explain` does not. `probe` is injectable so the
    precedence tests never touch git.
    """
    # `None` means "read the process environment"; `{}` means "the env layer
    # contributes nothing" (what `--no-env` asks for). Conflating them would make
    # `--no-env` the only honest way to test precedence.
    root = Path(cwd) if cwd else None
    locations = SRC.discover(cwd=root, repo_file=repo_file, user_file=user_file, env=env,
                             require_repo=require_repo)
    schema = schema if schema is not None else JS.build_schema()

    repo_source = SRC.load_toml(locations.repo_file, S.Layer.REPO, schema=schema) \
        if locations.repo_file else None
    user_source = SRC.load_toml(locations.user_file, S.Layer.USER, schema=schema) \
        if locations.user_file else SRC.Source(layer=S.Layer.USER, name="user config",
                                               present=False)
    env_src = SRC.env_source(env)
    flag_src = SRC.parse_flags(sets=sets, extends=extends, aliases=aliases, offline=offline)

    budgets = budgets if budgets is not None else _budgets_or_fallback(budgets_path)

    declared_class = size_class or _first_set(S.Layer.ORDER, [flag_src, env_src, repo_source,
                                                             user_source], "repo.size_class") \
        or "auto"
    resolved_class, reason = _resolve_size_class(declared_class, locations, budgets, probe)
    budget_table = budgets.for_class(resolved_class) if resolved_class in budgets.by_class \
        else {}

    defaults_values = S.defaults(budget_table)
    defaults_source = SRC.Source(layer=S.Layer.DEFAULTS, name="built-in defaults",
                                 values=defaults_values, present=True)

    ordered = [defaults_source, user_source, repo_source, env_src, flag_src]
    config = merge(ordered, locations=locations, size_class=resolved_class,
                   size_class_reason=reason,
                   budgets_source=budgets.path if budget_table else "built-in fallback")

    _check_paths_contained(config)
    _check_consent(config)
    _check_globs(config)
    _check_weights(config)
    _check_noise_shadowing(config)
    return config


def _budgets_or_fallback(budgets_path) -> SRC.Budgets:
    """`benchmarks/budgets.json` when it is reachable; the declared fallbacks when not.

    A shipped binary has the budget rows compiled in, so it cannot fail this way. The
    contract fixture can: the file lives at the repository root, and an installed copy of
    the package has no repository root. Degrading to the per-key fallbacks keeps every
    default defined and says which source was used, instead of pretending the caps are
    the size-class rows when they are not.
    """
    candidate = Path(budgets_path) if budgets_path else (
        Path(__file__).resolve().parents[3] / "benchmarks" / "budgets.json")
    if candidate.exists():
        return SRC.read_budgets(candidate)
    return SRC.Budgets(by_class={}, size_classes={}, path="unavailable (fallback defaults)")


def _first_set(order, sources, path: str):
    """The winning layer's value for one key, needed *before* the defaults layer is built.

    `repo.size_class` is the only key with this property: it selects which budget row the
    defaults layer is built from, so it has to be resolved in precedence order first. A
    second pass would be a second resolver, and the whole argument for one resolver is that
    there is never a second one.
    """
    for layer in order:
        for source in sources:
            if source is None or not source.present or source.layer != layer:
                continue
            value = _dig(source.values, path)
            if value is not None:
                return value
    return None


def _dig(tree, path: str):
    cur: object = tree
    for seg in path.split("."):
        if not isinstance(cur, dict) or seg not in cur:
            return None
        cur = cur[seg]
    return cur


def _resolve_size_class(declared: str, locations, budgets: SRC.Budgets, probe) -> tuple[str, str]:
    if declared in SRC.SIZE_CLASSES:
        return declared, "named explicitly (layer above the defaults)"
    root = locations.repo_root
    if root is None:
        return "small", "no repository root: smallest class, and this line says so"
    probe = probe or SRC.probe_repo
    try:
        commits, files = probe(root)
    except Exception:                                       # noqa: BLE001 - degrade, never
        return "small", "probe failed: smallest class, and this line says so"
    if not budgets.size_classes:                            # pragma: no cover
        return "small", "no budget table: smallest class"
    chosen = SRC.size_class_for(commits, files, budgets.size_classes)
    return chosen, "probed %s commits / %s files" % (_count(commits), _count(files))


def _count(value: int) -> str:
    """Thousands with a thin space, because `200000 files` is unreadable and this is a
    human-facing diagnostic."""
    return "{:,}".format(value).replace(",", "\u2009")


# ---------------------------------------------------------------------------
# merge
# ---------------------------------------------------------------------------
def merge(ordered_sources, *, locations=None, size_class="small", size_class_reason="",
          budgets_source="") -> Config:
    """Apply layers lowest-precedence-first, recording a decision per concrete path.

    Every concrete path — including each entry of an open map — gets its own `Decision`,
    which is what makes `deep` merge fall out of the data model instead of needing a
    strategy: `layers.ui` and `layers.store` are two paths with two provenances, so a repo
    file that names one cannot erase the other.
    """
    decisions: dict[str, Decision] = {}
    warnings: list[Warning] = []
    values: dict = {}
    seen_sources: list[SRC.Source] = []

    for source in ordered_sources:
        if source is None:
            continue
        seen_sources.append(source)
        warnings.extend(source.warnings)
        if not source.present:
            continue
        for flat, value in sorted(S.flatten(source.values).items()):
            # decisions and `values` are keyed by the *concrete* path (`risk.weights.size`,
            # `layers.ui`); the declared spec path rides along for docs and lookups.
            spec = S.abstract(flat)
            key = S.key_for(spec)
            if key is None:
                continue                    # already warned about when the layer was read
            decision = decisions.get(flat)
            if decision is None:
                decision = Decision(key=flat, key_spec=spec, merge=key.merge,
                                    secret=key.secret or S.is_secret(flat),
                                    declared_by=key.analyzer, ticket=key.ticket)
                decisions[flat] = decision

            if not SRC.claim(source, flat):
                secret = key.secret or S.is_secret(flat)
                decision.steps.append(Step(source.layer, source.source_of(flat), "suppressed",
                                           value=redact(flat, value, secret=secret),
                                           key_hint=flat,
                                           note="scope=user: a repo file may not set it"))
                continue


            action, effective = _combine(key.merge, decision, value,
                                         force_replace=flat in source.replaces
                                         or S.abstract(flat) in source.replaces)
            if source.layer == S.Layer.DEFAULTS:
                action = "default"
            decision.value = effective
            _set_nested(values, flat.split("."), effective)
            if action != "same-as-below":
                decision.winner = source.layer
                decision.source = source.source_of(flat)
            decision.steps.append(Step(source.layer, source.source_of(flat), action,
                                       value=value, note=_note_for(key, source, action),
                                       key_hint=flat))

    return Config(values=values, decisions=decisions, warnings=_dedupe(warnings),
                  sources=seen_sources, locations=locations, size_class=size_class,
                  size_class_reason=size_class_reason, budgets_source=budgets_source)


def _combine(merge_strategy: str, existing: Decision, value, *, force_replace=False):
    """How a layer's value combines with what is already there.

    `force_replace` is `--set` overriding the key's strategy: the flag is an imperative
    ("this run, this value, no discussion"), while a file or an env var honours the
    strategy the key declares.
    """
    if force_replace:
        return ("replaced", value)
    if merge_strategy == "extend" and isinstance(value, list) \
            and isinstance(existing.value, list):
        added = [v for v in value if v not in existing.value]
        if not added:
            return ("same-as-below", existing.value)
        return ("extended", list(existing.value) + added)
    if not existing.steps:
        return ("added", value)          # this key had no value below this layer
    if existing.value == value:
        return ("same-as-below", value)
    return ("replaced", value)


def _note_for(key: S.Key, source: SRC.Source, action: str) -> str:
    if key.is_budget() and source.layer == S.Layer.DEFAULTS:
        return "budget row `%s` for this size class" % key.budget
    if action == "entry-merged":
        return "entry-wise merge under an open map"
    return ""



def _set_nested(tree: dict, segments: list[str], value) -> None:
    cur = tree
    for seg in segments[:-1]:
        nxt = cur.get(seg)
        if not isinstance(nxt, dict):
            nxt = {}
            cur[seg] = nxt
        cur = nxt
    cur[segments[-1]] = value


def _dedupe(warnings: list[Warning]) -> list[Warning]:
    seen, out = set(), []
    for warning in warnings:
        token = (warning.code, warning.message, warning.source)
        if token not in seen:
            seen.add(token)
            out.append(warning)
    return out


# ---------------------------------------------------------------------------
# cross-key consistency: the checks a per-key schema cannot express
# ---------------------------------------------------------------------------
def _check_paths_contained(config: Config) -> None:
    """A repo file may not point a write path outside the repository (ADR-001 §4.6)."""
    root = config.locations.repo_root if config.locations else None
    for path in ("paths.store", "paths.objects", "paths.events", "paths.export",
               "ratchet.baseline"):
        decision = config.decision(path)
        if decision is None or decision.winner != S.Layer.REPO:
            continue
        value = str(decision.value or "")
        if SRC.display(value).startswith("~") or globs.escapes_root(value):
            raise ConfigInvalid(
                "%s=%r would write outside the repository" % (path, value),
                source=decision.source,
                hint="a repo-provided config may only name paths inside the repository "
                     "(put the store elsewhere with $FATHOM_PATHS__STORE or the user "
                     "config, where you wrote it yourself)")
        if root is not None and not _inside_repo(value, root):
            raise ConfigInvalid("%s=%r resolves outside %s" % (path, value, SRC.display(root)),
                                source=decision.source,
                                hint="use a path relative to the repository root")


def _inside_repo(value: str, root) -> bool:
    resolved = Path(root, value).resolve() if not Path(value).is_absolute() else Path(value)
    try:
        resolved.relative_to(Path(root).resolve())
    except ValueError:
        return False
    return True


def _check_consent(config: Config) -> None:
    """Consent is per-feature *and* cross-key: transport http needs the key, and so does
    any bind that is not loopback (ADR-002 §6.3 — a listener is a privacy decision)."""
    transport = config.get("mcp.transport", "stdio")
    consent = bool(config.get("privacy.mcp_http_transport", False))
    if transport == "http" and not consent:
        raise ConfigInvalid(
            "mcp.transport = \"http\" needs consent: privacy.mcp_http_transport is false",
            source=(config.decision("mcp.transport") or Decision("mcp.transport")).source,
            hint="set `privacy.mcp_http_transport = true` in your user config (a repo "
                 "file cannot grant it), or keep mcp.transport = \"stdio\"")
    bind = str(config.get("mcp.bind", "127.0.0.1"))
    if bind not in ("127.0.0.1", "localhost", "::1") and not consent:
        raise ConfigInvalid("mcp.bind = %r is not loopback and needs consent" % bind,
                            source=(config.decision("mcp.bind") or Decision("mcp.bind")).source,
                            hint="set privacy.mcp_http_transport = true only if you mean "
                                 "it; the token in mcp.auth_token is the only thing "
                                 "between that bind and the network")
    if bool(str(config.get("mcp.auth_token", ""))) and transport != "http":
        config.warnings.append(Warning(
            "unused_secret", "mcp.auth_token is set but mcp.transport is not \"http\"",
            source=(config.decision("mcp.auth_token") or Decision("mcp.auth_token")).source,
            suggestion="remove the token, or use it: an unread secret in a config file "
                       "invites it into a bug report"))


def _check_globs(config: Config) -> None:
    """Bad globs are a warning naming the pattern, not a crash per bad character."""
    for path in ("noise.globs", "test.globs", "non_code.patterns"):
        for pattern in config.get(path, []) or []:
            try:
                globs.compile_glob(pattern)
            except globs.BadPattern as exc:
                config.warnings.append(Warning(
                    "bad_glob", "%s contains an unusable glob: %s" % (path, exc),
                    source=(config.decision(path) or Decision(path)).source, key=path))


def _check_weights(config: Config) -> None:
    """`risk.tau` is a threshold on a weighted score; if the weights do not add up, the
    threshold means something other than what the user typed."""
    weights = config.get("risk.weights", {}) or {}
    total = sum(v for v in weights.values() if isinstance(v, (int, float)))
    if weights and not (0.5 <= total <= 1.5):
        config.warnings.append(Warning(
            "weights_sum", "risk.weights sum to %.2f, not ~1.0, so risk.tau=%s is not a "
                           "percentage-like threshold" % (total, config.get("risk.tau")),
            suggestion="scale the weights, or set risk.tau against the scale you chose"))


def _check_noise_shadowing(config: Config) -> None:
    """A `!negation` that appears *before* the broad glob it wants to rescue cannot
    rescue it. Warn, because this is the single easiest globs mistake to make."""
    for path in ("noise.globs", "test.globs", "non_code.patterns"):
        patterns = config.get(path, []) or []
        for index, pattern in enumerate(patterns):
            if globs.is_negated(pattern):
                broad = [p for p in patterns[index + 1:] if not globs.is_negated(p)]
                if any(p.rstrip("/").endswith("/**") or "**" in p for p in broad):
                    continue
                config.warnings.append(Warning(
                    "negation_too_early",
                    "%s in %s is negated before any broad glob that could re-promote it"
                    % (pattern, path),
                    source=(config.decision(path) or Decision(path)).source, key=path,
                    suggestion="put the `!%s` line after the glob it exempts"
                               % pattern.lstrip("!")))


def load_for_test(**kwargs) -> Config:
    """A convenience wrapper the contract tests use: no budgets file, no git probe."""
    kwargs.setdefault("budgets", SRC.Budgets(by_class={}, size_classes={}))
    kwargs.setdefault("require_repo", False)
    kwargs.setdefault("probe", lambda root: (0, 0))
    return load(**kwargs)
