## fathom check

**fathom check passed df78bcfc1486..3e3f22dcc47a: 31 finding(s) below the `never` threshold, 4 check(s) ran** — `df78bcfc1486..3e3f22dcc47a` · policy v1 · mode warn · fail at `never`

| check | volume | status | judged | findings |
|---|---|---|---|---|
| `layering` | error | blocked | — | 0 |

> `layering` could not run: fathom.toml declares no [layering] rules, so nothing is enforced
>
> Try: declare `[layers]` and `[layering]`, or set `[check.severity] layering = "off"` to say you meant it
| `cycles` | error | ran | 129 | 0 |
| `api` | error | blocked | — | 0 |

> `api` could not run: the surface read stopped at 6000 files per side, so a "no breaking change" answer here would be an answer about a surface this run did not read
>
> Try: raise `[check] api_files`, or narrow the tracked surface with `[api] ignore`
| `dependencies` | warning | ran | 271 | 4 |
| `tests` | warning | ran | 155 | 27 |
| `sign-off` | warning | ran | 40 | 0 |

| severity | rule | what | where |
|---|---|---|---|
| warning | `dependencies/major-version` | astral-dev-toolchain-hyperfine moved a major version (1.20.0 → 2.0.0) | uv.lock (hunk:@@ -1671,8 +1580,8 @@) |
| warning | `dependencies/new-direct` | jinja2 3.1.6 is a new direct dependency (python: new in python/py-fuzzer/uv.lock) | python/py-fuzzer/uv.lock (hunk:@@ -7,9 +7,21 @@) |
| warning | `dependencies/new-direct` | ruff_unicode_names2 0.0.18 is a new direct dependency (cargo: new in Cargo.lock) | Cargo.lock (hunk:@@ -3793,8 +3763,25 @@) |
| warning | `dependencies/new-direct` | ruff_unicode_names2_generator 0.0.18 is a new direct dependency (cargo: new in Cargo.lock) | Cargo.lock (hunk:@@ -3793,8 +3763,25 @@) |
| warning | `tests/untested-logic` | episode ep-0b857a545 adds 683 lines of new logic with 0 test lines beside it, under the gate's floor of 50% tested | crates/ty_python_semantic/src/types/call/ambiguous.rs (index:episode ep-0b857a545) |
| warning | `tests/untested-logic` | episode ep-0ff29dbce adds 440 lines of new logic with 1 test lines beside it, under the gate's floor of 50% tested | crates/ty_python_semantic/src/types/function.rs (index:episode ep-0ff29dbce) |
| warning | `tests/untested-logic` | episode ep-15a4c7e2f adds 436 lines of new logic with 0 test lines beside it, under the gate's floor of 50% tested | crates/ty_project/src/metadata.rs (index:episode ep-15a4c7e2f) |
| warning | `tests/untested-logic` | episode ep-16144a5bb adds 656 lines of new logic with 0 test lines beside it, under the gate's floor of 50% tested | crates/ty_python_semantic/src/types/infer/expression_truthiness.rs (index:episode ep-16144a5bb) |
| warning | `tests/untested-logic` | episode ep-1f164ce23 adds 281 lines of new logic with 0 test lines beside it, under the gate's floor of 50% tested | crates/ty_python_semantic/src/types/protocol_class.rs (index:episode ep-1f164ce23) |
| warning | `tests/untested-logic` | episode ep-2b5f237f2 adds 302 lines of new logic with 0 test lines beside it, under the gate's floor of 50% tested | python/py-fuzzer/fuzz.py (index:episode ep-2b5f237f2) |
| warning | `tests/untested-logic` | episode ep-37c5438a1 adds 549 lines of new logic with 0 test lines beside it, under the gate's floor of 50% tested | crates/ty_python_semantic/src/types/overrides/attributes.rs (index:episode ep-37c5438a1) |
| warning | `tests/untested-logic` | episode ep-3aa4af977 adds 285 lines of new logic with 0 test lines beside it, under the gate's floor of 50% tested | crates/ty_python_semantic/src/types/overrides/attributes.rs (index:episode ep-3aa4af977) |
| warning | `tests/untested-logic` | episode ep-411ea53a4 adds 233 lines of new logic with 0 test lines beside it, under the gate's floor of 50% tested | crates/ty_python_semantic/src/types/infer/comparisons.rs (index:episode ep-411ea53a4) |
| warning | `tests/untested-logic` | episode ep-439b9ab98 adds 204 lines of new logic with 0 test lines beside it, under the gate's floor of 50% tested | crates/ty_python_semantic/src/types/overrides/attributes.rs (index:episode ep-439b9ab98) |
| warning | `tests/untested-logic` | episode ep-45b3cab53 adds 634 lines of new logic with 137 test lines beside it, under the gate's floor of 50% tested | crates/ty_python_semantic/src/types/instance.rs (index:episode ep-45b3cab53) |
| warning | `tests/untested-logic` | episode ep-45c823807 adds 241 lines of new logic with 0 test lines beside it, under the gate's floor of 50% tested | crates/ruff_python_literal/src/escape.rs (index:episode ep-45c823807) |
| warning | `tests/untested-logic` | episode ep-61a9fdbc3 adds 340 lines of new logic with 0 test lines beside it, under the gate's floor of 50% tested | crates/ty_python_semantic/src/types/constraints.rs (index:episode ep-61a9fdbc3) |
| warning | `tests/untested-logic` | episode ep-6addfc10d adds 260 lines of new logic with 0 test lines beside it, under the gate's floor of 50% tested | crates/ty_python_semantic/src/types/constraints/solutions.rs (index:episode ep-6addfc10d) |
| warning | `tests/untested-logic` | episode ep-74755b1be adds 336 lines of new logic with 0 test lines beside it, under the gate's floor of 50% tested | crates/ty_python_semantic/src/types/constraints.rs (index:episode ep-74755b1be) |
| warning | `tests/untested-logic` | episode ep-7664b30c8 adds 226 lines of new logic with 0 test lines beside it, under the gate's floor of 50% tested | crates/ruff_python_parser/src/parser/tests.rs (index:episode ep-7664b30c8) |
| warning | `tests/untested-logic` | episode ep-83f58ee5b adds 502 lines of new logic with 49 test lines beside it, under the gate's floor of 50% tested | crates/ty_ide/src/inlay_hints.rs (index:episode ep-83f58ee5b) |
| warning | `tests/untested-logic` | episode ep-99e218b1a adds 2096 lines of new logic with 0 test lines beside it, under the gate's floor of 50% tested | crates/ruff_unicode_names2/src/lib.rs (index:episode ep-99e218b1a) |
| warning | `tests/untested-logic` | episode ep-a74fae026 adds 264 lines of new logic with 0 test lines beside it, under the gate's floor of 50% tested | crates/ruff_python_literal/src/escape.rs (index:episode ep-a74fae026) |
| warning | `tests/untested-logic` | episode ep-af2e15091 adds 421 lines of new logic with 0 test lines beside it, under the gate's floor of 50% tested | crates/ty_python_semantic/src/types/protocol_class.rs (index:episode ep-af2e15091) |
| warning | `tests/untested-logic` | episode ep-b3aa0d247 adds 288 lines of new logic with 0 test lines beside it, under the gate's floor of 50% tested | crates/ty_python_semantic/src/types/constraints/sequents.rs (index:episode ep-b3aa0d247) |
| warning | `tests/untested-logic` | episode ep-bb38e1334 adds 229 lines of new logic with 0 test lines beside it, under the gate's floor of 50% tested | crates/ty_python_semantic/src/types/class/static_literal.rs (index:episode ep-bb38e1334) |
| warning | `tests/untested-logic` | episode ep-c4ddec8d5 adds 567 lines of new logic with 0 test lines beside it, under the gate's floor of 50% tested | crates/ty_python_semantic/src/types/constraints/projection/tests.rs (index:episode ep-c4ddec8d5) |
| warning | `tests/untested-logic` | episode ep-c92f8991a adds 414 lines of new logic with 48 test lines beside it, under the gate's floor of 50% tested | crates/ruff_workspace/src/options.rs (index:episode ep-c92f8991a) |
| warning | `tests/untested-logic` | episode ep-cbeab0211 adds 374 lines of new logic with 0 test lines beside it, under the gate's floor of 50% tested | crates/ty_ide/src/hover.rs (index:episode ep-cbeab0211) |
| warning | `tests/untested-logic` | episode ep-e2d9d9391 adds 980 lines of new logic with 0 test lines beside it, under the gate's floor of 50% tested | crates/ty_python_semantic/src/types/call/arguments.rs (index:episode ep-e2d9d9391) |
| warning | `tests/untested-logic` | episode ep-e6741f39d adds 213 lines of new logic with 0 test lines beside it, under the gate's floor of 50% tested | crates/ty_python_semantic/src/types/constraints/solutions.rs (index:episode ep-e6741f39d) |

Notices:

- fathom.toml is empty, so the gate runs on its built-in defaults
- `check.risk_episodes` is 40, so the newest 40 of 155 episodes were scored for sign-off

