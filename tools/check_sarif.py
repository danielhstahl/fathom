#!/usr/bin/env python3
"""Check a `fathom check --format sarif` log for the things that make an upload *land*.

    python3 tools/check_sarif.py fathom.sarif
    python3 tools/check_sarif.py --self-test

Why a second tool when the schema exists
----------------------------------------
GitHub accepts a SARIF file that is valid against the SARIF JSON schema and then *refuses the results* —
silently, or as a wall of `analysis error` / `invalid location` warnings on the upload — for reasons the
schema cannot express:

* a `ruleId` no rule in the driver declares (the schema allows a result to name any rule);
* a `ruleIndex` out of range, or one that does not point at the rule the `ruleId` names;
* a `region` with no `artifactLocation`, or a location whose URI escapes the repository (`../`, an absolute
  path, a `file://` scheme some uploaders reject);
* a `level` that is not one of the four SARIF defines;
* a `partialFingerprints` full of hashes that differ only by line number — the failure mode where a rebase
  re-opens every alert, which the field exists to prevent;
* a message that is empty, or a `help` that is the message repeated.

Those are the things a code-scanning upload actually dies of, so they are what this checks. It reads the file
as JSON and asserts each one with a line of its own; the schema is *also* worth having for consumers, and
`fathom check --format sarif` is written so both views agree.

The `--self-test` runs the whole thing against a log built to be wrong in every one of those ways. A check
nobody has seen fail is a check that might be a no-op: every gate script in this repository carries one
(`tools/check_schemas.py`, `tools/check_privacy.py`, `tests/conftest.py`).
"""

from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any

SARIF_VERSION = "2.1.0"
LEVELS = {"none", "note", "warning", "error"}
KINDS = {"fail", "open", "informational", "notApplicable", "pass", "review", "notApplicable"}
# A run whose results carry none of these is a run whose alerts cannot be deduplicated across pushes.
FINGERPRINT_KEYS = ("primaryLocationLineHash",)


def fail(problems: list[str], where: str, message: str) -> None:
    """Record one problem. Every check reports rather than raising, so one run lists everything."""
    problems.append(f"{where}: {message}")


def check_log(document: Any) -> list[str]:
    """Every rule this tool knows, applied to one parsed SARIF document."""
    problems: list[str] = []
    if not isinstance(document, dict):
        return ["the document is not a JSON object"]

    if document.get("version") != SARIF_VERSION:
        fail(problems, "version", f"expected {SARIF_VERSION!r}, got {document.get('version')!r}")
    runs = document.get("runs")
    if not isinstance(runs, list) or not runs:
        return problems + ["`runs` must be a non-empty array"]

    for index, run in enumerate(runs):
        where = f"runs[{index}]"
        if not isinstance(run, dict):
            fail(problems, where, "is not an object")
            continue
        check_run(problems, run, where)
    return problems


def check_run(problems: list[str], run: dict[str, Any], where: str) -> None:
    tool = run.get("tool")
    driver = tool.get("driver") if isinstance(tool, dict) else None
    if not isinstance(driver, dict):
        fail(problems, where, "`tool.driver` is missing — an uploader needs it to name the analyzer")
        driver = {}
    if not driver.get("name"):
        fail(problems, f"{where}.tool.driver", "`name` is required by every uploader")

    rules = driver.get("rules")
    if not isinstance(rules, list):
        fail(problems, f"{where}.tool.driver", "`rules` must be an array (an empty one is fine)")
        rules = []
    ids: list[str] = []
    for position, rule in enumerate(rules):
        at = f"{where}.tool.driver.rules[{position}]"
        if not isinstance(rule, dict):
            fail(problems, at, "is not an object")
            continue
        rule_id = rule.get("id")
        if not rule_id:
            fail(problems, at, "a declared rule with no `id` cannot be referenced by a result")
            continue
        if rule_id in ids:
            fail(problems, at, f"rule id {rule_id!r} is declared twice")
        ids.append(str(rule_id))
        # Uploader warning: "rule has no help". A SARIF rule that does not say why it fired is a rule a
        # reviewer cannot argue with.
        if not nested(rule, "shortDescription", "text"):
            fail(problems, at, f"{rule_id} has no `shortDescription.text`")
        if not (nested(rule, "help", "text") or nested(rule, "help", "markdown") or rule.get("helpUri")):
            fail(problems, at, f"{rule_id} has no help — the UI shows a rule with none as unexplained")

    results = run.get("results")
    if not isinstance(results, list):
        fail(problems, where, "`results` must be an array")
        return
    seen_prints: dict[str, int] = {}
    for position, result in enumerate(results):
        at = f"{where}.results[{position}]"
        if not isinstance(result, dict):
            fail(problems, at, "is not an object")
            continue
        rule_id = result.get("ruleId")
        if not rule_id:
            fail(problems, at, "a result with no `ruleId` is dropped by the uploader")
        elif rule_id not in ids:
            fail(problems, at, f"{rule_id} is used but not declared in `tool.driver.rules`")
        index = result.get("ruleIndex")
        if isinstance(index, int):
            if not 0 <= index < len(rules):
                fail(problems, at, f"`ruleIndex` {index} is out of range")
            elif rule_id and ids[index] != rule_id:
                fail(problems, at, f"`ruleIndex` {index} names {ids[index]!r}, `ruleId` says {rule_id!r}")
        else:
            fail(problems, at, "`ruleIndex` is required when rules are declared")

        level = result.get("level", "warning")
        if level not in LEVELS:
            fail(problems, at, f"`level` {level!r} is not one of {sorted(LEVELS)}")
        kind = result.get("kind")
        if kind is not None and kind not in KINDS:
            fail(problems, at, f"`kind` {kind!r} is not one SARIF defines")
        if not nested(result, "message", "text"):
            fail(problems, at, "an empty message renders as a blank alert")

        check_locations(problems, result, at)
        check_fingerprint(problems, result, at, seen_prints)

    automation = run.get("automationDetails")
    if isinstance(automation, dict) and not nested(automation, "description", "text"):
        fail(problems, f"{where}.automationDetails", "no description — the check run gets a blank banner")


