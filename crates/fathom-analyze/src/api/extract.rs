//! Building a [`Surface`] from a corpus: the per-language export rules, the package attribution, and
//! the two things the extractor's own vocabulary makes mandatory.
//!
//! The input is a corpus of source text plus the manifest texts the caller chose to hand over
//! ([`Inputs`]), and the work is a pure function of them — no filesystem, no clock, no shell, exactly
//! as [`crate::structure::extract`] insists, because a surface that is not a pure function cannot be
//! a cache key and cannot be reproduced in a test.
//!
//! Three things here are obligations rather than choices:
//!
//! 1. **The export rule is the language's, never a convenient one.** Rust's is `pub` *plus* a public
//!    path from the crate root (which is what makes a `pub use` load-bearing); Python's is `__all__`
//!    where a module defines one, else every module-level name without a leading underscore;
//!    TypeScript's is the export list, including what a barrel forwards; Go's is a capital first rune,
//!    which is a property of the name and not of the declaration; Java's is the `public` modifier on a
//!    type; Ruby has none worth the name. [`crate::structure`] already applied them, so this module
//!    reads [`crate::structure::Structure::exports`] and does not re-derive it — the first place that
//!    re-derives an export rule is the first place that disagrees with itself.
//! 2. **Members of a public type are on the surface.** `e3.1` excludes members from the export set on
//!    purpose (the export relation is about names a file *owns*), but a caller who compiled against
//!    `Shape.area` is not interested in that distinction. So a member is on the surface when its root
//!    is exported and its own visibility is not private, and the check is per language
//!    (`member_is_public`, below).
//! 3. **A scope that cannot be enumerated marks its items.** Every item carries
//!    [`Item::synthetic`] and the reasons, copied from
//!    [`crate::structure::Structure::synthetic_scopes`], and a vanished synthetic item is the
//!    [`Reason::MayHaveShrunk`](crate::api::classify::Reason::MayHaveShrunk) row — the acceptance
//!    criterion that says a macro wrote it and only the running program knows.

use crate::api::package::{Attribution, CargoInfo, Manifests, PackageMap, RustdocIndex};
use crate::api::policy::ApiPolicy;
use crate::api::signature::{
    Field, Header, Signature, deprecation_of, fields_of, header_of, signature_of,
};
use crate::api::surface::{Access, Identity, Item, ItemId, Origin, Package, Surface, Unavailable};
use crate::structure::{
    self, ExtractPolicy, FileStructure, Language, SourceFile, Structure, Symbol, SymbolKind,
};
use std::collections::BTreeMap;

/// The version of the extraction rules themselves.
///
/// Distinct from [`crate::api::surface::SCHEMA_VERSION`] (the shape) because a rule change — a wider
/// member rule, a new widening test — changes what a stored surface *means* without changing its
/// fields, and a stored row has to be able to say which rules made it.
pub const RULES_VERSION: u32 = 1;

/// Everything [`surface`] reads. The caller owns every byte: `fathom-ingest` has the work area open,
/// an analysis does not open files.
pub struct Inputs<'a> {
    /// The source files whose surface is being recorded.
    pub sources: &'a [SourceFile<'a>],
    /// The manifest texts: `Cargo.toml`, `package.json`, `go.mod`, `pyproject.toml`, `*.gemspec`.
    /// Anything else is ignored. These decide package names and versions.
    pub manifests: &'a [SourceFile<'a>],
    /// `cargo metadata --format-version 1` output, when the caller has run Cargo. Optional precisely
    /// because running Cargo is not an analysis's business.
    pub cargo_metadata: Option<&'a str>,
    /// A rustdoc JSON for one crate, as `(krate, json)`. Added-only, per [`RustdocIndex`].
    pub rustdoc_json: Option<(&'a str, &'a str)>,
    /// The `[api]` policy.
    pub policy: &'a ApiPolicy,
    /// The structural extractor's policy (file size bound, resolution bounds). Its defaults are fine;
    /// a caller that already configured `[structure]` should pass what it used, so that a surface row
    /// and a structure row for the same revision mean the same thing.
    pub extract: ExtractPolicy,
}

