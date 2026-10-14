//! `revspec.rs` — resolving the revision expressions a review tool needs, and only those.
//!
//! `gix` has a full `git rev-parse` implementation behind the `revparse-regex` feature, and `regex` is a
//! dependency this crate does not carry (ADR-002 §6.1: a feature costs its dependency, and the
//! local-only gix allow-list in §4.2 is kept short on purpose). What fathom needs is a documented
//! subset, so the subset is implemented here in a hundred lines with a grammar stated in full:
//!
//! ```text
//! spec        := base step*
//! base        := "HEAD" | "@" | 40-hex | hex-prefix(4..=39) | ref-name
//! ref-name    := a local branch ("main"), a tag ("v1.2"), a remote branch ("origin/main"),
//!                or a full name ("refs/heads/main"); resolved in git's own order of preference
//! step        := "~" number? | "^" number? | "^{" "}" | "^{" kind "}"
//! number      := 0 | 1..  (default 1)
//! kind        := "commit" | "tree" | "blob" | "tag"
//! ```
//!
//! `~n` walks the first parent `n` times, `^n` takes the nth parent once, and `^{kind}` peels a tag to
//! that kind (`^{}` peels all the way). `HEAD@{2}`, `:/text`, date expressions, `@{-1}` and reflog
//! syntax are **not** supported: each needs reflog parsing or a history search, none is what a review
//! tool asks for, and an unsupported expression is a typed
//! [`crate::error::Error::RevSpecRejected`] that names the grammar rather than a
//! wrong answer.

use fathom_core::time::Timestamp;

use crate::error::Error;
use crate::oid::Oid;
use crate::repo::Repository;

/// One step of the suffix grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// `~n`: first parent, n times.
    FirstParentAncestor(usize),
    /// `^n`: the nth parent, once.
    NthParent(usize),
    /// `^{kind}`: peel a tag; `PeelTo(None)` is `^{}`.
    Peel(PeelTo),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PeelTo {
    Commit,
    Tree,
    Blob,
    Tag,
    Any,
}

/// Resolve `spec` to a commit in `repo`.
///
/// # Errors
///
/// [`Error::RevSpecRejected`] when the expression is outside the grammar, names nothing, is ambiguous
/// (an abbreviated id matching several objects), or ends at something that is not a commit.
pub fn resolve(repo: &Repository, spec: &str) -> Result<Oid, Error> {
    let rejected = |why: String| Error::RevSpecRejected {
        spec: spec.to_owned(),
        why,
    };
    let text = spec.trim();
    if text.is_empty() {
        return Err(rejected("it is empty".into()));
    }
    let (base, steps) = split_suffixes(text).ok_or_else(|| {
        rejected(
            "the `~`/`^` suffixes are malformed; the grammar is `main~3`, `HEAD^2`, `v1^{}`".into(),
        )
    })?;

    let mut current = resolve_base(repo, base)?;
    let explicit_peel = steps.iter().any(|step| matches!(step, Step::Peel(_)));
    for step in steps.iter().rev() {
        current = match step {
            Step::FirstParentAncestor(count) => {
                // An ancestry step on a tag means the tag's commit first — `v1.0~1` is the commit's
                // grandparent in git, and answering "the tag has no parents" would be technically true
                // and useless.
                let mut oid = peel_if_tag(repo, current);
                for _ in 0..*count {
                    oid = repo
                        .commit(oid)?
                        .and_then(|header| header.parents.first().copied())
                        .ok_or_else(|| {
                            rejected(format!("{oid} has no first parent (it is a root commit)"))
                        })?;
                }
                oid
            }
            Step::NthParent(index) => {
                let current = peel_if_tag(repo, current);
                let header = repo
                    .commit(current)?
                    .ok_or_else(|| rejected(format!("{current} is not a readable commit")))?;
                *header.parents.get(*index - 1).ok_or_else(|| {
                    rejected(format!(
                        "{current} has {} parent(s), no parent #{index}",
                        header.parents.len()
                    ))
                })?
            }
            Step::Peel(target) => peel(repo, current, *target)?,
        };
    }
    // A bare name, id or ref resolves to a *commit*: a tag is peeled to the commit it names, because
    // that is what `git checkout v1.0` and `git log v1.0..` mean. An explicit `^{tree}`/`^{tag}` says
    // otherwise, and is honoured rather than second-guessed.
    let peeled = if explicit_peel {
        Ok(current)
    } else {
        peel(repo, current, PeelTo::Commit)
    };
    match peeled {
        Ok(oid) => Ok(oid),
        Err(_) => {
            // Two very different reasons, so two different errors. An object that is *not there* means the
            // repository is degraded — shallow, filtered, or damaged — and gets
            // [`Error::ObjectUnavailable`] with the reason and the "fathom never fetches" remedy, because
            // that is a fact about the repository and not about what the caller typed. An object that is
            // there but is a blob or a tree is the expression being wrong, which is a rejected revision.
            match repo.object_header(current)? {
                None => Err(Error::ObjectUnavailable {
                    oid: current,
                    why: repo.why_unavailable(current),
                }),
                Some(header) => Err(rejected(format!(
                    "the expression resolves to a {}, not to a commit",
                    header.kind.as_str()
                ))),
            }
        }
    }
}

