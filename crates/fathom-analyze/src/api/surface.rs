//! The surface itself: what a package lets the outside world name, as data.
//!
//! [`Surface`] is the shape that a revision's public API has, and it is built to be *diffed*: every
//! list is sorted by a key that depends only on the input, every item carries the location a report
//! must print, and every item can produce [`Item::record`], the one-line byte form whose digest
//! answers "did the surface move at all?" before anything is compared field by field.
//!
//! Two granularities, because the languages do not agree about what a public name is:
//!
//! * [`Access::Symbol`] — Rust, Python, TypeScript, Go, Java. An item is a declaration; the diff is
//!   per name.
//! * [`Access::File`] — Ruby. An item is a file, and that is the whole of what Ruby can say: with
//!   eight exports for sixty-seven declarations and no per-symbol access control
//!   (`docs/concepts/structure-extraction.md`, finding 3), a Ruby column of symbol-level breaking
//!   changes would be a lie. Ruby's package carries [`Package::surface_sentence`], which says so in
//!   the report instead of in a footnote.
//!
//! Two *identities*, for the same reason: in Rust, Python and TypeScript a name is reached through the
//! file it lives in, so moving a file moves the name ([`Identity::Path`]); in Go and Java a name is
//! reached through its package, so a file move is a reorganisation and not a breaking change
//! ([`Identity::Unit`]). The diff uses this to decide what a moved item costs.
//!
//! The cheap question is answered first, and cheaply: [`fingerprint`] hashes bytes only, and
//! [`quick`] compares those hashes for the paths that can carry a surface at all. That is the fast
//! path the ticket asks for — most episodes touch no public surface, and the answer to those episodes
//! costs no grammar, no parse and no diff.

use crate::api::policy::ApiPolicy;
use crate::api::signature::{Field, Signature};
use crate::structure::{Language, SymbolKind, Visibility};
use serde::Serialize;
use sha2::{Digest as _, Sha256};

/// The version of *this* surface shape.
///
/// A stored surface row means something different after each bump, for the same reason
/// [`crate::structure::SCHEMA_VERSION`] exists: the fields of an [`Item`] are the contract a later
/// revision is diffed against, and a row written before the bump cannot be asked a question only the
/// new fields can answer.
pub const SCHEMA_VERSION: u32 = 1;

/// What one package's surface is *made of*.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    /// A declaration: a function, a type, a constant. Five of the six languages.
    Symbol,
    /// A file. Ruby, and the only honest answer it has (spike `s1`, finding 3).
    File,
}

impl Access {
    /// The stable spelling, which is also the JSON spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Access::Symbol => "symbol",
            Access::File => "file",
        }
    }
}

/// How a name in this package is reached from outside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Identity {
    /// Through the declaring file (or a file that re-exports it): Rust, Python, TypeScript. A move
    /// moves the name.
    Path,
    /// Through the package: Go, Java. A move within the package changes nothing a caller compiled
    /// against.
    Unit,
    /// The file *is* the name. Ruby.
    File,
}

impl Identity {
    /// The stable spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Identity::Path => "path",
            Identity::Unit => "unit",
            Identity::File => "file",
        }
    }

    /// The identity a language commits to, and the reason for it, in one place.
    #[must_use]
    pub fn of_language(language: Language) -> Identity {
        match language {
            Language::Go | Language::Java => Identity::Unit,
            Language::Ruby => Identity::File,
            Language::Rust | Language::Python | Language::TypeScript | Language::Unknown => {
                Identity::Path
            }
        }
    }
}

/// How an item's existence was established. Recorded on the item because the two sources have
/// different blind spots, and a report that merges them without saying so overstates what it knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// The tree-sitter extractor saw the declaration. This is the normal case and the only one with a
    /// signature, fields and a line.
    TreeSitter,
    /// A rustdoc JSON (`cargo doc --output-format json`, or `cargo public-items`) named the item and
    /// the tree-sitter pass did not. Added, and added *only*: a compiled view can widen the item list,
    /// but a rustdoc JSON that failed to build says nothing about what it did not emit, so the
    /// compiled view never removes anything the extractor found.
    RustdocJson,
}