impl<'a> Inputs<'a> {
    /// Split a revision's files into sources and manifests, which is the division the extractor draws
    /// and no other: a `.rs` file is a source whether or not its contents look like a source, and a
    /// `Cargo.toml` is a manifest even when it does not parse (the failure becomes a caveat).
    ///
    /// A caller that builds inputs by hand tends to get this wrong in the same direction — by dropping
    /// the files it decided were not interesting — which is why the split lives here.
    #[must_use]
    pub fn split(files: &'a [(String, Vec<u8>)]) -> (Vec<SourceFile<'a>>, Vec<SourceFile<'a>>) {
        let mut sources: Vec<SourceFile<'a>> = Vec::new();
        let mut manifests: Vec<SourceFile<'a>> = Vec::new();
        for (path, bytes) in files {
            let draft = SourceFile::new(path, bytes.as_slice());
            if crate::api::package::is_manifest(path) {
                manifests.push(draft);
            } else {
                sources.push(draft);
            }
        }
        (sources, manifests)
    }

    /// The minimal input: sources, an empty manifest set and the default policies. Tests and a
    /// single-file question use this; a real index pass supplies manifests and, where the repository
    /// has them, `cargo metadata`.
    #[must_use]
    pub fn new(sources: &'a [SourceFile<'a>], policy: &'a ApiPolicy) -> Inputs<'a> {
        Inputs {
            sources,
            manifests: &[],
            cargo_metadata: None,
            rustdoc_json: None,
            policy,
            extract: ExtractPolicy::default(),
        }
    }
}

/// One package's items, as they are collected.
struct Builder {
    name: String,
    language: Language,
    version: Option<String>,
    decided_by: String,
    items: Vec<Item>,
    unavailable: Vec<Unavailable>,
    overflow: usize,
}

impl Builder {
    fn new(name: &str, language: Language, attribution: &Attribution) -> Builder {
        Builder {
            name: name.to_string(),
            language,
            version: attribution.version.clone(),
            decided_by: attribution.decided_by.clone(),
            items: Vec::new(),
            unavailable: Vec::new(),
            overflow: 0,
        }
    }
}

