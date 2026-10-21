//! `format.rs` — the markdown the ledger is written in (`e4.6`, `reco-rlf`).
//!
//! The file is the authoritative copy (ADR-004 §10: class M lives in git, the row is an index over it), so
//! this module is the only place that knows its shape — read or written — and the golden file in
//! `crates/fathom-analyze/tests/golden/` is this format's contract test.
//!
//! # The shape
//!
//! ```text
//! # Fathom review ledger
//!
//! <!-- format: fathom-review/1 -->
//! ...
//! ## sg-1f5c0a2b9d3e4a77 | accepted | 2026-10-19T14:22:03Z | Dana Ruiz <dana@example.com>
//!
//! - episode: ep-…
//! - decision: accepted
//! …
//! - digest: sha256:…
//! ```
//!
//! One entry per `## ` block, `- key: value` field lines under it, the tamper digest last.
//!
//! # Why this shape and not a table, a JSON file, or one file per sign-off
//!
//! * **Append-only, oldest first.** Two reviewers signing off on parallel branches both append; git sees two
//!   additions at the end of the file, which is the one conflict shape a merge can resolve by hand in seconds.
//!   A re-sorted or re-rendered file turns every sign-off into a conflict and teaches people to `--theirs` the
//!   ledger, which is the same as deleting reviews. Nothing in this crate rewrites an entry it did not just
//!   create ([`append_entries`] is the only writer of existing files), and the digest makes a rewritten entry
//!   visible.
//! * **`key: value` lines, not a markdown table.** A table's column widths depend on every other row, so one
//!   long path rewrites every other row's line — a diff that says nothing. A `|`-delimited table also cannot
//!   hold a 64-hex digest and a free-text note without a horizontal scroll.
//! * **Not JSON or TOML.** The file is read in a PR by a person, and it has to survive a person opening it in
//!   an editor and fixing one typo — without a comma budget. (The mirror row carries the JSON `scope`
//!   document, where a machine reads it and nobody has to.)
//! * **Not one file per sign-off.** Nicer for concurrent appends, worse for the *audit* — the point of the
//!   ledger is that one file answers "what has been accepted here", and `git log -- .fathom/reviews/ledger.md`
//!   is the review log. Multiple files are still *read* ([`discover`] globs the directory, like the reference
//!   implementation does) so a repository that splits by topic is not broken; writing goes to one default
//!   file.
//!
//! # Reading is deliberately loud
//!
//! A field line this build does not recognise is a reported problem, not a skipped line: a typo in
//! `fingreprint:` would otherwise read as "signed against no digest", which is the one mistake a ledger may
//! not make. Likewise a block with no fields, a timestamp that is not the documented shape, and a missing
//! `digest:` line. Every problem carries the file and the line number it came from.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use super::entry::{Actor, ActorKind, Decision, PathFact, SignOff, entry_digest, parse_iso_utc};

/// The directory the ledger lives in, relative to the repository root (ADR-004 §3, §12.1).
pub const LEDGER_DIR: &str = ".fathom/reviews";

/// The default ledger file's name. `fathom review sign` appends here; `discover` reads everything beside it.
pub const LEDGER_FILE: &str = "ledger.md";

/// The largest ledger file [`parse`] will read.
///
/// A ledger is markdown prose with one block per decision; the fixture corpus's largest is a few tens of
/// kilobytes. The cap is here for the case a size was never meant to cover: a clone that names a huge file
/// (or a device) `x.md` in `.fathom/reviews/` would otherwise have every reader of the ledger read it into
/// memory (`e7.8`, threat model F-7).
pub const MAX_LEDGER_BYTES: u64 = 32 * 1024 * 1024;

/// The format tag line, and the format this build writes.
pub const FORMAT_TAG: &str = "fathom-review/1";