impl Origin {
    /// The stable spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::TreeSitter => "tree_sitter",
            Origin::RustdocJson => "rustdoc_json",
        }
    }
}

/// An item's identity, as the diff pairs two revisions.
///
/// It is `(package, path, name, kind)` deliberately: the *path* is part of it even for the languages
/// where the path is not part of how a caller names the thing, because [`Diff`](crate::api::diff::Diff)
/// pairs on it first and then asks the survivors whether a move happened — which is the only way a
/// move can be told apart from a removal and an unrelated addition.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct ItemId {
    /// The package the item belongs to, as [`crate::api::package`] attributes it.
    pub package: String,
    /// The declaring file, corpus-relative.
    pub path: String,
    /// The dotted name inside the file (`Shape.area`), or the file path for a Ruby item.
    pub name: String,
    /// The kind vocabulary is [`crate::structure`]'s; a change of kind is a change of item.
    pub kind: SymbolKind,
}

impl ItemId {
    /// The one-line form a report and a baseline both use.
    #[must_use]
    pub fn display(&self) -> String {
        format!(
            "{}::{}::{} ({})",
            self.package,
            self.path,
            self.name,
            self.kind.as_str()
        )
    }
}

/// One public item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Item {
    /// The identity the diff pairs on.
    pub id: ItemId,
    /// One-based line of the declaration. This is the `file:line` the ticket asks every change to
    /// carry, and it comes from the extractor's span, not from a guess.
    pub line: usize,
    /// Whether the surface is per symbol or per file.
    pub access: Access,
    /// How the name is reached. Carried per item so a change can explain itself without re-deriving
    /// the language.
    pub identity: Identity,
    /// Visibility as the language's own access rule put it, before the export rule.
    pub visibility: Visibility,
    /// Whether this item is on the public surface *now*. `false` items are carried so a change of
    /// visibility can be reported as a *change of visibility* rather than as a disappearance.
    pub on_surface: bool,
    /// The contract a caller compiles against.
    pub signature: Signature,
    /// The member lines of a type-like declaration (fields, interface members, enum variants).
    pub fields: Vec<Field>,
    /// The member list was capped by [`crate::api::signature::MAX_FIELDS_PER_ITEM`]: the item is
    /// generated-scale and its field list is a lower bound.
    pub fields_truncated: bool,
    /// Implemented and inherited names, sorted — losing one is breaking, gaining one is additive.
    pub supertypes: Vec<String>,
    /// The language's own spellings, copied from the extractor (`async`, `unsafe`, `abstract`,
    /// `classmethod`, `getter`). Kept because `static fn` becoming `fn` is a caller-visible fact.
    pub modifiers: Vec<String>,
    /// The author's deprecation note, if the declaration carries a deprecation marker.
    pub deprecated: Option<String>,
    /// Every path a consumer can name this item from — the declaring file plus every file that
    /// re-exports it. This is what makes a move-with-a-barrel `export` *not* breaking and a move that
    /// dropped the forwarder breaking, which is the difference between a re-export as a forwarding
    /// fact and a re-export as a mystery.
    pub reachable_from: Vec<String>,
    /// The declaring scope is one the extractor cannot enumerate, so this item may not be the only
    /// one — and a *vanished* one is never evidence of removal. See
    /// [`crate::structure::Structure::may_have_shrunk`].
    pub synthetic: bool,
    /// Why the scope cannot be enumerated (`macro_definition`, `decorated`, `wildcard_reexport`, …).
    pub synthetic_reasons: Vec<String>,
    /// Ruby only: the files loading this entry point pulls in. The whole of Ruby's surface claim.
    pub loaded: Vec<String>,
    /// How the item was established.
    pub origin: Origin,
}

impl Item {
    /// The identity, without re-cloning the fields.
    #[must_use]
    pub fn id(&self) -> &ItemId {
        &self.id
    }

    /// The `file:line` a reviewer clicks.
    #[must_use]
    pub fn location(&self) -> String {
        format!("{}:{}", self.id.path, self.line)
    }