/// Extract one revision's public API surface.
///
/// The whole of it is one pass over the corpus plus one pass to resolve re-export forwarding, and the
/// only thing that grows with the size of a declaration is reading that declaration's header.
#[must_use]
pub fn surface(inputs: &Inputs<'_>) -> Surface {
    let corpus = structure::extract(inputs.sources, &inputs.extract);
    let mut caveats: Vec<String> = Vec::new();
    let mut gaps: Vec<String> = Vec::new();

    let manifests = Manifests::new(inputs.manifests);
    let mut cargo: Option<CargoInfo> = None;
    if let Some(text) = inputs.cargo_metadata {
        match CargoInfo::from_metadata_json(text) {
            Some(info) => cargo = Some(info),
            None => caveats.push(
                "the cargo metadata JSON handed over did not parse, so crate names came from the \
                 Cargo.toml files in the inputs instead"
                    .to_string(),
            ),
        }
    }
    let mut rustdoc: Option<RustdocIndex> = None;
    if let Some((krate, json)) = inputs.rustdoc_json {
        match RustdocIndex::from_json(krate, json) {
            Some(index) => {
                caveats.push(format!(
                    "rustdoc JSON for `{krate}` ({}): {} public item name(s) were read from it and \
                     added where the source extractor had not seen them; it removes nothing, so a \
                     name the compiler did not emit is not reported as deleted",
                    index.parsed_from,
                    index.names.len()
                ));
                rustdoc = Some(index);
            }
            None => caveats.push(format!(
                "the rustdoc JSON handed over for `{krate}` was not a shape this reader knows, so \
                 the tree-sitter extractor decided the whole surface (that is the fallback, not an \
                 error)"
            )),
        }
    }

    let paths: Vec<&str> = inputs.sources.iter().map(|source| source.path).collect();
    let map = PackageMap::new(inputs.policy, manifests, cargo, &paths);
    let mut builders: BTreeMap<String, Builder> = BTreeMap::new();
    let mut files_ignored = 0usize;
    let mut ignored_paths: Vec<String> = Vec::new();
    let mut languages: Vec<String> = Vec::new();
    let mut files_parsed = 0usize;
    let mut error_parses: Vec<String> = Vec::new();
    let mut forwarding_files: Vec<String> = Vec::new();
    let mut seen = 0usize;

    // Pass one: attribute every file, record what could not be read, and index the re-export
    // forwarding that decides a name's import paths.
    let forwarders = forwarder_index(&corpus);
    let ruby_entry_rules = ruby_entry_rules(&corpus);

    for record in &corpus.files {
        seen += 1;
        let language = record.language;
        if !language.is_known() {
            continue;
        }
        if inputs.policy.ignores(&record.path) {
            files_ignored += 1;
            if ignored_paths.len() < 16 {
                ignored_paths.push(record.path.clone());
            }
            continue;
        }
        if !languages.iter().any(|have| have == language.as_str()) {
            languages.push(language.as_str().to_string());
        }
        let structure = match &record.structure {
            Some(structure) => structure,
            None => {
                // A file that could not be read is a gap with a name attached, never a smaller surface.
                let attribution = map.attribute(&record.path, language, None);
                let builder = builders
                    .entry(attribution.package.clone())
                    .or_insert_with(|| Builder::new(&attribution.package, language, &attribution));
                builder.unavailable.push(Unavailable {
                    path: record.path.clone(),
                    reason: record
                        .reason
                        .as_ref()
                        .map(|reason| reason.sentence())
                        .unwrap_or_else(|| "no reason was recorded".to_string()),
                });
                continue;
            }
        };
        files_parsed += 1;
        // A file that forwards names with a star, or merges ambient declarations into itself, is not a
        // smaller surface — but it does mean a name can be reachable from a file this surface never
        // listed, which is a sentence the report owes the reader.
        if structure.synthetic_scopes.iter().any(|scope| {
            scope.scope.is_empty()
                && scope.reasons.iter().any(|reason| {
                    matches!(
                        reason.as_str(),
                        crate::structure::synthetic::WILDCARD_REEXPORT
                            | crate::structure::synthetic::AMBIENT_MERGE
                    )
                })
        }) {
            forwarding_files.push(record.path.clone());
        }
        if record.file_level.parse_errors > 0 {
            error_parses.push(format!(
                "{} ({} error nodes)",
                record.path, record.file_level.parse_errors
            ));
        }
        let attribution = map.attribute(&record.path, language, structure.package.as_deref());
        let builder = builders
            .entry(attribution.package.clone())
            .or_insert_with(|| Builder::new(&attribution.package, language, &attribution));
        let text = match inputs
            .sources
            .iter()
            .find(|source| source.path == record.path)
            .and_then(|source| std::str::from_utf8(source.text).ok())
        {
            Some(text) => text,
            None => continue,
        };
        let scan = crate::api::textscan::Scan::new(text, language);
        let items = items_of(
            &corpus,
            record,
            structure,
            &scan,
            language,
            &builder.name,
            &forwarders,
            &ruby_entry_rules,
        );
        for item in items {
            if builder.items.len() >= inputs.policy.max_items_per_package {
                builder.overflow += 1;
                continue;
            }
            builder.items.push(item);
        }
    }

    // A language whose grammar was not compiled into this build is a hole in the answer with a name
    // attached. The per-package `unavailable` list already carries the files; this is the sentence that
    // says why, because "0 file(s) could not be read" next to "languages present: java" reads like a
    // clean bill of health when it is the opposite.
    let available = structure::available_languages();
    for language in &languages {
        if !available.iter().any(|have| have == language) {
            let files = builders
                .values()
                .filter(|builder| builder.language.as_str() == language)
                .map(|builder| builder.unavailable.len())
                .sum::<usize>();
            gaps.push(format!(
                "{language} file(s) are present and this build has no {language} grammar, so {}                  file(s) of that language were not read",
                files
            ));
        }
    }
    if !forwarding_files.is_empty() {
        caveats.push(format!(
            "{} file(s) forward names with a star or merge ambient declarations (first: {}), so a              forwarded name may be reachable from a file this surface did not list; their own              declarations were read normally",
            forwarding_files.len(),
            forwarding_files[..forwarding_files.len().min(3)].join(", ")
        ));
    }
    if !error_parses.is_empty() {
        gaps.push(format!(
            "{} file(s) parsed with error nodes, so their item lists are a lower bound (first: {})",
            error_parses.len(),
            error_parses[..error_parses.len().min(3)].join(", ")
        ));
    }
    if files_ignored > 0 {
        gaps.push(format!(
            "{files_ignored} path(s) were excluded by `[api] ignore` ({}), so this surface says \
             nothing about them",
            ignored_paths.join(", ")
        ));
    }
    if inputs.policy.mode == crate::api::policy::Mode::Off {
        caveats.push(
            "api.mode = off: the surface was extracted, and nothing here is being enforced"
                .to_string(),
        );
    }
    if let Some(index) = &rustdoc {
        add_rustdoc_items(&mut builders, index);
    }

    let packages: Vec<Package> = builders
        .into_values()
        .map(|builder| {
            let language = builder.language;
            Package {
                name: builder.name.clone(),
                language,
                access: access_of(language),
                identity: Identity::of_language(language),
                version: builder.version,
                decided_by: builder.decided_by.clone(),
                items: builder.items,
                unavailable: builder.unavailable,
                truncated: builder.overflow > 0,
                surface_sentence: surface_sentence(language, &corpus),
                digest: String::new(),
            }
        })
        .collect();

    let surface = Surface {
        schema_version: crate::api::surface::SCHEMA_VERSION,
        policy: inputs.policy.clone(),
        packages,
        files_seen: seen,
        files_parsed,
        files_ignored,
        ignored_paths,
        languages,
        available: structure::available_languages()
            .into_iter()
            .map(str::to_string)
            .collect(),
        caveats,
        gaps,
        digest: String::new(),
    };
    surface.seal()
}

