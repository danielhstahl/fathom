"""The suppression policy: what may be folded, who said so, and what the key is.

One table decides the whole product decision — *"this diff is noise, fold it"* — because
four places would otherwise each grow an opinion: the diff tree, the timeline's collapse,
the episode card (`e2.10`) and the export (`e5.7`). Each rule is a row here, and every row
carries four things at once:

1. **the config key** a repository uses to change its mind (`noise.suppress.reformat`),
   resolved through `e0.8`'s five layers like any other key, so the answer carries the
   layer and the file that won;
2. **the session override**, which is not configuration at all: one keystroke, this
   process, never written to disk, always winning (ADR-006's "the reviewer is never
   trapped by a default");
3. **the expand affordance** — the key that shows the folded content — printed with the
   fold, never discovered;
4. **the reason**: a sentence naming the detector, its evidence and the layer that enabled
   it. The rule is that a fold with no reason is not representable: :class:`Suppression`
   refuses to exist without one (:class:`~recommender.noise.policy.InvisibleSuppression`
   is raised rather than a blank line being rendered), which is AC 3 as a type, not a
   hope.

Precedence, highest first:

```
session keystroke            this process only; "stop folding, show me"
flag / env / repo / user     `fathom.toml` and `--set`, via e0.8's resolver
built-in default             what this file says
```

and beneath all of it, `noise.keep` — a list of globs whose paths are *never* folded,
which is how a repository says "fold the protobufs, never fold `src/gen/api.py`" without
turning off a detector everywhere.

`noise.expand` deserves a word of apology: it is the same switch as the keystroke, but
persisted per repository, so a reviewer who never wants folding can say it once. When it is
on, nothing is hidden and every line still prints *why* nothing was hidden — the invariant
is one-directional and does not depend on the policy being in its folding mood.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Iterable

from recommender.config.globs import matching as _glob_matching

#: Every rule that can fold something, in the order they are consulted.
REFORMAT = "reformat"
GENERATED_CHURN = "generated-churn"
LOCKFILE = "lockfile"
VENDORED = "vendored"
USER_GLOB = "user-glob"
NOISE_ONLY = "noise-only"

EPISODE_RULES = (NOISE_ONLY,)


@dataclass(frozen=True)
class Rule:
    """One row of the policy table."""

    id: str
    title: str
    config_key: str
    default: bool
    expand: str                       # the keystroke that reveals what was folded
    scope: str                        # "file" | "episode"
    ticket: str = "e2.2"
    doc: str = ""
    detector: str = ""
    #: The rule as a reviewer-facing noun phrase, lower-case and short enough to sit inside a
    #: fold line: `⊘ [press b to show it] · policy: repo fathom.toml · src/a.rs folded
    #: (reformat-only change)`. The title is for headings; this is for the one line that has
    #: to carry everything.
    short: str = ""

    @property
    def is_episode_rule(self) -> bool:
        return self.scope == "episode"

    @property
    def phrase(self) -> str:
        return (self.short or self.title).lower()


RULES: tuple[Rule, ...] = (
    Rule(REFORMAT, "Reformat-only change", "noise.suppress.reformat", True, "b", "file",
         detector="reformat.compare", short="reformat-only change",
         doc="Token streams identical, or every changed line whitespace-only. The formatter, "
             "not a person, wrote the diff."),
    Rule(GENERATED_CHURN, "Generated-file churn", "noise.suppress.generated_churn", True,
         "b", "file", detector="generated.inspect", short="generated-file churn",
         doc="A generated file whose diff is stamps, a re-order or a bulk regeneration. "
             "Deliberately narrower than 'the path is generated': a small edit inside a "
             "generated file stays visible."),
    Rule(LOCKFILE, "Lockfile without its manifest", "noise.suppress.lockfile", True,
         "b", "file", detector="lockfile.summarize", short="lockfile-only change",
         doc="Resolved dependency state that moved on its own. If the manifest moved too, "
             "this is a dependency decision and the rule stays out of the way."),
    Rule(VENDORED, "Vendored code", "noise.suppress.vendored", True, "b", "file",
         detector="taxonomy verdict", short="vendored code",
         doc="Someone else's source, committed. Counted, listed, and never interleaved with "
             "the diff a person wrote."),
    Rule(USER_GLOB, "Noise this repository declared", "noise.suppress.user_glob", True,
         "b", "file", detector="noise.globs", short="a path this repository called noise",
         doc="`noise.globs` said so. The rule and the layer that wrote it are printed with "
             "the fold, because a glob nobody can see is a mystery, not a policy."),
    Rule(NOISE_ONLY, "Episode that is nothing but noise", "noise.collapse.noise_only", True,
         "b", "episode", short="nothing but noise",
         doc="Every file in the episode was folded by an enabled rule, so the episode is one "
             "line instead of a file list. The line carries the reason and the count."),
)


RULE_INDEX: dict[str, Rule] = {rule.id: rule for rule in RULES}
FILE_RULES: tuple[Rule, ...] = tuple(r for r in RULES if not r.is_episode_rule)

#: Config keys this package reads, for the analyzer manifest (ADR-004 §5.1).
CONFIG_KEYS: tuple[str, ...] = (
    "noise.suppress.reformat", "noise.suppress.generated_churn", "noise.suppress.lockfile",
    "noise.suppress.vendored", "noise.suppress.user_glob", "noise.collapse.noise_only",
    "noise.keep", "noise.expand",
)


#: Marks a rendered fold. It lives here, next to the object that renders itself, so the
#: marker cannot be forgotten by a caller: `Suppression.render()` always emits it and
#: `render.audit` counts folds by looking for it.
FOLD_MARK = "\u2298 "


#: `noise.expand`'s built-in value: the same key the TUI already binds (`diff.noise_toggle`,
#: `e1.7`) so the fold line and the keymap make one promise.
DEFAULT_EXPAND_KEY = "b"

class InvisibleSuppression(Exception):
    """A fold without a visible reason. Raised instead of rendering one. (AC 3.)"""


@dataclass(frozen=True)
class Session:
    """The per-session override: one keystroke, this process, never persisted.

    `suppress=None` means "no keystroke has been pressed": the configured policy applies
    untouched. Any other value is absolute — `False` folds nothing, whatever every repo
    file says, and `True` folds everything the rules allow, whatever a repo file turned
    off for the *defaults* comparison (a repo's own `= false` still beats the built-in
    default when the session says "as configured"; see :meth:`Policy.effective`).
    """

    suppress: bool | None = None
    pressed: str = ""                       # the chord that produced this state, for the print

    @property
    def active(self) -> bool:
        return self.suppress is not None

    def toggled(self, chord: str = DEFAULT_EXPAND_KEY) -> "Session":
        """The state after the keystroke, starting from wherever we are."""
        current = True if self.suppress is None else self.suppress
        return Session(suppress=not current, pressed=chord)

    def as_dict(self) -> dict:
        return {"suppress": self.suppress, "pressed": self.pressed, "active": self.active}


@dataclass
class Suppression:
    """One fold, with everything the UI must print alongside it.

    Construction is the enforcement point: no reason, no evidence, or no expand key and
    the object does not exist. `render()` is the only supported way to show it, and it
    cannot render a line without the rule, the reason and the key.
    """

    rule: str
    path: str                             # the file, or "" for an episode-level fold
    reason: str
    evidence: list[str] = field(default_factory=list)
    expand: str = "b"
    policy_source: str = ""               # "session keystroke `B`" | "repo fathom.toml" | …
    detail: str = ""                      # the detector's own headline, e.g. the lock summary
    lines: int = 0                        # folded line count, for the counts line

    def __post_init__(self) -> None:
        if not self.reason or not self.reason.strip():
            raise InvisibleSuppression("%s: a fold must carry a reason" % (self.path or "episode"))
        if not self.expand:
            raise InvisibleSuppression("%s: a fold must name the key that expands it"
                                       % (self.path or "episode"))
        if not self.evidence:
            raise InvisibleSuppression("%s: a fold must cite at least one piece of evidence"
                                       % (self.path or "episode"))
        if not self.policy_source.strip():
            self.policy_source = "not recorded"

    @property
    def rule_title(self) -> str:
        return RULE_INDEX[self.rule].title if self.rule in RULE_INDEX else self.rule

    @property
    def phrase(self) -> str:
        """The rule as a short lower-case phrase — what the fold line prints (see `Rule`)."""
        return RULE_INDEX[self.rule].phrase if self.rule in RULE_INDEX else self.rule.lower()

    def render(self) -> str:
        """The fold's header line — the invariant tokens come **first**.

        Order is not cosmetic. A pane that runs out of width must lose the rule title, not
        the key that undoes the fold, so the marker, the key and the policy lead and the
        descriptive part trails. `render.audit` asserts against this line only.
        """
        where = self.path if self.path else "episode"
        return "%s[press %s to show it] · policy: %s · %s folded (%s)" % (
            FOLD_MARK, self.expand, self.policy_source, where, self.phrase)

    def reason_line(self) -> str:
        """The visible reason. Printed immediately under the header, always."""
        return "— %s" % self.reason

    def block(self, indent: int = 0) -> list[str]:
        """Header, reason and evidence as lines: the whole fold, or nothing."""
        pad = " " * indent
        lines = [pad + self.render(), pad + "  " + self.reason_line()]
        lines.extend(pad + "  · %s" % line for line in self.evidence)
        return lines

    def as_dict(self) -> dict:
        return {"rule": self.rule, "rule_title": self.rule_title, "path": self.path,
                "reason": self.reason, "evidence": list(self.evidence),
                "expand": self.expand, "policy": self.policy_source, "detail": self.detail,
                "lines": self.lines}


@dataclass
class Policy:
    """The effective suppression policy for one repository and one session."""

    enabled: dict[str, bool] = field(default_factory=dict)
    source: dict[str, str] = field(default_factory=dict)
    keep: list[str] = field(default_factory=list)
    #: The key that shows a folded change again (`noise.expand`), and who set it. The fold
    #: lines are rendered from this value, so rebinding the key cannot leave a stale
    #: "[press b]" on screen.
    expand_key: str = DEFAULT_EXPAND_KEY
    expand_key_source: str = "built-in default"
    session: Session = field(default_factory=Session)
    config_digest: str = ""

    # -- construction -------------------------------------------------------
    @classmethod
    def built_in(cls) -> "Policy":
        return cls(enabled={rule.id: rule.default for rule in RULES})

    @classmethod
    def from_config(cls, config=None, *, session: Session | None = None) -> "Policy":
        """Resolve the eight keys through `e0.8`'s layers, keeping who won."""
        policy = cls(enabled={}, session=session or Session())
        for rule in RULES:
            value, source = _lookup(config, rule.config_key, rule.default)
            policy.enabled[rule.id] = value
            policy.source[rule.id] = source
        keep, _ = _lookup(config, "noise.keep", [])
        policy.keep = list(keep or [])
        expand, policy.expand_key_source = _lookup(config, "noise.expand", DEFAULT_EXPAND_KEY)
        policy.expand_key = str(expand or DEFAULT_EXPAND_KEY)
        if session is not None and session.suppress is not None:
            # The keystroke is absolute in one direction only: it turns folding off. Turning
            # it "on" means "as configured", never "override a repository's `= false`".
            if session.suppress is False:
                for rule in RULES:
                    policy.enabled[rule.id] = False
                    policy.source[rule.id] = "session keystroke `%s`" % (session.pressed or DEFAULT_EXPAND_KEY)
            policy.session = session
        try:
            policy.config_digest = config.digest_for("noise.suppress") if config else ""
        except Exception:                        # noqa: BLE001 - no manifest, no digest
            policy.config_digest = ""
        return policy

    def with_session(self, session: Session) -> "Policy":
        """The same repository policy, re-resolved under a session override."""
        base = Policy(enabled=dict(self.enabled), source=dict(self.source), keep=list(self.keep),
                      expand_key=self.expand_key, expand_key_source=self.expand_key_source,
                      config_digest=self.config_digest)
        if session.suppress is False:
            for rule in RULES:
                base.enabled[rule.id] = False
                base.source[rule.id] = "session keystroke `%s`" % (session.pressed or DEFAULT_EXPAND_KEY)
        elif session.suppress is True:
            for rule in RULES:
                if not self.source.get(rule.id, "").startswith("repo"):
                    base.enabled[rule.id] = rule.default
        base.session = session
        return base

    # -- the questions ------------------------------------------------------
    def enabled_for(self, rule_id: str) -> bool:
        return bool(self.enabled.get(rule_id, RULE_INDEX[rule_id].default))

    def effective(self, rule_id: str) -> tuple[bool, str]:
        """(enabled, who said so) — `who` is printed with every fold."""
        rule = RULE_INDEX[rule_id]
        return self.enabled_for(rule.id), self.source.get(rule.id, "built-in default")

    def keeps(self, path: str) -> str:
        """The `noise.keep` glob that protects this path, or `""`.

        Matching goes through `recommender.config.globs`, the matcher `e0.8` and `e0.9`
        already use: a second glob dialect in the tree is the duplication
        [paths.md](../../../docs/concepts/paths.md) exists to prevent.
        """
        hits = _glob_matching(self.keep, path) or _glob_matching(
            self.keep, path.rsplit("/", 1)[-1])
        return hits[0] if hits else ""

    def fold_allowed(self, rule_id: str, path: str) -> tuple[bool, str]:
        """Whether a rule may fold this path, and why not when it may not."""
        enabled, source = self.effective(rule_id)
        if not enabled:
            return False, "%s is off (%s)" % (RULE_INDEX[rule_id].config_key, source)
        kept = self.keeps(path)
        if kept:
            return False, "kept visible by `noise.keep` entry %s" % kept
        return True, source

    # -- output ------------------------------------------------------------
    def describe(self) -> list[str]:
        """The policy as printed text: `fathom noise policy`, and the `?` overlay."""
        out: list[str] = ["noise suppression policy (highest precedence first)"]
        if self.session.active:
            out.append("  session   %s" % self._session_line())
        out.append("  expand    `%s` shows a folded change again (%s)"
                   % (self.expand_key, self.expand_key_source))
        if self.keep:
            out.append("  keep      %s" % ", ".join(self.keep))
        for rule in RULES:
            enabled, source = self.effective(rule.id)
            out.append("  %-14s %-6s %-34s [%s] — %s"
                       % (rule.id, "on" if enabled else "off", rule.config_key,
                          self.expand_key, source))
        return out

    def _session_line(self) -> str:
        if self.session.suppress is False:
            return ("`%s` pressed: suppression is OFF for this session; every raw line is "
                    "shown and no repository setting was changed" % self.session.pressed)
        return ("`%s` pressed: suppression is ON, as the repository configures it"
                % self.session.pressed)

    def as_dict(self) -> dict:
        return {"rules": {rule.id: {"enabled": self.enabled_for(rule.id),
                                    "source": self.source.get(rule.id, "built-in default"),
                                    "config_key": rule.config_key, "expand": rule.expand,
                                    "scope": rule.scope} for rule in RULES},
                "keep": list(self.keep), "expand_key": self.expand_key,
                "expand_key_source": self.expand_key_source, "session": self.session.as_dict(),
                "config_digest": self.config_digest}


