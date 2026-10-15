//! Redaction: making a report safe to attach to a bug report.
//!
//! `fathom doctor` is specified as "attachable to a bug report and contains no secrets or absolute home
//! paths" (`e1.10`), which is a stronger promise than "we do not print passwords", because everything a
//! diagnostic prints is a path, a URL, or a value read out of someone else's config. Three rules, all of
//! them mechanical rather than a blocklist that has to be complete:
//!
//! 1. **Paths are relative to the repository.** The store is `.fathom/index.sqlite3`, not
//!    `/home/taylor/work/acme/.fathom/index.sqlite3`. A path under the repository has no business
//!    repeating the directory it lives in, and the directory is the half that identifies the person.
//! 2. **A path under the home directory is spelled `~`.** Everything outside the repository but inside
//!    `$HOME` — the user config, an XDG cache, a work tree one level up — is still useful to a triager,
//!    and `~/.config/fathom/config.toml` carries the same information as the absolute form.
//! 3. **A URL's userinfo is dropped.** `https://octocat:ghp_secret@github.com/acme/api` becomes
//!    `https://***@github.com/acme/api`, because git allows credentials in a remote URL and users use
//!    that, and a remote URL is exactly the line a bug report wants.
//!
//! What is deliberately *not* here: printing config values at all. `doctor` reports a config file's path,
//! size and digest — enough to answer "did the file you think you edited get edited", not enough to
//! carry a token. A redactor whose rules depend on knowing what a secret looks like is a redactor that
//! leaks the first time a key is named `aws_stuff`.

use std::path::{Path, PathBuf};

/// The redaction context for one run: where the repository is, and where home is.
#[derive(Debug, Clone)]
pub struct Redactor {
    /// The repository root, canonicalised when that was possible.
    pub root: PathBuf,
    /// The home directory, if `$HOME` (or `%USERPROFILE%` on Windows) is set.
    home: Option<PathBuf>,
}

impl Redactor {
    /// Build the redactor for a run pointed at `root`.
    #[must_use]
    pub fn for_root(root: &Path) -> Self {
        Redactor {
            root: root.canonicalize().unwrap_or_else(|_| root.to_path_buf()),
            home: std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .filter(|text| !text.is_empty())
                .map(PathBuf::from),
        }
    }

    /// One path, in the spelling that is safe to print.
    ///
    /// A path that does not exist yet — the store a `fathom stats` has not created, a config layer that is
    /// absent — cannot be canonicalised, and printing the caller's literal `./x` would be a report about
    /// how the command line happened to be typed. So the nearest existing ancestor is resolved and the rest
    /// is appended, which makes a missing path comparable against the repository and the home directory
    /// exactly as an existing one is.
    #[must_use]
    pub fn path(&self, path: &Path) -> String {
        let candidate = resolved(path);
        if let Ok(relative) = candidate.strip_prefix(&self.root) {
            let text = relative.to_string_lossy();
            return if text.is_empty() {
                ".".to_owned()
            } else {
                text.into_owned()
            };
        }
        if let Some(home) = &self.home
            && let Ok(relative) = candidate.strip_prefix(home)
        {
            let text = relative.to_string_lossy();
            return if text.is_empty() {
                "~".to_owned()
            } else {
                format!("~/{text}")
            };
        }
        // Not the repository's and not the user's: a system path, a store opened by absolute path from
        // elsewhere on the machine. Printing it is what a triager needs, and it names no home.
        candidate.to_string_lossy().into_owned()
    }

    /// Free text with any embedded absolute path under `$HOME` rewritten to `~`.
    ///
    /// Errors from underneath (`gix` quoting a `.git` directory, rusqlite naming a file) arrive as
    /// sentences, and a sentence with a home path in it is the same leak as a field with one in it, so
    /// every string that reaches stdout or stderr from a report passes through here.
    #[must_use]
    pub fn text(&self, text: &str) -> String {
        let mut out = text.to_owned();
        if let Some(home) = &self.home {
            let absolute = home.to_string_lossy();
            if !absolute.is_empty() && absolute != "/" {
                out = replace_all(&out, &absolute, "~");
                // A canonicalised home and a home with a trailing slash are the same directory.
                if let Some(stripped) = absolute.strip_suffix('/') {
                    out = replace_all(&out, stripped, "~");
                }
            }
        }
        self.secret(&out)
    }