/// The symbol name → its re-exporting files, as `{(declaring file, symbol): [forwarding file]}`.
///
/// Two spellings of "forwarding", because the languages write it differently: Rust and TypeScript
/// say it in the statement (`pub use`, `export … from`), which
/// [`crate::structure::Import::reexport`] and the `reexport*` forms record; Python has no such
/// statement — *every* module-level name a module imports is importable from it, so any resolved
/// edge naming the symbol forwards it. That asymmetry is Python's access rule, not an approximation
/// of one.
fn forwarder_index(corpus: &structure::CorpusStructure) -> Forwarders {
    let mut index: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for record in &corpus.files {
        let Some(structure) = &record.structure else {
            continue;
        };
        let statement_forwards = |symbol: &str| -> bool {
            structure.imports.iter().any(|import| {
                (import.symbol == symbol || import.symbol == "*")
                    && (import.reexport || import.form.starts_with("reexport"))
            })
        };
        for edge in &structure.edges {
            if edge.target.is_empty() || edge.target == record.path {
                continue;
            }
            let forwards = match record.language {
                Language::Python => true,
                Language::Rust | Language::TypeScript => statement_forwards(&edge.symbol),
                _ => false,
            };
            if forwards {
                index
                    .entry((edge.target.clone(), edge.symbol.clone()))
                    .or_default()
                    .push(record.path.clone());
                // A star re-export forwards every symbol of the target, and the star is recorded as
                // the symbol, so the pair below is the one the lookup for a named item also tries.
                if edge.symbol == "*" {
                    index
                        .entry((edge.target.clone(), "*".to_string()))
                        .or_default()
                        .push(record.path.clone());
                }
            }
        }
    }
    Forwarders { index }
}

