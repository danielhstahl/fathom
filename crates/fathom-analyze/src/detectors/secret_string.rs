//! `secret_string.rs` — a committed string has the shape of a credential (`e4.3`).
//!
//! **What it claims.** An added line contains something that matches a vendor's published credential
//! shape, or the general `key = "value"` shape with a value that does not look like a placeholder. It is
//! the one detector in the catalogue where a miss is worse than a false positive by a wide margin: a real
//! key in a repository that gets cloned, mirrored and indexed is revoked in hours, and the cost of the
//! revocation lands on whoever notices. So this one errs toward the row.
//!
//! # The finding never repeats the secret
//!
//! This is the detector's central design rule and it is not negotiable: **the matched value is masked in
//! every field the finding has.** A finding that copied the key into its message would take a secret out
//! of one file and put it into the store, the delta document, the exported artefact, an MCP tool result and
//! possibly a paste in a chat window — it would be a leak amplifier wearing a warning label. The same
//! rule ADR-001 §6.3 applies to file contents in the delta document applies harder here.
//!
//! The mask keeps enough to *recognise*: `AKIA…OP (20 chars)`. A reviewer who needs the value has the
//! diff, which is where it already was.
//!
//! # Two confidences, and the guards that earn them
//!
//! * **vendor-shaped** (`0.85`): `AKIA…`, `ghp_…`, `xoxb-…`, `sk-…`, `AIza…`, `-----BEGIN … PRIVATE
//!   KEY-----`. A prefix plus the right alphabet and length is a credential from a provider's own
//!   convention; the only thing that makes it wrong is a documentation fixture, which is why the
//!   placeholder list below applies to these too.
//! * **generic** (`0.55`): an assignment to `api_key`, `token`, `secret`, `password`, `credential` (in
//!   any of the usual spellings) with a quoted value of twelve characters or more. This is the noisy one,
//!   and it is held down by three refusals: a value that reads as a placeholder (`changeme`, `example`,
//!   `your-key`, `<token>`, `${SECRET}`, `os.environ[...]`, all-same-character, all-letters-with-no-digit)
//!   is not a finding; a value with no digit and no symbol is not a finding; and a prose, vendored,
//!   lockfile or generated path is not read at all.
//!
//! **In a test path** the confidence drops (vendor `0.7`, generic `0.4`) and the message says so, rather
//! than the detector going quiet: a hardcoded credential in a test is still a credential, and a
//! `testdata/` directory full of fakes is the one place a synthetic key reliably lives. The reviewer decides.
//!
//! **What it cannot see, and says nothing about:** a secret assembled from pieces, one read from the
//! environment, one that predates this episode (out of scope: this is a *delta* detector), and a secret in
//! a language or quoting style the shape list does not cover.

use std::sync::OnceLock;

use regex::Regex;

use crate::detectors::finding::{Evidence, EvidenceKind, Finding, Severity};
use crate::detectors::model::{Corpus, Episode, FileChange};
use crate::detectors::policy::Policy;
use crate::detectors::text::{self, Dialect};
use crate::risk::Surface;

/// One credential shape.
struct Shape {
    /// What to call it in the finding, in the provider's own words.
    name: &'static str,
    /// A provider's published prefix: near-certain, and the difference between `high` and `medium`.
    vendor: bool,
    /// The pattern. Value is capture 1 when [`Self::value_group`] is `1`.
    pattern: &'static str,
    /// Which capture holds the value, `0` for the whole match.
    value_group: usize,
}

