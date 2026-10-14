"""The error vocabulary of configuration, with the exit code each one produces.

ADR-002 §7.3 makes the exit-code table a *published contract*: `5` is
`CONFIG_INVALID` and CI branches on it. A config error is therefore not a string, it
is a code plus the one thing a machine needs to act on it: where the bad value came
from. Every message produced here names a **source** (`fathom.toml:12`,
`$FATHOM_RISK__TAU`, `--set risk.tau=…`) because "your config is wrong" without a
file and line is the single most annoying diagnostic in existence, and because
`fathom config explain` promises to trace every value to a file — the error path has
to keep the same promise.

Python note: the Rust product returns `fathom_core::exit::ExitCode` from every verb;
here the code is an attribute on the exception and `cli.py` turns it into a process
exit.
"""

from __future__ import annotations

#: ADR-002 §7.3, verbatim. Kept as a table rather than magic numbers so a verb cannot
#: invent a code, which is how two scripts end up disagreeing about what `7` means.
EXIT_CODES = {
    "OK": 0,
    "ERROR": 1,
    "USAGE": 2,
    "NOT_A_REPOSITORY": 3,
    "STORE_CORRUPT": 4,
    "CONFIG_INVALID": 5,
    "CANCELLED": 6,
    "CHECK_VIOLATIONS": 7,
    "NETWORK_CONSENT_REQUIRED": 8,
    "FEATURE_ABSENT": 9,
    "UNSUPPORTED_PLATFORM": 10,
    "BUDGET_EXCEEDED": 11,
}


class ConfigError(Exception):
    """A configuration problem the user can act on.

    `hint` is mandatory-by-convention: ADR-001 §7.6 promises a stated partial answer
    with a remedy, and `fathom doctor` ("states a remedy for every problem it finds")
    reuses these objects rather than re-deriving advice from message text.
    """

    code = EXIT_CODES["CONFIG_INVALID"]
    kind = "config_invalid"

    def __init__(self, message: str, *, source: str = "", hint: str = ""):
        self.message = message
        self.source = source
        self.hint = hint
        super().__init__(self.render())

    def render(self) -> str:
        out = self.message
        if self.source:
            out = "%s (%s)" % (out, self.source)
        if self.hint:
            out += "\n  hint: %s" % self.hint
        return out

    @property
    def exit_code(self) -> int:
        return self.code


class NotARepository(ConfigError):
    """`<root>` is not a git repository, or `.git` is unreadable (exit 3).

    Distinct from `CONFIG_INVALID` because the remedy is different: CI running `fathom
    check` outside a checkout has a pipeline bug, not a config bug, and those two
    sentences must not arrive with the same code.
    """

    code = EXIT_CODES["NOT_A_REPOSITORY"]
    kind = "not_a_repository"


class ConfigInvalid(ConfigError):
    """`fathom.toml` failed the schema, or a value is the wrong type (exit 5)."""

    code = EXIT_CODES["CONFIG_INVALID"]
    kind = "config_invalid"


class ConfigVersionRejected(ConfigInvalid):
    """The file declares a config format newer than this binary understands.

    ADR-005 §6.6's rule — `schema_version` is checked before dispatch and never
    defaults to the newest version — applied to config: a file written by a newer
    Fathom must not be silently interpreted by an older one, because the older one
    would silently drop the keys it has never heard of and then *cache results under
    the wrong digest*.
    """

    kind = "config_version"


class UsageError(ConfigError):
    """A bad command-line argument: unknown key in `--set`, missing `=` (exit 2)."""

    code = EXIT_CODES["USAGE"]
    kind = "usage"


class NetworkConsentRequired(ConfigError):
    """A compiled-in `net-*` feature was asked to run without consent (exit 8).

    ADR-002 §6.3: the error message names the key that turns the feature on, so the
    user is never sent to the documentation to find out what to type.
    """

    code = EXIT_CODES["NETWORK_CONSENT_REQUIRED"]
    kind = "network_consent_required"


class Warning:
    """A non-fatal finding: an unknown key, a suppressed value, a coerced env var.

    Warnings are *data*, not prints. `explain`, `get` and `lint` all render them and
    CI asserts on their count, because a warning that only exists on stderr cannot be
    checked by anything.
    """

    __slots__ = ("code", "message", "source", "suggestion", "key")

    def __init__(self, code: str, message: str, *, source: str = "",
                 suggestion: str = "", key: str = ""):
        self.code = code
        self.message = message
        self.source = source
        self.suggestion = suggestion
        self.key = key

    def __eq__(self, other) -> bool:
        return (isinstance(other, Warning) and (self.code, self.message, self.source,
                                                self.suggestion, self.key) ==
                (other.code, other.message, other.source, other.suggestion, other.key))

    def __hash__(self):
        return hash((self.code, self.message, self.source, self.suggestion, self.key))

    def render(self) -> str:
        out = "warning[%s]: %s" % (self.code, self.message)
        if self.source:
            out += " (%s)" % self.source
        if self.suggestion:
            out += "\n  suggestion: %s" % self.suggestion
        return out

    def as_dict(self) -> dict:
        return {"code": self.code, "message": self.message, "source": self.source,
                "suggestion": self.suggestion, "key": self.key}

    def __repr__(self):
        return self.render()
