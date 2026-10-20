//! Where the reviewer's own configuration lives (ADR-001 §4.6, ADR-002 §4.5).
//!
//! One question, answered once: which files may a *person* write that change how Fathom behaves for
//! them? Three surfaces need that answer — `[theme] name` (`e5.6`), and `[ui] editor` / `[diff]
//! difftool` (`e5.8`) — and two lists of the same directories is two answers, which is how a
//! reviewer's config ends up honoured by one verb and ignored by another.
//!
//! Everything here is user-scoped by definition. A repository's own `fathom.toml` is a different layer
//! read elsewhere, and the keys in this file's tables are deliberately **not** repository keys: a clone
//! choosing your palette is rude, and a clone choosing a program your machine executes is worse than
//! rude (ADR-001 §4.6).

use std::path::PathBuf;

/// The user's Fathom configuration directory, if one can be named.
///
/// `$XDG_CONFIG_HOME/fathom` first, because honouring the XDG base directory is what lets a dotfiles
/// repository own the location without a symlink; `~/.config/fathom` after it, because that is where
/// the XDG default points and where most people's file already is.
#[must_use]
pub fn dir() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(home).join("fathom"));
    }
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(|home| PathBuf::from(home).join(".config").join("fathom"))
}

/// The user config files, first-existing-wins, in precedence order.
///
/// `~/.fathom/config.toml` is the older spelling and still read, because a file a person has had for
/// years does not stop being their configuration when a tool documents a tidier path.
#[must_use]
pub fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(dir) = dir() {
        out.push(dir.join("config.toml"));
    }
    if let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
        out.push(PathBuf::from(home).join(".fathom").join("config.toml"));
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn a_directory_is_always_named_and_the_config_is_inside_it() {
        // The values here come from the process's own environment, so the assertions are about shape:
        // a candidate list that is empty would mean a reviewer's `[ui] editor` is silently unreadable
        // on whatever machine runs this test.
        let candidates = candidates();
        assert!(
            !candidates.is_empty(),
            "there is always a place a user's config can live"
        );
        for path in candidates {
            assert_eq!(
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned()),
                Some("config.toml".to_owned()),
                "{path:?} is a file path, not a directory"
            );
        }
    }
}