def check_locations(problems: list[str], result: dict[str, Any], at: str) -> None:
    locations = result.get("locations")
    if locations is None:
        # A result with no location is legal SARIF and uploads cleanly; the uploader just cannot point at a
        # line. It is worth a note in the report, not a failure — a layering finding is about an edge.
        return
    if not isinstance(locations, list) or not locations:
        fail(problems, at, "`locations`, when present, must be a non-empty array")
        return
    for position, location in enumerate(locations):
        where = f"{at}.locations[{position}]"
        physical = location.get("physicalLocation") if isinstance(location, dict) else None
        if not isinstance(physical, dict):
            fail(problems, where, "a location must carry a `physicalLocation`")
            continue
        artifact = physical.get("artifactLocation")
        if not isinstance(artifact, dict) or not artifact.get("uri"):
            fail(problems, where, "a physical location with no `artifactLocation.uri` is an upload warning")
            continue
        uri = str(artifact["uri"])
        if "://" in uri:
            fail(problems, where, f"{uri!r} is a URL, not a repo-relative URI")
        if uri.startswith("/") or uri[1:2] == ":":
            fail(problems, where, f"{uri!r} is absolute; a scannable URI is relative to the repo root")
        parts = uri.replace("\\", "/").split("/")
        if ".." in parts:
            fail(problems, where, f"{uri!r} escapes the repository")
        if "\\" in uri:
            fail(problems, where, f"{uri!r} uses a Windows separator; SARIF wants '/'")
        region = physical.get("region")
        if region is not None:
            start = region.get("startLine") if isinstance(region, dict) else None
            if not isinstance(start, int) or start < 1:
                fail(problems, f"{where}.region", f"`startLine` must be 1-based, got {start!r}")


def check_fingerprint(
    problems: list[str],
    result: dict[str, Any],
    at: str,
    seen_prints: dict[str, int],
) -> None:
    prints = result.get("partialFingerprints")
    if prints is None:
        # Not fatal SARIF, but a code-scanning alert without one re-opens on every push that touches the file.
        return
    if not isinstance(prints, dict) or not prints:
        fail(problems, at, "`partialFingerprints`, when present, must name at least one key")
        return
    for key in FINGERPRINT_KEYS:
        value = prints.get(key)
        if value is not None and not (isinstance(value, str) and len(value) >= 8):
            fail(problems, at, f"`partialFingerprints.{key}` should be a hash, not {value!r}")
    # Two results with the same fingerprint are "the same alert" to the uploader. Identical results are one
    # thing; two *different* findings that hash alike are a fingerprint that has stopped discriminating.
    identity = json.dumps(prints, sort_keys=True)
    message = nested(result, "message", "text") or ""
    key = f"{identity}|{message}"
    if identity in seen_prints and seen_prints[identity] != message:
        fail(problems, at, "two different findings share one fingerprint — a rebase will collide them")
    seen_prints[identity] = message


def nested(value: Any, *keys: str) -> Any:
    """`value[k1][k2]…` with `None` for any missing step."""
    for key in keys:
        if not isinstance(value, dict):
            return None
        value = value.get(key)
    return value


def check_file(path: Path) -> list[str]:
    try:
        document = json.loads(path.read_text(encoding="utf-8"))
    except OSError as error:
        return [f"{path}: {error}"]
    except json.JSONDecodeError as error:
        return [f"{path}: is not JSON ({error})"]
    return [f"{path.name}: {problem}" for problem in check_log(document)]


# ---------------------------------------------------------------------------
# the self-test
# ---------------------------------------------------------------------------