struct Forwarders {
    index: BTreeMap<(String, String), Vec<String>>,
}

impl Forwarders {
    /// Every file a name declared in `path` can be imported from, including `path` itself. For the
    /// languages whose identity is the package, the answer is the package: that is what a caller names.
    fn reachable(&self, path: &str, name: &str, language: Language, package: &str) -> Vec<String> {
        match Identity::of_language(language) {
            Identity::Unit => vec![format!("package:{package}")],
            Identity::File => vec![path.to_string()],
            Identity::Path => {
                let mut out: Vec<String> = vec![path.to_string()];
                for key in [(path, name), (path, "*")] {
                    if let Some(found) = self.index.get(&(key.0.to_string(), key.1.to_string())) {
                        for file in found {
                            if !out.contains(file) {
                                out.push(file.clone());
                            }
                        }
                    }
                }
                out.sort();
                out
            }
        }
    }
}

/// Ruby's entry-point rule: which files of a gem are the surface.
///
/// Everything under `lib/` is `require`-able, which is the only thing Ruby can claim at file level,
/// and a repository whose `lib/` is absent (a script collection) gets every Ruby file rather than an
/// empty answer. `test/`, `spec/` and `*_test.rb`/`spec.rb` are never surface: a test file is not an
/// entry point anybody compiles against.
fn ruby_entry_rules(corpus: &structure::CorpusStructure) -> RubyRules {
    let files: Vec<&str> = corpus
        .files
        .iter()
        .filter(|record| record.language == Language::Ruby)
        .map(|record| record.path.as_str())
        .collect();
    let has_lib = files.iter().any(|path| path.starts_with("lib/"));
    RubyRules {
        require_lib: has_lib,
    }
}

struct RubyRules {
    require_lib: bool,
}

impl RubyRules {
    fn is_entry_point(&self, path: &str) -> bool {
        let name = path.rsplit('/').next().unwrap_or(path);
        if name.ends_with("_spec.rb")
            || name.ends_with("_test.rb")
            || path.starts_with("test/")
            || path.starts_with("spec/")
            || name == "Rakefile"
        {
            return false;
        }
        if self.require_lib {
            path.starts_with("lib/")
        } else {
            true
        }
    }
}