/// The preamble a freshly created ledger gets.
///
/// It is prose on purpose: the file is read in a PR by someone who has never seen this tool, and three
/// sentences is cheaper than a support ticket. The `format:` tag inside it is what a later build parses.
#[must_use]
pub fn preamble() -> String {
    format!(
        concat!(
            "# Fathom review ledger\n",
            "\n",
            "<!-- format: {FORMAT_TAG} -->\n",
            "<!--\n",
            "     One block per human decision, appended oldest first. This file is the durable record:\n",
            "     the index in .fathom/index.sqlite3 is a rebuildable mirror of it, and deleting the store\n",
            "     loses no review.\n",
            "\n",
            "     Do not edit, reorder or re-wrap a block that has been committed. The `digest:` line at the\n",
            "     end of each block hashes the lines above it, so an edit is detectable and `fathom review\n",
            "     verify` will name it. To correct a decision, append a new block saying so.\n",
            "\n",
            "     `members:` are the commits the decision covered, `content:` is the digest of the file\n",
            "     contents it covered, and `fathom review status` re-checks both against the current history.\n",
            "-->\n"
        ),
        FORMAT_TAG = FORMAT_TAG
    )
}

/// The rendered block for one entry: heading, fields, digest.
#[must_use]
pub fn render_entry(entry: &SignOff) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "\n## {} | {} | {} | {}\n\n",
        entry.id,
        entry.decision.as_str(),
        super::entry::iso_utc(entry.signed_utc),
        entry.actor.display
    ));
    for (key, value) in &entry.body {
        out.push_str(&format!("- {key}: {}\n", one_line(value)));
    }
    out.push_str(&format!("- digest: {}\n", entry.digest));
    out
}

/// The whole file, entries in the order given — the shape the golden test blesses.
#[must_use]
pub fn render(entries: &[SignOff]) -> String {
    let mut out = preamble();
    for entry in entries {
        out.push_str(&render_entry(entry));
    }
    out
}

/// One ledger file's worth of parse output.
#[derive(Debug, Default)]
pub struct Parsed {
    /// The entries, in file order (oldest first by convention).
    pub entries: Vec<SignOff>,
    /// Everything wrong with the file, each with a line number.
    pub problems: Vec<Problem>,
    /// The `format:` tag the file declared, if it declared one.
    pub format: Option<String>,
}

/// One complaint about a ledger file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The file the complaint is about, relative to the repository root where possible.
    pub file: String,
    /// The 1-based line.
    pub line: usize,
    /// What is wrong, phrased for a terminal.
    pub what: String,
}

impl Problem {
    /// `path:12: what`, the shape every printer here uses.
    #[must_use]
    pub fn line(&self) -> String {
        format!("{}:{}: {}", self.file, self.line, self.what)
    }
}

/// The ledger files to read, sorted by name: the default file first when it exists, then any others.
///
/// A missing directory is not a problem — a repository where nobody has reviewed anything has no ledger, and
/// `fathom review status` says so in a sentence rather than an error.
#[must_use]
pub fn discover(root: &Path) -> Vec<PathBuf> {
    let dir = root.join(LEDGER_DIR);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension().is_some_and(|ext| ext == "md")).then_some(path)
        })
        .collect();
    files.sort();
    // `ledger.md` is the file the tool writes, so it leads the list even though the sort already put it
    // there in most names — "most" is not a reason for a report whose order a person reads.
    files.sort_by_key(|path| {
        (
            path.file_name() != Some(std::ffi::OsStr::new(LEDGER_FILE)),
            path.clone(),
        )
    });
    files
}

/// Parse one ledger file.
///
/// Never fails: a file that is unreadable or not UTF-8 is a [`Problem`] with the file's name, because "the
/// ledger cannot be read" and "the ledger holds 3 reviews" must not be the same answer, and the caller has
/// to be able to print both and continue with whatever else it found.
///
/// Two files are refused rather than read, and both refusals are a [`Problem`] rather than a silence:
///
/// * one that does not **resolve inside the repository** — `.fathom/reviews/x.md -> ~/.ssh/id_rsa` is a
///   symlink a clone can ship, and reading it would smuggle an arbitrary file of the reviewer's into a
///   report (`e7.8`, threat model F-1);
/// * one bigger than [`MAX_LEDGER_BYTES`] — the ledger is a few hundred lines of markdown, and a 4 GiB
///   file (or a device a clone pointed the name at) is not a ledger.
#[must_use]
pub fn parse(path: &Path, root: &Path) -> Parsed {
    let display = relative(path, root);
    if let Some(what) = refusal(path, root) {
        return Parsed {
            problems: vec![Problem {
                file: display,
                line: 1,
                what,
            }],
            ..Parsed::default()
        };
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return Parsed {
            problems: vec![Problem {
                file: display,
                line: 1,
                what: "this ledger file could not be read (missing, or not valid UTF-8)".to_owned(),
            }],
            ..Parsed::default()
        };
    };
    parse_text(&display, &text)
}