    /// The one-line byte form the digests are built from, and the diff's "did this item change?" test.
    ///
    /// Everything a caller can compile against is in it — kind, visibility, membership, the signature
    /// record, the field records, the supertypes, the reachability set, the deprecation note — and
    /// nothing else is. A body change, a comment, a moved line inside the same file: none of them
    /// change this string.
    #[must_use]
    pub fn record(&self) -> String {
        let fields: Vec<String> = self.fields.iter().map(Field::record).collect();
        let mut out = format!(
            "{}|{}|{}|{}|{}|{}|{}|{}|{}",
            self.id.kind.as_str(),
            self.visibility.as_str(),
            if self.on_surface { "pub" } else { "internal" },
            self.identity.as_str(),
            self.signature.record(),
            fields.join(";"),
            self.supertypes.join(","),
            self.reachable_from.join(","),
            self.deprecated
                .as_deref()
                .map(|note| format!("deprecated:{note}"))
                .unwrap_or_default(),
        );
        if !self.modifiers.is_empty() {
            out.push_str("|mods:");
            out.push_str(&self.modifiers.join(","));
        }
        if !self.loaded.is_empty() {
            out.push_str("|loads:");
            out.push_str(&self.loaded.join(","));
        }
        if self.fields_truncated {
            out.push_str("|fields-truncated");
        }
        out
    }

    /// The short name a report leads with.
    #[must_use]
    pub fn display(&self) -> String {
        if self.access == Access::File {
            self.id.path.clone()
        } else {
            self.id.name.clone()
        }
    }
}

/// One file that could not contribute to the surface, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Unavailable {
    /// The path.
    pub path: String,
    /// The extractor's stated reason, in its own words
    /// ([`crate::structure::UnavailableReason::sentence`]).
    pub reason: String,
}

/// One package's surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Package {
    /// The package's name, as attributed by [`crate::api::package`]: a crate name, a Go import path,
    /// a `package.json` name, a dotted Python package, or a directory.
    pub name: String,
    /// The language that decided the rules.
    pub language: Language,
    /// Symbol-level or file-level.
    pub access: Access,
    /// How names are reached.
    pub identity: Identity,
    /// The version the manifest states, when a manifest was supplied. Reported, never used to derive a
    /// bump: this ticket recommends, it does not know what was released.
    pub version: Option<String>,
    /// How the package name was decided (`api.package pin`, `cargo metadata`, `Cargo.toml`, `go.mod`,
    /// `package.json`, `__init__.py chain`, `package clause`, `*.gemspec`, `directory`). Recorded
    /// because "the affected package" is a claim, and a reader is entitled to know what it rested on.
    pub decided_by: String,
    /// The items, sorted by [`ItemId`]. Includes not-currently-public candidates, so a visibility
    /// change is a change and not a disappearance (see [`Item::on_surface`]).
    pub items: Vec<Item>,
    /// Files of this package that could not be read, so the item list is a lower bound.
    pub unavailable: Vec<Unavailable>,
    /// The item ceiling was reached: the list is a lower bound.
    pub truncated: bool,
    /// The sentence that says what *this* language's surface can and cannot claim. Ruby's is the one
    /// the ticket demands; the others say what the extractor's normalisation rules hide (fields that
    /// are not symbols in the model but are here, private types whose members were not walked).
    pub surface_sentence: String,
    /// The digest of every item record, which is what a package-level fast path compares.
    pub digest: String,
}

impl Package {
    /// The public items only.
    pub fn public_items(&self) -> impl Iterator<Item = &Item> {
        self.items.iter().filter(|item| item.on_surface)
    }

    /// How many public items, counted without consuming the iterator.
    #[must_use]
    pub fn public_count(&self) -> usize {
        self.items.iter().filter(|item| item.on_surface).count()
    }

    /// Whether this package's item list can be trusted as complete.
    #[must_use]
    pub fn complete(&self) -> bool {
        self.unavailable.is_empty() && !self.truncated
    }