/// The items one file contributes.
///
/// Eight arguments, and they are the whole revision context rather than a smell: the file's own record,
/// the corpus it sits in, the graph of forwarders, the policy-shaped knobs. Bundling them into a struct
/// would name nothing that this list does not already name.
#[allow(clippy::too_many_arguments)]
fn items_of(
    corpus: &structure::CorpusStructure,
    record: &FileStructure,
    structure: &Structure,
    scan: &crate::api::textscan::Scan,
    language: Language,
    package: &str,
    forwarders: &Forwarders,
    ruby: &RubyRules,
) -> Vec<Item> {
    let access = access_of(language);
    if access == Access::File {
        if !ruby.is_entry_point(&record.path) {
            return Vec::new();
        }
        let loaded = structure::loads(corpus, &record.path);
        let mut loaded_files = loaded.files;
        loaded_files.sort();
        return vec![Item {
            id: ItemId {
                package: package.to_string(),
                path: record.path.clone(),
                name: record.path.clone(),
                kind: SymbolKind::Module,
            },
            line: 1,
            access,
            identity: Identity::File,
            visibility: structure::Visibility::Public,
            on_surface: true,
            signature: Signature {
                head: format!("{} ({} lines)", record.path, record.file_level.newlines + 1),
                ..Signature::default()
            },
            fields: Vec::new(),
            fields_truncated: false,
            supertypes: Vec::new(),
            modifiers: Vec::new(),
            deprecated: None,
            reachable_from: vec![record.path.clone()],
            synthetic: false,
            synthetic_reasons: Vec::new(),
            loaded: loaded_files,
            origin: Origin::TreeSitter,
        }];
    }
    // The names this file's export rule claimed, and the scopes it could not enumerate.
    let exported: Vec<&str> = structure.exports.iter().map(String::as_str).collect();
    let members: Vec<&Symbol> = structure.symbols.iter().collect();
    let mut out: Vec<Item> = Vec::new();
    for symbol in &structure.symbols {
        let root = symbol.name.split('.').next().unwrap_or("");
        let top = symbol.container.is_empty();
        let on_surface = name_is_public(language, &symbol.name)
            && if top {
                symbol.exported || exported.contains(&symbol.name.as_str())
            } else {
                exported.contains(&root) && member_is_public(language, symbol)
            };
        // A candidate is an item worth carrying even when it is not public *yet*: the alternative is
        // that a widening or narrowing of visibility looks like an item appearing from nowhere.
        let candidate = on_surface || symbol.visibility != structure::Visibility::Private;
        if !candidate {
            continue;
        }
        let header = header_of(scan, language, symbol.kind, &symbol.span);
        let signature = signature_of(language, symbol.kind, &header.head, header.unparsed);
        let own: Vec<&Symbol> = members
            .iter()
            .copied()
            .filter(|member| member.container == symbol.name)
            .collect();
        let (fields, fields_truncated) = if type_like(symbol.kind) {
            fields_of(scan, language, &header, &symbol.span, &own)
        } else {
            (Vec::new(), false)
        };
        let scope = if symbol.container.is_empty() {
            ""
        } else {
            symbol.container.as_str()
        };
        let synthetic_reasons = synthetic_reasons(structure, scope, &symbol.name);
        out.push(Item {
            id: ItemId {
                package: package.to_string(),
                path: record.path.clone(),
                name: symbol.name.clone(),
                kind: symbol.kind,
            },
            line: symbol.span.start_line,
            access,
            identity: Identity::of_language(language),
            visibility: symbol.visibility,
            on_surface,
            signature,
            fields,
            fields_truncated,
            supertypes: supertypes_of(structure, &symbol.name),
            modifiers: symbol.modifiers.clone(),
            deprecated: deprecation_of(scan, language, &symbol.span),
            reachable_from: forwarders.reachable(&record.path, &symbol.name, language, package),
            synthetic: !synthetic_reasons.is_empty(),
            synthetic_reasons,
            loaded: Vec::new(),
            origin: Origin::TreeSitter,
        });
    }
    out
}

/// Whether a name can be public API in its language, before any export rule is consulted.
///
/// Only Python needs this, because only Python marks privacy in the *name*: a leading underscore is
/// the marker, and the structural extractor reports visibility, not the convention. A dunder is not
/// private — `__init__` is the signature callers actually call — except `__all__`, which is this
/// module's own input: counting the export list as an exported item would report the same fact twice.
#[must_use]
pub fn name_is_public(language: Language, name: &str) -> bool {
    // A member's own name is the last segment: `Shape.__describe` is private whatever `Shape` is.
    let last = name.rsplit('.').next().unwrap_or(name);
    match language {
        Language::Python => {
            last != "__all__" && !(last.starts_with('_') && !last.starts_with("__"))
        }
        // Go's access rule is the first rune of the name, and the extractor's visibility column agrees
        // with it for the symbols it declares; reading it here too means a member name read from source
        // text is judged by the same rule as a symbol read from the tree.
        Language::Go => !last.starts_with(char::is_lowercase),
        _ => true,
    }
}

