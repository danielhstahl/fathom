"""The labelled corpus: hand-authored pairs whose answer is known before the code runs.

`e2.1`'s corpus is `(path, content, class)`. This one is
`(path, before, after, expected claim)`, because the thing under test is a *relation
between two revisions* — and a fixture set for a relation has to contain the near-misses.
Every formatter-only case here is paired with a case that looks like it and is not: the
rustfmt commit beside the commit that changes one literal, the black re-wrap beside the
statement joined with a semicolon, the gofmt re-indent beside the gofmt re-indent that
also flips `==` to `===`. A corpus of only positives would let a whitespace squash pass at
100 %, which is precisely the failure this ticket exists to avoid.

Three honesty rules, copied from `e2.1` because they are right:

* **Expected answers are what a reviewer would agree with**, checked against the formatter
  conventions each case names in its `note`, never against what the detector currently
  says. Where the detector is deliberately conservative (a YAML block sequence becoming a
  flow sequence, shfmt's `${x}` quoting, a raw-string rewrite) the row says so in its note
  and expects `content`: the cost of that answer is a diff a reviewer skips by hand, and
  the cost of the other answer is a diff nobody looks at.
* **Every language in the table gets rows, and so does every refusal.** An unknown
  extension, an unterminated literal and a truncated blob are rows, not gaps: the claim
  they must produce (`content`, never a silent `reformat`) is the interesting part.
* **Groups are named by shape** (`rustfmt`, `prettier`, `black`, `gofmt`, `generated`,
  `lockfile`, `refusal`) so a regression reads as "the gofmt rows went down" instead of a
  number moving.

Episode scenarios are a second table: an episode is what gets collapsed, so the scenarios
say which ones must collapse, which must not, and what the collapsed line has to
mention. `eval.py` scores both tables, `fathom noise score` prints them, and CI gates on
the false-suppression rate — precision first, because that is the error that hides work.
"""

from __future__ import annotations

from dataclasses import dataclass, field

# ---------------------------------------------------------------------------
# source pairs used by several cases
# ---------------------------------------------------------------------------

RUST_PLAIN = '''use std::collections::HashMap;
use std::collections::BTreeMap;

/// Sum two numbers and keep the map around.
pub fn thing(a: i32, b: i32) -> i32 {
    let mut map = HashMap::new();
    map.insert("a", a);
    let total = a + b;
    total + map.len() as i32
}
'''

RUST_REFORMATTED = '''use std::collections::BTreeMap;
use std::collections::HashMap;

/// Sum two numbers and keep the map around.
pub fn thing(
    a: i32,
    b: i32,
) -> i32 {
    let mut map = HashMap::new();
    map.insert("a", a);
    let total = a + b;
    total + map.len() as i32
}
'''

RUST_REFORMATTED_TIGHT = '''use std::collections::BTreeMap;
use std::collections::HashMap;

/// Sum two numbers and keep the map around.
pub fn thing(a: i32, b: i32) -> i32 {
    let mut map = HashMap::new();
    map.insert("a", a);
    let total = a + b;
    total + map.len() as i32
}
'''

JS_PLAIN = '''import { createStore } from 'redux';
import rootReducer from "./root";

const double = (x) => x * 2;

export function build(name, items) {
  const out = { name: name, items: items.map(x => double(x)) };
  const label = 'built: ' + name;
  return out;
}
'''

JS_PRETTIER = '''import { createStore } from "redux";
import rootReducer from "./root";

const double = x => x * 2;

export function build(name, items) {
  const out = { name: name, items: items.map(x => double(x)) };
  const label = "built: " + name;
  return out;
}
'''

JS_JSX = '''export function Card({ title, items }) {
  return (
    <div className="card">
      <h2>{title}</h2>
      <ul>
        {items.map(item => <li key={item.id}>{item.label}</li>)}
      </ul>
    </div>
  );
}
'''

JS_JSX_REWRAPPED = '''export function Card({ title, items }) {
\treturn (
\t\t<div className="card">
\t\t\t<h2>{title}</h2>
\t\t\t<ul>
\t\t\t\t{items.map((item) => (
\t\t\t\t\t<li key={item.id}>{item.label}</li>
\t\t\t\t))}
\t\t\t</ul>
\t\t</div>
\t);
}
'''

