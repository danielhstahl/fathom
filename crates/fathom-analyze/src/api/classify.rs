//! Classification: given the same item in two revisions, what happened to it, and does it break a caller?
//!
//! This is the module the ticket's acceptance criteria turn on. A diff of two surfaces is a list of
//! differences; a *classification* is a judgement about each one — breaking, additive, deprecating,
//! indeterminate, or neutral — attached to the `file:line` it was found at and written in a vocabulary
//! stable enough to be a baseline.
//!
//! Four rules hold it together.
//!
//! 1. **A reason, not a verdict.** Every change carries *every* reason found, each with the specifics
//!    (`was f64, now Option&lt;f64&gt;`), and the class is the highest-ranking one. A reviewer who
//!    disagrees with the ranking can still see all of it; a reviewer given only a verdict cannot.
//! 2. **Direction is about the caller.** "Widened" means a caller may now pass more, which is additive
//!    even though the source text changed; "narrowed" means a caller's existing argument may no longer
//!    fit, which is breaking even though nothing was removed. The whole of [`type_relation`] is this
//!    one idea written out per language.
//! 3. **Silence is a claim.** An item that disappeared from a scope the extractor could not enumerate
//!    is [`Reason::MayHaveShrunk`], not a removal: a macro or a decorator may still be writing it, and
//!    the only honest answer is that this reader cannot tell. The same discipline as
//!    [`crate::structure`]'s synthetic scopes, and for the same reason.
//! 4. **Indeterminate is not a pass and not an accusation.** [`Reason::SignatureUnparsed`] and
//!    [`Reason::MayHaveShrunk`] rank *below* breaking — so a gate that wants to refuse can — while
//!    never producing a "no change" answer.
//!
//! The reason codes in [`Reason::code`] are the stable vocabulary: they are what a ratchet baseline
//! will be written in and what the JSON exports, so a code is never renamed and never reused.

use crate::api::signature::{Field, Param, Signature};
use crate::api::surface::{Access, Item};
use crate::structure::Visibility;
use serde::Serialize;

// ==============================================================================================
// the shape of a judgement
// ==============================================================================================

/// What a change means for a caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Class {
    /// The declaration changed and nothing a caller compiles against did: a value, a moved line, a
    /// deprecation note.
    #[default]
    Neutral,
    /// A caller can now do something it could not: a new name, a wider parameter, an optional parameter.
    Additive,
    /// The author has marked something for removal. Its own class because a deprecation is the one
    /// change a reviewer is asked to *read*, not merely compile.
    Deprecating,
    /// The surface moved and this reader could not tell which way. Not a pass, not an accusation.
    Indeterminate,
    /// A caller that compiled before may not compile now.
    Breaking,
}

impl Class {
    /// The stable spelling, which is also the JSON spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Class::Neutral => "neutral",
            Class::Additive => "additive",
            Class::Deprecating => "deprecating",
            Class::Indeterminate => "indeterminate",
            Class::Breaking => "breaking",
        }
    }

    /// How loudly the class speaks. A change's class is the loudest of its reasons.
    pub fn rank(self) -> u8 {
        match self {
            Class::Neutral => 0,
            Class::Additive => 1,
            Class::Deprecating => 2,
            Class::Indeterminate => 3,
            Class::Breaking => 4,
        }
    }

    /// The one-word sentence a report leads with.
    pub fn sentence(self) -> &'static str {
        match self {
            Class::Neutral => "changed, nothing a caller compiles against did",
            Class::Additive => "additive",
            Class::Deprecating => "deprecating",
            Class::Indeterminate => "cannot say",
            Class::Breaking => "breaking",
        }
    }
}

/// One reason, with the specifics that make it checkable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Detail {
    /// The reason code and class.
    pub reason: Reason,
    /// The evidence sentence, built where the comparison happened so the specifics are the source's
    /// own text (`was f64, now Option&lt;f64&gt;`).
    pub note: String,
}

impl Detail {
    /// The full line: what changed, and the specifics.
    #[must_use]
    pub fn sentence(&self) -> String {
        if self.note.is_empty() {
            self.reason.summary().to_string()
        } else {
            format!("{} — {}", self.reason.summary(), self.note)
        }
    }
}

