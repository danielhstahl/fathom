//! `debug_leftover.rs` — something written to look at the program survived into the commit (`e4.3`).
//!
//! **Two tiers, because "a print statement" is two different claims.**
//!
//! Tier A is a marker that is never production code: `dbg!`, `debugger`, `pdb.set_trace()`,
//! `breakpoint()`, `binding.pry`, `var_dump(`, `console.debug(`, `@Trace`. Its finding is `medium` at
//! `0.8` — not `0.95`, because a Rust file that defines its own macro called `dbg!`, or a document that
//! quotes one, exists somewhere.
//!
//! Tier B is a print that *may* be the program talking to its user: `println!`, `print!`, `fmt.Println`,
//! `System.out.print`, `console.log`, Python's `print(`, Ruby's `puts`. Its finding is `low` at `0.45`.
//!
//! **Tier B needs a shape, not a marker (`reco-pfb`).** `print(` is not evidence of something left behind;
//! `print(` plus a shape is. The shape is that the line looks like a look at the program:
//!
//! * **it prints a value and says nothing about it** — `print(row)`, `console.log(user)`,
//!   `fmt.Println(err)`, `println!("{}", n)`: a report line has words in it, a leftover has a variable;
//! * **it calls itself a look** — a `dbg`, `debug`, `trace`, `dump`, `TODO`, `FIXME`, `hack` word in the
//!   text it prints, a `>>>` gutter, or a value rendered in its debug form (`{x:?}`, `{x:#?}`, `{x!r}`,
//!   `repr(`, `.inspect`, `JSON.stringify(`);
//! * **it goes past the program's output** — `eprintln!`, `print(…, file=sys.stderr)`, `System.err.print`.
//!
//! That is the whole of the reason this file used to fill an artefact with findings about `tools/*.py`.
//! Those files are CLI tools and their product *is* the text they print; so is ruff's
//! `python/py-fuzzer/fuzz.py`, which the published G3 corpus has two hand-written dismissals for, and so
//! is every report generator in every repository this detector will ever read. A detector that fires on
//! the house's own style is a detector whose output gets skimmed and then ignored, which costs the *true*
//! findings more than silence would. Two refusals came with that choice and are worth keeping in view: it
//! is **not** scoped to `crates/` — this repository's library happens to live there and most repositories'
//! does not, and a rule written around one tree is a rule that has to be re-written at the next repository;
//! and it does **not** grow an ignore list of its own, because [`crate::detectors::policy`] already has one
//! — `detectors.suppress` takes `debug_leftover:tools/**` today, in the same glob language as every other
//! scope in the product, and a second mechanism for the same answer is a second answer.
//!
//! Tier B is also silent in the places where printing is the job:
//!
//! * an entrypoint — `src/main.rs`, `src/bin/**`, `cmd/**`, `bin/**`, `tools/**`, `examples/**`,
//!   `*/scripts/**`, `main.py`, `__main__.py`, `cli.py`: a CLI that does not print is broken;
//! * **any file that carries its own entry point** — an `if __name__ == "__main__"` dispatch, an
//!   `argparse`, a `fn main(`. The file is saying out loud that it is a program, which is better evidence
//!   than its address, and it is what catches the scripts that live in a directory this table has no name
//!   for;
//! * the test surface — a test that prints is untidy, not left-behind;
//! * prose and configuration, where the marker is being *quoted*;
//! * generated and vendored churn, which nobody hand-edited.
//!
//! **What tier B now misses, and what that costs.** `println!("cache hit: {}", n)` in a library is a real
//! leftover and it is now silent, because at line level a print of a labelled value is indistinguishable
//! from a logger call in a house whose logging idiom this file does not know. That is the price of the
//! shape rule and it is the right way round: one missed `low` note is cheaper than a reviewer who has
//! stopped reading the tier.
//!
//! **Why this ships on while the loop detectors do not.** A `dbg!` in a commit is a fact about a line, and
//! the line is quoted in the finding: there is no judgement between the marker and the claim, which is the
//! property the four disabled detectors lack. The worst this detector does in a repository whose logging
//! idiom it does not know is print a `low` finding once per file, on a path the reviewer can silence with
//! `debug_leftover:src/term/**`.
//!
//! **What it will not do:** invent a line number. The evidence locator is the hunk header the store
//! actually holds, and the quoted lines are the added lines themselves.

