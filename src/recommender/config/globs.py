"""Glob matching for config-supplied path patterns — and nothing else.

`e0.9` (`reco-5vz`) owns the *taxonomy*: what a path **is**. This module only answers
the config-side question `fathom config explain --path P` needs: does this
user-supplied pattern match this path, and what did it come from. Keeping the matcher
here rather than inventing a taxonomy here is the difference between feeding e0.9 and
duplicating it, and duplicated glob knowledge is exactly the thing e0.9's ticket exists
to make impossible ("classifiers and detectors never encode their own glob lists").

Syntax, which is the syntax the glossary's *Noise suppression* entry implies:

* `*` within one segment, `**` across segments, `?` one character;
* `{a,b}` alternation;
* a leading `!` marks the pattern as a **demotion** — e0.9's engine and `explain`
  render those differently, and a negated match still counts as "this key is relevant
  to this path", which is the whole point of the `--path` view;
* matching is against the *repository-relative, forward-slashed* path only. Absolute
  paths are refused rather than guessed at, because a matcher that accepts an absolute
  path can be pointed outside the repository by a cloned config value.
"""

from __future__ import annotations

import re

_CACHE: dict[str, re.Pattern] = {}


class BadPattern(ValueError):
    """A glob that cannot be compiled: reported per key, not as a crash."""


def compile_glob(pattern: str) -> re.Pattern:
    """Translate one glob (without its `!`) to an anchored regex."""
    pat = pattern
    if pat.startswith("!"):
        pat = pat[1:]
    cached = _CACHE.get(pat)
    if cached is not None:
        return cached
    out = ["\\A"]
    i, n = 0, len(pat)
    while i < n:
        ch = pat[i]
        if ch == "*":
            if pat.startswith("**", i):
                # `**/` spans zero or more segments (so `**/dist/**` matches `dist/app.js`,
                # not only `a/dist/app.js`); a bare `**` spans anything.
                if pat.startswith("/", i + 2):
                    out.append("(?:[^/]+/)*")
                    i += 3
                    continue
                out.append(".*")
                i += 2
                continue
            out.append("[^/]*")
            i += 1
            continue
        if ch == "?":
            out.append("[^/]")
            i += 1
            continue
        if ch == "{":
            end = pat.find("}", i)
            if end == -1:
                raise BadPattern("unbalanced { in %r" % pattern)
            alts = pat[i + 1:end].split(",")
            out.append("(?:%s)" % "|".join(re.escape(a) for a in alts))
            i = end + 1
            continue
        if ch == "[":
            end = pat.find("]", i)
            if end == -1:
                raise BadPattern("unbalanced [ in %r" % pattern)
            out.append("[%s]" % pat[i + 1:end])
            i = end + 1
            continue
        out.append(re.escape(ch))
        i += 1
    out.append("\\Z")
    rx = re.compile("".join(out))
    _CACHE[pat] = rx
    return rx


def is_negated(pattern: str) -> bool:
    return pattern.startswith("!")


def matches(pattern: str, path: str) -> bool:
    """Whether `pattern` matches `path`. Directory-ish patterns also match their contents.

    `**/dist/**` is written as "everything under any `dist`", and `dist/**` should mean
    the same thing whether or not the caller wrote the trailing slash, so a pattern
    ending in `/` or `/**` additionally matches the directory itself.
    """
    path = normalise(path)
    try:
        rx = compile_glob(pattern)
    except BadPattern:
        return False
    if rx.match(path):
        return True
    if pattern.endswith("/**") and rx.match(path + "/"):
        return True
    if pattern.endswith("/") and path.startswith(pattern):
        return True
    return False


def matching(patterns, path: str) -> list[str]:
    """The subset of `patterns` that speak to `path`, in declaration order.

    Order is preserved and reported because e0.9's precedence and a user's mental model
    both depend on it: a `!keep-me.txt` *after* a broad `**/dist/**` means something
    different from one before it.
    """
    return [p for p in patterns if matches(p, path)]


def normalise(path: str) -> str:
    """Repo-relative, forward-slashed, no leading `./`. Refuses absolute paths."""
    text = str(path).replace("\\", "/").strip()
    if text.startswith("./"):
        text = text[2:]
    while text.startswith("/"):
        text = text[1:]
    return text


def inside(child: str, parent: str) -> bool:
    """Containment test used by the repo-file path rule (ADR-004 §12)."""
    c, p = normalise(child), normalise(parent)
    if not p:
        return True
    return c == p or c.startswith(p.rstrip("/") + "/")


def escapes_root(value: str) -> bool:
    """True if `value` cannot be resolved *inside* the repository root.

    A *cloned* `fathom.toml` pointing `paths.store` at `$HOME/.ssh/authorized_keys`
    is the exact threat ADR-001 §4.6 names ("a clone must never be able to make Fathom
    … write outside `.fathom/`"). The check is textual on purpose: the path need not
    exist to be refused, and resolving symlinks here would make the check itself
    raceable. Absolute and `~` paths are refused from a repo file, which is stricter
    than is strictly necessary and is the right trade for a value that names a
    database a process will open for write.
    """
    raw = str(value)
    if raw.startswith("~") or raw.startswith("/") or re.match(r"^[A-Za-z]:[\\/]", raw):
        return True
    return any(part == ".." for part in normalise(raw).split("/"))