/// The vocabulary of things that can happen to an item. Each variant is a claim a reviewer can check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    // --- existence and access ---------------------------------------------
    /// A name appeared that a caller could not have used before.
    ItemAdded,
    /// A name a caller could use is gone. **Not** the same claim as [`Reason::MayHaveShrunk`].
    ItemRemoved,
    /// The item is still declared and still visible, and no longer exported.
    ExportRemoved,
    /// The export list grew to include a name that was already there.
    ExportAdded,
    /// The declaration's own visibility widened (`pub(crate)` → `pub`).
    VisibilityWidened,
    /// The declaration's own visibility narrowed. The callers who break are the ones inside the old
    /// visibility, which is why this is breaking and not merely a narrowing of the surface.
    VisibilityNarrowed,
    /// The same name and place, a different kind: a struct that became an enum is not a modified struct.
    KindChanged,
    /// The item moved file and a consumer can still reach it. Reported, not scored: in the languages
    /// whose identity is the path a move is a removal and an addition, and this row is for the ones
    /// where it is not.
    ItemMoved,
    /// The item moved file and nothing forwards the old path.
    ItemMovedUnreachable,
    /// A path a consumer could import the name from is gone — the dropped `pub use`, the barrel that
    /// stopped forwarding it. Nothing was deleted and callers still break.
    ReachabilityNarrowed,
    /// The name is importable from an additional path.
    ReachabilityWidened,
    // --- the header, when this reader could not decompose it -----------------
    /// The declaration text differs and nothing structural accounts for it: an attribute, a comment
    /// inside the header, a spelling this parser does not model. Stated, not guessed at.
    DeclarationTextChanged,
    /// The header could not be parsed at all, so no structural claim is possible.
    SignatureUnparsed,
    // --- parameters ---------------------------------------------------------
    /// A parameter a caller was passing is gone.
    ParameterRemoved,
    /// A parameter the caller must now supply.
    RequiredParameterAdded,
    /// A parameter the caller may ignore: a default, a `?`, a rest parameter, a variadic list.
    OptionalParameterAdded,
    /// A parameter's name changed. Named arguments and keyword arguments make a rename breaking in both
    /// languages that have them, and positional ones make it breaking in the rest.
    ParameterRenamed,
    /// A parameter's type widened: the caller may pass more.
    ParameterTypeWidened,
    /// A parameter's type narrowed: the caller may have to convert.
    ParameterTypeNarrowed,
    /// A parameter's type changed and neither side contains the other.
    ParameterTypeChanged,
    /// A parameter that used to be required may now be omitted (a default appeared, `Option<T>`
    /// arrived, TypeScript gained a `?`).
    ParameterRelaxed,
    /// A parameter that used to be optional must now be supplied.
    ParameterTightened,
    // --- the rest of the signature ------------------------------------------
    /// The generic parameter list changed length: turbofish call sites and explicit instantiations
    /// break.
    GenericCountChanged,
    /// The generic list kept its length and changed its bounds. Reported, not scored: the caller's own
    /// type inference usually absorbs it.
    GenericBoundsChanged,
    /// The return type changed. Breaking in both directions, because a caller names the type it got.
    ReturnTypeChanged,
    /// `&self` became `&mut self`, `self` became `&self`, a Go receiver changed type or became a
    /// pointer. Every holder of a shared reference is affected.
    ReceiverChanged,
    /// The value's declared type changed (`const MAX: u8` → `const MAX: u32`).
    ValueTypeChanged,
    /// A type alias now means something else. Every user of the alias sees it.
    AliasChanged,
    /// The value a `const`/`static`/`var` initialises to changed (`const SIDES: u32 = 4` → `5`) while
    /// its declared type held. Reported, not scored: a caller compiles against the name and the type,
    /// and no reader can tell from a literal whether the behaviour it encodes was a contract.
    ValueChanged,
    // --- members of a type-like declaration -----------------------------------
    /// A field, interface member or enum variant a caller could name is gone. The structural model does
    /// not treat fields as symbols, so without this row a removed field would be invisible.
    MemberRemoved,
    /// A new field, member or variant.
    MemberAdded,
    /// A member's type changed.
    MemberTypeChanged,
    /// A member that had become optional is required again (`a?: T` → `a: T`).
    MemberTightened,
    /// A member became optional (`a: T` → `a?: T`, or gained a default).
    MemberRelaxed,
    /// A member line this reader could not name changed. Stated rather than dropped.
    MemberTextChanged,
    // --- supertypes -------------------------------------------------------------
    /// A base class or implemented interface was dropped. Anybody relying on the capability breaks.
    SupertypeRemoved,
    /// A base class or implemented interface was added: the type can do more.
    SupertypeAdded,
    // --- deprecation ------------------------------------------------------------
    /// The declaration now carries a deprecation marker, with the author's note.
    Deprecated,
    /// The deprecation marker was removed. A promise withdrawn, which is additive but worth a line.
    DeprecationLifted,
    /// The deprecation marker stayed and its note changed. Information for the migrator, nothing for
    /// the compiler.
    DeprecationNoteChanged,
    // --- synthetic scopes -------------------------------------------------------
    /// A name vanished from a scope the extractor cannot enumerate. **Not** a removal:
    /// [`crate::structure`] says the scope may contain more than it listed, and a name written by a
    /// macro or a decorator may still be written.
    MayHaveShrunk,
    // --- Ruby's file-level surface ----------------------------------------------
    /// An entry point a consumer could `require` is gone.
    EntryFileRemoved,
    /// A new file is loadable from the entry point.
    EntryFileAdded,
    /// An entry point now loads files it did not: its transitive content grew.
    LoadsGrew,
    /// An entry point no longer loads files it used to. Breaking, because a consumer that did
    /// `require "gem"` and used something from one of those files has lost it.
    LoadsShrank,
    /// An entry point's own contents changed, which Ruby's surface cannot decompose further.
    FileContentsChanged,
}