    /// The sentence that states what this package's surface is, in the report.
    #[must_use]
    pub fn caveat(&self) -> Option<String> {
        if !self.complete() {
            let mut out = format!(
                "{}: the surface is a lower bound — {} file(s) could not be read{}{}",
                self.name,
                self.unavailable.len(),
                if self.truncated {
                    ", and the item ceiling was reached"
                } else {
                    ""
                },
                if self.unavailable.is_empty() {
                    String::new()
                } else {
                    format!(
                        " ({})",
                        self.unavailable
                            .iter()
                            .take(3)
                            .map(|gap| format!("{}: {}", gap.path, gap.reason))
                            .collect::<Vec<_>>()
                            .join("; ")
                    )
                }
            );
            out.push_str("; no claim of \"no change\" is made for this package");
            return Some(out);
        }
        None
    }
}

/// One revision's public API surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Surface {
    /// The schema version of this shape.
    pub schema_version: u32,
    /// The policy that produced it, echoed for provenance (the same discipline
    /// [`crate::structure::CorpusStructure`] follows).
    pub policy: ApiPolicy,
    /// The packages, sorted by name.
    pub packages: Vec<Package>,
    /// Every input file counted, whether or not it contributed.
    pub files_seen: usize,
    /// Files that contributed items.
    pub files_parsed: usize,
    /// Files excluded by `[api] ignore`. Counted and named, because "no public API change" has to say
    /// how much of the repository it chose not to look at.
    pub files_ignored: usize,
    /// The paths excluded by `[api] ignore`, capped.
    pub ignored_paths: Vec<String>,
    /// The languages present in the input, whether or not this build can read them.
    pub languages: Vec<String>,
    /// The grammars this build actually has, as
    /// [`crate::structure::available_languages`] reports them.
    pub available: Vec<String>,
    /// What was *told* about the read: the mode, an optional input that was used, an optional input that
    /// was not. Informational — a surface with a caveat here is not a worse answer, only a more
    /// transparent one.
    pub caveats: Vec<String>,
    /// What could *not* be read: a file whose grammar is missing, a file that parsed with error nodes,
    /// paths the policy refused to track. `complete()` is about these, because they are the statements
    /// from which "no public API change" does not follow.
    pub gaps: Vec<String>,
    /// The digest of every package digest. The whole-revision fast path.
    pub digest: String,
}

impl Surface {
    /// Build the digests and sort everything. Called by [`crate::api::extract`] once the items are
    /// known, and by nothing else, so an out-of-order or undigested surface cannot be constructed by
    /// accident.
    pub(crate) fn seal(mut self) -> Surface {
        for package in &mut self.packages {
            package.items.sort_by(|a, b| a.id.cmp(&b.id));
            package.digest = digest_of(
                &package
                    .items
                    .iter()
                    .filter(|item| item.on_surface)
                    .map(Item::record)
                    .collect::<Vec<_>>(),
            );
            package.unavailable.sort_by(|a, b| a.path.cmp(&b.path));
        }
        self.packages.sort_by(|a, b| a.name.cmp(&b.name));
        self.digest = digest_of(
            &self
                .packages
                .iter()
                .map(|package| format!("{}={}", package.name, package.digest))
                .collect::<Vec<_>>(),
        );
        self.caveats.sort();
        self.caveats.dedup();
        self.languages.sort();
        self.languages.dedup();
        self
    }

    /// Every public item across every package.
    pub fn items(&self) -> impl Iterator<Item = &Item> {
        self.packages.iter().flat_map(Package::public_items)
    }

    /// The item with this identity, public or not.
    #[must_use]
    pub fn item(&self, id: &ItemId) -> Option<&Item> {
        self.packages
            .iter()
            .find(|package| package.name == id.package)?
            .items
            .iter()
            .find(|item| item.id == *id)
    }

    /// The package with this name.
    #[must_use]
    pub fn package(&self, name: &str) -> Option<&Package> {
        self.packages.iter().find(|package| package.name == name)
    }

    /// Public items, counted across every package.
    #[must_use]
    pub fn public_count(&self) -> usize {
        self.packages.iter().map(Package::public_count).sum()
    }