/// Whether a member is on the surface by its own right, per language.
fn member_is_public(language: Language, symbol: &Symbol) -> bool {
    // Rust spells access on the member, and `pub(crate)` is *not* public API: the extractor tells
    // `Public` and `Crate` apart precisely so this line can.
    if language == Language::Rust {
        return symbol.visibility == structure::Visibility::Public
            && name_is_public(language, &symbol.name);
    }
    if language == Language::Go {
        return name_is_public(language, &symbol.name);
    }
    match language {
        // Rust needs `pub` on the method; Python's convention is the leading underscore (which the
        // extractor already maps to `Crate`); Go's is the capital first rune; TypeScript's is the
        // absence of `private`/`#`; Java's is `public` or `protected`.
        Language::Rust
        | Language::Python
        | Language::Go
        | Language::Java
        | Language::TypeScript => symbol.visibility != structure::Visibility::Private,
        Language::Ruby | Language::Unknown => false,
    }
}

/// Whether a declaration's body is part of its contract.
fn type_like(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Struct | SymbolKind::Class | SymbolKind::Interface | SymbolKind::Enum
    )
}

/// The scopes a symbol's declaration sits in that cannot be enumerated, with their reasons merged.
fn synthetic_reasons(structure: &Structure, scope: &str, name: &str) -> Vec<String> {
    let mut reasons: Vec<String> = Vec::new();
    let mut push = |candidate: &str| {
        if let Some(scope) = structure
            .synthetic_scopes
            .iter()
            .find(|scope| scope.scope == candidate)
        {
            for reason in &scope.reasons {
                if !item_synthetic(reason) || reasons.contains(reason) {
                    continue;
                }
                reasons.push(reason.clone());
            }
        }
    };
    if scope.is_empty() {
        push("");
    } else {
        push(scope);
    }
    // A member of a synthetic member (`Circle::area` under a decorated `Circle`) is also unauditable.
    if let Some(parent) = name.rsplit_once('.') {
        push(parent.0);
    }
    reasons.sort();
    reasons
}

/// Whether a synthetic-scope reason threatens *this item's* existence.
///
/// Two of [`crate::structure::synthetic`]'s categories say something about the names a file
/// *forwards* rather than about the declarations it holds: a barrel's `export * from "./shapes"` and a
/// TypeScript ambient merge both leave every declaration in the file perfectly enumerable. Marking an
/// item in such a file as "may have shrunk" would turn a real removal into an indeterminate answer
/// because a neighbouring line re-exported with a star — and would say so in a report a reviewer reads
/// as a shrug. They are reported where they belong instead: as a caveat on the package, saying that a
/// forwarded name may be reachable from a file this surface did not list.
#[must_use]
pub fn item_synthetic(reason: &str) -> bool {
    !matches!(
        reason,
        crate::structure::synthetic::WILDCARD_REEXPORT | crate::structure::synthetic::AMBIENT_MERGE
    )
}