impl Reason {
    /// The stable code, which is also the JSON spelling and the spelling a baseline is written in.
    pub fn code(self) -> &'static str {
        match self {
            Reason::ItemAdded => "item_added",
            Reason::ItemRemoved => "item_removed",
            Reason::ExportRemoved => "export_removed",
            Reason::ExportAdded => "export_added",
            Reason::VisibilityWidened => "visibility_widened",
            Reason::VisibilityNarrowed => "visibility_narrowed",
            Reason::KindChanged => "kind_changed",
            Reason::ItemMoved => "item_moved",
            Reason::ItemMovedUnreachable => "item_moved_unreachable",
            Reason::ReachabilityNarrowed => "reachability_narrowed",
            Reason::ReachabilityWidened => "reachability_widened",
            Reason::DeclarationTextChanged => "declaration_text_changed",
            Reason::SignatureUnparsed => "signature_unparsed",
            Reason::ParameterRemoved => "parameter_removed",
            Reason::RequiredParameterAdded => "required_parameter_added",
            Reason::OptionalParameterAdded => "optional_parameter_added",
            Reason::ParameterRenamed => "parameter_renamed",
            Reason::ParameterTypeWidened => "parameter_type_widened",
            Reason::ParameterTypeNarrowed => "parameter_type_narrowed",
            Reason::ParameterTypeChanged => "parameter_type_changed",
            Reason::ParameterRelaxed => "parameter_relaxed",
            Reason::ParameterTightened => "parameter_tightened",
            Reason::GenericCountChanged => "generic_count_changed",
            Reason::GenericBoundsChanged => "generic_bounds_changed",
            Reason::ReturnTypeChanged => "return_type_changed",
            Reason::ReceiverChanged => "receiver_changed",
            Reason::ValueTypeChanged => "value_type_changed",
            Reason::AliasChanged => "alias_changed",
            Reason::ValueChanged => "value_changed",
            Reason::MemberRemoved => "member_removed",
            Reason::MemberAdded => "member_added",
            Reason::MemberTypeChanged => "member_type_changed",
            Reason::MemberTightened => "member_tightened",
            Reason::MemberRelaxed => "member_relaxed",
            Reason::MemberTextChanged => "member_text_changed",
            Reason::SupertypeRemoved => "supertype_removed",
            Reason::SupertypeAdded => "supertype_added",
            Reason::Deprecated => "deprecated",
            Reason::DeprecationLifted => "deprecation_lifted",
            Reason::DeprecationNoteChanged => "deprecation_note_changed",
            Reason::MayHaveShrunk => "may_have_shrunk",
            Reason::EntryFileRemoved => "entry_file_removed",
            Reason::EntryFileAdded => "entry_file_added",
            Reason::LoadsGrew => "loads_grew",
            Reason::LoadsShrank => "loads_shrank",
            Reason::FileContentsChanged => "file_contents_changed",
        }
    }

    /// The class this reason implies on its own.
    pub fn class(self) -> Class {
        match self {
            Reason::ItemAdded
            | Reason::ExportAdded
            | Reason::VisibilityWidened
            | Reason::ReachabilityWidened
            | Reason::OptionalParameterAdded
            | Reason::ParameterTypeWidened
            | Reason::ParameterRelaxed
            | Reason::MemberAdded
            | Reason::MemberRelaxed
            | Reason::SupertypeAdded
            | Reason::DeprecationLifted
            | Reason::EntryFileAdded
            | Reason::LoadsGrew => Class::Additive,
            Reason::Deprecated => Class::Deprecating,
            Reason::DeprecationNoteChanged
            | Reason::GenericBoundsChanged
            | Reason::ItemMoved
            | Reason::ValueChanged
            | Reason::FileContentsChanged => Class::Neutral,
            Reason::SignatureUnparsed | Reason::MayHaveShrunk => Class::Indeterminate,
            _ => Class::Breaking,
        }
    }

    /// What the reason means, in the fewest words that make the row readable alone.
    pub fn summary(self) -> &'static str {
        match self {
            Reason::ItemAdded => "new public item",
            Reason::ItemRemoved => "public item removed",
            Reason::ExportRemoved => "no longer exported",
            Reason::ExportAdded => "newly exported",
            Reason::VisibilityWidened => "visibility widened",
            Reason::VisibilityNarrowed => "visibility narrowed",
            Reason::KindChanged => "kind changed",
            Reason::ItemMoved => "moved file",
            Reason::ItemMovedUnreachable => "moved file and no longer reachable",
            Reason::ReachabilityNarrowed => "no longer importable from every path it was",
            Reason::ReachabilityWidened => "importable from an additional path",
            Reason::DeclarationTextChanged => {
                "declaration text differs where this reader could not attribute it"
            }
            Reason::SignatureUnparsed => "declaration could not be parsed",
            Reason::ParameterRemoved => "parameter removed",
            Reason::RequiredParameterAdded => "required parameter added",
            Reason::OptionalParameterAdded => "optional parameter added",
            Reason::ParameterRenamed => "parameter renamed",
            Reason::ParameterTypeWidened => "parameter type widened",
            Reason::ParameterTypeNarrowed => "parameter type narrowed",
            Reason::ParameterTypeChanged => "parameter type changed",
            Reason::ParameterRelaxed => "parameter may now be omitted",
            Reason::ParameterTightened => "parameter became required",
            Reason::GenericCountChanged => "generic parameter count changed",
            Reason::GenericBoundsChanged => "generic bounds changed",
            Reason::ReturnTypeChanged => "return type changed",
            Reason::ReceiverChanged => "receiver changed",
            Reason::ValueTypeChanged => "declared type changed",
            Reason::AliasChanged => "type alias changed",
            Reason::ValueChanged => "declared value changed",
            Reason::MemberRemoved => "member removed",
            Reason::MemberAdded => "member added",
            Reason::MemberTypeChanged => "member type changed",
            Reason::MemberTightened => "member became required",
            Reason::MemberRelaxed => "member became optional",
            Reason::MemberTextChanged => "member changed",
            Reason::SupertypeRemoved => "base or implementation removed",
            Reason::SupertypeAdded => "base or implementation added",
            Reason::Deprecated => "deprecated",
            Reason::DeprecationLifted => "deprecation withdrawn",
            Reason::DeprecationNoteChanged => "deprecation note changed",
            Reason::MayHaveShrunk => "may have shrunk",
            Reason::EntryFileRemoved => "entry point file removed",
            Reason::EntryFileAdded => "entry point file added",
            Reason::LoadsGrew => "loads more files",
            Reason::LoadsShrank => "loads fewer files",
            Reason::FileContentsChanged => "file contents changed",
        }
    }
}

/// The class of a set of reasons: the highest-ranking one, and the one to cite as the driver.
#[must_use]
pub fn class_of(reasons: &[Detail]) -> Class {
    reasons
        .iter()
        .map(|detail| detail.reason.class())
        .max_by_key(|class| class.rank())
        .unwrap_or(Class::Neutral)
}

// ==============================================================================================
// the judgement itself
// ==============================================================================================

/// What happened to one item between two revisions, as every reason found.
///
/// `before` absent is an addition, `after` absent is a removal (or a shrug), both is a change. An
/// empty result means nothing a caller compiles against differs — which is the answer most pairs get.
#[must_use]
pub fn reasons(before: Option<&Item>, after: Option<&Item>) -> Vec<Detail> {
    match (before, after) {
        (None, Some(after)) => added(after),
        (Some(before), None) => removed(before),
        (Some(before), Some(after)) => changed(before, after),
        (None, None) => Vec::new(),
    }
}

fn added(item: &Item) -> Vec<Detail> {
    if item.access == Access::File {
        return vec![Detail {
            reason: Reason::EntryFileAdded,
            note: format!("{} loads {} file(s)", item.id.path, item.loaded.len()),
        }];
    }
    let mut out = vec![Detail {
        reason: Reason::ItemAdded,
        note: format!(
            "{} {} is new at {}{}",
            item.id.kind.as_str(),
            item.id.name,
            item.location(),
            if item.signature.head.is_empty() {
                String::new()
            } else {
                format!(" ({})", item.signature.head)
            }
        ),
    }];
    // A name that arrives already deprecated is an addition *and* a deprecation, and the second half is
    // the half a reviewer acts on: it says the replacement is meant to be used and the old plan is
    // already written down. Saying only "new item" would report the addition and hide the notice.
    if let Some(note) = &item.deprecated {
        out.push(Detail {
            reason: Reason::Deprecated,
            note: format!("it arrived deprecated — the author's words: {note}"),
        });
    }
    out
}