    /// Whether the surface is a complete answer, which is what licenses a report to say *no change*.
    /// An empty surface from a build with no grammars is not a small surface, and a diff that reads it
    /// as one would report every API as removed — or, worse, report nothing when the *other* side was
    /// read.
    #[must_use]
    pub fn complete(&self) -> bool {
        self.gaps.is_empty() && self.packages.iter().all(Package::complete)
    }

    /// The sentence that says why the surface is not a complete answer, when it is not.
    ///
    /// Built from [`Surface::gaps`] and not from [`Surface::caveats`]: "rustdoc JSON was used" is a
    /// caveat, "three files could not be parsed" is a hole, and conflating them would make every honest
    /// surface read as an unreliable one.
    #[must_use]
    pub fn gap_sentence(&self) -> Option<String> {
        if self.complete() {
            return None;
        }
        let mut out = String::from("the surface is a lower bound: ");
        if !self.gaps.is_empty() {
            out.push_str(&self.gaps.join("; "));
        }
        let gaps: Vec<String> = self
            .packages
            .iter()
            .filter(|package| !package.complete())
            .map(|package| format!("{} ({} unread)", package.name, package.unavailable.len()))
            .collect();
        if !gaps.is_empty() {
            out.push_str("; packages: ");
            out.push_str(&gaps.join("; "));
        }
        Some(out)
    }
}

/// The digest of a list of records: one hash, order-dependent, over a length-prefixed framing so two
/// different lists cannot produce the same bytes.
#[must_use]
pub fn digest_of(records: &[String]) -> String {
    let mut hasher = Sha256::new();
    for record in records {
        hasher.update(record.len().to_le_bytes());
        hasher.update(record.as_bytes());
    }
    let bytes = hasher.finalize();
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from_digit((byte >> 4).into(), 16).unwrap_or('0'));
        out.push(char::from_digit((byte & 0x0f).into(), 16).unwrap_or('0'));
    }
    out
}

// ==============================================================================================
// the byte fast path
// ==============================================================================================

/// One file, hashed and classified, with nothing parsed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fingerprint {
    /// The corpus-relative path.
    pub path: String,
    /// The digest of the file's bytes.
    pub digest: String,
    /// Whether this path could carry a public symbol at all: one of the six languages and not excluded
    /// by `[api] ignore`.
    pub bears_surface: bool,
    /// The detected language, for the report.
    pub language: String,
}

/// Hash every input's bytes and say which paths could carry a surface. This is the whole cost of the
/// fast path, and it is one pass over the bytes the caller already read.
#[must_use]
pub fn fingerprint(
    sources: &[crate::structure::SourceFile<'_>],
    policy: &ApiPolicy,
) -> Vec<Fingerprint> {
    let mut out: Vec<Fingerprint> = sources
        .iter()
        .map(|source| {
            let language = Language::detect(source.path);
            let bears = language.is_known() && !policy.ignores(source.path);
            Fingerprint {
                path: source.path.to_string(),
                digest: digest_of(&[source.path.to_string(), hex(source.text)]),
                bears_surface: bears,
                language: language.as_str().to_string(),
            }
        })
        .collect();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

fn hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let bytes = hasher.finalize();
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from_digit((byte >> 4).into(), 16).unwrap_or('0'));
        out.push(char::from_digit((byte & 0x0f).into(), 16).unwrap_or('0'));
    }
    out
}

/// What the byte fast path answered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "answer")]
pub enum Quick {
    /// Nothing that could carry a public name changed. **This is the answer most episodes get**, and it
    /// cost one hash per file and no parse: no grammar was opened, no header was read, no item was
    /// built. The counts are here so a report can say what it did not do.
    NoPublicApiChange {
        /// Files compared.
        files: usize,
        /// How many of them could have carried a surface.
        surface_bearing: usize,
        /// Files excluded by `[api] ignore` — the honest asterisk on "no change".
        ignored: usize,
    },
    /// These paths changed and could carry a surface, so the surface has to be extracted and diffed.
    NeedsSurfaceDiff {
        /// The changed, added and removed surface-bearing paths, sorted.
        changed: Vec<String>,
        /// Paths that changed but could not carry a surface, counted rather than listed: a changed
        /// `README.md` is not an API question, and a report that listed it would train the reader to
        /// ignore the report.
        other_changed: usize,
    },
}