PY_PLAIN = '''import sys
import os

def report(rows, prefix='total'):
    out = {}
    for row in rows:
        out[row.name] = row.value
    return out
'''

PY_BLACK = '''import os
import sys


def report(rows, prefix="total"):
    out = {}
    for row in rows:
        out[row.name] = row.value
    return out
'''

PY_LONGCALL = '''from collections import OrderedDict, defaultdict


def render(title, rows, style, show_totals):
    return [title] + list(rows) + [style, str(show_totals)]
'''

PY_BLACK_WRAPPED = '''from collections import OrderedDict, defaultdict


def render(
    title, rows, style, show_totals
):
    return [title] + list(rows) + [style, str(show_totals)]
'''

GO_PLAIN = '''package main

import (
	"fmt"
)

// Print writes the sum.
func Print(a, b int) {
	// keep the alignment
	fmt.Println(a+b)
}
'''

GO_GOFMT = '''package main

import (
	"fmt"
)

// Print writes the sum.
func Print(a, b int) {
\t// keep the alignment
\tfmt.Println(a + b)
}
'''

PROTO_PLAIN = '''syntax = "proto3";

package fathom.v1;

message Episode {
  string id = 1;
  string title = 2;
  repeated string files = 3;
}
'''

PROTO_REFLOWED = '''syntax = "proto3";
package fathom.v1;

message Episode
{
	string id = 1;
	string title = 2;
	repeated string files = 3;
}
'''

SNAPSHOT_A = '''// Jest Snapshot v1, https://goo.gl/fbAQLP

exports[`renders the queue 1`] = `
<div class="queue">
  <li>ep-1</li>
</div>
`;
'''

SNAPSHOT_B = '''// Jest Snapshot v1, https://goo.gl/fbAQLP

exports[`renders the queue 1`] = `
<div class="queue">
  <li>ep-1</li>
  <li>ep-2</li>
</div>
`;
'''

MD_PLAIN = '''# Suppression

The policy folds a diff when the diff carries no
reviewable information, and prints the rule that did it.

* reformat
+ generated churn
- lockfile churn

| rule | default |
| ---- | ------- |
| b    | on      |
'''

MD_PRETTIER = '''# Suppression

The policy folds a diff when the diff carries no reviewable information, and prints the
rule that did it.

- reformat
- generated churn
- lockfile churn

| rule | default |
| ---- | ------- |
| b    | on      |
'''

MD_CODE_MOVED = '''# Suppression

The policy folds a diff when the diff carries no reviewable information.

```bash
fathom noise explain src/a.rs
fathom noise policy
```
'''

MD_CODE_REWRAPPED = '''# Suppression

The policy folds a diff when the diff carries no reviewable information.

```bash
fathom   noise   explain   src/a.rs
fathom noise policy
```
'''

CARGO_LOCK_A = '''# This file is automatically @generated by Cargo.
# It is not intended for manual editing.
version = 3

[[package]]
name = "fathom-core"
version = "0.1.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "1111111111111111111111111111111111111111111111111111111111111111"

[[package]]
name = "serde"
version = "1.0.219"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "2222222222222222222222222222222222222222222222222222222222222222"

[[package]]
name = "serde_derive"
version = "1.0.219"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "3333333333333333333333333333333333333333333333333333333333333333"
'''

CARGO_LOCK_B = CARGO_LOCK_A.replace("1.0.219", "1.0.220")

CARGO_LOCK_RESORTED = '''# This file is automatically @generated by Cargo.
# It is not intended for manual editing.
version = 3

[[package]]
name = "serde"
version = "1.0.219"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "2222222222222222222222222222222222222222222222222222222222222222"

[[package]]
name = "serde_derive"
version = "1.0.219"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "3333333333333333333333333333333333333333333333333333333333333333"

[[package]]
name = "fathom-core"
version = "0.1.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "1111111111111111111111111111111111111111111111111111111111111111"
'''