    /// Strip credentials from anything URL-shaped, and from `key = value` pairs whose key says secret.
    #[must_use]
    pub fn secret(&self, text: &str) -> String {
        // Per line, because a diagnostic is a line or a sentence and never a document: the separator rule
        // below only makes sense with one assignment per line.
        strip_userinfo(text)
            .split('\n')
            .map(redact_line)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// The key names that mean "the value after this is a credential".
///
/// This is a *key* list, not a value-guessing heuristic: a value is only ever redacted when the name it is
/// stored under says it is a credential. A report that printed the value and then guessed whether it looked
/// secret is a report that prints the first secret with a boring name.
const SECRET_KEYS: [&str; 10] = [
    "token",
    "secret",
    "password",
    "passwd",
    "apikey",
    "api_key",
    "access_key",
    "private_key",
    "bearer",
    "credential",
];

/// One line: if its key names a credential, print the key and not the value.
fn redact_line(line: &str) -> String {
    // The separator is the first `=` (a config line) or the first `:` that is not part of a scheme (a URL,
    // a `file:line`, a `key: value` diagnostic).
    let Some((key, value)) = split_assignment(line) else {
        return line.to_owned();
    };
    let last_segment = key
        .rsplit(['.', '_', '-', ' ', '"'])
        .find(|part| !part.is_empty())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !SECRET_KEYS.contains(&last_segment.as_str()) || value.trim().is_empty() {
        return line.to_owned();
    }
    format!("{}=<redacted>", key.trim_end())
}

/// Split a line into its key and the rest of it, if it is an assignment at all.
fn split_assignment(line: &str) -> Option<(String, String)> {
    if let Some(at) = line.find('=') {
        return Some((line[..at].to_owned(), line[at + 1..].to_owned()));
    }
    // A colon is only an assignment separator when the left half is not a URL scheme or a `file:line`.
    let at = line.find(':')?;
    let key = &line[..at];
    let value = line[at + 1..].to_owned();
    let key_is_a_name = key.chars().all(|c| {
        c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' || c == ' ' || c == '"'
    });
    let value_starts_a_url =
        value.trim_start().contains("://") || value.trim_start().starts_with("//");
    if !key_is_a_name || value_starts_a_url {
        return None;
    }
    Some((key.to_owned(), value))
}

/// Canonicalise what exists and re-attach what does not, so a missing path is still an absolute one.
fn resolved(path: &Path) -> PathBuf {
    if let Ok(absolute) = path.canonicalize() {
        return absolute;
    }
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    let mut cursor = path;
    while !cursor.exists() {
        if let Some(name) = cursor.file_name() {
            tail.push(name.to_os_string());
        }
        let Some(parent) = cursor.parent() else { break };
        cursor = parent;
    }
    let mut base = cursor
        .canonicalize()
        .unwrap_or_else(|_| cursor.to_path_buf());
    for name in tail.iter().rev() {
        base.push(name);
    }
    base
}

/// Replace every occurrence of `needle` in `hay` (a `str::replace` with the argument order the call site
/// reads naturally for).
fn replace_all(hay: &str, needle: &str, with: &str) -> String {
    hay.replace(needle, with)
}

/// `scheme://user:pass@host` → `scheme://***@host`, and `scheme://user@host` → `scheme://***@host`.
///
/// Written as a scan rather than a parse because the input is a diagnostic line, not a URL: there may be
/// several in one string, and a URL this function cannot understand is a URL whose userinfo this function
/// cannot find, so the fallback is to keep the line rather than to guess.
fn strip_userinfo(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("://") {
        let authority_start = start + 3;
        out.push_str(&rest[..authority_start]);
        let authority_end = rest[authority_start..]
            .find(['/', '?', '#'])
            .map(|offset| authority_start + offset)
            .unwrap_or(rest.len());
        let authority = &rest[authority_start..authority_end];
        if let Some(at) = authority.rfind('@') {
            out.push_str("***");
            out.push_str(&authority[at..]);
        } else {
            out.push_str(authority);
        }
        rest = &rest[authority_end..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use std::path::Path;

    #[test]
    fn a_path_under_the_repository_is_relative() {
        // Built the way the verbs build it — with a canonicalised root — because the whole point of `path`
        // is that a path that does not exist yet is compared as if it did.
        let dir = std::env::temp_dir().join("fathom-sanitize-fixture");
        let redactor = Redactor::for_root(&dir);
        assert_eq!(redactor.path(&dir), ".");
        assert_eq!(
            redactor.path(&dir.join(".fathom").join("index.sqlite3")),
            ".fathom/index.sqlite3",
            "a store that does not exist yet is still named from the repository root"
        );
        assert_eq!(
            redactor.path(Path::new("/nowhere/deeper")),
            "/nowhere/deeper"
        );
    }

    #[test]
    fn a_path_under_home_is_a_tilde() {
        let redactor = Redactor {
            root: PathBuf::from("/nowhere"),
            home: Some(PathBuf::from("/home/taylor")),
        };
        assert_eq!(redactor.path(Path::new("/home/taylor")), "~");
        assert_eq!(
            redactor.path(Path::new("/home/taylor/.config/fathom/config.toml")),
            "~/.config/fathom/config.toml"
        );
    }

    #[test]
    fn credentials_never_survive() {
        let redactor = Redactor {
            root: PathBuf::from("/nowhere"),
            home: None,
        };
        assert_eq!(
            redactor.secret("https://octocat:ghp_secret@github.com/acme/api.git"),
            "https://***@github.com/acme/api.git"
        );
        assert_eq!(
            redactor.secret("git@github.com:acme/api.git"),
            "git@github.com:acme/api.git",
            "an ssh-style remote has no userinfo to strip and must survive untouched"
        );
        assert_eq!(
            redactor.secret("provider.github.token = ghp_secret"),
            "provider.github.token=<redacted>"
        );
    }

    #[test]
    fn home_disappears_from_prose_too() {
        let redactor = Redactor {
            root: PathBuf::from("/nowhere"),
            home: Some(PathBuf::from("/home/taylor")),
        };
        assert_eq!(
            redactor.text("cannot read /home/taylor/work/acme/.git/config"),
            "cannot read ~/work/acme/.git/config"
        );
    }
}