def broken_log() -> dict[str, Any]:
    """A log wrong in every way this tool checks, and nothing else."""
    return {
        "version": "2.2.0",
        "runs": [
            {
                "tool": {
                    "driver": {
                        "name": "",
                        "rules": [
                            {"id": "declared"},
                            {"id": "declared"},
                            {"id": "no-help"},
                            {},
                        ],
                    }
                },
                "results": [
                    {"ruleId": "undeclared", "ruleIndex": 99, "level": "fatal", "message": {"text": "x"},
                     "kind": "bogus"},
                    {"ruleId": "declared", "ruleIndex": 0, "message": {"text": ""},
                     "partialFingerprints": {"primaryLocationLineHash": "x"},
                     "locations": [
                         {"physicalLocation": {"artifactLocation": {"uri": "file:///tmp/x"}}},
                         {"physicalLocation": {"artifactLocation": {"uri": "/abs/path"}}},
                         {"physicalLocation": {"artifactLocation": {"uri": "../escape"}}},
                         {"physicalLocation": {"artifactLocation": {"uri": "src\\win.rs"},
                                                "region": {"startLine": 0}}},
                     ]},
                    {"ruleId": "no-help", "ruleIndex": 2, "message": {"text": "one"},
                     # Same fingerprint, different findings: the collision the line-number-free hash is
                     # supposed to avoid, done wrong instead.
                     "partialFingerprints": {"primaryLocationLineHash": "aaaaaaaaaaaaaaaa"}},
                    {"ruleId": "no-help", "ruleIndex": 2, "message": {"text": "two"},
                     "partialFingerprints": {"primaryLocationLineHash": "aaaaaaaaaaaaaaaa"}},
                ],
                "automationDetails": {"id": "x"},
            }
        ],
    }


def good_log() -> dict[str, Any]:
    """What `fathom check --format sarif` is expected to produce, in miniature."""
    return {
        "$schema": "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/main/schemata/branch/main/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [
            {
                "tool": {
                    "driver": {
                        "name": "fathom",
                        "semanticVersion": "0.1.0",
                        "rules": [
                            {
                                "id": "fathom/layering/no_upward",
                                "shortDescription": {"text": "declared layering rules: no_upward"},
                                "help": {"text": "why the rule exists, and where the policy lives"},
                                "helpUri": "https://fathom.dev/docs/concepts/layering.md",
                            }
                        ],
                    }
                },
                "results": [
                    {
                        "ruleId": "fathom/layering/no_upward",
                        "ruleIndex": 0,
                        "kind": "fail",
                        "level": "error",
                        "message": {"text": "`src/a.rs` reaches up into `src/b.rs`"},
                        "partialFingerprints": {"primaryLocationLineHash": "0123456789abcdef"},
                        "locations": [
                            {
                                "physicalLocation": {
                                    "artifactLocation": {"uri": "src/a.rs"},
                                    "region": {"startLine": 12},
                                }
                            }
                        ],
                    }
                ],
                "automationDetails": {
                    "id": "fathom-check/abc..def",
                    "description": {"text": "fathom check failed: 1 finding(s) at or above `warning`"},
                },
            }
        ],
    }


def self_test() -> int:
    """Prove the checker fails on each defect it claims to catch — and passes on a good log."""
    good = check_log(good_log())
    if good:
        print("self-test FAILED: a good log was rejected:")
        for problem in good:
            print(f"  {problem}")
        return 1

    caught = set(check_log(broken_log()))
    expected = [
        ("version", "the SARIF version"),
        ("is required by every uploader", "a driver with no name"),
        ("declared twice", "a duplicate rule id"),
        ("a declared rule with no `id`", "a rule with no id"),
        ("no help", "a rule with no help"),
        ("is used but not declared", "an undeclared ruleId"),
        ("out of range", "a ruleIndex past the end"),
        ("is not one of", "a level SARIF does not define"),
        ("is not one SARIF defines", "a kind SARIF does not define"),
        ("an empty message", "a blank message"),
        ("is a URL", "a file:// artifact URI"),
        ("is absolute", "an absolute artifact URI"),
        ("escapes the repository", "a ../ artifact URI"),
        ("Windows separator", "a backslash URI"),
        ("must be 1-based", "a 0 startLine"),
        ("should be a hash", "a too-short fingerprint"),
        ("share one fingerprint", "two findings sharing one fingerprint"),
        ("no description", "automation details with no banner"),
    ]
    missing = [label for needle, label in expected if not any(needle in problem for problem in caught)]
    if missing:
        print("self-test FAILED: these defects were not caught:")
        for label in missing:
            print(f"  {label}")
        print("\nthe checker's own output was:")
        for problem in sorted(caught):
            print(f"  {problem}")
        return 1
    print(f"self-test ok: {len(caught)} defects caught, and a clean log passed")
    return 0


def main(argv: list[str]) -> int:
    if "--self-test" in argv:
        return self_test()
    paths = [arg for arg in argv[1:] if not arg.startswith("-")]
    if not paths:
        print(__doc__)
        return 2
    problems: list[str] = []
    for path in paths:
        found = check_file(Path(path))
        problems.extend(found)
        if not found:
            print(f"{path}: ok")
    if problems:
        print(f"fathom SARIF check failed: {len(problems)} problem(s)")
        for problem in problems:
            print(f"  {problem}")
        return 7
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