/// Split the `~n`/`^n`/`^{…}` suffixes off the end, returning the base and the steps **in reverse order**
/// (the order they were found in, innermost last).
fn split_suffixes(text: &str) -> Option<(&str, Vec<Step>)> {
    let mut steps = Vec::new();
    let mut end = text.len();
    let bytes = text.as_bytes();
    loop {
        if end == 0 {
            break;
        }
        let rest = &text[..end];
        if let Some(open) = rest.strip_suffix('}') {
            // Must be `…^{kind}`: the last `{` is the one this `}` closes, which is what lets
            // `v1.0^{}^{tree}` split into two steps instead of one confused one.
            let (head, kind) = open.rsplit_once('{')?;
            let caret = head.strip_suffix('^')?;
            let target = match kind.trim() {
                "" => PeelTo::Any,
                "commit" => PeelTo::Commit,
                "tree" => PeelTo::Tree,
                "blob" => PeelTo::Blob,
                "tag" => PeelTo::Tag,
                _ => return None,
            };
            steps.push(Step::Peel(target));
            end = caret.len();
            continue;
        }
        let Some(tail_index) = rest.rfind(['~', '^']) else {
            break;
        };
        let marker = bytes[tail_index];
        let digits = &rest[tail_index + 1..];
        if digits.is_empty() {
            steps.push(if marker == b'~' {
                Step::FirstParentAncestor(1)
            } else {
                Step::NthParent(1)
            });
            end = tail_index;
            continue;
        }
        if digits.chars().all(|ch| ch.is_ascii_digit()) {
            let count: usize = digits.parse().unwrap_or(0);
            if count == 0 && !digits.chars().all(|ch| ch == '0') {
                return None;
            }
            steps.push(if marker == b'~' {
                Step::FirstParentAncestor(count)
            } else {
                Step::NthParent(count.max(1))
            });
            end = tail_index;
            continue;
        }
        // The `~`/`^` belongs to the name (a branch called `fix^1`? unusual but legal), so stop here.
        break;
    }
    if end == 0 {
        return None;
    }
    Some((&text[..end], steps))
}