def _lookup(config, key: str, default):
    """(value, source) for one key, from a resolved `Config` or a bare dict."""
    if config is None:
        return default, "built-in default"
    decision = None
    getter = getattr(config, "decision", None)
    if callable(getter):
        decision = getter(key)
    if decision is not None:
        value = getattr(decision, "value", None)
        if value is None:
            value = default
        label = _layer_label(getattr(decision, "winner", "defaults"),
                             getattr(decision, "source", ""),
                             getattr(decision, "from_defaults", False))
        return value, label
    values = getattr(config, "values", config)
    cur: object = values
    for segment in key.split("."):
        if not isinstance(cur, dict) or segment not in cur:
            return default, "built-in default"
        cur = cur[segment]
    return cur, "config values (unresolved)"


def _layer_label(layer: str, source: str, from_defaults: bool) -> str:
    if from_defaults or layer in ("defaults", "default", ""):
        return "built-in default"
    return "%s layer%s" % (layer, " (%s)" % source if source else "")


def describe_rules() -> list[str]:
    """The rule table on its own: what each fold means, independent of any repository."""
    out: list[str] = []
    for rule in RULES:
        out.append("%-16s %-32s %s" % (rule.id, rule.config_key, rule.doc))
    return out


def rules_for_scope(scope: str) -> Iterable[Rule]:
    return tuple(rule for rule in RULES if rule.scope == scope)