PB_GO_SMALL_A = '''// Code generated by protoc-gen-go. DO NOT EDIT.
// versions:
// \tprotoc-gen-go v1.36.1
// \tprotoc\t\tv3.21.12

package pb

type Episode struct {
\tId    string `protobuf:"bytes,1,opt,name=id" json:"id,omitempty"`
\tTitle string `protobuf:"bytes,2,opt,name=title" json:"title,omitempty"`
}
'''

PB_GO_SMALL_B = '''// Code generated by protoc-gen-go. DO NOT EDIT.
// versions:
// \tprotoc-gen-go v1.36.6
// \tprotoc\t\tv3.21.12

package pb

type Episode struct {
\tId    string `protobuf:"bytes,1,opt,name=id" json:"id,omitempty"`
\tTitle string `protobuf:"bytes,2,opt,name=title" json:"title,omitempty"`
}
'''

PB_GO_RENAMED_FIELD = PB_GO_SMALL_B.replace("\tId    string", "\tIdent string")

PB_GO_BULK_A = PB_GO_SMALL_A + "\n".join(
    "type Msg%d struct {\n\tA string `json:\"a\"`\n}" % i for i in range(60)) + "\n"
PB_GO_BULK_B = PB_GO_SMALL_A.replace("v1.36.1", "v1.36.6") + "\n".join(
    "type Msg%d struct {\n\tA string `json:\"a,omitempty\"`\n}" % i for i in range(60)) + "\n"


# ---------------------------------------------------------------------------
# the pair table
# ---------------------------------------------------------------------------
@dataclass(frozen=True)
class Pair:
    """One file pair with the claim a reviewer agrees with."""

    name: str
    path: str
    before: str
    after: str
    expect: str                       # reformat | content | undetermined
    shape: str
    strength: str = ""                # tokens | whitespace, when it matters
    note: str = ""


