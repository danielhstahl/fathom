//! `scope.rs` — the declared scope of an episode, and what it means for a change to leave it
//! (`e4.2`, `reco-c54`).
//!
//! [`fathom_ingest::intent`] stores the globs somebody *declared* — a hook payload's `scope`, or a
//! `Scope:` trailer — and this module answers the one question those globs exist to answer: **did the
//! change stay inside them?** It is the input to the scope-creep detector, which is `e4.3`'s
//! (`reco-2ox`): that detector turns a [`Deviation::Outside`] into a finding with a reason, a jump target,
//! a confidence and a suppression key, and `e4.4` folds the finding into a score. This module deliberately
//! stops at the answer. A deviation is not a finding — a finding is something a human can dismiss, and
//! dismissal is `e4.3`'s vocabulary, not this one's.
//!
//! # The empty scope is a state, not a zero
//!
//! [`Deviation::Unknown`] is returned when nothing was declared, and it is *not* "nothing was allowed": a
//! repository with no hooks and no trailers is `e4.2`'s normal case, and a detector that read an undeclared
//! scope as an empty allow-list would flag every commit in it. The three answers are therefore three, and
//! the middle one is the common one.
//!
//! Nothing here infers a scope from the diff. That is the same refusal [`fathom_ingest::intent`] makes about
//! the intent text itself: a scope derived from the files that arrived could never report the only case this
//! exists for — the file that arrived which nobody asked for.
//!
//! # One glob language
//!
//! The patterns are compiled with `globset`, the matcher the path taxonomy uses, so a scope glob and a
//! taxonomy glob mean the same thing to the person who writes `fathom.toml`. The semantics, each pinned by a
//! test below:
//!
//! * `*` matches inside one path segment — `docs/*.md` matches `docs/a.md` and not `docs/nested/a.md`, which
//!   is `literal_separator` in `globset`'s words and the `.gitignore` meaning in everybody else's;
//! * `**` crosses segments — `src/parser/**` matches `src/parser/grammar/lr.rs`;
//! * `?` matches one character and `[a-z]` one character of the class;
//! * a pattern with **no** wildcard names either a file or a directory: `crates/fathom-store` matches that
//!   file *and* everything under it, because "in that crate" and "that file" are the same keystroke and a
//!   silent wrong answer about which was meant is worth one extra rule;
//! * a trailing slash means the directory, always — `docs/` matches `docs/a.md` and never `docs.md`;
//! * patterns are matched against repo-relative, slash-separated paths, which is how the store spells them,
//!   so a Windows capture and a Linux one compare equal.
//!
//! A pattern that does not compile is dropped and the scope goes on without it; a scope whose every pattern
//! fails is [`Deviation::Unknown`], because a scope nothing can read expressed no opinion. It is never
//! treated as "everything is inside": the intent ladder already refused the shapes that fail here (absolute
//! paths, `..`, `\`, a NUL) and named them in `scope_rejected`, so what arrives here is either readable or
//! already a finding.

use std::collections::BTreeSet;

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

/// What the declared scope has to say about a change.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Deviation {
    /// Nothing was declared, so there is no opinion — and emphatically **not** "nothing was allowed". The
    /// normal state of a repository with no hooks and no `Scope:` trailer.
    #[default]
    Unknown,
    /// Every touched path is inside the declared scope.
    Inside,
    /// These touched paths are outside it, deduplicated, in the order the caller first saw them.
    Outside(Vec<String>),
}

impl Deviation {
    /// The published spelling, for a document or a finding's evidence line.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Deviation::Unknown => "unknown",
            Deviation::Inside => "inside",
            Deviation::Outside(_) => "outside",
        }
    }

    /// Whether the scope said nothing.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Deviation::Unknown)
    }

    /// The paths outside the scope; empty for the other two states.
    #[must_use]
    pub fn outside(&self) -> &[String] {
        match self {
            Deviation::Outside(paths) => paths,
            _ => &[],
        }
    }

    /// One clause saying the verdict, or `None` when there is no verdict to give.
    ///
    /// A sentence and not a number, because the reader of a scope line is deciding whether to be annoyed,
    /// and "3 of 5 touched paths are outside the declared scope: …" is the whole finding. `e4.3` prints this
    /// same clause as its finding's reason rather than writing a second one.
    #[must_use]
    pub fn summary(&self, touched: usize) -> Option<String> {
        match self {
            Deviation::Unknown => None,
            Deviation::Inside => Some(format!(
                "all {touched} touched path(s) are inside the declared scope"
            )),
            Deviation::Outside(paths) => Some(format!(
                "{} of {touched} touched path(s) are outside the declared scope: {}",
                paths.len(),
                paths.join(", ")
            )),
        }
    }
}

