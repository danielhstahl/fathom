//! The redaction table: the secret shapes that may never leave this process, and the pass that removes
//! them (ADR-005 §6 rule 4, §4.3).
//!
//! One table, two doors. A harness payload is redacted on the way *in* (`fathom-ingest`, `e6.1`) and a tool
//! result is redacted on the way *out* (`fathom-mcp`, `e6.4`), and the two doors disagreeing about what a
//! secret looks like is the failure this module exists to prevent — hence its home in `fathom-core`, the one
//! crate both may name. A rule added here fires at both doors on the same build.
//!
//! Two properties the callers depend on:
//!
//! * **The marker names the rule.** `[REDACTED:aws-access-key]`, not `[REDACTED]`. A reader who can see
//!   *which* rule fired can tell "the scanner worked" from "the scanner was confused by a long identifier",
//!   and a reviewer looking at an artefact is exactly the person who has to make that call.
//! * **Redaction is a value-level pass, not a field list.** [`redact_value`] walks every string leaf,
//!   because an allowlist of sensitive field names fails the first time a vendor adds one (§4.3).
//!
//! The matchers are hand-rolled rather than a `regex` dependency, for the reason ADR-002 §6 rule 1 gives:
//! every dependency is a decision, and this one buys a table five rules deep. The list is deliberately the
//! same five rules, in the same order, as `src/recommender/harness/harness.py`'s `REDACT_RULES`, so the
//! reference implementation and the shipped one print the same rule name for the same bytes.

use serde_json::Value;

/// One redaction rule: a name and a hand-rolled matcher (the workspace takes no `regex` dependency, and
/// ADR-002 §6 rule 1 makes every dependency a decision rather than a default).
#[derive(Debug, Clone, Copy)]
pub struct RedactRule {
    /// The stable rule name, printed as `[REDACTED:<rule>]`.
    pub name: &'static str,
    /// What it looks for, as a substring to find a candidate and a check to confirm it.
    pub matcher: fn(&str) -> Option<(usize, usize)>,
}

/// The rules, in the order `src/recommender/harness/harness.py`'s `REDACT_RULES` lists them, so the two
/// implementations agree on which name a redaction carries.
pub const REDACT_RULES: &[RedactRule] = &[
    RedactRule {
        name: "aws-access-key",
        matcher: find_aws_key,
    },
    RedactRule {
        name: "generic-token",
        matcher: find_generic_token,
    },
    RedactRule {
        name: "private-key",
        matcher: find_private_key,
    },
    RedactRule {
        name: "bearer",
        matcher: find_bearer,
    },
    RedactRule {
        name: "high-entropy",
        matcher: find_high_entropy,
    },
];

/// The marker a redaction leaves behind.
pub fn redaction_marker(rule: &str) -> String {
    format!("[REDACTED:{rule}]")
}

fn find_aws_key(text: &str) -> Option<(usize, usize)> {
    let mut start = 0;
    while let Some(relative) =
        find_from(text, "AKIA", start).or_else(|| find_from(text, "ASIA", start))
    {
        let end = (relative + 20).min(text.len());
        let candidate = &text[relative..end];
        if candidate.len() == 20
            && candidate
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        {
            return Some((relative, end));
        }
        start = relative + 4;
    }
    None
}

/// A `key = value` / `"key": "value"` assignment of something the vendor called a credential.
///
/// The value is taken whole — quotes and all that are left of it — because a redaction that leaves the
/// quote behind is a redaction a reader has to trust. Both shapes matter: a hook line spells it
/// `api_key=…`, and a JSON payload spells it `"api_key": "…"`, and the closing quote after the key name is
/// part of the separator, not part of the value. Before `e6.4` rewired this table into a second door, the
/// JSON shape read as an empty value and a secret in a JSON field walked out unredacted; that is the kind
/// of bug a rule table hides until something outside it tries to use it.
fn find_generic_token(text: &str) -> Option<(usize, usize)> {
    const KEYS: [&str; 6] = [
        "api_key",
        "api-key",
        "apikey",
        "access_token",
        "secret",
        "password",
    ];
    let lower = text.to_ascii_lowercase();
    for key in KEYS {
        let mut from = 0;
        while let Some(at) = find_from(&lower, key, from) {
            let rest = &text[at + key.len()..];
            let mut separator = rest.trim_start();
            let mut gap = rest.len() - separator.len();
            if separator.starts_with(['"', '\'']) {
                separator = &separator[1..];
                gap += 1;
            }
            if separator.starts_with(':') || separator.starts_with('=') {
                let after = &separator[1..];
                let lead = after.len() - after.trim_start().len();
                let value_start = at + key.len() + gap + 1 + lead;
                let value = &text[value_start.min(text.len())..];
                let opening = value.chars().next().filter(|c| matches!(c, '"' | '\''));
                let body = if opening.is_some() {
                    &value[1..]
                } else {
                    value
                };
                let body_end = if opening.is_some() {
                    body.find(['"', '\'']).unwrap_or(body.len())
                } else {
                    body.find(['"', '\'', ',', ' ', '\n']).unwrap_or(body.len())
                };
                if body_end > 0 {
                    return Some((at, value_start + (value.len() - body.len()) + body_end));
                }
            }
            from = at + key.len();
        }
    }
    None
}

fn find_private_key(text: &str) -> Option<(usize, usize)> {
    let at = text.find("-----BEGIN ")?;
    let rest = text[at..].find("PRIVATE KEY-----")?;
    Some((at, at + rest + "PRIVATE KEY-----".len()))
}