PAIRS: tuple[Pair, ...] = (
    # --- rustfmt -----------------------------------------------------------
    Pair("rustfmt rewrap + trailing comma + import sort", "src/thing.rs", RUST_PLAIN,
         RUST_REFORMATTED, "reformat", "rustfmt", "tokens",
         "rustfmt re-wraps the signature, adds the trailing comma and sorts `use` lines"),
    Pair("rustfmt wrap only", "src/thing.rs", RUST_PLAIN, RUST_REFORMATTED_TIGHT, "reformat",
         "rustfmt", "tokens", "the same file at a narrower max_width"),
    Pair("rustfmt moves a trailing comment", "src/thing.rs",
         RUST_PLAIN.replace("let total = a + b;", "let total =\n        a + b; // sum"),
         RUST_PLAIN.replace("let total = a + b;", "let total = a + b; // sum"),
         "reformat", "rustfmt", "tokens",
         "the comment's text is unchanged and only its column moved"),
    Pair("rust edits a comment", "src/thing.rs", RUST_PLAIN,
         RUST_PLAIN.replace("Sum two numbers", "Add two numbers"), "content", "rustfmt", "",
         "a comment is content: rustfmt does not rewrite prose"),
    Pair("rust one literal changes", "src/thing.rs", RUST_PLAIN,
         RUST_PLAIN.replace('map.insert("a", a)', 'map.insert("b", a)'), "content", "rustfmt",
         "", "the classic false positive of a whitespace flag"),
    Pair("rust one operator changes", "src/thing.rs", RUST_PLAIN,
         RUST_PLAIN.replace("a + b", "a - b"), "content", "rustfmt"),
    Pair("rust adds a derive", "src/thing.rs", "#[derive(Clone)]\n" + RUST_PLAIN, RUST_PLAIN,
         "content", "rustfmt", "", "deleting an attribute is a semantic change"),
    Pair("rust char vs string", "src/thing.rs", "static A: char = 'x';\n",
         'static A: &str = "x";\n', "content", "rust", "",
         "a char is not a string; the delimiters are not interchangeable here"),
    Pair("rust macro rewrap", "src/thing.rs", "vec_stream!(a, b, c, d, e, f, g)",
         "vec_stream!(\n    a,\n    b,\n    c,\n    d,\n    e,\n    f,\n    g,\n)",
         "reformat", "rustfmt", "tokens"),
    # --- prettier ----------------------------------------------------------
    Pair("prettier quotes + arrow parens", "src/build.js", JS_PLAIN, JS_PRETTIER, "reformat",
         "prettier", "tokens", "prettier's two non-whitespace edits, plus the ones a flag owns"),
    Pair("prettier reindents JSX", "src/card.jsx", JS_JSX, JS_JSX_REWRAPPED, "reformat",
         "prettier", "tokens", "tabs, wrap, and `(item) =>` parentheses"),
    Pair("prettier renames a prop", "src/card.jsx", JS_JSX,
         JS_JSX.replace("item.label", "item.title"), "content", "prettier"),
    Pair("prettier splits an expression", "src/build.js", JS_PLAIN,
         JS_PRETTIER.replace("x * 2", "x * 3"), "content", "prettier"),
    Pair("template literal reflow", "src/t.js", "const q = `SELECT\n  a\nFROM t`;",
         "const q = `SELECT a FROM t`;", "content", "prettier", "",
         "template text is data; prettier will not reflow it and neither do we"),
    Pair("strict equality change", "src/x.ts", "if (a === b) {\n  return 1;\n}\n",
         "if (a == b) {\n  return 1;\n}\n", "content", "typescript"),
    Pair("ts type wrap", "src/x.ts", "export type A = { one: string; two: number };\n",
         "export type A = {\n  one: string;\n  two: number\n};\n", "reformat", "typescript",
         "tokens", "the member separators stay; a trailing one would be a token change"),
    Pair("apostrophe in JSX text", "src/card2.jsx", JS_JSX.replace("{title}", "{title}: it's here"),
         JS_JSX_REWRAPPED.replace("{title}", "{title}: it's here"), "reformat", "prettier",
         "tokens", "an apostrophe in text is not the start of a string"),
    # --- black / isort -----------------------------------------------------
    Pair("black + isort", "pkg/report.py", PY_PLAIN, PY_BLACK, "reformat", "black", "tokens",
         "import order, blank-line rule and the quote delimiter"),
    Pair("black wraps a signature", "pkg/render.py", PY_LONGCALL, PY_BLACK_WRAPPED,
         "reformat", "black", "tokens"),
    Pair("black joins a dict", "pkg/j.py",
         "x = {\n    'a': 1,\n    'b': 2,\n}\n", 'x = {"a": 1, "b": 2}\n', "reformat",
         "black", "tokens", "the magic trailing comma disappears with the newline"),
    Pair("string prefix case", "pkg/raw.py", "pattern = R'\\d+'\n", "pattern = r'\\d+'\n",
         "reformat", "black", "tokens", "black lowercases string prefixes"),
    Pair("statements joined with a semicolon", "pkg/j2.py", "a = 1\nb = 2\n", "a = 1; b = 2\n",
         "content", "black", "", "black never does this, so claiming it would be a lie"),
    Pair("one dict entry removed", "pkg/j.py", "x = {\n    'a': 1,\n    'b': 2,\n}\n",
         'x = {"a": 1}\n', "content", "black"),
    Pair("f-string changes", "pkg/f.py", 'msg = f"{a} items"\n', 'msg = f"{a} item(s)"\n',
         "content", "black"),
    # --- gofmt -------------------------------------------------------------
    Pair("gofmt reindents", "pkg/print.go", GO_PLAIN, GO_GOFMT, "reformat", "gofmt", "tokens"),
    Pair("goimports sorts a block", "pkg/m.go",
         'package main\n\nimport (\n\t"fmt"\n\t"os"\n)\n\nfunc f() {}\n',
         'package main\n\nimport (\n\t"os"\n\t"fmt"\n)\n\nfunc f() {}\n', "reformat", "gofmt",
         "tokens", "goimports sorts within a group; the group itself still matters"),
    Pair("gofmt hides an operator change", "pkg/print.go", GO_PLAIN,
         GO_GOFMT.replace("a + b", "a-b"), "content", "gofmt"),
    Pair("== becomes ===, wrapped", "src/eq.rs", "if a == b {\n  c();\n}\n",
         "if a ===\n  b\n{\n  c();\n}\n", "content", "rust"),
    # --- JSON / YAML / TOML / Markdown / CSS / shell -----------------------
    Pair("json reindent", "cfg/x.json", '{\n  "a": 1,\n  "b": [1, 2]\n}\n', '{"a":1,"b":[1,2]}',
         "reformat", "json", "json"),
    Pair("json number text", "cfg/x.json", '{"a": 1.0}\n', '{"a": 1}\n', "content", "json", "",
         "the scanned text keeps `1.0`: a reformatter that drops it changed a value"),
    Pair("json key order", "cfg/x.json", '{"a":1,"b":2}', '{"b":2,"a":1}', "content", "json",
         "", "JSON object order is data; no formatter is allowed to reorder it"),
    Pair("yaml reindent and padding", ".github/workflows/ci.yml",
         "jobs:\n    build:\n        runs-on: ubuntu-latest   # same\n",
         "jobs:\n  build:\n    runs-on: ubuntu-latest # same\n", "reformat", "yaml"),
    Pair("yaml block scalar whitespace", "a.yaml",
         "text: |\n  keep  me\n    exactly\nother: 1\n", "text: |\n  keep me\n  exactly\nx: 1\n",
         "content", "yaml", "", "inside `|` the whitespace is the value"),
    Pair("yaml block to flow sequence", "a.yaml", "list:\n  - a\n  - b\n", "list: [a, b]\n",
         "content", "yaml", "", "conservative on purpose: a style change in a sequence is\n"
                                  "reported, not folded"),
    Pair("toml realigns", "fathom.toml", '[noise]\nglobs  = ["a"]   # one\n\n\nx=1\n',
         '[noise]\nglobs = ["a"] # one\nx = 1\n', "reformat", "toml"),
    Pair("toml multi-line string", "a.toml", 'a = """\nx  y\n"""\n', 'a = """\nx y\n"""\n',
         "content", "toml"),
    Pair("markdown rewrap and markers", "docs/x.md", MD_PLAIN, MD_PRETTIER, "reformat",
         "markdown", "words", "prettier rewraps the paragraph and normalises list markers"),
    Pair("markdown code block edits", "docs/x.md", MD_CODE_MOVED, MD_CODE_REWRAPPED, "content",
         "markdown", "", "a fenced block is compared line by line: reflowing a command is a\n"
                          "change to the command"),
    Pair("markdown one word changes", "docs/x.md", MD_PLAIN,
         MD_PRETTIER.replace("reviewable", "actionable"), "content", "markdown"),
    Pair("css reindent", "web/x.css", ".a {\n    color: red;   /* keep */\n}\n",
         ".a {\n  color: red; /* keep */\n}\n", "reformat", "css"),
    Pair("shfmt reindents", "scripts/x.sh", "if [ -f a ]; then\n    echo one\necho two\nfi\n",
         "if [ -f a ]; then\necho one\necho two\nfi\n", "reformat", "shell"),
    Pair("shfmt quoting is a change", "scripts/x.sh", 'echo $HOME\n', 'echo "${HOME}"\n',
         "content", "shell", "", "shfmt's `-l`/`-w` can rewrite expansions; that is not layout"),
    # --- the weak claim, and the refusals ----------------------------------
    Pair("proto whitespace only", "proto/episode.proto", PROTO_PLAIN, PROTO_REFLOWED,
         "reformat", "refusal", "whitespace",
         "no token model for .proto, and every changed line differs only in whitespace"),
    Pair("proto real change", "proto/episode.proto", PROTO_PLAIN,
         PROTO_REFLOWED.replace("repeated string files = 3;", "repeated string files = 4;"),
         "undetermined", "refusal", "",
         "with no token model the honest answer is that we cannot tell a re-layout from a\n"
          "change — which is not folded, and is printed as a thing the tool cannot say"),
    Pair("unterminated literal refuses", "src/broken.rs", 'fn f() {\n  let s = "ok";\n}\n',
         'fn f() {\n  let s = "oops;\n}\n', "undetermined", "refusal", "",
         "a revision that cannot be scanned cannot be called a reformat, and cannot be\n"
          "called a change either: the scanner says so and the fold is refused"),
    Pair("unparsable python refuses", "pkg/broken.py", PY_PLAIN, "def broken(:\n  pass\n",
         "undetermined", "refusal", "",
         "the token claim is unavailable and the line claim sees a difference: that is\n"
          "`undetermined`, reported as a thing the tool cannot say"),
    Pair("unknown extension, whitespace only", "schemas/x.glz", "a b\n  c\n", "a b\nc\n",
         "reformat", "refusal", "whitespace"),
    Pair("unknown extension, real change", "schemas/x.glz", "a b\n", "a c\n",
         "undetermined", "refusal"),
    # --- generated files ----------------------------------------------------
    Pair("jest snapshot grows", "test/__snap__/queue.test.js.snap", SNAPSHOT_A, SNAPSHOT_B,
         "content", "generated", "",
         "a snapshot is generated, but this pair is a real assertion change: `generated.py`\n"
          "keeps it visible and the fold has to come from a stamp or a rebuild"),
    Pair("lockfile re-sorted", "Cargo.lock", CARGO_LOCK_A, CARGO_LOCK_RESORTED, "content",
         "lockfile", "", "same entries, different order: the lockfile rule folds it with its\n"
                          "own evidence, not the reformat rule"),
    Pair("lockfile version bump", "Cargo.lock", CARGO_LOCK_A, CARGO_LOCK_B, "content",
         "lockfile"),
)