impl Quick {
    /// The one-line sentence a surface prints.
    #[must_use]
    pub fn sentence(&self) -> String {
        match self {
            Quick::NoPublicApiChange {
                files,
                surface_bearing,
                ignored,
            } => {
                let mut out = format!(
                    "no public API change: {files} file(s) hashed, {surface_bearing} of them \
                     able to carry a surface, none of them changed — no grammar was opened and \
                     nothing was parsed"
                );
                if *ignored > 0 {
                    out.push_str(&format!(
                        "; {ignored} path(s) were excluded by `[api] ignore`"
                    ));
                }
                out
            }
            Quick::NeedsSurfaceDiff {
                changed,
                other_changed,
            } => {
                let mut out = format!(
                    "{} surface-bearing path(s) changed, so the surface has to be read: {}",
                    changed.len(),
                    if changed.is_empty() {
                        "(none listed)".to_string()
                    } else {
                        changed
                            .iter()
                            .take(8)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    }
                );
                if *other_changed > 0 {
                    out.push_str(&format!(
                        "; {other_changed} other file(s) changed and cannot carry a surface"
                    ));
                }
                out
            }
        }
    }
}

/// The fast path itself: two fingerprints, one answer, no parse.
///
/// The comparison is only over surface-bearing paths, and *any* difference in that set — a changed
/// digest, an added path, a removed path — is an answer of `NeedsSurfaceDiff`. A rename is a removal
/// plus an addition, which is exactly how it should be reported: a moved file is a moved name in the
/// languages whose identity is the path.
#[must_use]
pub fn quick(before: &[Fingerprint], after: &[Fingerprint]) -> Quick {
    let surface_before: std::collections::BTreeMap<&str, &str> = before
        .iter()
        .filter(|entry| entry.bears_surface)
        .map(|entry| (entry.path.as_str(), entry.digest.as_str()))
        .collect();
    let surface_after: std::collections::BTreeMap<&str, &str> = after
        .iter()
        .filter(|entry| entry.bears_surface)
        .map(|entry| (entry.path.as_str(), entry.digest.as_str()))
        .collect();
    let mut changed: Vec<String> = Vec::new();
    for (path, digest) in &surface_after {
        match surface_before.get(path) {
            None => changed.push((*path).to_string()),
            Some(previous) if previous != digest => changed.push((*path).to_string()),
            Some(_) => {}
        }
    }
    for path in surface_before.keys() {
        if !surface_after.contains_key(path) {
            changed.push((*path).to_string());
        }
    }
    changed.sort();
    changed.dedup();
    // Paths that are one of the six languages but were excluded by `[api] ignore`: the asterisk that
    // has to be printed with "no change", because that half of the repository was not examined.
    let ignored = before
        .iter()
        .chain(after.iter())
        .filter(|entry| !entry.bears_surface && Language::detect(&entry.path).is_known())
        .map(|entry| entry.path.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let all_before: std::collections::BTreeMap<&str, &str> = before
        .iter()
        .map(|entry| (entry.path.as_str(), entry.digest.as_str()))
        .collect();
    let all_after: std::collections::BTreeMap<&str, &str> = after
        .iter()
        .map(|entry| (entry.path.as_str(), entry.digest.as_str()))
        .collect();
    let mut other_changed = 0usize;
    for entry in after.iter().filter(|entry| !entry.bears_surface) {
        if all_before
            .get(entry.path.as_str())
            .is_none_or(|previous| *previous != entry.digest)
        {
            other_changed += 1;
        }
    }
    for entry in before.iter().filter(|entry| !entry.bears_surface) {
        if !all_after.contains_key(entry.path.as_str()) {
            other_changed += 1;
        }
    }
    if changed.is_empty() {
        Quick::NoPublicApiChange {
            files: before.len().max(after.len()),
            surface_bearing: surface_after.len(),
            ignored,
        }
    } else {
        Quick::NeedsSurfaceDiff {
            changed,
            other_changed,
        }
    }
}