const SHAPES: &[Shape] = &[
    Shape {
        name: "AWS access key id",
        vendor: true,
        pattern: r"\b(AKIA|ASIA)[0-9A-Z]{16}\b",
        value_group: 0,
    },
    Shape {
        name: "GitHub token",
        vendor: true,
        pattern: r"\b(gh[pousr]_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{40,})\b",
        value_group: 0,
    },
    Shape {
        name: "GitLab token",
        vendor: true,
        pattern: r"\bglpat-[A-Za-z0-9_-]{20}\b",
        value_group: 0,
    },
    Shape {
        name: "Slack token",
        vendor: true,
        pattern: r"\bxox[baprs]-[0-9A-Za-z-]{10,}\b",
        value_group: 0,
    },
    Shape {
        name: "OpenAI-style key",
        vendor: true,
        pattern: r"\bsk-[A-Za-z0-9_-]{32,}\b",
        value_group: 0,
    },
    Shape {
        name: "Stripe live key",
        vendor: true,
        pattern: r"\bsk_live_[0-9a-zA-Z]{24,}\b",
        value_group: 0,
    },
    Shape {
        name: "Google API key",
        vendor: true,
        pattern: r"\bAIza[0-9A-Za-z_-]{35}\b",
        value_group: 0,
    },
    Shape {
        name: "SendGrid key",
        vendor: true,
        pattern: r"\bSG\.[A-Za-z0-9_-]{20,32}\.[A-Za-z0-9_-]{16,}\b",
        value_group: 0,
    },
    Shape {
        name: "npm token",
        vendor: true,
        pattern: r"\bnpm_[A-Za-z0-9]{36}\b",
        value_group: 0,
    },
    Shape {
        name: "private key block",
        vendor: true,
        pattern: r"-----BEGIN (?:[A-Z]+ )?PRIVATE KEY-----",
        value_group: 0,
    },
    Shape {
        name: "a key/value assignment with a secret-looking value",
        vendor: false,
        pattern: r#"(?i)\b(?:api[_-]?key|apikey|secret[_-]?key|access[_-]?token|refresh[_-]?token|auth[_-]?token|client[_-]?secret|password|passwd|passphrase|token|secret)\b["']?\s*[:=]\s*["']([^"'\s]{12,})["']"#,
        value_group: 1,
    },
];

/// The compiled table, built once per process: these patterns are constants, and compiling them per
/// episode would make a scan cost a function of the corpus size for no reason.
fn compiled() -> &'static [(&'static Shape, Regex)] {
    static COMPILED: OnceLock<Vec<(&'static Shape, Regex)>> = OnceLock::new();
    COMPILED.get_or_init(|| {
        SHAPES
            .iter()
            .filter_map(|shape| Regex::new(shape.pattern).ok().map(|re| (shape, re)))
            .collect()
    })
}

/// Values that are obviously not real, in any spelling a repository uses for them.
const PLACEHOLDER_MARKERS: &[&str] = &[
    "changeme",
    "change-me",
    "placeholder",
    "example",
    "sample",
    "your-",
    "your_",
    "my-key",
    "my_key",
    "dummy",
    "fake",
    "dummy",
    "redacted",
    "xxxx",
    "0000000",
    "deadbeef",
    "notareal",
    "not-a-real",
    "s3cr3t",
    "hunter2",
    "abcdefgh",
    "todo",
    "test",
    "fixture",
    "golden",
    "process.env",
    "os.environ",
    "dotenv",
    "secret_ref",
    "none",
    "null",
    "empty",
];

pub fn detect(episode: &Episode, _corpus: &Corpus<'_>, _policy: &Policy) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in &episode.files {
        if !reads(file) {
            continue;
        }
        let in_tests = file.is_test();
        let mut seen: Vec<(String, String, bool)> = Vec::new();
        for line in file.added() {
            if text::is_blank_or_comment(
                line,
                text::dialect(&file.path).unwrap_or(Dialect::Unknown),
            ) {
                // A commented-out key is still a committed key, but this detector's claim is about the
                // program, and a commented shape is documentation about a shape. The gap is stated in the
                // module header rather than silently excluded.
                continue;
            }
            for (shape, re) in compiled() {
                let Some(caps) = re.captures(line) else {
                    continue;
                };
                let whole = caps.get(0).map_or("", |m| m.as_str());
                let value = caps
                    .get(shape.value_group)
                    .or_else(|| caps.get(0))
                    .map_or(whole, |m| m.as_str());
                if looks_placeholder(value) {
                    continue;
                }
                if !shape.vendor && !looks_credential_like(value) {
                    continue;
                }
                let key = (shape.name, value.to_owned());
                if seen
                    .iter()
                    .any(|(name, held, _)| *name == key.0 && held == &key.1)
                {
                    continue;
                }
                seen.push((shape.name.to_owned(), value.to_owned(), shape.vendor));
            }
        }
        for (name, value, vendor) in seen {
            let confidence = match (vendor, in_tests) {
                (true, false) => 0.85,
                (true, true) => 0.7,
                (false, false) => 0.55,
                (false, true) => 0.4,
            };
            findings.push(
                Finding::build(
                    "secret_string",
                    1,
                    &episode.id,
                    file.path.clone(),
                    format!(
                        "{} adds {} — masked {} ({} characters){}",
                        file.path,
                        name,
                        mask(&value),
                        value.chars().count(),
                        if in_tests {
                            ", in a test path, where a synthetic value is possible"
                        } else {
                            ""
                        }
                    ),
                )
                .severity(if vendor {
                    Severity::High
                } else {
                    Severity::Medium
                })
                .confidence(confidence)
                .evidence(Evidence::observed(
                    EvidenceKind::Range,
                    format!(
                        "{} {}",
                        file.path,
                        file.hunks
                            .first()
                            .map_or_else(String::new, |hunk| hunk.header.clone())
                    ),
                    format!("shape: {name}; value: {}", mask(&value)),
                ))
                .evidence(Evidence::inferred(
                    EvidenceKind::Range,
                    format!("fathom show {} --patch", episode.id),
                    "the unmasked line is in the episode's diff, which is where it already was",
                ))
                .jump_path(&file.path)
                .finish(),
            );
        }
    }
    findings
}

