//! The paths and commits a printed verb shows, linked where the terminal asked for it (`e5.8`).
//!
//! [`fathom_core::links`] holds the rule and the probe; this module is the thin, repository-shaped
//! wrapper the printers call, because a link needs three things a bare string does not have: the
//! repository's absolute root (a `file://` URL of a relative path is a link that breaks when the
//! terminal's directory does), the repository's remote (a commit is only linkable to a forge that
//! serves it), and the answer about the stream (`--plain` into a pipe is not a terminal, and no amount
//! of advertising changes that).
//!
//! # The promise a printer gets for free
//!
//! [`Printer::path`] and [`Printer::commit`] return a [`String`] that is either the plain text or the
//! text wrapped in OSC-8, and the *visible* text is identical either way. That is deliberate: a caller
//! that pads a column, counts a width or greps a line keeps working with links on and off, so wiring a
//! link into a printer cannot change any other number on the line (which is also what makes it safe to
//! do at all).
//!
//! # What is deliberately not linked
//!
//! Anything the run could not resolve to an absolute path or a forge URL is printed plain, without a
//! note. A link to nowhere is worse than no link, and a message about every unlinkable path would be a
//! message about nothing.

use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};

use fathom_core::links::{Hyperlinks, commit_url, file_url};

/// What a printer may link to, and whether this stream will show it.
#[derive(Debug, Clone)]
pub struct Printer {
    /// The verdict about the stream, with the sentence that explains it.
    links: Hyperlinks,
    /// The repository's absolute root, joined onto every relative path before it becomes a URL.
    root: PathBuf,
    /// The remote to build commit URLs from, when one was readable and shaped like a forge.
    remote: Option<String>,
    /// What was advertised but could not be used — tmux, GNU screen, a `dumb` terminal. One sentence,
    /// printed once, by the verb that is about to print text the reviewer expected to be clickable.
    pub notice: Option<String>,
}

impl Printer {
    /// Read the terminal, the root and the remote; decide everything up front.
    ///
    /// A repository that cannot be opened is fine here — `fathom open --print` in a directory with no
    /// git in it still prints paths, they just do not resolve to commits.
    #[must_use]
    pub fn for_root(root: &Path) -> Self {
        let tty = std::io::stdout().is_terminal();
        // The environment describes the terminal; only this process knows whether its own stdout goes
        // to it, and `fathom show | fzf` sits inside a Ghostty that advertises. So the stream goes in
        // as a fact of the decision rather than as a patch over it (ADR-006 §6.7).
        let links = Hyperlinks::from_stream(tty, |key| std::env::var(key).ok());
        // A middleman that drops the sequences is the one case worth a sentence: the terminal *did*
        // advertise, so the reviewer's expectation is reasonable and only the route is not there.
        let notice = if tty && links.advertised() && !links.enabled() {
            Some(format!("no hyperlinks this run: {}", links.why()))
        } else {
            None
        };
        let remote = fathom_ingest::repo::Repository::discover(root)
            .ok()
            .and_then(|repo| repo.config_string("remote.origin.url"))
            .filter(|value| !value.trim().is_empty());
        Printer {
            links,
            root: root.canonicalize().unwrap_or_else(|_| root.to_path_buf()),
            remote,
            notice,
        }
    }

    /// Is anything going to be emitted? The verbs that print a decision line read this.
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.links.enabled()
    }

    /// Did the terminal advertise OSC-8, whatever we decided about this stream? `doctor` reports the
    /// two facts separately, because "it advertises and I still got nothing" is a different question
    /// from "it never advertised".
    #[must_use]
    pub fn advertised(&self) -> bool {
        self.links.advertised()
    }

    /// The sentence about the stream, for the caller that owes the reviewer one.
    #[must_use]
    pub fn why(&self) -> String {
        self.links.why().to_owned()
    }

    /// A repository-relative path, as it should be printed: linked to the absolute file when this
    /// stream takes links, exactly the input when it does not.
    #[must_use]
    pub fn path(&self, path: &str) -> String {
        self.path_line(path, None)
    }

    /// The same, pointed at a line — the `file://…#L42` form, which is the only line anchor a
    /// `file://` URL can carry and the one a browser-shaped handler understands.
    #[must_use]
    pub fn path_line(&self, path: &str, line: Option<u32>) -> String {
        let absolute = self.root.join(path);
        let text = absolute.to_string_lossy().into_owned();
        match file_url(&text, line) {
            Some(url) => self.links.wrap(&url, path),
            None => path.to_owned(),
        }
    }

    /// A commit oid, linked to the forge the remote names — or plain, when there is no remote, no
    /// forge, or no reason to guess.
    #[must_use]
    pub fn commit(&self, oid: &str) -> String {
        let display: String = oid.chars().take(7).collect();
        match &self.remote {
            Some(remote) => match commit_url(remote, oid) {
                Some(url) => self.links.wrap(&url, &display),
                None => display,
            },
            None => display,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn a_link_is_the_same_text_with_something_wrapped_around_it() {
        let root = std::env::temp_dir();
        let mut printer = Printer::for_root(&root);
        printer.links = Hyperlinks::always("test");
        let linked = printer.path("src/store.rs");
        assert_eq!(fathom_core::links::strip(&linked), "src/store.rs");
        assert!(linked.contains("file://"), "{linked}");
        assert!(linked.contains("src/store.rs"));

        printer.links = Hyperlinks::never("test");
        assert_eq!(printer.path("src/store.rs"), "src/store.rs");
    }

    #[test]
    fn a_line_number_rides_the_url_and_not_the_text() {
        let mut printer = Printer::for_root(&std::env::temp_dir());
        printer.links = Hyperlinks::always("test");
        let linked = printer.path_line("src/store.rs", Some(42));
        assert!(linked.contains("#L42"), "{linked}");
        assert_eq!(fathom_core::links::strip(&linked), "src/store.rs");
    }

    #[test]
    fn a_commit_without_a_remote_is_seven_characters_and_nothing_more() {
        let mut printer = Printer::for_root(Path::new("/nowhere"));
        printer.remote = None;
        assert_eq!(
            printer.commit("0123456789abcdef0123456789abcdef01234567"),
            "0123456"
        );
    }

    #[test]
    fn a_forge_is_needed_before_a_commit_is_linked() {
        let mut printer = Printer::for_root(&std::env::temp_dir());
        printer.links = Hyperlinks::always("test");
        printer.remote = Some("git@github.com:org/repo.git".to_owned());
        let linked = printer.commit("0123456789abcdef0123456789abcdef01234567");
        assert!(
            linked.contains("https://github.com/org/repo/commit/0123456789abcdef"),
            "{linked}"
        );
        assert_eq!(
            fathom_core::links::strip(&linked),
            "0123456",
            "the visible text stays the short oid a log expects"
        );
        printer.remote = Some("/srv/git/repo".to_owned());
        assert_eq!(
            fathom_core::links::strip(&printer.commit("0123456789abcdef0123456789abcdef01234567")),
            "0123456",
            "a local remote earns no link"
        );
    }
}
