"""`fathom config …` — the verb surface of the config system.

The Rust product exposes these as `fathom config <subverb>` from `fathom-cli`
(ADR-002 §4.3: verbs parse, build a `Context`, dispatch, and return an exit code); this
module is the same surface in the contract fixture, so the acceptance criteria are
executable: `python3 -m recommender.config explain --path src/foo.rs`.

Every verb returns an exit code from ADR-002 §7.3 rather than raising, because a config
error is something CI branches on: `5` means "the file is wrong", `2` means "the command
was wrong", and the difference is the difference between "fix your repo" and "fix your
pipeline".
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from . import explain as X
from . import json_schema as JS
from . import layer as L
from . import schema as S
from . import sources as SRC
from .errors import ConfigError, EXIT_CODES


def _common() -> argparse.ArgumentParser:
    """Discovery flags that work before *and* after the subcommand.

    `SUPPRESS` defaults so a subparser-level occurrence does not clobber a top-level one
    with `None`: `fathom config --repo X explain` and
    `fathom config explain --repo X` are the same command, and a user should never have to
    learn argparse's opinion about that.
    """
    common = argparse.ArgumentParser(add_help=False)
    group = common.add_argument_group("discovery and overrides")
    group.add_argument("--repo", default=argparse.SUPPRESS,
                       help="repository root (default: discovered by walking up for .git)")
    group.add_argument("--repo-file", default=argparse.SUPPRESS,
                       help="explicit repo fathom.toml (default: <repo>/fathom.toml)")
    group.add_argument("-c", "--user-file", default=argparse.SUPPRESS,
                       help="explicit user config (default: $FATHOM_CONFIG, else "
                            "$XDG_CONFIG_HOME/fathom/config.toml, else "
                            "~/.config/fathom/config.toml)")
    group.add_argument("--no-env", action="store_true", default=argparse.SUPPRESS,
                       help="ignore FATHOM_* variables (the `env` layer contributes nothing)")
    group.add_argument("--set", action="append", default=argparse.SUPPRESS, metavar="KEY=VALUE",
                       help="set a key at the highest-precedence layer (repeatable)")
    group.add_argument("--extend", action="append", default=argparse.SUPPRESS,
                       metavar="KEY=VALUE",
                       help="append to a list-valued key instead of replacing it (repeatable)")
    group.add_argument("--offline", action="store_true", default=argparse.SUPPRESS,
                       help="force every privacy.* consent key off for this run")
    return common


def common_flags() -> argparse.ArgumentParser:
    """Public alias for :func:`_common`, so another verb can reuse the discovery flags.

    `e0.9`'s `fathom explain <path>` (`recommender.taxonomy.cli`) parents them onto its own
    parser rather than inventing a second set of flags for finding `fathom.toml`: two verbs
    with two opinions about discovery is how a person ends up explaining one path against
    the wrong file.
    """
    return _common()


def build_parser() -> argparse.ArgumentParser:
    common = _common()
    ap = argparse.ArgumentParser(
        prog="fathom config", parents=[common],
        description="Fathom configuration: discovery, layering and explanation (e0.8 / "
                    "reco-dq3). Precedence: %s." % " > ".join(S.Layer.ORDER))
    sub = ap.add_subparsers(dest="command", required=True)

    p = sub.add_parser("explain", parents=[common],
                       help="print which layer won for each effective value")
    p.add_argument("--path", default=None,
                   help="explain the resolved decision chain for one repository path")
    p.add_argument("--key", action="append", default=[], metavar="KEY",
                   help="explain only these keys (dotted path or section, repeatable)")
    p.add_argument("--all", action="store_true", help="include keys at their built-in default")
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("get", parents=[common], help="print one effective value")
    p.add_argument("key")
    p.add_argument("--json", action="store_true")

    p = sub.add_parser("schema", parents=[common],
                       help="print, check or use schemas/fathom.schema.json")
    p.add_argument("--write", nargs="?", const=JS.CHECKED_IN, default=None,
                   help="write the generated schema (default path if none given)")
    p.add_argument("--check", action="store_true",
                   help="exit 5 if the checked-in schema is not what this build generates")
    p.add_argument("--validate", default=None, metavar="FILE",
                   help="validate a fathom.toml against the schema; unknown keys warn, "
                        "wrong types exit 5")

    p = sub.add_parser("lint", parents=[common],
                       help="check config files without resolving a run")
    p.add_argument("--file", default=None, help="a fathom.toml to check (default: the pair)")
    p.add_argument("--strict", action="store_true",
                   help="exit 5 on warnings too (unknown keys included); off by default "
                        "so a newer file's keys never break an older binary's CI")

    sub.add_parser("privacy", parents=[common],
                   help="the network-consent matrix `fathom doctor --privacy` uses")
    sub.add_parser("keys", parents=[common],
                   help="list every key, its type and its layer scope")

    p = sub.add_parser("doc", parents=[common],
                       help="generate the config reference tables into a document")
    p.add_argument("--write", action="store_true")
    p.add_argument("--check", action="store_true")
    p.add_argument("--out", default="docs/concepts/config.md")

    p = sub.add_parser("init", parents=[common], help="print a commented starter fathom.toml")
    p.add_argument("--out", default=None)
    return ap


def _opt(args, name, fallback=None):
    """Read a flag that may have been given on either side of the subcommand."""
    return getattr(args, name, fallback)


def _load(args, *, require_repo: bool = True) -> L.Config:
    X.set_analyzer_ids(_analyzer_ids())
    env = {} if _opt(args, "no_env", False) else None
    return L.load(_opt(args, "repo"), repo_file=_opt(args, "repo_file"),
                  user_file=_opt(args, "user_file"), env=env, require_repo=require_repo,
                  sets=_opt(args, "set", []), extends=_opt(args, "extend", []),
                  offline=_opt(args, "offline", False))


def _analyzer_ids() -> list[str]:
    try:
        from recommender.store.cache import ANALYZERS

        return [analyzer.id for analyzer in ANALYZERS]
    except Exception:                                       # noqa: BLE001 - render must not fail
        return []


def main(argv=None) -> int:
    args = build_parser().parse_args(argv)
    try:
        return _dispatch(args)
    except ConfigError as exc:
        print("fathom config: %s" % exc.render(), file=sys.stderr)
        print("fathom config: exit %d (%s)"
              % (exc.exit_code, _code_name(exc.exit_code)), file=sys.stderr)
        return exc.exit_code
    except FileNotFoundError as exc:                        # pragma: no cover
        print("fathom config: %s" % exc, file=sys.stderr)
        return EXIT_CODES["ERROR"]


def _code_name(code: int) -> str:
    for name, value in EXIT_CODES.items():
        if value == code:
            return name
    return str(code)


def _dispatch(args) -> int:
    if args.command == "schema":
        return _cmd_schema(args)
    if args.command == "doc":
        return _cmd_doc(args)
    if args.command == "init":
        return _cmd_init(args)

    config = _load(args)

    if args.command == "explain":
        if args.path:
            if args.json:
                payload = {"path": args.path, "header": X.header_lines(config),
                           "decisions": [config.decisions[p].as_dict()
                                         for p in config.keys() if X.is_path_relevant(p)]}
                print(json.dumps(payload, indent=2, sort_keys=True))
            else:
                sys.stdout.write(X.explain_path_text(config, args.path))
            return 0
        text = X.explain_text(config, keys=args.key, all_keys=args.all)
        if args.json:
            print(json.dumps(config.as_dict(all_keys=args.all), indent=2, sort_keys=True))
        else:
            sys.stdout.write(text)
        return 0

    if args.command == "get":
        decision = config.decision(S.abstract(args.key)) or config.decision(args.key)
        table = config.get(args.key, None)
        if decision is None and isinstance(table, dict):
            # An open map or a section has no single decision: its entries do, and each of
            # those has its own provenance, so printing "the winner" here would be a lie.
            print(L.render_value(args.key, table))
            print("(a table: every entry has its own winning layer — "
                  "`fathom config explain --key %s`)" % args.key, file=sys.stderr)
            return 0
        if decision is None:
            suggestions = S.nearest(args.key)
            raise ConfigError("no config key %r" % args.key,
                              hint="did you mean %s?" % ", ".join(repr(s) for s in suggestions)
                              if suggestions else "see `fathom config keys`")
        if args.json:
            print(json.dumps(decision.as_dict(), indent=2, sort_keys=True))
        else:
            print(L.render_value(decision.key, decision.value, secret=decision.secret))
        return 0

    if args.command == "lint":
        return _cmd_lint(args, config)

    if args.command == "privacy":
        sys.stdout.write(X.privacy_text(config))
        return 0

    if args.command == "keys":
        for key in S.KEY_LIST:
            print("%-42s %-12s %-8s %-8s %s" % (key.path, key.kind, key.merge,
                                                key.scope + ("*" if key.secret else ""),
                                                key.doc.split(". ")[0]))
        print("\n* = redacted in every rendered output; scope `user` = never settable by a "
              "repo file")
        return 0

    return EXIT_CODES["USAGE"]


def _cmd_schema(args) -> int:
    if args.validate:
        schema = JS.build_schema()
        source = SRC.load_toml(args.validate, S.Layer.REPO, schema=schema)
        problems = [w for w in source.warnings]
        print("schema: %s" % JS.SCHEMA_ID)
        print("draft:  %s" % JS.DRAFT)
        print("keys:   %d declared" % len(S.KEY_LIST))
        for warning in problems:
            print(warning.render())
        print("validated %s: %d warning(s), 0 fatal" % (SRC.display(args.validate),
                                                        len(problems)))
        return 0
    if args.write:
        Path(args.write).write_text(JS.serialize(), encoding="utf-8")
        print("wrote %s (%d bytes)" % (args.write, len(JS.serialize())))
        return 0
    if args.check:
        path = _find_checked_in_schema()
        ok, message = JS.drift(path)
        if not ok:
            print("fathom config: %s" % message, file=sys.stderr)
            return EXIT_CODES["CONFIG_INVALID"]
        print("schema OK: %s matches this build (%d keys)" % (path, len(S.KEY_LIST)))
        return 0
    sys.stdout.write(JS.serialize())
    return 0


def _find_checked_in_schema() -> Path:
    root = Path(__file__).resolve().parents[3]
    return root / JS.CHECKED_IN


def _cmd_doc(args) -> int:
    path = Path(args.out)
    if args.write:
        if not path.exists():
            raise ConfigError("%s does not exist; write the prose first (the generator "
                              "fills blocks, it does not author the document)" % path)
        path.write_text(X.render_doc_blocks(path.read_text(encoding="utf-8")),
                        encoding="utf-8")
        print("wrote generated blocks into %s" % path)
        return 0
    if args.check:
        if not path.exists():
            print("fathom config: %s is missing" % path, file=sys.stderr)
            return EXIT_CODES["CONFIG_INVALID"]
        problems = X.check_doc(path)
        for problem in problems:
            print("fathom config: %s" % problem, file=sys.stderr)
        if problems:
            return EXIT_CODES["CONFIG_INVALID"]
        print("config doc OK: %s's generated tables match the key spec" % path)
        return 0
    sys.stdout.write("".join(X.doc_blocks().values()))
    return 0


def _cmd_init(args) -> int:
    text = render_starter_file()
    if args.out:
        Path(args.out).write_text(text, encoding="utf-8")
        print("wrote %s" % args.out)
        return 0
    sys.stdout.write(text)
    return 0


#: Example entries for the open maps, so `fathom config init` shows the shape rather
#: than the placeholder (`layers.<layer-name>` is true and useless as a line to edit).
OPEN_EXAMPLES = {
    "layers.<layer-name>": ("[layers]\n# api = [\"src/api\", \"src/edge\"]",
                             "one entry per layer name"),
    "taxonomy.classes.<class-name>": ('[taxonomy.classes]\n# generated = ["src/gen/**"]\n'
                                      '# source = ["vendor/our-fork/**"]',
                                      "one entry per taxonomy class (see `fathom taxonomy classes`)"),
    "risk.weights.<weight>": ("[risk.weights]\n# new_signal = 0.1",
                               "one entry per signal, 0..1"),
    "keys.overrides.<mode>.<binding>": ('[keys.overrides.queue]\n# j = "move_down"',
                                        "one entry per mode and key"),
}


def render_starter_file() -> str:
    """A commented `fathom.toml` generated from the key spec.

    Every key is commented out — an uncommented default is a value that has left the
    defaults layer and must now be maintained, and `init` should not silently move 48 keys
    into a file the user then has to review. The documentation lands where a person will
    actually read it: above the key, at the point of typing.
    """
    lines = [
        "# fathom.toml — Fathom configuration for this repository.",
        "#",
        "# Every key is optional and every key below is commented out. Layered as",
        "#   flags > $FATHOM_* > this file > user config > built-in defaults",
        "# and every effective value is explainable with `fathom config explain`,",
        "# including which of these four places a value came from.",
        "#",
        "# Keys with scope `user` are ignored in this file on purpose: a clone may not",
        "# grant network consent or hold a listener token on your behalf.",
        "",
    ]
    current = None
    for key in S.KEY_LIST:
        head = key.path.split(".")[0]
        if head != current:
            current = head
            lines.append("")
            lines.append("[%s]  # %s — %s" % (head, S.SECTIONS[head]["title"],
                                              S.SECTIONS[head]["owner"]))
        else:
            lines.append("")
        for sentence in _sentences(key.doc):
            lines.append(sentence)
        if key.scope == "user":
            lines.append("#   (user-scoped: setting this in a repo file has no effect)")
        if key.enum:
            lines.append("#   one of: %s" % ", ".join(key.enum))
        if key.units:
            lines.append("#   units: %s" % key.units)
        if key.merge == "extend":
            lines.append("#   merge: extend — this file adds to the layers below it")
        if key.is_open():
            example, note = OPEN_EXAMPLES[key.path]
            lines.append("#   open map — %s. e.g." % note)
            for row in example.splitlines():
                lines.append("# %s" % row)
        else:
            shown = key.fallback if key.is_budget() else key.default
            lines.append("# %s = %s" % (key.path, _tomlish(shown)))
    return "\n".join(lines).rstrip() + "\n"


def _sentences(doc: str) -> list[str]:
    """Doc string -> comment lines, one per sentence, at the point of typing."""
    return ["#   %s." % part.strip().rstrip(".")
            for part in doc.rstrip(".").split(". ")]



def _toml_string(value: str) -> str:
    """One TOML string literal, escaped into a form TOML can read back.

    A literal (`'...'`) string cannot contain an apostrophe, and a bare `"%s"` interpolation
    of a value that itself contains `"` produces TOML that does not parse at all — which the
    starter-file test discovers by uncommenting every line and parsing the result. A default
    like `non_code.iac.widened_patterns`, whose first entry is the *text* `"*"`, is the case
    that proves it. JSON's string escapes are a subset of TOML's basic-string escapes, so
    `json.dumps` is the shortest correct encoder here.
    """
    if "'" not in value and '"' not in value:
        return "'%s'" % value
    return json.dumps(value)


def _tomlish(value) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, list):
        return "[%s]" % ", ".join(_toml_string(str(item)) for item in value)
    if isinstance(value, dict):
        return "{ %s }" % ", ".join("%s = %s" % (k, _tomlish(v))
                                    for k, v in sorted(value.items()))
    if value is None:
        return '""'
    if isinstance(value, str):
        return _toml_string(value)
    return str(value)


def _cmd_lint(args, config: L.Config) -> int:
    """`config lint`: what the files say wrong, without needing a full run context."""
    checked = []
    if args.file:
        checked.append(SRC.load_toml(args.file, S.Layer.REPO))
    else:
        locations = config.locations
        if locations.repo_file:
            checked.append(SRC.load_toml(locations.repo_file, S.Layer.REPO))
        if locations.user_file:
            checked.append(SRC.load_toml(locations.user_file, S.Layer.USER))
    if not checked:
        print("config lint: no fathom.toml to check (looked in %s)"
              % (SRC.display(config.locations.repo_root) if config.locations else "."))
        return 0
    problems = 0
    for source in checked:
        for warning in source.warnings:
            problems += 1
            print(warning.render())
    if config.warnings:
        for warning in config.warnings:
            problems += 1
            print(warning.render())
    print("config lint: %d file(s) checked, %d finding(s)" % (len(checked), problems))
    if problems and args.strict:
        print("config lint: --strict makes findings fatal", file=sys.stderr)
        return EXIT_CODES["CONFIG_INVALID"]
    return 0


if __name__ == "__main__":                                        # pragma: no cover
    sys.exit(main())