/// Hand-written source only. Prose *documents* keys, a lockfile is somebody else's hashes, and vendored
/// code is not this repository's credential to find.
fn reads(file: &FileChange) -> bool {
    !file.binary
        && !file.noisy()
        && !file.is_delete()
        && !matches!(
            file.surface(),
            Surface::Docs | Surface::Vendored | Surface::Dependency | Surface::Generated
        )
        && text::dialect(&file.path).is_some_and(Dialect::is_code)
}

/// The masked form: enough to recognise, not enough to use.
fn mask(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 6 {
        return "•••".to_owned();
    }
    let head: String = chars[..2].iter().collect();
    let tail: String = chars[chars.len() - 2..].iter().collect();
    format!("{head}…{tail}")
}

/// A value that is obviously a stand-in.
fn looks_placeholder(value: &str) -> bool {
    let folded = value.to_ascii_lowercase();
    if folded.len() < 8 {
        return true;
    }
    if PLACEHOLDER_MARKERS
        .iter()
        .any(|marker| folded.contains(marker))
    {
        return true;
    }
    if folded
        .chars()
        .all(|ch| ch == folded.chars().next().unwrap_or(' '))
    {
        return true;
    }
    // A template of some kind: `${X}`, `{{ x }}`, `%(...)s`, `<token>`, `%TOKEN%`.
    folded.contains("${")
        || folded.contains("{{")
        || folded.contains("%(")
        || folded.starts_with('<')
}