/// Measure one change against one declared scope.
///
/// `declared` is [`fathom_ingest::intent::Intent::scope`] — already normalised, sorted and deduplicated by
/// the ladder, which also stripped the globs it refused. `touched` is the episode's paths, as the store
/// spells them. The order of `touched` is kept in the outside list, because a reviewer follows a finding
/// best in the order the diff showed it.
///
/// An empty `declared` is [`Deviation::Unknown`] whatever was touched. An empty `touched` — a merge, an
/// empty commit — is [`Deviation::Inside`]: the scope was declared and nothing left it.
#[must_use]
pub fn deviation<S: AsRef<str>, P: AsRef<str>>(declared: &[S], touched: &[P]) -> Deviation {
    let globs = declared
        .iter()
        .map(AsRef::as_ref)
        .map(str::trim)
        .filter(|glob| !glob.is_empty())
        .collect::<Vec<_>>();
    if globs.is_empty() {
        return Deviation::Unknown;
    }
    let Some(set) = compile(&globs) else {
        return Deviation::Unknown;
    };
    let mut outside: Vec<String> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for path in touched {
        let path = path.as_ref();
        if set.is_match(path) {
            continue;
        }
        if seen.insert(path.to_owned()) {
            outside.push(path.to_owned());
        }
    }
    if outside.is_empty() {
        Deviation::Inside
    } else {
        Deviation::Outside(outside)
    }
}

/// Compile the declared globs into one matcher, widening the ones that name a directory.
///
/// `None` when no pattern at all could be compiled, which the caller reads as "no opinion" — never as "the
/// whole repository is inside".
fn compile(globs: &[&str]) -> Option<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    let mut usable = 0_usize;
    for glob in globs {
        for pattern in expansions(glob) {
            // `literal_separator` is the whole content of the `*` rule above: a star does not cross a
            // slash. `**` is unaffected by it, which is why `**` is what crosses.
            let built = GlobBuilder::new(&pattern)
                .literal_separator(true)
                .case_insensitive(false)
                .build();
            match built {
                Ok(glob) => {
                    builder.add(glob);
                    usable += 1;
                }
                Err(_) => continue,
            }
        }
    }
    if usable == 0 {
        return None;
    }
    builder.build().ok()
}