fn find_bearer(text: &str) -> Option<(usize, usize)> {
    let lower = text.to_ascii_lowercase();
    let at = find_from(&lower, "bearer", 0)?;
    let rest = &text[at + "bearer".len()..];
    let gap = rest.len() - rest.trim_start().len();
    let value = rest.trim_start();
    let value_end = value
        .find(['"', '\'', ',', ' ', '\n'])
        .unwrap_or(value.len());
    if value[..value_end].chars().count() < 20 {
        return None;
    }
    Some((at, at + "bearer".len() + gap + value_end))
}

/// A long run of base64-ish characters: the shape of a token a vendor echoed back verbatim.
///
/// One deliberate exclusion, and it is this product's own: a run that is **pure hexadecimal** is a digest,
/// not a credential. Git object ids, `e4.6`'s sign-off fingerprints and every evidence pointer the delta
/// model carries are 40-or-64-character hex strings, and an egress filter that ate them would return an
/// answer whose every citation said `[REDACTED]` — a certain loss of the evidence trail traded against the
/// unlikely case of a credential that is both 40+ characters and pure hex. A secret that arrives behind a
/// key name (`api_key = <hex>`) is still caught by [`find_generic_token`], which redacts the assignment
/// rather than guessing from the shape of the value.
fn find_high_entropy(text: &str) -> Option<(usize, usize)> {
    let token_byte = |byte: u8| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=');
    let bytes = text.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if !token_byte(bytes[index]) {
            index += 1;
            continue;
        }
        let start = index;
        while index < bytes.len() && token_byte(bytes[index]) {
            index += 1;
        }
        let candidate = &text[start..index];
        if candidate.len() < 40 || digest_shaped(candidate) {
            continue;
        }
        // A run of 40 base64 characters that is *all* lowercase prose is a long identifier, not a token;
        // require the mixed case or symbol content that random bytes actually have.
        let distinct = candidate
            .chars()
            .filter(|c| {
                c.is_ascii_uppercase() || c.is_ascii_digit() || matches!(c, '+' | '/' | '=')
            })
            .count();
        if distinct >= 8 {
            return Some((start, index));
        }
    }
    None
}

/// Is this run only hex characters? Then it is a hash of something, and this table does not redact hashes
/// the model needs in order to cite itself.
fn digest_shaped(candidate: &str) -> bool {
    candidate.len() >= 32 && candidate.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn find_from(haystack: &str, needle: &str, from: usize) -> Option<usize> {
    if from >= haystack.len() {
        return None;
    }
    haystack[from..].find(needle).map(|at| at + from)
}

/// Redact every rule out of one string, reporting which rules fired.
#[must_use]
pub fn redact(text: &str) -> (String, Vec<&'static str>) {
    let mut out = text.to_owned();
    let mut hits: Vec<&'static str> = Vec::new();
    for rule in REDACT_RULES {
        // A rule can fire more than once; keep going until it stops matching, bounded so a pathological
        // overlap cannot spin forever.
        for _ in 0..64 {
            let Some((start, end)) = (rule.matcher)(&out) else {
                break;
            };
            if !hits.contains(&rule.name) {
                hits.push(rule.name);
            }
            out.replace_range(start..end, &redaction_marker(rule.name));
        }
    }
    if !hits.is_empty() {
        hits.sort_unstable();
    }
    (out, hits)
}

/// Redact a whole payload in place: every string leaf, wherever it sits.
///
/// Redaction runs on outbound content rather than on an allowlist of fields, because an allowlist fails
/// the first time a producer adds one (ADR-005 §4.3). Both callers are named in that ADR's own rule 4 —
/// `fathom-ingest` redacts a harness payload on the way *in*, and the MCP server redacts a tool result on
/// the way *out* — and they must agree, which is why the pass lives here rather than in either of them.
pub fn redact_value(value: &mut Value, hits: &mut Vec<&'static str>) {
    match value {
        Value::String(text) => {
            let (clean, fired) = redact(text);
            if !fired.is_empty() {
                *text = clean;
                hits.extend(fired);
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_value(item, hits);
            }
        }
        Value::Object(map) => {
            for (_key, item) in map.iter_mut() {
                redact_value(item, hits);
            }
        }
        _ => {}
    }
}
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn a_git_object_id_survives_because_it_is_an_evidence_pointer_not_a_secret() {
        // The shape every fingerprint in the delta model has. Redacting these would make an MCP answer
        // unable to cite its own evidence, which is the property ADR-001 §7.10 is built on.
        let text = "commit eedbfbbfb0ea2416e0d66747a5ec50665d27c498 in ep-1e48d7a8575201572244";
        let (clean, hits) = redact(text);
        assert_eq!(clean, text, "nothing here is a credential: {clean}");
        assert!(hits.is_empty(), "{hits:?}");
    }

    #[test]
    fn a_mixed_case_base64_token_still_fires() {
        let text = "the vendor echoed Authorization: XYZabcDEF1234567890gHiJkLmNoPqRsTuVwXyZ777";
        let (clean, hits) = redact(text);
        assert!(clean.contains("[REDACTED:high-entropy]"), "{clean}");
        assert!(hits.contains(&"high-entropy"));
    }

    #[test]
    fn a_hex_secret_behind_a_key_name_is_still_refused() {
        // The exclusion above is about a bare digest in prose; an assignment is a different shape and the
        // generic-token rule redacts the whole of it.
        let text = "\"api_key\": \"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef\"";
        let (clean, hits) = redact(text);
        assert!(clean.contains("[REDACTED:generic-token]"), "{clean}");
        assert!(!clean.contains("deadbeef"), "{clean}");
        assert!(hits.contains(&"generic-token"));
    }
}