fn resolve_base(repo: &Repository, base: &str) -> Result<Oid, Error> {
    let rejected = |why: String| Error::RevSpecRejected {
        spec: base.to_owned(),
        why,
    };
    if base.eq_ignore_ascii_case("head") || base == "@" {
        return repo
            .head()?
            .oid()
            .ok_or_else(|| rejected("HEAD points at a branch with no commits yet".to_owned()));
    }
    if base.eq_ignore_ascii_case("index") || base.contains("@{") {
        return Err(rejected(
            "reflog and `@{…}` expressions are outside the supported grammar".into(),
        ));
    }
    let all_hex = !base.is_empty() && base.bytes().all(|byte| byte.is_ascii_hexdigit());
    if all_hex && base.len() == Oid::LEN * 2 {
        let oid = base
            .parse::<Oid>()
            .map_err(|err| rejected(format!("not an object id: {err}")))?;
        if !repo.has_object(oid) {
            return Err(rejected("no such object in this repository".into()));
        }
        return Ok(oid);
    }
    if all_hex && base.len() >= 4 {
        return resolve_prefix(repo, base).ok_or_else(|| {
            rejected(if resolve_prefix(repo, base).is_none() {
                "no object matches this prefix, or more than one does".to_owned()
            } else {
                "no object matches this prefix".to_owned()
            })
        });
    }
    // Ref names, in git's `resolve_ref_unsafe` order of preference.
    let candidates = [
        format!("refs/{base}"),
        base.to_owned(),
        format!("refs/tags/{base}"),
        format!("refs/heads/{base}"),
        format!("refs/remotes/{base}"),
        format!("refs/remotes/{base}/HEAD"),
    ];
    let refs = repo.refs(crate::repo::RefFilter::All)?;
    for candidate in &candidates {
        if let Some(found) = refs.iter().find(|reference| &reference.name == candidate) {
            return resolve_ref_target(repo, found).ok_or_else(|| {
                rejected(format!("{candidate} does not point at a readable commit"))
            });
        }
    }
    Err(rejected(
        "no branch, tag, remote branch or full ref name matches it".into(),
    ))
}

fn resolve_ref_target(repo: &Repository, reference: &crate::repo::Ref) -> Option<Oid> {
    use crate::repo::RefTarget;
    match &reference.target {
        RefTarget::Direct { oid } => Some(*oid),
        RefTarget::Symbolic { to } => {
            // Follow one hop (`HEAD` -> `refs/heads/main`); git allows a chain, and a chain in a real
            // repository is a bug, so one hop plus a clear failure is the better trade.
            let name = to.as_str();
            let refs = repo.refs(crate::repo::RefFilter::All).ok()?;
            refs.iter()
                .find(|candidate| candidate.name == name)
                .and_then(|found| resolve_ref_target(repo, found))
        }
    }
}

/// Resolve an abbreviated id. Ambiguity is `None`: git's own rule is that an ambiguous prefix is an
/// error, and inventing a tie-break would make `fathom` disagree with `git` about which commit a
/// paste-on-the-terminal id means.
fn resolve_prefix(repo: &Repository, prefix: &str) -> Option<Oid> {
    if repo.object_hash_kind() != gix::hash::Kind::Sha1 {
        return None;
    }
    let parsed = gix::hash::Prefix::try_from(prefix).ok()?;
    let outcome = repo.gix().objects.lookup_prefix(parsed, None).ok()??;
    outcome.ok().map(|oid| crate::repo::from_gix(oid.as_ref()))
}

/// Peel a tag to its commit, and leave anything else alone: an ancestry step implies the peel, an
/// explicit `^{…}` does not.
fn peel_if_tag(repo: &Repository, oid: Oid) -> Oid {
    peel(repo, oid, PeelTo::Commit).unwrap_or(oid)
}

fn peel(repo: &Repository, oid: Oid, target: PeelTo) -> Result<Oid, Error> {
    let header = repo
        .object_header(oid)?
        .ok_or_else(|| Error::ObjectUnavailable {
            oid,
            why: repo.why_unavailable(oid),
        })?;
    match (header.kind, target) {
        (crate::repo::ObjectKind::Tag, PeelTo::Tag) => Ok(oid),
        (crate::repo::ObjectKind::Tag, _) => {
            // Follow the tag's target. `gix` peels with a cached `peeled` field on refs, but a tag
            // reached through a chain of commits has no such cache, so read the tag object.
            let mut current = oid;
            for _ in 0..16 {
                let next = tag_target(repo, current)?;
                let kind = repo.object_header(next)?.map(|header| header.kind);
                match (kind, target) {
                    (Some(crate::repo::ObjectKind::Tag), PeelTo::Tag) => return Ok(next),
                    (Some(crate::repo::ObjectKind::Tag), _) => current = next,
                    (Some(kind), PeelTo::Any) if kind != crate::repo::ObjectKind::Tag => {
                        return Ok(next);
                    }
                    (Some(crate::repo::ObjectKind::Commit), PeelTo::Commit) => {
                        return Ok(next);
                    }
                    (Some(crate::repo::ObjectKind::Tree), PeelTo::Tree) => {
                        return Ok(next);
                    }
                    (Some(crate::repo::ObjectKind::Blob), PeelTo::Blob) => {
                        return Ok(next);
                    }
                    _ => {
                        return Err(Error::RevSpecRejected {
                            spec: format!("{oid}^{{}}"),
                            why: "the tag does not peel to the requested kind".into(),
                        });
                    }
                }
            }
            Err(Error::RevSpecRejected {
                spec: format!("{oid}^{{}}"),
                why: "a chain of more than 16 tags is not a tag, it is a mistake".into(),
            })
        }
        (_, PeelTo::Any | PeelTo::Commit) if header.kind == crate::repo::ObjectKind::Commit => {
            Ok(oid)
        }
        // A commit's tree is what `HEAD^{tree}` means, and it is the only way to ask for a tree through
        // this grammar without walking to the root commit by name.
        (crate::repo::ObjectKind::Commit, PeelTo::Tree) => repo
            .commit(oid)?
            .map(|header| header.tree)
            .ok_or_else(|| Error::RevSpecRejected {
                spec: format!("{oid}^{{tree}}"),
                why: "the commit could not be read".into(),
            }),
        (_, PeelTo::Tree) if header.kind == crate::repo::ObjectKind::Tree => Ok(oid),
        (_, PeelTo::Blob) if header.kind == crate::repo::ObjectKind::Blob => Ok(oid),
        _ => Err(Error::RevSpecRejected {
            spec: oid.hex(),
            why: format!("{} is not a tag", header.kind.as_str()),
        }),
    }
}