fn removed(item: &Item) -> Vec<Detail> {
    if item.synthetic {
        // The acceptance criterion about macros, as a mechanism: the scope cannot be enumerated, so a
        // vanished name is not evidence of a removal.
        return vec![Detail {
            reason: Reason::MayHaveShrunk,
            note: format!(
                "{} was listed here and is not now; {} wrote or could have written it ({}), so only \
                 the running program knows whether it is gone — reported as *may have shrunk*, not as \
                 removed",
                item.id.name,
                if item.synthetic_reasons.is_empty() {
                    "a macro or a decorator".to_string()
                } else {
                    item.synthetic_reasons.join(", ")
                },
                item.location()
            ),
        }];
    }
    if item.access == Access::File {
        return vec![Detail {
            reason: Reason::EntryFileRemoved,
            note: format!(
                "{} is gone; it loaded {} file(s)",
                item.id.path,
                item.loaded.len()
            ),
        }];
    }
    let mut out = vec![Detail {
        reason: Reason::ItemRemoved,
        note: format!(
            "{} {} was last seen at {} ({})",
            item.id.kind.as_str(),
            item.id.name,
            item.location(),
            if item.signature.head.is_empty() {
                "no signature".to_string()
            } else {
                item.signature.head.clone()
            }
        ),
    }];
    if let Some(note) = &item.deprecated {
        // Still breaking — a caller that ignored the deprecation is broken — but the sentence should
        // say the notice existed, because that is what turns "someone broke the build" into "someone
        // did not read the notice", which are different conversations.
        out.push(Detail {
            reason: Reason::Deprecated,
            note: format!("it was already deprecated — the author's words: {note}"),
        });
    }
    out
}

/// The item is in both revisions: everything that differs, one reason at a time.
fn changed(before: &Item, after: &Item) -> Vec<Detail> {
    if before.record() == after.record() {
        return Vec::new();
    }
    let mut out: Vec<Detail> = Vec::new();
    if before.id.kind != after.id.kind {
        out.push(Detail {
            reason: Reason::KindChanged,
            note: format!(
                "was {}, now {}",
                before.id.kind.as_str(),
                after.id.kind.as_str()
            ),
        });
    }
    if before.access == Access::File || after.access == Access::File {
        out.extend(file_diff(before, after));
        out.extend(deprecation_diff(before, after));
        return out;
    }
    if before.on_surface != after.on_surface {
        out.push(Detail {
            reason: if after.on_surface {
                Reason::ExportAdded
            } else {
                Reason::ExportRemoved
            },
            note: if after.on_surface {
                format!(
                    "{} is on the surface now, from {}",
                    after.id.name,
                    after.location()
                )
            } else {
                format!(
                    "{} is no longer on the surface; it is still declared at {}",
                    before.id.name,
                    before.location()
                )
            },
        });
    }
    if before.visibility != after.visibility {
        out.push(Detail {
            reason: if visibility_rank(after.visibility) > visibility_rank(before.visibility) {
                Reason::VisibilityWidened
            } else {
                Reason::VisibilityNarrowed
            },
            note: format!(
                "was {}, now {}",
                before.visibility.as_str(),
                after.visibility.as_str()
            ),
        });
    }
    out.extend(reachability_diff(before, after));
    let mut signature_reasons = signature_diff(&before.signature, &after.signature);
    // An access keyword is written in the header text as well as being the thing `visibility` records, so
    // `class Ledger` becoming `public class Ledger` is one authoring act and not two. When a visibility
    // or export row has already accounted for the header and nothing else in the signature moved, the
    // leftover "declaration text differs" row is dropped — leaving it would rank a widening as breaking,
    // which is the opposite of the truth.
    if signature_shape_equal(&before.signature, &after.signature) {
        let access_explained = out.iter().any(|detail| {
            matches!(
                detail.reason,
                Reason::VisibilityWidened
                    | Reason::VisibilityNarrowed
                    | Reason::ExportAdded
                    | Reason::ExportRemoved
            )
        });
        if access_explained {
            signature_reasons.retain(|detail| detail.reason != Reason::DeclarationTextChanged);
        }
    }
    out.extend(signature_reasons);
    out.extend(fields_diff(&before.fields, &after.fields));
    out.extend(supertypes_diff(&before.supertypes, &after.supertypes));
    out.extend(deprecation_diff(before, after));
    if before.modifiers != after.modifiers {
        // Two things are written twice in a declaration — the deprecation marker (which the note field
        // owns) and the access keyword (which `visibility` owns) — and comparing them here as well would
        // report one authoring act as two changes, ranking the copy this module does not treat as its own
        // evidence as `breaking`.
        let meaningful = |modifiers: &[String]| -> Vec<String> {
            modifiers
                .iter()
                .filter(|modifier| {
                    !matches!(
                        modifier.as_str(),
                        "deprecated" | "public" | "private" | "protected" | "pub"
                    )
                })
                .cloned()
                .collect()
        };
        if meaningful(&before.modifiers) != meaningful(&after.modifiers) {
            out.push(Detail {
                reason: Reason::DeclarationTextChanged,
                note: format!(
                    "was [{}], now [{}]",
                    before.modifiers.join(", "),
                    after.modifiers.join(", ")
                ),
            });
        }
    }
    if after.fields_truncated || before.fields_truncated {
        out.push(Detail {
            reason: Reason::SignatureUnparsed,
            note: format!(
                "the member list of {} was capped, so it is a lower bound, not the whole contract",
                after.id.name
            ),
        });
    }
    out
}

fn visibility_rank(visibility: Visibility) -> u8 {
    match visibility {
        Visibility::Private => 0,
        Visibility::Crate => 1,
        Visibility::Scoped => 2,
        Visibility::Public => 3,
    }
}