/// The patterns one declared glob stands for: itself, and the subtree under it when it names a directory.
fn expansions(glob: &str) -> Vec<String> {
    let trimmed = glob.trim_end_matches('/');
    let mut out = vec![trimmed.to_owned()];
    let directory = glob.ends_with('/') || !trimmed.contains(['*', '?', '[']);
    if directory && !trimmed.is_empty() {
        out.push(format!("{trimmed}/**"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Deviation, deviation};

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn nothing_declared_is_no_opinion_not_an_empty_allow_list() {
        // The state the ticket calls the normal one. A detector that read this as "nothing was allowed"
        // would flag every commit in a repository that has no hooks.
        assert_eq!(deviation::<String, String>(&[], &[]), Deviation::Unknown);
        assert_eq!(
            deviation(&strings(&["", "  "]), &strings(&["src/a.rs"])),
            Deviation::Unknown,
            "blank globs are not globs"
        );
        assert!(Deviation::Unknown.is_unknown());
        assert_eq!(Deviation::Unknown.summary(5), None);
        assert_eq!(Deviation::default(), Deviation::Unknown);
    }

    #[test]
    fn a_star_stays_inside_one_segment_and_a_double_star_crosses_them() {
        let scope = strings(&["docs/*.md", "src/parser/**"]);
        assert_eq!(
            deviation(&scope, &strings(&["docs/a.md", "src/parser/grammar/lr.rs"])),
            Deviation::Inside
        );
        assert_eq!(
            deviation(&scope, &strings(&["docs/nested/a.md"])),
            Deviation::Outside(strings(&["docs/nested/a.md"])),
            "`docs/*.md` is one level, which is what the typewriter meant"
        );
        assert_eq!(
            deviation(&scope, &strings(&["src/other.rs"])),
            Deviation::Outside(strings(&["src/other.rs"]))
        );
    }

    #[test]
    fn a_glob_with_no_wildcard_names_a_file_or_a_directory() {
        let scope = strings(&["crates/fathom-store", "README.md"]);
        assert_eq!(
            deviation(&scope, &strings(&["crates/fathom-store/src/episode.rs"])),
            Deviation::Inside,
            "\"in that crate\" is the same keystroke as \"that file\""
        );
        assert_eq!(
            deviation(&scope, &strings(&["README.md"])),
            Deviation::Inside,
            "a bare path names itself"
        );
        assert_eq!(
            deviation(&scope, &strings(&["crates/fathom-tui/src/app.rs"])),
            Deviation::Outside(strings(&["crates/fathom-tui/src/app.rs"]))
        );
    }

    #[test]
    fn a_trailing_slash_means_the_directory_always() {
        let scope = strings(&["docs/"]);
        assert_eq!(
            deviation(&scope, &strings(&["docs/a.md"])),
            Deviation::Inside
        );
        assert_eq!(
            deviation(&scope, &strings(&["docs.md"])),
            Deviation::Outside(strings(&["docs.md"])),
            "`docs/` must not match a file that merely shares the prefix"
        );
    }

    #[test]
    fn a_class_and_a_question_mark_match_one_character() {
        let scope = strings(&["src/?.rs", "tests/test_*.py"]);
        assert_eq!(
            deviation(&scope, &strings(&["src/a.rs"])),
            Deviation::Inside
        );
        assert_eq!(
            deviation(&scope, &strings(&["src/ab.rs"])),
            Deviation::Outside(strings(&["src/ab.rs"]))
        );
        assert_eq!(
            deviation(&scope, &strings(&["tests/test_intent.py"])),
            Deviation::Inside
        );
    }

    #[test]
    fn outside_paths_are_deduplicated_in_the_order_they_first_appear() {
        let scope = strings(&["src/**"]);
        let out = deviation(
            &scope,
            &strings(&["docs/b.md", "docs/a.md", "docs/b.md", "src/x.rs"]),
        );
        assert_eq!(
            out,
            Deviation::Outside(strings(&["docs/b.md", "docs/a.md"]))
        );
        assert_eq!(out.as_str(), "outside");
        assert_eq!(
            out.summary(4),
            Some(
                "2 of 4 touched path(s) are outside the declared scope: docs/b.md, docs/a.md"
                    .to_owned()
            )
        );
    }

    #[test]
    fn a_change_with_no_paths_left_the_scope() {
        // A merge or an empty commit: nothing arrived, so nothing escaped. `Inside`, not `Unknown` — the
        // scope *was* declared, and it was not violated.
        let none: Vec<String> = Vec::new();
        assert_eq!(deviation(&strings(&["src/**"]), &none), Deviation::Inside);
        assert_eq!(
            Deviation::Inside.summary(0),
            Some("all 0 touched path(s) are inside the declared scope".to_owned())
        );
        assert!(Deviation::Inside.outside().is_empty());
    }

    #[test]
    fn a_glob_that_cannot_compile_matches_nothing_rather_than_everything() {
        // The intent ladder refuses the shapes that fail here, so this is a malformed scope arriving from
        // somewhere else. One readable glob still says what it says; the unreadable one widens nothing.
        let scope = strings(&["src/[.rs", "docs/*.md"]);
        assert_eq!(
            deviation(&scope, &strings(&["docs/a.md"])),
            Deviation::Inside
        );
        assert_eq!(
            deviation(&scope, &strings(&["src/a.rs"])),
            Deviation::Outside(strings(&["src/a.rs"])),
            "an unreadable glob must not make `src/a.rs` inside"
        );
        // …and a scope whose every pattern is unreadable is no opinion at all.
        assert_eq!(
            deviation(&strings(&["[", "src/[.rs"]), &strings(&["src/a.rs"])),
            Deviation::Unknown
        );
    }

    #[test]
    fn several_globs_are_one_scope_not_a_vote() {
        let scope = strings(&["src/parser/**", "tests/**", "docs/*.md"]);
        assert_eq!(
            deviation(
                &scope,
                &strings(&["src/parser/lexer.rs", "tests/parser.rs", "docs/notes.md"])
            ),
            Deviation::Inside
        );
        assert_eq!(Deviation::Inside.as_str(), "inside");
        assert_eq!(Deviation::Unknown.as_str(), "unknown");
        assert!(Deviation::Unknown.outside().is_empty());
    }
}