/// Why this path may not be read, in words, or `None` when it may.
fn refusal(path: &Path, root: &Path) -> Option<String> {
    if !resolves_inside(root, path) {
        return Some(
            "this ledger file resolves outside the repository (it is a symlink, or sits under one), and \
             its contents are not read — a clone must not be able to point a review at a file of \
             somebody else's"
                .to_owned(),
        );
    }
    match std::fs::metadata(path) {
        Ok(meta) if !meta.is_file() => Some(
            "this is not a regular file (a directory, a device or a fifo was named `*.md`), so it is not \
             read"
                .to_owned(),
        ),
        Ok(meta) if meta.len() > MAX_LEDGER_BYTES => Some(format!(
            "this ledger file is {} bytes, over the {} byte cap, so it is not read",
            meta.len(),
            MAX_LEDGER_BYTES
        )),
        Err(err) => Some(format!("this ledger file could not be stat'ed: {err}")),
        _ => None,
    }
}

/// Does `path` resolve, symlinks and all, to somewhere inside `root`?
///
/// `root` is canonicalised first (it is the caller's `--root`, which may be `.`), and the ledger file is
/// resolved through the filesystem. A path that cannot be resolved at all is treated as outside: a file
/// the reader cannot place is a file the reader does not open.
#[must_use]
fn resolves_inside(root: &Path, path: &Path) -> bool {
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    match path.canonicalize() {
        Ok(resolved) => resolved.starts_with(&root),
        Err(_) => false,
    }
}

/// Parse a ledger from its text. Split out so a test and the golden file exercise the same reader the file
/// does.
#[must_use]
pub fn parse_text(display: &str, text: &str) -> Parsed {
    let mut out = Parsed::default();
    let mut block: Option<Block> = None;
    let mut format: Option<String> = None;

    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let trimmed = raw.trim();
        if format.is_none()
            && let Some(rest) = trimmed.strip_prefix("<!-- format:")
        {
            format = Some(rest.trim_end_matches("-->").trim().to_owned());
        }
        if let Some(heading) = trimmed.strip_prefix("## ") {
            let finished = block.take();
            if let Some(finished) = finished {
                out.push_block(finished, display);
            }
            block = Some(Block {
                line,
                id: heading
                    .split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .to_owned(),
                body: Vec::new(),
                digest: None,
                digest_line: 0,
                saw_fields: false,
            });
            continue;
        }
        let Some(block) = block.as_mut() else {
            continue;
        };
        if trimmed.starts_with("<!") || trimmed.is_empty() || trimmed.starts_with("# ") {
            continue;
        }
        let Some(field) = trimmed.strip_prefix("- ") else {
            out.problems.push(Problem {
                file: display.to_owned(),
                line,
                what: format!(
                    "text inside an entry block that is not a `- key: value` line: {trimmed}"
                ),
            });
            continue;
        };
        let Some((key, value)) = field.split_once(':') else {
            out.problems.push(Problem {
                file: display.to_owned(),
                line,
                what: format!("a `-` line with no `key: value` in it: {field}"),
            });
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        block.saw_fields = true;
        if key == "digest" {
            block.digest = Some(value.to_owned());
            block.digest_line = line;
            continue;
        }
        if !SignOff::known_keys().contains(&key) {
            out.problems.push(Problem {
                file: display.to_owned(),
                line,
                what: format!("`{key}` is not a field this build knows; a typo here would read as a missing value"),
            });
            continue;
        }
        block.body.push((key.to_owned(), value.to_owned()));
    }
    if let Some(finished) = block.take() {
        out.push_block(finished, display);
    }
    out.format = format;
    if out.format.is_none() {
        out.problems.push(Problem {
            file: display.to_owned(),
            line: 1,
            what: format!("no `format: {FORMAT_TAG}` tag, so this file's shape is unclaimed"),
        });
    }
    out
}