# ---------------------------------------------------------------------------
# generated-file cases
# ---------------------------------------------------------------------------
@dataclass(frozen=True)
class GeneratedCase:
    """A generated-file pair and the kind a reviewer agrees with."""

    name: str
    path: str
    before: str
    after: str
    expect: str                       # churn | hand-edit
    kind: str = ""                    # stamp | resort | regen
    path_class: str = "generated"
    band: str = "confirmed"
    hint: str = "protoc-gen-go"
    note: str = ""


GENERATED: tuple[GeneratedCase, ...] = (
    GeneratedCase("generator version stamp only", "src/pb/api.pb.go", PB_GO_SMALL_A,
                  PB_GO_SMALL_B, "churn", "stamp",
                  note="only the `protoc-gen-go v…` line moved"),
    GeneratedCase("small hand edit inside a generated file", "src/pb/api.pb.go",
                  PB_GO_SMALL_A, PB_GO_RENAMED_FIELD, "hand-edit",
                  note="below the bulk floor and not a stamp: this is the case a bulk-only\n"
                       "rule would hide, and the reason this detector has a fourth row"),
    GeneratedCase("bulk regeneration", "src/pb/big.pb.go", PB_GO_BULK_A, PB_GO_BULK_B,
                  "churn", "regen", note="the whole file came out of the generator again"),
    GeneratedCase("re-sorted generated index", "src/gen/index.rs",
                  "\n".join("pub mod m%d;" % i for i in range(12)) + "\n",
                  "\n".join("pub mod m%d;" % i for i in reversed(range(12))) + "\n",
                  "churn", "resort", hint="build-script",
                  note="the same twelve lines in the other order: nothing was added"),
    GeneratedCase("not a generated file", "src/hand.rs", "fn a() {}\n", "fn b() {}\n",
                  "not-generated", "", path_class="source", band="", hint="",
                  note="the detector refuses to speak for a path it was not told is generated"),
)