fn tag_target(repo: &Repository, oid: Oid) -> Result<Oid, Error> {
    let id = crate::repo::to_gix(oid, repo.object_hash_kind());
    let object = repo
        .gix()
        .try_find_object(id)
        .map_err(|err| Error::RepositoryUnreadable {
            root: repo.identity().display_name.clone(),
            reason: format!("reading a tag: {err}"),
        })?
        .ok_or_else(|| Error::ObjectUnavailable {
            oid,
            why: repo.why_unavailable(oid),
        })?;
    let tag = object
        .try_into_tag()
        .map_err(|err| Error::RepositoryUnreadable {
            root: repo.identity().display_name.clone(),
            reason: format!("decoding a tag: {err}"),
        })?;
    let target = tag.target_id().map_err(|err| Error::RepositoryUnreadable {
        root: repo.identity().display_name.clone(),
        reason: format!("reading a tag target: {err}"),
    })?;
    Ok(crate::repo::from_gix(target.as_ref()))
}

/// The largest timestamp a walk could see, kept for the date-window checks in `e1.2`.
///
/// This lives here rather than in `e1.2` because `--since`/`--until` need the same conversion from a
/// human date to the `Timestamp` the store keeps, and one conversion is one bug fewer.
#[must_use]
pub fn timestamp_from_unix_seconds(seconds: i64) -> Timestamp {
    Timestamp::from_unix_micros(seconds.saturating_mul(1_000_000))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::Step;
    use super::split_suffixes;

    #[test]
    fn splits_the_documented_suffixes() {
        assert_eq!(split_suffixes("main"), Some(("main", vec![])));
        assert_eq!(
            split_suffixes("HEAD~3"),
            Some(("HEAD", vec![Step::FirstParentAncestor(3)]))
        );
        assert_eq!(
            split_suffixes("HEAD~"),
            Some(("HEAD", vec![Step::FirstParentAncestor(1)]))
        );
        assert_eq!(
            split_suffixes("HEAD^2"),
            Some(("HEAD", vec![Step::NthParent(2)]))
        );
        assert_eq!(
            split_suffixes("HEAD^^"),
            Some(("HEAD", vec![Step::NthParent(1), Step::NthParent(1)]))
        );
        assert_eq!(
            split_suffixes("release~2^"),
            Some((
                "release",
                vec![Step::NthParent(1), Step::FirstParentAncestor(2)]
            ))
        );
    }

    #[test]
    fn leaves_a_marker_inside_a_name_alone() {
        // A branch called `fix^1` is unusual and legal; only a trailing `~`/`^` is a step.
        assert_eq!(split_suffixes("fix^1x"), Some(("fix^1x", vec![])));
        assert_eq!(split_suffixes("a/b~c"), Some(("a/b~c", vec![])));
    }
}