/// The generic shape's own guard: a credential has entropy, an English word does not.
fn looks_credential_like(value: &str) -> bool {
    let has_digit = value.chars().any(|ch| ch.is_ascii_digit());
    let has_symbol = value.chars().any(|ch| !ch.is_ascii_alphanumeric());
    let has_upper = value.chars().any(|ch| ch.is_ascii_uppercase());
    let has_lower = value.chars().any(|ch| ch.is_ascii_lowercase());
    (has_digit && (has_symbol || has_upper)) || (has_upper && has_lower && has_symbol)
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
                "@@ -1,1 +1,2 @@",
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

    /// An AWS-shaped id that is not the documentation's own example. The example
    /// (`AKIAIOSFODNN7EXAMPLE`) carries the word "example" in it and the placeholder guard refuses it —
    /// correctly, since a committed AWS example id is a paste from a doc page and not a credential.
    const AWS: &str = "AKIA5QYKVJ6LR4D7AB3Z";

    fn findings_for(path: &str, line: &str) -> Vec<Finding> {
        detect(
            &episode(vec![file(path, &[line])]),
            &Corpus::default(),
            &Policy::default(),
        )
    }

    #[test]
    fn every_vendor_shape_is_high() {
        let cases = [
            &format!("let key = \"{AWS}\";"),
            "token := \"ghp_16C7e42F292c6912E7710c838347Ae178B4a\"",
            "s = 'xoxb-1234567890abcdef'",
            "key: AIzaSyD9t0G0xq0z1Yx1j6F0k2p3r4s5t6u7v8w",
            "-----BEGIN RSA PRIVATE KEY-----",
        ];
        for line in cases {
            let findings = findings_for("src/config.rs", line);
            assert_eq!(findings.len(), 1, "{line}: {findings:?}");
            assert_eq!(findings[0].severity, Severity::High, "{line}");
            assert!(findings[0].confidence >= 0.8, "{line}");
        }
    }

    #[test]
    fn a_finding_never_carries_the_secret_it_found() {
        let secret = AWS;
        let findings = findings_for("src/config.rs", &format!("let key = \"{secret}\";"));
        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        let text = format!(
            "{} {} {:?}",
            finding.message,
            finding.suppression_key,
            finding
                .evidence
                .iter()
                .map(|evidence| format!("{} {}", evidence.locator, evidence.detail))
                .collect::<Vec<_>>()
        );
        assert!(
            !text.contains(secret),
            "the finding leaked the value: {text}"
        );
        assert!(
            finding.message.contains("AK…3Z"),
            "but it identifies it: {}",
            finding.message
        );
    }

    #[test]
    fn a_generic_assignment_needs_entropy_to_count() {
        let real = findings_for("src/config.rs", "api_key = \"a9F8b7c6d5e4f3g2\"");
        assert_eq!(real.len(), 1);
        assert_eq!(
            real[0].severity,
            Severity::Medium,
            "the generic shape is a medium"
        );
        assert!(real[0].confidence <= 0.6);

        for placeholder in [
            "api_key = \"changeme123456\"",
            "api_key = \"your-api-key-here\"",
            "api_key = \"${API_KEY_FROM_ENV}\"",
            "api_key = \"aaaaaaaaaaaaaaaa\"",
            "api_key = \"thequickbrownfox\"",
            "api_key = \"example-value-12\"",
        ] {
            assert!(
                findings_for("src/config.rs", placeholder).is_empty(),
                "a stand-in is not a leak: {placeholder}"
            );
        }
    }

    #[test]
    fn prose_vendored_and_lockfiles_are_not_read() {
        for path in [
            "docs/secrets.md",
            "vendor/github.com/x/y/auth.go",
            "Cargo.lock",
            "src/generated/client.rs",
        ] {
            assert!(
                findings_for(path, &format!("token = \"{AWS}\"")).is_empty(),
                "{path} should not be scanned"
            );
        }
    }

    #[test]
    fn a_test_path_lowers_the_confidence_and_says_so_rather_than_going_quiet() {
        let findings = findings_for("tests/fixtures/api.rs", &format!("key = \"{AWS}\""));
        assert_eq!(
            findings.len(),
            1,
            "a hardcoded credential in a test is still one"
        );
        assert!(findings[0].confidence < 0.85);
        assert!(
            findings[0].message.contains("test path"),
            "{}",
            findings[0].message
        );
    }

    #[test]
    fn a_commented_key_is_not_claimed() {
        assert!(findings_for("src/config.rs", &format!("// key = \"{AWS}\" // doc")).is_empty());
    }

    #[test]
    fn the_same_shape_twice_in_one_file_is_one_finding() {
        let episode = episode(vec![file(
            "src/config.rs",
            &[
                &format!("let a = \"{AWS}\";"),
                &format!("let b = \"{AWS}\";"),
            ],
        )]);
        let findings = detect(&episode, &Corpus::default(), &Policy::default());
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn the_documentations_own_example_id_is_a_placeholder() {
        // AWS's own documentation example, pasted. The guard is a substring test on purpose: a value
        // that says "example" in it is a stand-in until a person says otherwise.
        assert!(findings_for("src/config.rs", "let key = \"AKIAIOSFODNN7EXAMPLE\";").is_empty());
    }

    #[test]
    fn masking_keeps_enough_to_recognise_and_not_enough_to_use() {
        assert_eq!(mask(AWS), "AK…3Z");
        assert_eq!(mask("short"), "•••");
    }

    #[test]
    fn ordinary_code_is_silent() {
        for line in [
            "let token = lexer.next();",
            "let password = read_password();",
            "assert_eq!(config.api_key.as_deref(), Some(\"\"));",
            "/// The `secret` field is never logged.",
        ] {
            assert!(findings_for("src/config.rs", line).is_empty(), "{line}");
        }
    }
}