use crate::detectors::finding::{Evidence, EvidenceKind, Finding, Severity};
use crate::detectors::model::{Corpus, Episode, FileChange};
use crate::detectors::policy::Policy;
use crate::detectors::text::{self, Dialect};

/// One marker, the dialects it means something in, and whether it is ever legitimate.
struct Marker {
    text: &'static str,
    dialects: Dialects,
}

/// The dialect set a marker applies to. A marker in an unknown dialect is not a marker: `print(` in a
/// file fathom cannot name is a line that could be anything.
struct Dialects(&'static [Dialect]);

impl Dialects {
    fn applies(&self, dialect: Dialect) -> bool {
        self.0.contains(&dialect)
    }
}

const JS: &[Dialect] = &[Dialect::JavaScriptTypeScript];
const PY: &[Dialect] = &[Dialect::Python];
const RB: &[Dialect] = &[Dialect::Ruby];
const RS: &[Dialect] = &[Dialect::Rust];
const GO: &[Dialect] = &[Dialect::Go];
const JVM: &[Dialect] = &[Dialect::Java, Dialect::Kotlin, Dialect::CSharp];

/// Tier A: never production code.
const CERTAIN: &[Marker] = &[
    Marker {
        text: "dbg!",
        dialects: Dialects(RS),
    },
    Marker {
        text: "debugger",
        dialects: Dialects(JS),
    },
    Marker {
        text: "console.debug(",
        dialects: Dialects(JS),
    },
    Marker {
        text: "console.trace(",
        dialects: Dialects(JS),
    },
    Marker {
        text: "pdb.set_trace()",
        dialects: Dialects(PY),
    },
    Marker {
        text: "ipdb.set_trace()",
        dialects: Dialects(PY),
    },
    Marker {
        text: "breakpoint()",
        dialects: Dialects(PY),
    },
    Marker {
        text: "binding.pry",
        dialects: Dialects(RB),
    },
    Marker {
        text: "var_dump(",
        dialects: Dialects(&[Dialect::Php]),
    },
    Marker {
        text: "dd(",
        dialects: Dialects(&[Dialect::Php]),
    },
    Marker {
        text: "TRACE!(",
        dialects: Dialects(&[Dialect::CSharp]),
    },
];

/// Tier B: could be the program, probably is not in this position.
const PROBABLE: &[Marker] = &[
    Marker {
        text: "println!(",
        dialects: Dialects(RS),
    },
    Marker {
        text: "print!(",
        dialects: Dialects(RS),
    },
    Marker {
        text: "eprintln!(",
        dialects: Dialects(RS),
    },
    Marker {
        text: "eprint!(",
        dialects: Dialects(RS),
    },
    Marker {
        text: "log::trace!(",
        dialects: Dialects(RS),
    },
    Marker {
        text: "console.log(",
        dialects: Dialects(JS),
    },
    Marker {
        text: "console.info(",
        dialects: Dialects(JS),
    },
    Marker {
        text: "fmt.Println(",
        dialects: Dialects(GO),
    },
    Marker {
        text: "fmt.Printf(",
        dialects: Dialects(GO),
    },
    Marker {
        text: "System.out.print",
        dialects: Dialects(JVM),
    },
    Marker {
        text: "System.err.print",
        dialects: Dialects(JVM),
    },
    Marker {
        text: "print(",
        dialects: Dialects(PY),
    },
    Marker {
        text: "pp(",
        dialects: Dialects(RB),
    },
    Marker {
        text: "p ",
        dialects: Dialects(RB),
    },
    Marker {
        text: "puts ",
        dialects: Dialects(RB),
    },
];

/// The words only a debugging run says, matched as whole lower-cased words of the text a tier-B print
/// prints. Whole words is the point: `traceability`, `dumped` and `hacking` are report prose, and a list
/// matched as substrings turns every document about debugging into a leftover.
const DEBUG_WORDS: &[&str] = &[
    "dbg",
    "debug",
    "debugging",
    "trace",
    "traceback",
    "dump",
    "todo",
    "fixme",
    "xxx",
    "hack",
];

/// The gutter a debugging run puts on its own line.
const DEBUG_GUTTERS: &[&str] = &[">>>", "!!!"];

/// A value being shown in its debug representation, which is what a person types when they want to see
/// what a thing *is* rather than what the program says about it.
const DEBUG_RENDERS: &[&str] = &[
    "repr(",
    ".inspect",
    "JSON.stringify(",
    "print_r(",
    "var_export(",
];

/// Writes that go past the program's own output. A line that picks the error stream to say something in
/// is looking at the run, not being the run.
const STDERR_WRITES: &[&str] = &[
    "eprintln!(",
    "eprint!(",
    "file=sys.stderr",
    "file=stderr",
    "file: std::io::stderr",
    "System.err.print",
    "$stderr.puts",
];

/// The lines that say *this file is a program*: it dispatches on being run, parses its own arguments, or
/// defines the entry point. A file carrying one of these is a program whatever directory it lives in, and
/// a program's stdout is its product — which is the fact the path table can only guess at, and the reason
/// ruff's `python/py-fuzzer/fuzz.py` and this repository's own `src/recommender/**` scripts are quiet here
/// without either name being written down.
///
/// It reads the lines the episode added, so it can only ever *raise* the claim that a file is a program:
/// a diff that touches one print line in a script says nothing, and the shape of the line carries that one.
const PROGRAM_SHAPES: &[&str] = &[
    "__main__",
    "argparse",
    "fn main(",
    "func main(",
    "int main(",
    "public static void main",
];

/// What a printed line actually says, taken apart without parsing it.
#[derive(Debug, Default)]
struct Utterance {
    /// Did the line carry a string literal at all? A print with none is printing a bare value.
    literal: bool,
    /// The text inside the literals, with every `{…}` placeholder taken out — the words the program chose
    /// to put on the screen, which is what a report is made of and a leftover usually is not.
    text: String,
    /// A placeholder that renders its value in debug form: Rust's `{x:?}` and `{:#?}`, Python's `{x!r}`.
    debug_render: bool,
}

/// Read one line's literals. Lexical, like everything in [`crate::detectors::text`]: it tracks quotes and
/// `{…}` interpolation because both decide whether a `{` is a placeholder or a character, and it stops
/// there rather than pretending to parse.
#[must_use]
fn utterance(line: &str) -> Utterance {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Utterance::default();
    let mut quote: Option<char> = None;
    let mut placeholder = String::new();
    let mut depth = 0_usize;
    let mut index = 0_usize;
    while index < chars.len() {
        let c = chars[index];
        if quote.is_some() && c == '\\' {
            index += 2;
            continue;
        }
        match quote {
            None => {
                if matches!(c, '"' | '\'' | '`') {
                    quote = Some(c);
                    out.literal = true;
                }
            }
            Some(open) => {
                if c == open {
                    quote = None;
                } else if c == '{' {
                    depth += 1;
                    placeholder.clear();
                } else if depth > 0 {
                    if c == '}' {
                        depth -= 1;
                        if placeholder.contains(['?', '!']) {
                            out.debug_render = true;
                        }
                    } else {
                        placeholder.push(c);
                    }
                } else {
                    out.text.push(c);
                }
            }
        }
        index += 1;
    }
    out
}

/// Does this tier-B line read as a look at the program rather than as the program talking?
///
/// The default for a line this function cannot read is `false`: an uncertain print is the program's own
/// output, because the cost of the wrong guess is one `low` finding nobody asked for. See the module
/// header for the three shapes and what they cost.
#[must_use]
fn reads_as_debug(line: &str) -> bool {
    if STDERR_WRITES.iter().any(|write| line.contains(write))
        || DEBUG_RENDERS.iter().any(|render| line.contains(render))
    {
        return true;
    }
    let said = utterance(line);
    // No literal at all: the line prints a value and says nothing about it.
    if !said.literal || said.debug_render {
        return true;
    }
    // Placeholders and punctuation are not words. `println!("{}", n)` says a number; a report line says
    // what the number is.
    if !said.text.chars().any(char::is_alphabetic) {
        return true;
    }
    DEBUG_GUTTERS
        .iter()
        .any(|gutter| said.text.contains(gutter))
        || said
            .text
            .to_ascii_lowercase()
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|word| !word.is_empty() && DEBUG_WORDS.contains(&word))
}