# ---------------------------------------------------------------------------
# lockfile cases
# ---------------------------------------------------------------------------
@dataclass(frozen=True)
class LockCase:
    """One lockfile pair, its ecosystem, and the moved entries a reader should see."""

    name: str
    path: str
    before: str
    after: str
    expect: dict = field(default_factory=dict)     # kind → count
    manifest: str = ""                             # the manifest that must be named
    manifest_changed: bool = False
    note: str = ""


LOCKS: tuple[LockCase, ...] = (
    LockCase("cargo upgrade", "Cargo.lock", CARGO_LOCK_A, CARGO_LOCK_B,
             {"upgraded": 2}, manifest="Cargo.toml"),
    LockCase("npm v3 packages", "package-lock.json",
             '{"name":"x","lockfileVersion":3,"packages":{"":{},"node_modules/semver":'
             '{"version":"7.3.8"}}}',
             '{"name":"x","lockfileVersion":3,"packages":{"":{},"node_modules/semver":'
             '{"version":"7.6.0"}}}',
             {"upgraded": 1}, manifest="package.json"),
    LockCase("npm v1 dependencies", "npm-shrinkwrap.json",
             '{"lockfileVersion":1,"dependencies":{"semver":{"version":"7.3.8"}}}',
             '{"lockfileVersion":1,"dependencies":{"semver":{"version":"7.6.0"}}}',
             {"upgraded": 1}, manifest="package.json"),
    LockCase("pnpm lock", "pnpm-lock.yaml",
             "lockfileVersion: '9.0'\n\npackages:\n\n  semver@7.3.8:\n    resolution: {}\n",
             "lockfileVersion: '9.0'\n\npackages:\n\n  semver@7.6.0:\n    resolution: {}\n",
             {"upgraded": 1}, manifest="package.json"),
    LockCase("yarn lock", "yarn.lock",
             '# yarn lockfile v1\n\n\nsemver@^7.0.0:\n  version "7.3.8"\n  integrity sha1-aa\n',
             '# yarn lockfile v1\n\n\nsemver@^7.0.0:\n  version "7.6.0"\n  integrity sha1-bb\n',
             {"upgraded": 1}, manifest="package.json"),
    LockCase("poetry lock", "poetry.lock",
             '[[package]]\nname = "pytest"\nversion = "8.3.2"\n',
             '[[package]]\nname = "pytest"\nversion = "8.3.3"\n', {"upgraded": 1},
             manifest="pyproject.toml"),
    LockCase("pipenv lock", "Pipfile.lock",
             '{"default": {"pytest": {"version": "==8.3.2"}}}',
             '{"default": {"pytest": {"version": "==8.3.3"}}}', {"upgraded": 1},
             manifest="Pipfile"),
    LockCase("go.sum", "go.sum",
             "github.com/x/y v1.2.3 h1:aaa=\ngithub.com/x/y v1.2.3/go.mod h1:bbb=\n",
             "github.com/x/y v1.2.4 h1:ccc=\ngithub.com/x/y v1.2.4/go.mod h1:ddd=\n",
             {"upgraded": 2}, manifest="go.mod",
             note="a module and its `/go.mod` line are separate entries, so two rows move"),
    LockCase("Gemfile.lock", "Gemfile.lock",
             "GEM\n  specs:\n    rake (13.0.6)\n    rubocop (1.60.0)\n",
             "GEM\n  specs:\n    rake (13.1.0)\n    rubocop (1.60.0)\n", {"upgraded": 1},
             manifest="Gemfile"),
    LockCase("mix.lock", "mix.lock",
             '%{"phoenix": {:hex, :phoenix, "1.7.10", "aa", [:mix], []}}\n',
             '%{"phoenix": {:hex, :phoenix, "1.7.12", "bb", [:mix], []}}\n',
             {"upgraded": 1}, manifest="mix.exs"),
    LockCase("composer.lock", "composer.lock",
             '{"packages":[{"name":"vendor/pkg","version":"1.2.3"}]}',
             '{"packages":[{"name":"vendor/pkg","version":"1.2.4"}]}', {"upgraded": 1},
             manifest="composer.json"),
    LockCase("packages.lock.json", "packages.lock.json",
             '{"version":1,"dependencies":{"net8.0":{"Newtonsoft.Json":{"type":"Direct","'
             'requested":"13.0.1","resolved":"13.0.1"}}}}',
             '{"version":1,"dependencies":{"net8.0":{"Newtonsoft.Json":{"type":"Direct","'
             'requested":"13.0.1","resolved":"13.0.3"}}}}', {"upgraded": 1},
             manifest="Project.csproj"),
    LockCase("requirements.txt", "requirements.txt",
             "pytest==8.3.2\nrich==13.7.0\n", "pytest==8.3.3\nrich==13.7.0\n",
             {"upgraded": 1}, manifest="pyproject.toml"),
    LockCase("unparseable lockfile", "package-lock.json", '{"packages": {oops',
             '{"packages": {oops}}', {}, note="a broken lock is `unreadable`, with line counts,"
                                              "never a silent zero"),
)


