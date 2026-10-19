#!/usr/bin/env python3
"""Write the `e4.3` detector fixtures.

Run from the repository root: `python3 crates/fathom-analyze/tests/fixtures/detectors/make_fixtures.py`

Each fixture is an *input*: one episode, plus the episodes before it where the detector needs a
history. The expected findings beside them are blessed by the Rust test
(`cargo test -p fathom-analyze --test detector_fixtures` with `FATHOM_BLESS=1`), never hand-written,
because a hand-written expectation is a second opinion nobody ran.
"""

from __future__ import annotations

import json
import pathlib

HERE = pathlib.Path(__file__).resolve().parent
FIXTURES = HERE / "fixtures"
CLEAN = HERE / "clean"


def hunk(header, added=(), removed=()):
    return {"header": header, "added": list(added), "removed": list(removed)}


def file(path, added=(), removed=(), change="modified", add=None, gone=None, **extra):
    row = {
        "path": path,
        "change": change,
        "add": add if add is not None else len(added),
        "del": gone if gone is not None else len(removed),
        "hunks": [hunk(f"@@ -1,4 +1,{4 + len(added) - len(removed)} @@", added, removed)],
    }
    row.update(extra)
    return row


def episode(id_, files, scope=(), seq=4, prior_content="full", content="full", **extra):
    row = {
        "id": id_,
        "summary": extra.pop("summary", "invoice-api: agent turn"),
        "author_kind": extra.pop("author_kind", "agent"),
        "when_utc": 1_770_000_000_000 + seq * 3_600_000,
        "lineage_ref": "refs/heads/main",
        "lineage_seq": seq,
        "n_commits": 1,
        "declared_scope": list(scope),
        "files": files,
        "content": content,
    }
    row.update(extra)
    return row


def write(directory, name, detector, ep, prior=()):
    body = {"detector": detector, "episode": ep, "prior": list(prior)}
    (directory / f"{name}.json").write_text(json.dumps(body, indent=2) + "\n")