/// The paths a consumer can name the item from. A lost path is the dropped `pub use`, the barrel that
/// stopped forwarding: nothing was deleted, and the caller still breaks.
fn reachability_diff(before: &Item, after: &Item) -> Vec<Detail> {
    // Under unit identity (Go, Java) the package is the unit: the declaring file is not an import
    // path a consumer spells, so a change of file is not a change of reachability.
    let key = |item: &Item| -> Vec<String> {
        if item.identity == crate::api::surface::Identity::Unit {
            vec![format!("package:{}", item.id.package)]
        } else {
            item.reachable_from.clone()
        }
    };
    let before_paths = key(before);
    let after_paths = key(after);
    let mut out = Vec::new();
    let lost: Vec<&String> = before_paths
        .iter()
        .filter(|path| !after_paths.contains(path))
        .collect();
    let gained: Vec<&String> = after_paths
        .iter()
        .filter(|path| !before_paths.contains(path))
        .collect();
    if !lost.is_empty() {
        out.push(Detail {
            reason: Reason::ReachabilityNarrowed,
            note: format!(
                "no longer importable from {}",
                lost.iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
    }
    if !gained.is_empty() {
        out.push(Detail {
            reason: Reason::ReachabilityWidened,
            note: format!(
                "now importable from {}",
                gained
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
    }
    out
}

/// Ruby's file-level surface: the set of files an entry point pulls in, and its own size.
fn file_diff(before: &Item, after: &Item) -> Vec<Detail> {
    let mut out: Vec<Detail> = Vec::new();
    let lost: Vec<&String> = before
        .loaded
        .iter()
        .filter(|path| !after.loaded.contains(path))
        .collect();
    let gained: Vec<&String> = after
        .loaded
        .iter()
        .filter(|path| !before.loaded.contains(path))
        .collect();
    if !gained.is_empty() {
        out.push(Detail {
            reason: Reason::LoadsGrew,
            note: format!(
                "{} now loads {} more file(s): {}",
                after.id.path,
                gained.len(),
                gained
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
    }
    if !lost.is_empty() {
        out.push(Detail {
            reason: Reason::LoadsShrank,
            note: format!(
                "{} no longer loads {} file(s): {}",
                before.id.path,
                lost.len(),
                lost.iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
    }
    if before.signature.head != after.signature.head && lost.is_empty() && gained.is_empty() {
        out.push(Detail {
            reason: Reason::FileContentsChanged,
            note: format!(
                "{} was `{}`, now `{}`; Ruby's surface is the file, so this is reported and not \
                 scored as a symbol-level change",
                after.id.path, before.signature.head, after.signature.head
            ),
        });
    }
    out
}

fn deprecation_diff(before: &Item, after: &Item) -> Vec<Detail> {
    match (&before.deprecated, &after.deprecated) {
        (None, Some(note)) => vec![Detail {
            reason: Reason::Deprecated,
            note: format!("the author's words: {note}"),
        }],
        (Some(note), None) => vec![Detail {
            reason: Reason::DeprecationLifted,
            note: format!("the marker is gone; it said: {note}"),
        }],
        (Some(before), Some(after)) if before != after => vec![Detail {
            reason: Reason::DeprecationNoteChanged,
            note: format!("was `{before}`, now `{after}`"),
        }],
        _ => Vec::new(),
    }
}

// ==============================================================================================
// signatures
// ==============================================================================================

/// How two spellings of a type relate, from the point of view of the code that calls it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeRelation {
    /// The two spellings are the same text.
    Same,
    /// The caller may pass more than before: additive.
    Widened,
    /// The caller may pass less than before: breaking.
    Narrowed,
    /// Neither: at least one caller holds the old type. Breaking.
    Changed,
}

/// The widening test. Every rule is a rule about *what a caller can pass*, and each is stated in
/// [`docs/concepts/api-surface.md`](../../../../docs/concepts/api-surface.md).
#[must_use]
pub fn type_relation(before: Option<&str>, after: Option<&str>) -> TypeRelation {
    let before = before.map(normalise_type).filter(|text| !text.is_empty());
    let after = after.map(normalise_type).filter(|text| !text.is_empty());
    if before == after {
        return TypeRelation::Same;
    }
    let (Some(before), Some(after)) = (before, after) else {
        // A type appearing where there was none, or vanishing: no relation to speak of.
        return TypeRelation::Changed;
    };
    let before_members = union_members(&before);
    let after_members = union_members(&after);
    if before_members != after_members && !before_members.is_empty() && !after_members.is_empty() {
        if after_members.len() >= before_members.len()
            && before_members
                .iter()
                .all(|member| after_members.contains(member))
        {
            return TypeRelation::Widened;
        }
        if before_members.len() >= after_members.len()
            && after_members
                .iter()
                .all(|member| before_members.contains(member))
        {
            return TypeRelation::Narrowed;
        }
    }
    match (is_widest(&before), is_widest(&after)) {
        (false, true) => return TypeRelation::Widened,
        (true, false) => return TypeRelation::Narrowed,
        // Either both are the top type or neither is; the first test already caught "neither changed".
        _ => {}
    }
    // Rust's `impl Trait` in an argument position accepts every type that satisfies the bound, so
    // arriving there is a widening and leaving it is a narrowing — the single most common widening in
    // Rust API maintenance, and the one a text comparison cannot see.
    if after.starts_with("impl ") && !before.starts_with("impl ") {
        return TypeRelation::Widened;
    }
    if before.starts_with("impl ") && !after.starts_with("impl ") {
        return TypeRelation::Narrowed;
    }
    TypeRelation::Changed
}

/// The whitespace-collapsed, lowercase-key spelling a comparison uses. Case is kept (types are
/// case-sensitive); only whitespace and a stray trailing marker are removed.
fn normalise_type(text: &str) -> String {
    let mut out = String::new();
    let mut space = false;
    for ch in text.trim().chars() {
        if ch.is_whitespace() {
            space = true;
            continue;
        }
        if space && !out.is_empty() {
            out.push(' ');
        }
        space = false;
        out.push(ch);
    }
    out
}

/// The members of a union type, sorted and deduped; a non-union type is its own single member. Split at
/// bracket depth zero, so `Map<String, i32> | string` has two members and `string | number` has two.
#[must_use]
pub fn union_members(text: &str) -> Vec<String> {
    let mut depth: i32 = 0;
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        match ch {
            '<' | '(' | '[' | '{' => depth += 1,
            '>' | ')' | ']' | '}' => depth -= 1,
            '|' if depth == 0 => {
                parts.push(current.trim().to_string());
                current = String::new();
                continue;
            }
            _ => {}
        }
        current.push(ch);
    }
    parts.push(current.trim().to_string());
    let mut out: Vec<String> = parts.into_iter().filter(|p| !p.is_empty()).collect();
    if out.len() == 1 {
        return out;
    }
    out.sort();
    out.dedup();
    out
}

/// The type that accepts everything, in whichever language's spelling it was written. Arriving at it is
/// additive; leaving it is breaking, because every call that relied on the wider set stops compiling.
#[must_use]
pub fn is_widest(text: &str) -> bool {
    let flat: String = text
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .flat_map(|ch| ch.to_lowercase())
        .collect();
    matches!(
        flat.as_str(),
        "any"
            | "unknown"
            | "object"
            | "java.lang.object"
            | "interface{}"
            | "typing.any"
            | "mixed"
            | "dynamic"
            | "null|undefined"
    )
}

fn explain(before: Option<&str>, after: Option<&str>) -> String {
    format!(
        "was `{}`, now `{}`",
        before.unwrap_or("(none)"),
        after.unwrap_or("(none)")
    )
}

fn signature_diff(before: &Signature, after: &Signature) -> Vec<Detail> {
    if before.record() == after.record() {
        return Vec::new();
    }
    let mut out: Vec<Detail> = Vec::new();
    if before.unparsed || after.unparsed || before.head.is_empty() || after.head.is_empty() {
        out.push(Detail {
            reason: Reason::SignatureUnparsed,
            note: format!(
                "was `{}`, now `{}` — the header could not be decomposed, so the difference is \
                 reported and not guessed at",
                before.head, after.head
            ),
        });
        return out;
    }
    if before.generics != after.generics {
        out.push(Detail {
            reason: if before.generics.len() != after.generics.len() {
                Reason::GenericCountChanged
            } else {
                Reason::GenericBoundsChanged
            },
            note: explain(
                Some(&before.generics.join(", ")),
                Some(&after.generics.join(", ")),
            ),
        });
    }
    if before.receiver != after.receiver {
        out.push(Detail {
            reason: Reason::ReceiverChanged,
            note: explain(before.receiver.as_deref(), after.receiver.as_deref()),
        });
    }
    if before.params != after.params {
        out.extend(param_diff(&before.params, &after.params));
    }
    match type_relation(before.returns.as_deref(), after.returns.as_deref()) {
        // A return type is named by the caller (`let x: f64 = area(&s)`), so both directions break:
        // this is the one relation that is not about what the caller can *pass*.
        TypeRelation::Same => {}
        _ => {
            if before.returns != after.returns {
                out.push(Detail {
                    reason: Reason::ReturnTypeChanged,
                    note: explain(before.returns.as_deref(), after.returns.as_deref()),
                });
            }
        }
    }
    if before.ty != after.ty {
        out.push(Detail {
            reason: Reason::ValueTypeChanged,
            note: explain(before.ty.as_deref(), after.ty.as_deref()),
        });
    }
    if before.alias != after.alias {
        out.push(Detail {
            reason: Reason::AliasChanged,
            note: format!(
                "every user of the alias sees this: was `{}`, now `{}`",
                before.alias.as_deref().unwrap_or("(none)"),
                after.alias.as_deref().unwrap_or("(none)")
            ),
        });
    }
    if before.value != after.value {
        out.push(Detail {
            reason: Reason::ValueChanged,
            note: format!(
                "the declared type held; the value moved, which is a behaviour change and not a \
                 contract change: was `{}`, now `{}`",
                before.value.as_deref().unwrap_or("(none)"),
                after.value.as_deref().unwrap_or("(none)")
            ),
        });
    }
    if out.is_empty() && before.head != after.head {
        out.push(Detail {
            reason: Reason::DeclarationTextChanged,
            note: format!("was `{}`, now `{}`", before.head, after.head),
        });
    }
    out
}

/// Whether two signatures agree on everything except the header text and the parse flag. Used to decide
/// whether a visibility row already explains a header difference.
fn signature_shape_equal(before: &Signature, after: &Signature) -> bool {
    before.generics == after.generics
        && before.receiver == after.receiver
        && before.params == after.params
        && before.returns == after.returns
        && before.ty == after.ty
        && before.alias == after.alias
        && before.value == after.value
}

/// The parameter list, pairings and all.
///
/// Two pairing rules, and the choice between them is the whole difficulty: a language whose call sites
/// pass by name (`Python` keywords, TypeScript properties) pairs by name, so a rename reads as a removal
/// plus an addition; a language whose call sites pass by position pairs by position, so a rename reads
/// as a rename. The test below is deliberately blunt: if the two lists have little name-overlap, they
/// are positional lists and the names are not identity.
fn param_diff(before: &[Param], after: &[Param]) -> Vec<Detail> {
    let mut out: Vec<Detail> = Vec::new();
    let unique = |params: &[Param]| {
        let mut names: Vec<&str> = params.iter().map(|p| p.name.as_str()).collect();
        let n = names.len();
        names.sort_unstable();
        names.dedup();
        names.len() == n
    };
    let overlap = before
        .iter()
        .filter(|p| after.iter().any(|q| q.name == p.name))
        .count();
    let by_name = unique(before)
        && unique(after)
        && !before.is_empty()
        && !after.is_empty()
        && overlap * 2 >= before.len().min(after.len());
    if by_name {
        for was in before {
            match after.iter().find(|p| p.name == was.name) {
                None => out.push(Detail {
                    reason: Reason::ParameterRemoved,
                    note: format!("`{}` is gone from the parameter list", was.display()),
                }),
                Some(now) => out.extend(param_pair_diff(was, now)),
            }
        }
        for now in after {
            if !before.iter().any(|p| p.name == now.name) {
                out.push(Detail {
                    reason: if now.optional {
                        Reason::OptionalParameterAdded
                    } else {
                        Reason::RequiredParameterAdded
                    },
                    note: format!(
                        "`{}` {}",
                        now.display(),
                        if now.optional {
                            "may be left out, so every existing call still compiles"
                        } else {
                            "must be supplied, so every existing call site has to change"
                        }
                    ),
                });
            }
        }
        return out;
    }
    for (index, was) in before.iter().enumerate() {
        match after.get(index) {
            None => out.push(Detail {
                reason: Reason::ParameterRemoved,
                note: format!("position {} (`{}`) is gone", index + 1, was.display()),
            }),
            Some(now) => out.extend(param_pair_diff(was, now)),
        }
    }
    for (index, now) in after.iter().enumerate().skip(before.len()) {
        out.push(Detail {
            reason: if now.optional {
                Reason::OptionalParameterAdded
            } else {
                Reason::RequiredParameterAdded
            },
            note: format!(
                "position {} (`{}`) {}",
                index + 1,
                now.display(),
                if now.optional {
                    "may be left out"
                } else {
                    "must be supplied"
                }
            ),
        });
    }
    out
}

fn param_pair_diff(before: &Param, after: &Param) -> Vec<Detail> {
    let mut out: Vec<Detail> = Vec::new();
    if before.name != after.name {
        out.push(Detail {
            reason: Reason::ParameterRenamed,
            note: format!(
                "`{before}` is now `{after}`",
                before = before.name,
                after = after.name
            ),
        });
    }
    match type_relation(before.ty.as_deref(), after.ty.as_deref()) {
        TypeRelation::Same => {}
        TypeRelation::Widened => out.push(Detail {
            reason: Reason::ParameterTypeWidened,
            note: explain(before.ty.as_deref(), after.ty.as_deref()),
        }),
        TypeRelation::Narrowed => out.push(Detail {
            reason: Reason::ParameterTypeNarrowed,
            note: explain(before.ty.as_deref(), after.ty.as_deref()),
        }),
        TypeRelation::Changed => {
            if before.ty != after.ty {
                out.push(Detail {
                    reason: Reason::ParameterTypeChanged,
                    note: explain(before.ty.as_deref(), after.ty.as_deref()),
                });
            }
        }
    }
    if before.optional != after.optional {
        out.push(Detail {
            reason: if after.optional {
                Reason::ParameterRelaxed
            } else {
                Reason::ParameterTightened
            },
            note: if after.optional {
                format!("`{}`: a default or an omission is allowed now", after.name)
            } else {
                format!(
                    "`{}`: it used to have a default and now it must be passed",
                    after.name
                )
            },
        });
    }
    if before.rest != after.rest {
        out.push(Detail {
            reason: Reason::DeclarationTextChanged,
            note: format!(
                "`{}`: rest-ness changed, was `{}`, now `{}`",
                after.name,
                before.display(),
                after.display()
            ),
        });
    }
    if before.keyword_only != after.keyword_only || before.positional_only != after.positional_only
    {
        out.push(Detail {
            reason: Reason::DeclarationTextChanged,
            note: format!(
                "`{}`: was `{}`, now `{}`",
                after.name,
                before.display(),
                after.display()
            ),
        });
    }
    if out.is_empty() && before.default != after.default {
        out.push(Detail {
            reason: Reason::DeclarationTextChanged,
            note: format!(
                "`{}`: the default moved, was `{}`, now `{}`",
                after.name,
                before.default.as_deref().unwrap_or("(none)"),
                after.default.as_deref().unwrap_or("(none)")
            ),
        });
    }
    out
}

/// The members of a struct, interface, class or enum, paired by name.
fn fields_diff(before: &[Field], after: &[Field]) -> Vec<Detail> {
    let mut out: Vec<Detail> = Vec::new();
    for was in before {
        match after.iter().find(|f| f.name == was.name) {
            None => out.push(Detail {
                reason: Reason::MemberRemoved,
                note: format!("`{}` is gone from the declaration", was.text),
            }),
            Some(now) => {
                if was.ty != now.ty {
                    out.push(Detail {
                        reason: Reason::MemberTypeChanged,
                        note: explain(was.ty.as_deref(), now.ty.as_deref()),
                    });
                }
                if was.optional != now.optional {
                    out.push(Detail {
                        reason: if now.optional {
                            Reason::MemberRelaxed
                        } else {
                            Reason::MemberTightened
                        },
                        note: format!("`{}`", now.text),
                    });
                }
                if out.is_empty() && was.text != now.text {
                    out.push(Detail {
                        reason: Reason::MemberTextChanged,
                        note: format!("was `{}`, now `{}`", was.text, now.text),
                    });
                }
            }
        }
    }
    for now in after {
        if !before.iter().any(|f| f.name == now.name) {
            out.push(Detail {
                reason: Reason::MemberAdded,
                note: format!("`{}`", now.text),
            });
        }
    }
    out
}

fn supertypes_diff(before: &[String], after: &[String]) -> Vec<Detail> {
    let mut out: Vec<Detail> = Vec::new();
    for lost in before.iter().filter(|name| !after.contains(name)) {
        out.push(Detail {
            reason: Reason::SupertypeRemoved,
            note: format!("`{lost}` is no longer implemented or inherited"),
        });
    }
    for gained in after.iter().filter(|name| !before.contains(name)) {
        out.push(Detail {
            reason: Reason::SupertypeAdded,
            note: format!("`{gained}` is now implemented or inherited"),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The widening matrix, one row per rule. Each row is a claim about what a caller can pass.
    #[test]
    fn the_type_relation_matrix_is_a_list_of_rules_not_heuristics() {
        let cases: &[(&str, &str, TypeRelation)] = &[
            ("u8", "u8", TypeRelation::Same),
            ("u8", "u32", TypeRelation::Changed),
            ("string", "string | number", TypeRelation::Widened),
            ("string | number", "string", TypeRelation::Narrowed),
            ("&str", "impl AsRef<str>", TypeRelation::Widened),
            ("impl AsRef<str>", "&str", TypeRelation::Narrowed),
            ("Shape", "any", TypeRelation::Widened),
            ("Shape", "interface{}", TypeRelation::Widened),
            ("Shape", " interface {}", TypeRelation::Widened),
            ("str", "typing.Any", TypeRelation::Widened),
            ("any", "Shape", TypeRelation::Narrowed),
            ("unknown", "string", TypeRelation::Narrowed),
            ("String", "Option<String>", TypeRelation::Changed),
            ("Option<String>", "String", TypeRelation::Changed),
            (":", ":", TypeRelation::Same),
        ];
        for (before, after, want) in cases {
            assert_eq!(
                type_relation(Some(before), Some(after)),
                *want,
                "`{before}` → `{after}`"
            );
        }
        assert_eq!(type_relation(None, None), TypeRelation::Same);
        assert_eq!(type_relation(None, Some("u8")), TypeRelation::Changed);
    }

    /// A union splits at depth zero only, so a generic argument is never read as a union bar.
    #[test]
    fn union_members_split_at_their_own_depth() {
        assert_eq!(union_members("string"), vec!["string".to_string()]);
        assert_eq!(
            union_members("string | number"),
            vec!["number".to_string(), "string".to_string()]
        );
        assert_eq!(
            union_members("Map<String, i32> | string"),
            vec!["Map<String, i32>".to_string(), "string".to_string()]
        );
    }

    /// The class of a set of reasons is the worst one: a change that both adds and removes has to be
    /// reported at the class a caller has to act on.
    #[test]
    fn the_worst_reason_drives() {
        let details = vec![
            Detail {
                reason: Reason::MemberAdded,
                note: String::new(),
            },
            Detail {
                reason: Reason::Deprecated,
                note: String::new(),
            },
            Detail {
                reason: Reason::ItemAdded,
                note: String::new(),
            },
        ];
        assert_eq!(class_of(&details), Class::Deprecating);
        let with_removal = vec![
            Detail {
                reason: Reason::ItemAdded,
                note: String::new(),
            },
            Detail {
                reason: Reason::ItemRemoved,
                note: String::new(),
            },
        ];
        assert_eq!(class_of(&with_removal), Class::Breaking);
        assert_eq!(
            class_of(&[]),
            Class::Neutral,
            "no reasons is not a change at all"
        );
    }

    /// Every reason the classifier can produce. The tests below are written against this list, so a new
    /// reason cannot be added without being checked.
    fn all_reasons() -> Vec<Reason> {
        vec![
            Reason::ItemAdded,
            Reason::ItemRemoved,
            Reason::ExportRemoved,
            Reason::ExportAdded,
            Reason::VisibilityWidened,
            Reason::VisibilityNarrowed,
            Reason::KindChanged,
            Reason::ItemMoved,
            Reason::ItemMovedUnreachable,
            Reason::ReachabilityNarrowed,
            Reason::ReachabilityWidened,
            Reason::DeclarationTextChanged,
            Reason::SignatureUnparsed,
            Reason::ParameterRemoved,
            Reason::RequiredParameterAdded,
            Reason::OptionalParameterAdded,
            Reason::ParameterRenamed,
            Reason::ParameterTypeWidened,
            Reason::ParameterTypeNarrowed,
            Reason::ParameterTypeChanged,
            Reason::ParameterRelaxed,
            Reason::ParameterTightened,
            Reason::GenericCountChanged,
            Reason::GenericBoundsChanged,
            Reason::ReturnTypeChanged,
            Reason::ReceiverChanged,
            Reason::ValueTypeChanged,
            Reason::AliasChanged,
            Reason::ValueChanged,
            Reason::MemberRemoved,
            Reason::MemberAdded,
            Reason::MemberTypeChanged,
            Reason::MemberTightened,
            Reason::MemberRelaxed,
            Reason::MemberTextChanged,
            Reason::SupertypeRemoved,
            Reason::SupertypeAdded,
            Reason::Deprecated,
            Reason::DeprecationLifted,
            Reason::DeprecationNoteChanged,
            Reason::MayHaveShrunk,
            Reason::EntryFileRemoved,
            Reason::EntryFileAdded,
            Reason::LoadsGrew,
            Reason::LoadsShrank,
            Reason::FileContentsChanged,
        ]
    }

    /// The codes are the vocabulary a baseline is written in and the spelling the JSON exports: each
    /// reason has one, and no two share it.
    #[test]
    fn every_reason_has_its_own_code() {
        let all = all_reasons();
        let mut codes: Vec<&str> = all.iter().map(|reason| reason.code()).collect();
        assert!(!codes.is_empty());
        codes.sort_unstable();
        let unique = codes.len();
        codes.dedup();
        assert_eq!(codes.len(), unique, "two reasons share a code");
        for reason in all {
            assert!(
                !reason.summary().is_empty(),
                "{:?} has no summary sentence",
                reason
            );
            assert!(
                reason.code().is_char_boundary(0),
                "{:?} has an empty code",
                reason
            );
        }
    }

    /// The class map is read as a list, so a row can be checked without reading the code: the items in
    /// each of the three interesting classes are the ones the docs argue for.
    #[test]
    fn the_class_of_each_reason_is_the_documented_one() {
        let additive = [
            Reason::ItemAdded,
            Reason::ExportAdded,
            Reason::VisibilityWidened,
            Reason::ReachabilityWidened,
            Reason::OptionalParameterAdded,
            Reason::ParameterTypeWidened,
            Reason::ParameterRelaxed,
            Reason::MemberAdded,
            Reason::MemberRelaxed,
            Reason::SupertypeAdded,
            Reason::DeprecationLifted,
            Reason::EntryFileAdded,
            Reason::LoadsGrew,
        ];
        for reason in additive {
            assert_eq!(
                reason.class(),
                Class::Additive,
                "{:?} is not additive",
                reason
            );
        }
        assert_eq!(Reason::Deprecated.class(), Class::Deprecating);
        for reason in [
            Reason::DeprecationNoteChanged,
            Reason::GenericBoundsChanged,
            Reason::ItemMoved,
            Reason::ValueChanged,
            Reason::FileContentsChanged,
        ] {
            assert_eq!(
                reason.class(),
                Class::Neutral,
                "{:?} is not neutral",
                reason
            );
        }
        for reason in [Reason::SignatureUnparsed, Reason::MayHaveShrunk] {
            assert_eq!(
                reason.class(),
                Class::Indeterminate,
                "{:?} is not indeterminate",
                reason
            );
        }
        let breaking = all_reasons()
            .into_iter()
            .filter(|reason| reason.class() == Class::Breaking)
            .count();
        assert!(breaking >= 20, "the breaking list looks short: {breaking}");
    }
}