# ---------------------------------------------------------------------------
# episode scenarios
# ---------------------------------------------------------------------------
@dataclass(frozen=True)
class EpisodeCase:
    """A whole episode and what the collapsed line has to say about it."""

    name: str
    files: dict                            # path → (before, after)
    collapse: bool
    shape: str
    must_mention: tuple[str, ...] = ()
    note: str = ""


EPISODES: tuple[EpisodeCase, ...] = (
    EpisodeCase("rustfmt-only commit", {"src/thing.rs": (RUST_PLAIN, RUST_REFORMATTED),
                                        "src/other.rs": (RUST_PLAIN, RUST_REFORMATTED)},
                True, "rustfmt", ("0 semantic changes", "rustfmt"),
                "AC 1: the headline says 0 semantic changes and names the formatter"),
    EpisodeCase("prettier-only commit", {"src/build.js": (JS_PLAIN, JS_PRETTIER),
                                         "src/card.jsx": (JS_JSX, JS_JSX_REWRAPPED)},
                True, "prettier", ("0 semantic changes", "prettier")),
    EpisodeCase("cargo lock only", {"Cargo.lock": (CARGO_LOCK_A, CARGO_LOCK_B)}, True,
                "lockfile", ("lockfile-only", "serde", "no manifest change"),
                "AC 2: one line, the moved packages, and the manifest that did not move"),
    EpisodeCase("lock and manifest together",
                {"Cargo.lock": (CARGO_LOCK_A, CARGO_LOCK_B),
                 "Cargo.toml": ('serde = "1.0.219"\n', 'serde = "1.0.220"\n')},
                False, "lockfile", (),
                "the manifest is the reviewable part; nothing may fold this episode"),
    EpisodeCase("reformat plus one real line",
                {"src/thing.rs": (RUST_PLAIN, RUST_REFORMATTED),
                 "src/lib.rs": (RUST_PLAIN, RUST_PLAIN.replace("a + b", "a - b"))},
                False, "mixed", (), "one visible file keeps the episode expanded"),
    EpisodeCase("generated stamp only", {"src/pb/api.pb.go": (PB_GO_SMALL_A, PB_GO_SMALL_B)},
                True, "generated", ("generated-file churn",)),
    EpisodeCase("hand edit inside a generated file",
                {"src/pb/api.pb.go": (PB_GO_SMALL_A, PB_GO_RENAMED_FIELD)}, False, "generated",
                (), "the case the fold must never swallow"),
    EpisodeCase("vendored tree only",
                {"vendor/libx/thing.go": (GO_PLAIN, GO_PLAIN.replace('fmt.Println(a+b)',
                                                                     'fmt.Println(a, b)'))},
                True, "vendored", ("vendored",),
                "a content change, so only the vendored rule can fold it"),
    EpisodeCase("docs rewrap plus code change",
                {"docs/x.md": (MD_PLAIN, MD_PRETTIER),
                 "src/thing.rs": (RUST_PLAIN, RUST_PLAIN.replace("a + b", "a * b"))},
                False, "mixed"),
    EpisodeCase("added and deleted files",
                {"src/new.rs": (None, "fn a() {}\n"), "src/gone.rs": ("fn b() {}\n", None)},
                False, "refusal", (),
                "one side missing is not nothing-but-noise: an added file is a real change"),
)


def pairs() -> tuple[Pair, ...]:
    return PAIRS


def shapes() -> dict[str, int]:
    out: dict[str, int] = {}
    for case in PAIRS:
        out[case.shape] = out.get(case.shape, 0) + 1
    return out