/// Does this marker occur in this line? One marker is asked differently.
///
/// Ruby's `p ` is a substring of ordinary code — `map {`, `help text`, `snap width` — so it counts only in
/// the one place Ruby spells the debug call: at the start of its statement. Every other marker here is
/// distinctive enough to be found anywhere on the line.
#[must_use]
fn marker_on(marker: &Marker, line: &str) -> bool {
    if marker.text == "p " {
        return line.trim_start().starts_with("p ");
    }
    line.contains(marker.text)
}

pub fn detect(episode: &Episode, _corpus: &Corpus<'_>, policy: &Policy) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in &episode.files {
        if !reads(file) {
            continue;
        }
        let Some(dialect) = text::dialect(&file.path) else {
            continue;
        };
        let added = file.added();
        if added.is_empty() {
            continue;
        }
        let entrypoint = printing_is_the_job(&file.path, &added);
        let mut certain: Vec<&str> = Vec::new();
        let mut probable: Vec<&str> = Vec::new();
        for raw in &added {
            if text::is_blank_or_comment(raw, dialect) {
                continue;
            }
            // A marker inside a trailing comment — `let x = 1; // dbg!(y)` — is a note *about*
            // debugging, not debugging that runs. The scan is on the code half of the line.
            let line = text::strip_inline_comment(raw, dialect);
            if CERTAIN
                .iter()
                .any(|marker| marker.dialects.applies(dialect) && marker_on(marker, line))
            {
                certain.push(line);
            } else if !entrypoint
                && PROBABLE
                    .iter()
                    .any(|marker| marker.dialects.applies(dialect) && marker_on(marker, line))
                && reads_as_debug(line)
            {
                probable.push(line);
            }
        }

        for (lines, severity, confidence) in [
            (&certain, Severity::Medium, 0.8),
            (&probable, Severity::Low, 0.45),
        ] {
            if lines.is_empty() {
                continue;
            }
            findings.push(
                Finding::build(
                    "debug_leftover",
                    1,
                    &episode.id,
                    file.path.clone(),
                    format!(
                        "{} added {} debug output line(s) that read as left behind: {}",
                        file.path,
                        lines.len(),
                        crate::detectors::named(
                            &lines.iter().map(|line| line.trim()).collect::<Vec<_>>(),
                            3
                        )
                    ),
                )
                .severity(severity)
                .confidence(confidence)
                .evidence(Evidence::observed(
                    EvidenceKind::Hunk,
                    format!(
                        "{} {}",
                        file.path,
                        file.hunks
                            .first()
                            .map_or_else(String::new, |hunk| hunk.header.clone())
                    ),
                    crate::detectors::quote(lines, policy.evidence_lines),
                ))
                .jump_path(&file.path)
                .finish(),
            );
        }
    }
    findings
}

