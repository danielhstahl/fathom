//! The egress filter: what leaves the process, and what never does (ADR-005 §4.3).
//!
//! §4.3's rule is absolute — *never, in any tool response, in any field* — and a rule that absolute cannot
//! be carried by an allowlist of fields the sources happen to fill in. This module is the choke point every
//! tool answer passes through on its way to the wire, and it enforces three things:
//!
//! * **Secrets are redacted here**, by the same rule table the ingest door uses ([`fathom_core::redact`]),
//!   because a value assembled by a tool out of several stored fields can spell a secret that no single
//!   field contained. §6 rule 4 asks for redaction at ingest *and* again at MCP egress; this is the second
//!   one.
//! * **File bodies are refused by shape, not by trust.** A source that puts a `content`, `diff`, `patch`,
//!   `hunk` or `body` key into a response gets that value dropped and replaced with a marker, whatever it
//!   intended. "Show me the file" is the agent's problem, through its own permission system, where the user
//!   can see it happen (§4.3).
//! * **Long strings are bounded.** Every leaf is capped, and the answer as a whole is capped by the caller
//!   ([`crate::server`], which truncates the item list and hands back a cursor when the envelope will not
//!   fit). Token-size discipline is a property of the transport, so it is enforced in the transport.

use serde_json::{Value, json};

use fathom_core::redact;

/// The longest any single string in an answer may be.
///
/// Chosen to hold a sentence a human would write and a path a tool would print, and to be far below anything
/// that could be a file body, a diff or a prompt payload. A leaf that gets cut is reported in the
/// response's `unknowns`, because a silently shortened field is the same lie as a silently short page.
pub const MAX_STRING_BYTES: usize = 2048;

/// The keys that may never appear in an answer, whatever produced them.
///
/// These are the names a file body, a diff hunk or a raw payload travels under — `lines` and `hunk` are what
/// a lockfile proof is made of, `snippet` is what a search hit is made of, and both of those are produced
/// upstream by code this crate does not control. `note` is deliberately not here: `add_review_note`'s note
/// is a review's own words, which is the model, not a body.
pub const FORBIDDEN_KEYS: &[&str] = &[
    "body",
    "content",
    "contents",
    "diff",
    "file_body",
    "hunk",
    "patch",
    "lines",
    "patch",
    "raw",
    "snippet",
    "snippets",
    "source_code",
    "text",
];

/// What one pass through [`filter`] changed, so the caller can say so in `unknowns`.
#[derive(Debug, Default, Clone)]
pub struct Filtered {
    /// Redaction rule names that fired (`aws-access-key`, `bearer`, …).
    pub redactions: Vec<&'static str>,
    /// Keys dropped because they carry file bodies.
    pub refused_keys: Vec<String>,
    /// How many strings were cut to [`MAX_STRING_BYTES`].
    pub truncated_strings: usize,
}

impl Filtered {
    /// The sentences to append to the response's `unknowns`, empty when nothing was changed.
    #[must_use]
    pub fn notes(&self) -> Vec<String> {
        let mut notes = Vec::new();
        if !self.redactions.is_empty() {
            notes.push(format!(
                "secret-shaped text was redacted at egress by the rules [{}] (ADR-005 §4.3)",
                self.redactions.join(", ")
            ));
        }
        if !self.refused_keys.is_empty() {
            notes.push(format!(
                "field(s) [{}] were dropped: file contents never leave the model (ADR-005 §4.3); ask the \
                 agent to read the file through its own permissions",
                self.refused_keys.join(", ")
            ));
        }
        if self.truncated_strings > 0 {
            notes.push(format!(
                "{} string(s) longer than {MAX_STRING_BYTES} bytes were cut; the full text is in the \
                 document `fathom analyze <episode> --json` prints",
                self.truncated_strings
            ));
        }
        notes
    }
}

/// Run the egress filter over an answer in place.
pub fn filter(value: &mut Value, out: &mut Filtered) {
    match value {
        Value::Object(map) => {
            let forbidden: Vec<String> = map
                .keys()
                .filter(|key| FORBIDDEN_KEYS.contains(&key.as_str()))
                .cloned()
                .collect();
            for key in forbidden {
                map.insert(
                    key.clone(),
                    json!("[refused: file contents never leave the model (ADR-005 §4.3)]"),
                );
                out.refused_keys.push(key);
            }
            for item in map.values_mut() {
                filter(item, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                filter(item, out);
            }
        }
        Value::String(text) => {
            let (clean, hits) = redact::redact(text);
            if !hits.is_empty() {
                for hit in hits {
                    if !out.redactions.contains(&hit) {
                        out.redactions.push(hit);
                    }
                }
                *text = clean;
            }
            if text.len() > MAX_STRING_BYTES {
                let cut = floor_char_boundary(text, MAX_STRING_BYTES);
                text.truncate(cut);
                text.push_str("…[cut at ");
                text.push_str(&MAX_STRING_BYTES.to_string());
                text.push_str(" bytes]");
                out.truncated_strings += 1;
            }
        }
        _ => {}
    }
}

/// The largest byte index `<= limit` that is a character boundary.
fn floor_char_boundary(text: &str, limit: usize) -> usize {
    if limit >= text.len() {
        return text.len();
    }
    let mut index = limit;
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}