def main() -> None:
    for directory in (FIXTURES, CLEAN):
        directory.mkdir(parents=True, exist_ok=True)
        for stale in directory.glob("*.json"):
            # The expected findings beside these inputs are blessed by the Rust test, not authored here;
            # regenerating the inputs must not silently delete the answer it is checked against.
            if stale.name.endswith(".expected.json"):
                continue
            stale.unlink()

    # --- scope_creep -------------------------------------------------------
    write(
        FIXTURES,
        "scope_creep",
        "scope_creep",
        episode(
            "ep-fix-scope-creep",
            [
                file("src/api/orders.rs", added=["pub fn total(&self) -> Money { self.lines.total() }"]),
                file(
                    "db/migrations/0007_add_charge_column.sql",
                    change="added",
                    added=["ALTER TABLE charges ADD COLUMN refunded boolean;", "COMMENT ON COLUMN charges.refunded IS 'money';"],
                ),
                file("src/billing/charge.rs", added=["pub fn refund(&mut self) { self.amount = Zero; }"]),
            ],
            scope=["src/api/**"],
        ),
    )

    # --- untested_new_logic ------------------------------------------------
    logic = [
        "pub fn parse_grammar(input: &str) -> Grammar {",
        "    let mut items = Vec::new();",
        "    while let Some(item) = self.next() {",
        "        items.push(item?);",
        "    }",
        "    Grammar { items, source: input.to_owned() }",
        "}",
        "pub struct Grammar {",
        "    pub items: Vec<Item>,",
        "    pub source: String,",
        "}",
        "impl Grammar {",
        "    pub fn walk(&self, visit: &mut dyn FnMut(&Item)) {",
        "        for item in &self.items {",
        "            visit(item);",
        "        }",
        "    }",
        "}",
        "pub fn compile(&self) -> Program {",
        "        self.walk(&mut |item| self.emit(item));",
        "        Program::default()",
        "}",
        "pub fn lower(&self) -> Ir {",
        "        Ir::default()",
        "}",
    ]
    write(
        FIXTURES,
        "untested_new_logic",
        "untested_new_logic",
        episode("ep-fix-untested", [file("src/parser/grammar.rs", added=logic, gone=0)], scope=["src/**"]),
    )

    # --- test_weakening ----------------------------------------------------
    write(
        FIXTURES,
        "test_weakening",
        "test_weakening",
        episode(
            "ep-fix-test-weakening",
            [
                file(
                    "tests/api.rs",
                    added=["fn check_all(x: &[Order]) { assert!(true); }"],
                    removed=[f"assert_eq!(order.total(), {n});" for n in range(12)],
                )
            ],
            scope=["src/**", "tests/**"],
        ),
    )

    # --- debug_leftover ----------------------------------------------------
    write(
        FIXTURES,
        "debug_leftover",
        "debug_leftover",
        episode(
            "ep-fix-debug-leftover",
            [
                file(
                    "src/parse/lexer.rs",
                    added=["let token = self.next();", "dbg!(&token);", "self.position += 1;"],
                )
            ],
            scope=["src/**"],
        ),
    )

    # --- todo_debt ---------------------------------------------------------
    write(
        FIXTURES,
        "todo_debt",
        "todo_debt",
        episode(
            "ep-fix-todo-debt",
            [
                file(
                    "src/api/retry.rs",
                    added=[
                        "// TODO: back off exponentially, not flat",
                        "fn retry(&self) -> Result<()> {",
                        "    todo!()",
                        "}",
                    ],
                )
            ],
            scope=["src/**"],
        ),
    )

    # --- generated_edit ----------------------------------------------------
    write(
        FIXTURES,
        "generated_edit",
        "generated_edit",
        episode(
            "ep-fix-generated-edit",
            [
                file(
                    "src/generated/api_client.rs",
                    added=["        request.headers.insert(\"x-track\", self.track.clone());"],
                    path_class="generated",
                    noise_class="generated",
                )
            ],
            scope=["src/**"],
        ),
    )

    # --- secret_string -----------------------------------------------------
    write(
        FIXTURES,
        "secret_string",
        "secret_string",
        episode(
            "ep-fix-secret-string",
            [file("src/config.rs", added=['let stripe_key = "sk_live_51H8xQKZbT4n9rYq0Z3mW7c1Ld6Xe2Pf";'])],
            scope=["src/**"],
        ),
    )

    # --- lint_escape -------------------------------------------------------
    write(
        FIXTURES,
        "lint_escape",
        "lint_escape",
        episode(
            "ep-fix-lint-escape",
            [
                file(
                    "src/parse/lexer.rs",
                    added=["#[allow(clippy::unwrap_used)]", "fn peek(&self) -> char { self.buf[0].unwrap() }"],
                )
            ],
            scope=["src/**"],
        ),
    )

    # --- churn_loop --------------------------------------------------------
    region = "fn parse_item("
    churn = [
        episode(
            "ep-churn-1",
            [file("src/parse/item.rs", added=["        let price = self.consume();"], removed=["        let price = self.peek();"])],
            seq=1,
        ),
        episode(
            "ep-churn-2",
            [file("src/parse/item.rs", added=["        let price = self.peek();"], removed=["        let price = self.consume();"])],
            seq=2,
        ),
    ]
    churn_now = episode(
        "ep-churn-3",
        [file("src/parse/item.rs", added=["        let price = self.consume();"], removed=["        let price = self.peek();"])],
        seq=3,
    )
    # Same region each time: give every hunk the function context.
    for step in churn + [churn_now]:
        step["files"][0]["hunks"][0]["header"] = f"@@ -10,3 +10,3 @@ {region}"
    write(FIXTURES, "churn_loop", "churn_loop", churn_now, prior=churn)

    # --- rework_loop -------------------------------------------------------
    rework_lines = [
        "fn validate(order: &Order) -> Result<()> {",
        "    if order.total().is_negative() {",
        "        return Err(Error::Negative);",
        "    }",
        "    Ok(())",
        "}",
        "fn legacy_validate(order: &Order) -> bool {",
        "    order.total().as_cents() >= 0",
        "}",
        "// the old shape, kept for the migration window",
        "const LEGACY: bool = true;",
        "fn stamp() -> u64 { 0 }",
    ]
    rework_prior = episode(
        "ep-rework-1",
        [file("src/api/validate.rs", added=rework_lines, removed=[], change="added")],
        seq=1,
    )
    rework_now = episode(
        "ep-rework-2",
        [file("src/api/validate.rs", added=[], removed=rework_lines, change="modified")],
        seq=2,
    )
    write(FIXTURES, "rework_loop", "rework_loop", rework_now, prior=[rework_prior])

    # --- shotgun_edit ------------------------------------------------------
    clusters = ["src/parser", "src/api", "crates/fathom-store", "tools/migrate"]
    spread = []
    for cluster in clusters:
        for n in (1, 2):
            spread.append(file(f"{cluster}/file{n}.rs", added=[f"let tuned{n} = adjust();"]))
    write(FIXTURES, "shotgun_edit", "shotgun_edit", episode("ep-fix-shotgun", spread))

    # --- copy_paste_growth -------------------------------------------------
    block = [f"let charge_{n} = ledger.post(amount_{n}, currency);" for n in range(12)]
    write(
        FIXTURES,
        "copy_paste_growth",
        "copy_paste_growth",
        episode(
            "ep-fix-copy-paste",
            [file("src/billing/ledger.rs", added=block + block)],
            scope=["src/**"],
        ),
    )

    # --- the clean corpus: every one of the twelve stays out of the way -----
    write(
        CLEAN,
        "clean-tested-change",
        "*",
        episode(
            "ep-clean-1",
            [
                file("src/api/orders.rs", added=["pub fn total(&self) -> Money { self.lines.total() }"]),
                file(
                    "tests/api/orders.rs",
                    change="added",
                    added=["#[test]", "fn totals_the_order() {", "    assert_eq!(order.total(), Money::from(12));", "}"],
                ),
            ],
            scope=["src/**", "tests/**"],
        ),
    )

    write(
        CLEAN,
        "clean-moved-tests",
        "*",
        episode(
            "ep-clean-2",
            [
                file(
                    "tests/api.rs",
                    added=[f"assert_eq!(order.total(), Money::from({n}));" for n in range(20)],
                    removed=[f"assert_eq!(order.total(), {n});" for n in range(12)],
                )
            ],
            scope=["src/**", "tests/**"],
        ),
    )

    write(
        CLEAN,
        "clean-hard-bug",
        "*",
        episode(
            "ep-clean-3",
            [file("src/parse/item.rs", added=["        let price = self.consume_item(); // the fix"], removed=["        let price = self.peek();"])],
            seq=3,
            prior=[
                episode(
                    "ep-clean-3-a",
                    [file("src/parse/lexer.rs", added=["        self.skip_whitespace();"], removed=["        self.skip_spaces();"])],
                    seq=1,
                ),
                episode(
                    "ep-clean-3-b",
                    [file("src/parse/item.rs", added=["        let qty = self.consume_qty();"], removed=["        let qty = self.peek_qty();"])],
                    seq=2,
                ),
            ],
        ),
    )

    counts = len(list(FIXTURES.glob("*.json"))), len(list(CLEAN.glob("*.json")))
    print(f"wrote {counts[0]} fixtures and {counts[1]} clean episodes")


if __name__ == "__main__":
    main()