struct Block {
    line: usize,
    id: String,
    body: Vec<(String, String)>,
    digest: Option<String>,
    digest_line: usize,
    saw_fields: bool,
}

impl Parsed {
    fn push_block(&mut self, block: Block, display: &str) {
        if !block.saw_fields {
            self.problems.push(Problem {
                file: display.to_owned(),
                line: block.line,
                what: "an entry block with no fields in it".to_owned(),
            });
            return;
        }
        let Some(digest) = block.digest.clone() else {
            self.problems.push(Problem {
                file: display.to_owned(),
                line: block.line,
                what: format!(
                    "entry `{}` has no `digest:` line, so it cannot be checked for edits",
                    block.id
                ),
            });
            return;
        };
        let expected = entry_digest(&block.id, &block.body);
        let tampered = expected != digest;
        if tampered {
            self.problems.push(Problem {
                file: display.to_owned(),
                line: block.digest_line,
                what: format!(
                    "entry `{}`: its `digest:` is {digest} and its lines hash to {expected} — the text changed after it was written",
                    block.id
                ),
            });
        }
        match to_entry(&block.id, &block.body, &digest) {
            Ok(entry) => self.entries.push(entry),
            Err(what) => self.problems.push(Problem {
                file: display.to_owned(),
                line: block.line,
                what: format!("entry `{}`: {what}", block.id),
            }),
        }
    }
}

/// Turn parsed field lines into a [`SignOff`], refusing a block whose required fields are unusable.
fn to_entry(id: &str, body: &[(String, String)], digest: &str) -> Result<SignOff, String> {
    let get = |key: &str| {
        body.iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .unwrap_or_default()
    };
    let episode = get("episode").to_owned();
    if episode.is_empty() {
        return Err("no `episode:` field, so nothing here names what was reviewed".to_owned());
    }
    let Some(decision) = Decision::parse(get("decision")) else {
        return Err(format!(
            "`{}` is not a decision (accepted, flagged, dismissed, snoozed)",
            get("decision")
        ));
    };
    let signed_utc = match parse_iso_utc(get("signed")) {
        Some(stamp) => stamp,
        None => {
            return Err(format!(
                "`signed: {}` is not a UTC instant like 2026-10-19T14:22:03Z",
                get("signed")
            ));
        }
    };
    let actor = Actor {
        display: if get("actor").is_empty() {
            "unknown".to_owned()
        } else {
            get("actor").to_owned()
        },
        kind: ActorKind::parse(get("actor-kind")),
    };
    let members = split_list(get("members"));
    let paths: Vec<PathFact> = split_list(get("paths"))
        .iter()
        .map(|text| PathFact::parse(text))
        .collect();
    let scale = super::entry::Scale {
        commits: members.len().try_into().unwrap_or(i64::MAX),
        files: paths.len().try_into().unwrap_or(i64::MAX),
        add: number_after(get("scale"), '+', ' '),
        del: number_after(get("scale"), '-', ' '),
    };
    let (risk, risk_model) = match get("risk") {
        "" => (None, String::new()),
        text => (
            text.split([' ', '(']).next().and_then(|n| n.parse().ok()),
            text.split('(')
                .nth(1)
                .map(|rest| rest.trim_end_matches(')').to_owned())
                .unwrap_or_default(),
        ),
    };
    let entry = SignOff {
        id: id.to_owned(),
        file: String::new(),
        episode,
        fingerprint: digest_field(get("fingerprint")),
        fallback: digest_field(get("fallback")),
        shape: digest_field(get("shape")),
        content: digest_field(get("content")),
        decision,
        reason: get("reason").to_owned(),
        note: get("note").to_owned(),
        actor,
        signed_utc,
        members,
        paths,
        scale,
        risk,
        risk_model,
        body: body.to_vec(),
        digest: digest.to_owned(),
    };
    // The id is derived from the act, so an id that no longer matches its fields is itself a tamper signal —
    // but the digest check is the one that catches it, and `verify` prints that one. Nothing to do here
    // beyond keeping the id the file claims: a re-labelled id must not silently re-key the mirror.
    Ok(entry)
}