/// The bases and implementations recorded for one symbol.
fn supertypes_of(structure: &Structure, name: &str) -> Vec<String> {
    let mut out: Vec<String> = structure
        .supertypes
        .iter()
        .filter(|supertype| supertype.of == name)
        .map(|supertype| format!("{} {}", supertype.kind.as_str(), supertype.base))
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Symbol-level for five languages, file-level for Ruby. The one line that decides the shape of every
/// Ruby answer in this module.
#[must_use]
pub fn access_of(language: Language) -> Access {
    match language {
        Language::Ruby => Access::File,
        _ => Access::Symbol,
    }
}

/// What this language's surface can claim, and what it cannot. Written per language because the
/// honest sentence is different for each, and printed in every report so a reader of a package's
/// line knows what the line is built from.
fn surface_sentence(language: Language, corpus: &structure::CorpusStructure) -> String {
    let counts = corpus.summary.by_language.get(language.as_str());
    let (symbols, exports) = counts
        .map(|entry| (entry.symbols, entry.exports))
        .unwrap_or((0, 0));
    match language {
        Language::Ruby => format!(
            "Ruby's public API surface is the *file*, not the symbol: this corpus has {exports} \
             exports for {symbols} declarations and no per-symbol access control, so a column of \
             per-symbol breaking changes would be a lie (spike s1, finding 3). What is reported \
             here is the set of loadable files and, for each, the files loading it pulls in."
        ),
        Language::Go => format!(
            "Go: export is a capital first rune, and the unit is the *package* ({exports} exported \
             names across {symbols} declarations here), so a name reached by moving a file within a \
             package has not moved at all; dependency facts about a package are package-granular, \
             not file-granular."
        ),
        Language::Rust => format!(
            "Rust: an item is on the surface when it is `pub` *and* reachable by a public path from \
             the crate root, which is what a `pub use` buys ({exports} exported names across \
             {symbols} declarations here); a body change is not a contract change, a field change is."
        ),
        Language::Python => format!(
            "Python: `__all__` where the module defines one, else every module-level name without a \
             leading underscore ({exports} exported names across {symbols} declarations here); \
             names are untyped more often than not, so a signature change may be text-only to a \
             caller and breaking to a type checker."
        ),
        Language::TypeScript => format!(
            "TypeScript: the export list, including what a barrel forwards and what a `.d.ts` \
             declares ({exports} exported names across {symbols} declarations here); optionality \
             (`?`, a default) is the difference between additive and breaking more often than the \
             type is."
        ),
        Language::Java => format!(
            "Java: public types and their public or protected members, per package clause ({exports} \
             exported names across {symbols} declarations here); the package is the unit, so a \
             moved file is not a moved name."
        ),
        Language::Unknown => "not one of the languages fathom reads".to_string(),
    }
}

/// Adopt the rustdoc JSON's names that the source pass did not see.
fn add_rustdoc_items(builders: &mut BTreeMap<String, Builder>, index: &RustdocIndex) {
    let Some(builder) = builders.get_mut(&index.krate) else {
        return;
    };
    for entry in &index.names {
        let known = builder.items.iter().any(|item| {
            item.id.name == entry.name || item.id.name.ends_with(&format!(".{}", entry.name))
        });
        if known {
            continue;
        }
        let kind = entry.kind.unwrap_or(SymbolKind::Type);
        builder.items.push(Item {
            id: ItemId {
                package: builder.name.clone(),
                path: format!("(rustdoc:{})", index.krate),
                name: entry.name.clone(),
                kind,
            },
            line: 0,
            access: Access::Symbol,
            identity: Identity::Path,
            visibility: structure::Visibility::Public,
            on_surface: true,
            signature: Signature {
                head: String::new(),
                unparsed: true,
                ..Signature::default()
            },
            fields: Vec::new(),
            fields_truncated: false,
            supertypes: Vec::new(),
            modifiers: Vec::new(),
            deprecated: None,
            reachable_from: vec![format!("(rustdoc:{})", index.krate)],
            synthetic: false,
            synthetic_reasons: Vec::new(),
            loaded: Vec::new(),
            origin: Origin::RustdocJson,
        });
    }
}

/// The fields and signature of one declaration, from its text. Re-exported for a caller that wants to
/// ask about a single declaration without building a whole surface.
#[must_use]
pub fn declare(
    language: Language,
    kind: SymbolKind,
    text: &str,
    span: &structure::Span,
) -> (Signature, Vec<Field>, Option<String>) {
    let scan = crate::api::textscan::Scan::new(text, language);
    let header: Header = header_of(&scan, language, kind, span);
    let signature = signature_of(language, kind, &header.head, header.unparsed);
    let (fields, _) = fields_of(&scan, language, &header, span, &[]);
    (signature, fields, deprecation_of(&scan, language, span))
}