/// Only hand-written code files: prose quotes markers, generated churn was not hand-edited, tests that
/// print are untidy rather than forgotten, and configuration has no statements.
fn reads(file: &FileChange) -> bool {
    !file.binary
        && !file.noisy()
        && !file.is_test()
        && !file.is_delete()
        && text::dialect(&file.path).is_some_and(Dialect::is_code)
}

/// Does this file print on purpose? Either the repository says so with a path (**reco-pfb**'s list below)
/// or the file says so itself by carrying its own entry point.
#[must_use]
fn printing_is_the_job(path: &str, added: &[&str]) -> bool {
    is_entrypoint(path)
        || added
            .iter()
            .any(|line| PROGRAM_SHAPES.iter().any(|shape| line.contains(shape)))
}

/// The places where printing to stdout *is* the program.
///
/// A path answer, and priced as one. `tools/` and `examples/` are here because a repository's scripts' wing
/// is as much a program as its `main` is: this repository's own `tools/*.py` and ruff's
/// `python/py-fuzzer/fuzz.py` — which the published G3 corpus hand-dismisses twice — are the two cases that
/// said a file whose product is its output is not a leftover. A repository that ships a *library* under one
/// of these names says so with `debug_leftover:<path>` in `detectors.suppress`, in the one glob language the
/// product already has, rather than with a rename. Since **reco-pfb** the tier-B bar is the *shape* of the
/// line as well as its path, so this table only has to catch the prints that look like report lines.
fn is_entrypoint(path: &str) -> bool {
    const HEADS: &[&str] = &[
        "src/main.rs",
        "src/bin/",
        "cmd/",
        "bin/",
        "scripts/",
        "tools/",
        "examples/",
        "main.py",
        "__main__.py",
        "cli.py",
    ];
    let folded = path.to_ascii_lowercase();
    HEADS
        .iter()
        .any(|head| folded.starts_with(head) || folded.contains(&format!("/{head}")))
        || folded.ends_with("_cli.rs")
        || folded.ends_with("/cli.ts")
        || folded.ends_with("/cli.js")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detectors::model::{Content, Hunk, change};

    fn file(path: &str, added: &[&str]) -> FileChange {
        FileChange {
            path: path.to_owned(),
            change: change::MODIFIED.to_owned(),
            add: added.len() as i64,
            hunks: vec![Hunk::new(
                "@@ -10,3 +10,5 @@ fn parse(",
                added.to_vec(),
                Vec::<String>::new(),
            )],
            ..Default::default()
        }
    }

    fn episode(files: Vec<FileChange>) -> Episode {
        Episode {
            id: "ep-1".to_owned(),
            files,
            content: Content::Full,
            ..Episode::new("ep-1")
        }
    }

    #[test]
    fn a_dbg_is_a_medium_finding_that_quotes_the_line() {
        let findings = detect(
            &episode(vec![file(
                "src/parse.rs",
                &["let v = items.pop();", "dbg!(&items);"],
            )]),
            &Corpus::default(),
            &Policy::default(),
        );
        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.detector_id, "debug_leftover");
        assert_eq!(finding.severity, Severity::Medium);
        assert!((finding.confidence - 0.8).abs() < f64::EPSILON);
        assert!(finding.evidence[0].detail.contains("dbg!(&items);"));
        assert_eq!(finding.suppression_key, "debug_leftover:src/parse.rs");
        assert_eq!(finding.jump.display(), "diff ep-1#src/parse.rs");
    }

    #[test]
    fn every_tier_a_marker_fires_and_every_tier_b_marker_is_only_a_note() {
        let certain_cases = [
            ("src/a.py", "pdb.set_trace()"),
            ("src/a.py", "breakpoint()"),
            ("src/a.rb", "binding.pry"),
            ("src/a.ts", "debugger;"),
            ("src/a.ts", "console.debug('x');"),
        ];
        for (path, marker) in certain_cases {
            let findings = detect(
                &episode(vec![file(path, &[marker])]),
                &Corpus::default(),
                &Policy::default(),
            );
            assert_eq!(findings.len(), 1, "{path} {marker}");
            assert_eq!(findings[0].severity, Severity::Medium, "{path} {marker}");
        }
        let probable_cases = [
            ("src/a.rs", "println!(\"{n}\");"),
            ("src/a.go", "fmt.Println(x)"),
            ("src/a.py", "print(x)"),
        ];
        for (path, marker) in probable_cases {
            let findings = detect(
                &episode(vec![file(path, &[marker])]),
                &Corpus::default(),
                &Policy::default(),
            );
            assert_eq!(findings.len(), 1, "{path} {marker}");
            assert_eq!(
                findings[0].severity,
                Severity::Low,
                "{path} {marker} must be a note, not a demand"
            );
            assert!(findings[0].confidence <= 0.5);
        }
    }

    #[test]
    fn a_cli_that_prints_is_not_left_behind() {
        for path in [
            "src/main.rs",
            "src/bin/convert.rs",
            "cmd/tool/main.go",
            "tools/cli.py",
        ] {
            let findings = detect(
                &episode(vec![file(path, &["println!(\"done\");"])]),
                &Corpus::default(),
                &Policy::default(),
            );
            assert!(findings.is_empty(), "{path} prints on purpose");
        }
    }

    #[test]
    fn the_tiers_are_split_and_a_file_with_both_gets_two_findings() {
        let findings = detect(
            &episode(vec![file(
                "src/parse.rs",
                &["dbg!(x);", "println!(\"{}\", x);"],
            )]),
            &Corpus::default(),
            &Policy::default(),
        );
        assert_eq!(
            findings.len(),
            2,
            "one row per tier: a reviewer dismisses them differently"
        );
        assert_eq!(findings[0].severity, Severity::Medium);
        assert_eq!(findings[1].severity, Severity::Low);
    }

    /// The `reco-pfb` regression: a report line is the program talking, not a look at the program. The
    /// lines are verbatim from this repository's own `tools/*.py`, whose entire product is the text they
    /// print; the paths are `src/**` on purpose, so what silences them is the shape of the line and not
    /// where the file lives.
    #[test]
    fn a_line_made_of_words_is_the_program_talking_not_a_leftover() {
        for line in [
            r#"print(f"{path} is missing; the prose around this block is written, the block is generated")"#,
            r#"print("note: %s not built; no command check performed" % binary)"#,
            r#"print("\n%d documented command(s) the binary will not accept" % len(found))"#,
            r#"println!("{written} rows written to {out}");"#,
            r#"fmt.Printf("%d clusters in %d files\n", n, f);"#,
            r#"console.log("loaded the manifest");"#,
        ] {
            let findings = detect(
                &episode(vec![file("src/render/report.py", &[line])]),
                &Corpus::default(),
                &Policy::default(),
            );
            assert!(findings.is_empty(), "{line} is a report line: {findings:?}");
        }
    }

    /// What the shape rule must keep: a print of a value with nothing said about it.
    #[test]
    fn a_bare_value_print_is_still_a_leftover() {
        for (path, line) in [
            ("src/render.py", "print(row)"),
            ("src/render.py", "print(f\"{a} {b}\")"),
            ("src/parse.rs", "println!(\"{}\", n);"),
            ("src/load.go", "fmt.Println(err)"),
            ("src/view.ts", "console.log(user);"),
            ("src/views/show.rb", "puts current_user"),
            // A table row of interpolated values says nothing about what it is either, and at line level
            // it is the same shape as a debug dump. The path table, not the shape, is what keeps a real
            // report generator quiet — which is why `tools/demo_acceptance.py` prints exactly this line.
            (
                "src/table.py",
                r#"print(f"  [{mark}] {verdict} · {row['pathology']}")"#,
            ),
        ] {
            let findings = detect(
                &episode(vec![file(path, &[line])]),
                &Corpus::default(),
                &Policy::default(),
            );
            assert_eq!(findings.len(), 1, "{path} {line}");
            assert_eq!(findings[0].severity, Severity::Low, "{path} {line}");
        }
    }

    /// The other two shapes: the line names itself a look, or it goes past the program's output.
    #[test]
    fn a_line_that_names_itself_a_look_is_a_leftover_wherever_it_lives() {
        for (path, line) in [
            ("src/render.py", r#"print(f"dbg: {row}")"#),
            ("src/render.py", r#"print(f"{user!r}")"#),
            ("src/render.py", r#"print(">>> leaving main")"#),
            ("src/render.py", "print(cfg, file=sys.stderr)"),
            ("src/parse.rs", "println!(\"{session:#?}\");"),
            ("src/parse.rs", "eprintln!(\"cannot parse {line}\");"),
            ("src/view.ts", "console.log(JSON.stringify(cfg));"),
            ("src/views/show.rb", "puts current_user.inspect"),
        ] {
            let findings = detect(
                &episode(vec![file(path, &[line])]),
                &Corpus::default(),
                &Policy::default(),
            );
            assert_eq!(findings.len(), 1, "{path} {line}: {findings:?}");
            assert_eq!(findings[0].severity, Severity::Low, "{path} {line}");
        }
    }

    /// A word that only rhymes with debugging is report prose: the word list is whole words, so a report
    /// about traceability or about rows it dumped does not read as a leftover.
    #[test]
    fn a_word_that_rhymes_with_debugging_is_not_one() {
        for line in [
            r#"print(f"{n} traceability notes emitted")"#,
            r#"print(f"{n} rows dumped to {out}")"#,
            r#"println!("{} hacks documented in the guide");"#,
        ] {
            let findings = detect(
                &episode(vec![file("src/render/report.py", &[line])]),
                &Corpus::default(),
                &Policy::default(),
            );
            assert!(findings.is_empty(), "{line} is prose: {findings:?}");
        }
    }

    /// The scripts' wing: a directory of programs, in any repository, wherever it is called what.
    #[test]
    fn a_directory_of_programs_prints_on_purpose() {
        for (path, line) in [
            ("tools/check_docs_site.py", "print(problem)"),
            ("tools/demo_acceptance.py", r#"print(f"{verdict} · {row}")"#),
            (
                "crates/fathom-analyze/examples/graph_harness.rs",
                "println!(\"{}\", problem);",
            ),
            ("examples/quickstart.rs", "println!(\"{}\", problem);"),
        ] {
            let findings = detect(
                &episode(vec![file(path, &[line])]),
                &Corpus::default(),
                &Policy::default(),
            );
            assert!(
                findings.is_empty(),
                "{path} prints on purpose: {findings:?}"
            );
        }
    }

    /// A file that carries its own entry point is a program wherever it lives, and a program's stdout is
    /// its product. `python/py-fuzzer/fuzz.py` is ruff's, twice hand-dismissed in the published G3 corpus;
    /// `src/recommender/store/bench.py` is this repository's, and neither name is written down anywhere.
    #[test]
    fn a_file_that_dispatches_as_a_program_prints_on_purpose() {
        for (path, dispatch, line) in [
            (
                "src/recommender/store/bench.py",
                "if __name__ == \"__main__\":",
                "print(json.dumps(report))",
            ),
            (
                "python/py-fuzzer/fuzz.py",
                "parser = argparse.ArgumentParser(prog=\"fuzz\")",
                "print(self.maybe_bug.source)",
            ),
            (
                "services/worker/run.go",
                "func main() {",
                "fmt.Println(err)",
            ),
        ] {
            let findings = detect(
                &episode(vec![file(path, &[line, dispatch])]),
                &Corpus::default(),
                &Policy::default(),
            );
            assert!(findings.is_empty(), "{path} is a program: {findings:?}");
        }
    }

    /// Ruby's `p ` is a debug call and a substring of `map {`, `snap width` and `help text`. It counts at
    /// statement position, which is the only place Ruby spells the call.
    #[test]
    fn the_ruby_p_marker_is_a_statement_not_a_substring() {
        let findings = detect(
            &episode(vec![file(
                "lib/demo/report.rb",
                &["@items.map { |i| i.total }"],
            )]),
            &Corpus::default(),
            &Policy::default(),
        );
        assert!(
            findings.is_empty(),
            "`map {{` is not a debug call: {findings:?}"
        );
        let findings = detect(
            &episode(vec![file("lib/demo/report.rb", &["p @items"])]),
            &Corpus::default(),
            &Policy::default(),
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Low);
    }

    /// Tier A is untouched by any of this: a `dbg!` in a script's wing is still a `dbg!`.
    #[test]
    fn a_marker_is_still_a_marker_in_the_scripts_wing() {
        let findings = detect(
            &episode(vec![file("examples/graph_harness.rs", &["dbg!(report);"])]),
            &Corpus::default(),
            &Policy::default(),
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Medium);
    }

    #[test]
    fn a_comment_quoting_a_marker_is_not_a_leftover() {
        let findings = detect(
            &episode(vec![file(
                "src/parse.rs",
                &["// remove the dbg! before merging"],
            )]),
            &Corpus::default(),
            &Policy::default(),
        );
        assert!(findings.is_empty());
    }

    #[test]
    fn prose_generated_and_vendored_files_are_not_read() {
        for path in [
            "docs/debugging.md",
            "src/generated/api.rs",
            "vendor/lib/print.go",
            "tests/printing.rs",
        ] {
            let case = FileChange {
                path: path.to_owned(),
                add: 1,
                hunks: vec![Hunk::new(
                    "@@ -1,1 +1,2 @@",
                    ["dbg!(x);"],
                    Vec::<String>::new(),
                )],
                ..Default::default()
            };
            let findings = detect(&episode(vec![case]), &Corpus::default(), &Policy::default());
            assert!(
                findings.is_empty(),
                "{path} should not be read: {findings:?}"
            );
        }
    }

    #[test]
    fn an_unknown_extension_claims_nothing() {
        let case = FileChange {
            path: "assets/shader.glsl".to_owned(),
            add: 1,
            hunks: vec![Hunk::new(
                "@@ -1,1 +1,2 @@",
                ["print(x);"],
                Vec::<String>::new(),
            )],
            ..Default::default()
        };
        assert!(detect(&episode(vec![case]), &Corpus::default(), &Policy::default()).is_empty());
    }
}