/// `unmeasured (…)` and a bare 64-hex digest are the two spellings; anything else is kept as written so a
/// reader can see it rather than being shown an empty string.
fn digest_field(value: &str) -> String {
    if value.starts_with("unmeasured") {
        String::new()
    } else {
        value.to_owned()
    }
}

/// A comma-separated field into a list; empty and `-` both mean none.
fn split_list(value: &str) -> Vec<String> {
    if value.trim().is_empty() || value.trim() == "-" {
        return Vec::new();
    }
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `3 commit(s), 2 file(s), +41 -12` → the number after `sign`, up to `stop`.
fn number_after(text: &str, sign: char, stop: char) -> i64 {
    let Some(rest) = text.rsplit(sign).next() else {
        return 0;
    };
    let token: String = rest
        .trim()
        .chars()
        .take_while(|c| *c != stop && !c.is_whitespace())
        .collect();
    token.parse().unwrap_or_default()
}

/// Append entries to a ledger file, creating it (with its preamble) when it does not exist.
///
/// The file is flushed **and fsynced before returning**, which is ADR-004 §7.4's R8: a mirrored sign-off
/// whose file is still in the OS buffer cache is a lost review with a row claiming otherwise. The mirror
/// write happens after this call returns, never before.
pub fn append_entries(path: &Path, entries: &[SignOff]) -> std::io::Result<usize> {
    if entries.is_empty() {
        return Ok(0);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let fresh = !path.exists();
    let trailing_newline = std::fs::read(path)
        .map(|bytes| !bytes.is_empty() && bytes[bytes.len() - 1] != b'\n')
        .unwrap_or(false);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    if fresh {
        file.write_all(preamble().as_bytes())?;
    } else if trailing_newline {
        // Append onto a proper last line: a file whose final newline was lost by a hand edit would otherwise
        // glue the new entry's heading onto the previous entry's digest.
        file.write_all(b"\n")?;
    }
    for entry in entries {
        file.write_all(render_entry(entry).as_bytes())?;
    }
    file.flush()?;
    file.sync_all()?;
    Ok(entries.len())
}

/// The path `fathom review sign` writes.
#[must_use]
pub fn default_path(root: &Path) -> PathBuf {
    root.join(LEDGER_DIR).join(LEDGER_FILE)
}

/// A path relative to the repository root where that is possible, absolute where it is not.
fn relative(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Collapse newlines in a value so every field stays on one line.
///
/// A note is one line. A note with a newline in it would either break the `- key: value` rule or need a
/// continuation syntax nobody will parse correctly, and the note is a sentence ("checked the blast radius"),
/// not an essay. The paragraph break becomes ` ⏎ `, the same marker the findings use, so the shape of the
/// original is still visible.
#[must_use]
pub fn one_line(value: &str) -> String {
    let folded = value.replace("\r\n", " ⏎ ").replace(['\n', '\r'], " ⏎ ");
    let mut out = String::with_capacity(folded.len());
    let mut last_space = false;
    for ch in folded.chars() {
        let space = ch == ' ';
        if space && last_space {
            continue;
        }
        last_space = space;
        out.push(ch);
    }
    out.trim().to_owned()
}

/// The line a caller prints for one entry's actor/decision — shared so the CLI and a test print one shape.
#[must_use]
pub fn summary_line(entry: &SignOff, extra: &str) -> String {
    format!(
        "{}  {:<9} {:>19}  {}{}",
        entry.id,
        entry.decision.as_str(),
        super::entry::iso_utc(entry.signed_utc),
        entry.episode,
        if extra.is_empty() {
            String::new()
        } else {
            format!("  {extra}")
        }
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::ledger::entry::{Actor, ActorKind, Decision, DiffTouch, EpisodeFacts, sign};

    fn actor() -> Actor {
        Actor {
            display: "Dana Ruiz <dana@example.com>".to_owned(),
            kind: ActorKind::Human,
        }
    }

    fn episode() -> EpisodeFacts {
        EpisodeFacts {
            id: "ep-abc".to_owned(),
            kind: "committed".to_owned(),
            live: true,
            fingerprint: "fp-1".to_owned(),
            fallback_fingerprint: "fp-2".to_owned(),
            shape_fingerprint: "fp-3".to_owned(),
            summary: "feat: the thing".to_owned(),
            members: vec!["aaaa".repeat(10), "bbbb".repeat(10)],
            content: crate::ledger::entry::ContentProfile::from_diff_rows(
                &[DiffTouch {
                    ordinal: 0,
                    path: "src/a.rs",
                    change_kind: "add",
                    add: 10,
                    del: 0,
                    blob: Some("ccc"),
                }],
                true,
            ),
            ..EpisodeFacts::default()
        }
    }

    /// The invariant the whole tamper story rests on: what `sign` digests is what the file holds, so reading
    /// the file back cannot disagree with the entry that was written.
    ///
    /// This is the test that the first implementation failed — `note` was not a known key, and `one_line`
    /// folded text the digest had already hash — and the failure mode was not a crash but every honest entry
    /// reported as tampered. A digest that does not survive an honest round trip detects nothing; it only
    /// cries.
    #[test]
    fn a_signed_entry_survives_the_round_trip() {
        for note in [
            "",
            "ok",
            "two   spaces   here",
            "line\nbreak",
            "colon: inside, and — dash, and \"quotes\"",
            "trailing space ",
        ] {
            let entry = sign(
                &episode(),
                &actor(),
                Decision::Accepted,
                note,
                1_760_000_000_000,
            )
            .unwrap_or_else(|err| panic!("signing failed: {}", err.message()));
            let parsed = parse_text("ledger.md", &render(std::slice::from_ref(&entry)));
            assert!(
                parsed.problems.is_empty(),
                "note {note:?} round-tripped with problems: {:?}",
                parsed.problems
            );
            assert_eq!(parsed.entries.len(), 1, "note {note:?}");
            let back = &parsed.entries[0];
            assert_eq!(back.id, entry.id);
            assert_eq!(
                back.body, entry.body,
                "the file's lines are the digested lines for {note:?}"
            );
            assert_eq!(
                back.digest, entry.digest,
                "the digest verifies for {note:?}"
            );
            assert_eq!(
                back.missing_keys(),
                Vec::<&str>::new(),
                "nothing required is missing"
            );
        }
    }

    #[test]
    fn a_file_without_the_format_tag_says_so() {
        let parsed = parse_text(
            "ledger.md",
            "## sg-x | accepted | 2026-10-19T14:22:03Z | Dana\n",
        );
        assert!(
            parsed
                .problems
                .iter()
                .any(|problem| problem.what.contains("format")),
            "{:?}",
            parsed.problems
        );
    }

    #[test]
    fn an_unknown_field_is_a_problem_rather_than_a_silently_missing_value() {
        let entry = sign(&episode(), &actor(), Decision::Accepted, "", 1)
            .expect("the fixture episode signs");
        let text = format!(
            "{}- fingerpint: typo\n",
            render_entry(&entry)
                .trim_end_matches("\n")
                .replace(&format!("- digest: {}", entry.digest), "")
        );
        let parsed = parse_text("ledger.md", &text);
        assert!(
            parsed
                .problems
                .iter()
                .any(|problem| problem.what.contains("not a field this build knows")),
            "{:?}",
            parsed.problems
        );
    }

    #[test]
    fn an_entry_with_no_digest_line_cannot_be_checked_and_says_so() {
        let entry = sign(&episode(), &actor(), Decision::Accepted, "", 1)
            .expect("the fixture episode signs");
        let text = render_entry(&entry).replace(&format!("- digest: {}\n", entry.digest), "");
        let parsed = parse_text("ledger.md", &preamble_with(&text));
        assert!(
            parsed
                .problems
                .iter()
                .any(|problem| problem.what.contains("no `digest:` line")),
            "{:?}",
            parsed.problems
        );
    }

    /// The preamble is what makes a file a ledger; a bare entry block is not one.
    fn preamble_with(body: &str) -> String {
        format!("{}{body}", preamble())
    }
}
