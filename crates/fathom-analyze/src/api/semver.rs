//! The semver recommendation, and the one change that drove it.
//!
//! "Emit a semver recommendation for the affected packages" is easy to state and easy to do badly,
//! because the bad version — "there is a breaking change, so major" — tells a reviewer nothing they did
//! not already believe. So a [`Recommendation`] here is a *sentence with a witness in it*: the bump, the
//! specific change that decided it, that change's location, and what would have made the bump smaller.
//!
//! ```text
//! major  fathom-core
//!        driven by Shape::area, lib/geo.rs:14 — return type changed — was `f64`, now `Option<f64>`
//!        (additive: 2, deprecating: 0, indeterminate: 0)
//! ```
//!
//! The precedence is [`Bump::rank`]'s: **major** for any breaking change; **indeterminate** when the
//! surface moved but this reader could not classify the direction (a synthetic scope that may have
//! shrunk, an unread signature) — which is deliberately *not* folded into either "major" (an
//! accusation with no evidence) or "minor" (a pass with no evidence); **minor** for additive or
//! deprecating changes; **no change** when nothing landed on the surface. A package named in
//! `[api] private` is reported with its changes and without a version claim, because recommending a
//! release for a crate that ships to nobody is noise that will be ignored.
//!
//! The "what would have made it smaller" line is not decoration. A breaking change that a default
//! value would have made additive is a change the author may want to reconsider while it is still in
//! review, and it is the difference between a gate that says no and a gate that teaches.

use crate::api::classify::{Class, Detail, Reason};
use serde::Serialize;

/// The counts of a change set, by class.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    /// Changes that can break a caller.
    pub breaking: usize,
    /// Changes that only add.
    pub additive: usize,
    /// Changes that mark something deprecated.
    pub deprecating: usize,
    /// Changes this reader could not classify. Not a pass; not an accusation.
    pub indeterminate: usize,
    /// Changes that touched the declaration and not the contract.
    pub neutral: usize,
}

impl Counts {
    /// Fold one class in.
    pub fn add(&mut self, class: Class) {
        match class {
            Class::Breaking => self.breaking += 1,
            Class::Additive => self.additive += 1,
            Class::Deprecating => self.deprecating += 1,
            Class::Indeterminate => self.indeterminate += 1,
            Class::Neutral => self.neutral += 1,
        }
    }

    /// Whether no change of any kind landed on the surface.
    #[must_use]
    pub fn empty(&self) -> bool {
        self.breaking + self.additive + self.deprecating + self.indeterminate == 0
    }

    /// The change-set total.
    #[must_use]
    pub fn total(&self) -> usize {
        self.breaking + self.additive + self.deprecating + self.indeterminate + self.neutral
    }

    /// The compact form a report prints beside a verdict.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "breaking {}, additive {}, deprecating {}, indeterminate {}, neutral {}",
            self.breaking, self.additive, self.deprecating, self.indeterminate, self.neutral
        )
    }
}

/// The recommended version bump for one package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Bump {
    /// Nothing landed on the public surface. The fast path's answer, and the common one.
    NoChange,
    /// Reserved: nothing in this module can produce it, and saying so is better than inventing a rule
    /// for "documentation-only", which a source-level reader cannot distinguish from no change.
    Patch,
    /// Additive or deprecating: a new name, a widened parameter, a deprecation note.
    Minor,
    /// A caller that compiled before may not compile now.
    Major,
    /// The surface moved and the direction could not be determined. Blocks a "no change" claim and
    /// refuses to make an accusation.
    Indeterminate,
    /// The package is named in `[api] private`: changes are reported, no version is proposed.
    Internal,
}

impl Bump {
    /// The stable spelling, which is also the JSON spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Bump::NoChange => "no_change",
            Bump::Patch => "patch",
            Bump::Minor => "minor",
            Bump::Major => "major",
            Bump::Indeterminate => "indeterminate",
            Bump::Internal => "internal",
        }
    }

    /// The precedence a package's bump is taken from.
    pub fn rank(self) -> u8 {
        match self {
            Bump::NoChange => 0,
            Bump::Patch => 1,
            Bump::Minor => 2,
            Bump::Indeterminate => 3,
            Bump::Internal => 3,
            Bump::Major => 4,
        }
    }

    /// The sentence a report leads with.
    pub fn sentence(self) -> &'static str {
        match self {
            Bump::NoChange => "no public API change",
            Bump::Patch => "patch",
            Bump::Minor => "minor",
            Bump::Major => "major",
            Bump::Indeterminate => "cannot recommend a bump",
            Bump::Internal => "internal package, no bump recommended",
        }
    }
}

/// The change that decided a bump, with its location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Driver {
    /// The item's display name.
    pub item: String,
    /// The `file:line` the change was found at — the ticket's "each API change links to the file:line",
    /// repeated here so the recommendation is self-contained.
    pub location: String,
    /// The reason that drove the bump.
    pub reason: Reason,
    /// The reason's specifics.
    pub note: String,
    /// The episode that made the change, when the caller identified one.
    pub episode: Option<String>,
}

/// One package's recommendation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Recommendation {
    /// The package.
    pub package: String,
    /// The bump.
    pub bump: Bump,
    /// The counts it rests on.
    pub counts: Counts,
    /// The change that drove it, when there is one.
    pub driver: Option<Driver>,
    /// The sentence, assembled so a surface can print one line and be done.
    pub why: String,
    /// What would have made the bump smaller, when anything would have. Empty when the change is what
    /// it is (a removal cannot be made additive by any spelling of itself — only by not doing it, or by
    /// deprecating first).
    pub smaller: Option<String>,
    /// The caveat that limits the claim: an unreadable file, a truncated item list, an indeterminate
    /// change. A recommendation without its caveats is a claim to completeness this module does not
    /// have.
    pub caveat: Option<String>,
}

impl Recommendation {
    /// Decide one package's bump from its changes and their driver.
    #[must_use]
    pub fn build(
        package: &str,
        counts: Counts,
        driver: Option<Driver>,
        private: bool,
        caveat: Option<String>,
    ) -> Recommendation {
        let bump = if private {
            Bump::Internal
        } else if counts.breaking > 0 {
            Bump::Major
        } else if counts.indeterminate > 0 {
            Bump::Indeterminate
        } else if counts.deprecating > 0 || counts.additive > 0 {
            Bump::Minor
        } else {
            Bump::NoChange
        };
        let smaller = driver
            .as_ref()
            .and_then(|driver| smaller_than(driver.reason));
        let mut why = match bump {
            Bump::NoChange => format!("{package}: no public API change ({})", counts.summary()),
            Bump::Internal => format!(
                "{package}: {} — the package is named in `[api] private`, so no version is \
                 recommended for it",
                counts.summary()
            ),
            other => {
                let lead = match &driver {
                    Some(driver) => format!(
                        "{}: {} — driven by {}, {} — {} {}",
                        package,
                        other.sentence(),
                        driver.item,
                        driver.location,
                        driver.reason.code(),
                        driver.note
                    ),
                    None => format!("{}: {}", package, other.sentence()),
                };
                format!("{lead} ({})", counts.summary())
            }
        };
        if let Some(smaller) = &smaller {
            why.push_str(&format!("; would have been smaller if: {smaller}"));
        }
        if let Some(caveat) = &caveat {
            why.push_str(&format!("; note: {caveat}"));
        }
        Recommendation {
            package: package.to_string(),
            bump,
            counts,
            driver,
            why,
            smaller,
            caveat,
        }
    }
}

/// The advice column: what spelling of the same intent would have cost a smaller bump. Every row is a
/// concrete alternative, not a lecture, and `None` means there is no such spelling.
#[must_use]
pub fn smaller_than(reason: Reason) -> Option<String> {
    Some(match reason {
        Reason::RequiredParameterAdded => "give the new parameter a default (Python, TypeScript) or an \
                                           `Option<T>` (Rust) and callers that do not pass it still \
                                           compile, which is a minor"
            .to_string(),
        Reason::ParameterTypeNarrowed => "accept the wider type (`impl Trait` in Rust, a union in \
                                         TypeScript) and the old calls keep compiling, which is a \
                                         minor"
            .to_string(),
        Reason::ParameterTypeChanged | Reason::ParameterRenamed => "add the new spelling and keep the \
                                                                   old one (an overload, a second \
                                                                   function, a deprecated alias), \
                                                                   which is a minor"
            .to_string(),
        Reason::ItemRemoved | Reason::ExportRemoved | Reason::VisibilityNarrowed => "deprecate it \
                                                                                    this release and \
                                                                                    remove it in the \
                                                                                    next, which is a \
                                                                                    minor now and a \
                                                                                    major the reviewer \
                                                                                    can plan for"
            .to_string(),
        Reason::MemberRemoved => "keep the field and deprecate it: a struct that gains an accessor for \
                                  the old field keeps every caller compiling"
            .to_string(),
        Reason::MemberTightened => "keep `?` or give the member a default, which is a minor"
            .to_string(),
        Reason::ParameterTightened => "keep a default for the parameter, which is a minor"
            .to_string(),
        Reason::ReturnTypeChanged => "return the old type and put the new information behind a second, \
                                      additive function, or deprecate the old one first"
            .to_string(),
        Reason::ReceiverChanged => "keep the shared-receiver method and add the mutable one, which is a \
                                    minor"
            .to_string(),
        Reason::AliasChanged | Reason::MemberTypeChanged | Reason::ValueTypeChanged => {
            "introduce the new type under a new name and alias the old one, then deprecate"
            .to_string()
        }
        Reason::SupertypeRemoved => "keep the implementation and deprecate it; dropping a trait a \
                                     caller may rely on has no additive spelling"
            .to_string(),
        _ => return None,
    })
}

/// Pick the driver out of a change's reasons: the highest-ranking one, since that is the one that
/// decided the class.
#[must_use]
pub fn driver_detail(reasons: &[Detail]) -> Option<&Detail> {
    reasons
        .iter()
        .max_by_key(|detail| (detail.reason.class().rank(), detail.reason.code()))
}

/// The detail that an indeterminate change deserves, said once rather than per package.
#[must_use]
pub fn indeterminate_sentence(count: usize) -> String {
    format!(
        "{count} change(s) could not be classified — a signature this reader could not parse, or a \
         symbol that vanished from a scope only the running program can enumerate. They are not \
         counted as breaking, and they are not counted as clean: a bump cannot be recommended over \
         them."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(items: &[Class]) -> Counts {
        let mut counts = Counts::default();
        for class in items {
            counts.add(*class);
        }
        counts
    }

    fn driver(reason: Reason) -> Driver {
        Driver {
            item: "f".to_string(),
            location: "src/lib.rs:7".to_string(),
            reason,
            note: "a note".to_string(),
            episode: None,
        }
    }

    /// The bump is the worst class present, in the order the docs argue for: breaking outranks
    /// everything, and "cannot say" outranks "additive" because an unprovable answer is not a minor.
    #[test]
    fn the_bump_is_the_worst_class_present() {
        let cases: &[(&[Class], Option<Reason>, Bump)] = &[
            (&[], None, Bump::NoChange),
            (&[Class::Neutral], None, Bump::NoChange),
            (&[Class::Additive], Some(Reason::ItemAdded), Bump::Minor),
            (&[Class::Deprecating], Some(Reason::Deprecated), Bump::Minor),
            (&[Class::Breaking], Some(Reason::ItemRemoved), Bump::Major),
            (
                &[Class::Additive, Class::Breaking],
                Some(Reason::ItemRemoved),
                Bump::Major,
            ),
            (
                &[Class::Indeterminate],
                Some(Reason::MayHaveShrunk),
                Bump::Indeterminate,
            ),
            (
                &[Class::Breaking, Class::Indeterminate],
                Some(Reason::ItemRemoved),
                Bump::Major,
            ),
        ];
        for (classes, reason, want) in cases {
            let got = Recommendation::build("p", counts(classes), reason.map(driver), false, None);
            assert_eq!(got.bump, *want, "{classes:?} → {:?}", got.bump);
        }
        // A private package gets no recommendation whatever the diff says.
        let private = Recommendation::build(
            "p",
            counts(&[Class::Breaking]),
            Some(driver(Reason::ItemRemoved)),
            true,
            None,
        );
        assert_eq!(private.bump, Bump::Internal);
    }

    /// The sentence has to name the driver, with its location and its code, and repeat the counts. This
    /// is the acceptance criterion "the semver recommendation explains which specific change drove it",
    /// as an assertion rather than an intention.
    #[test]
    fn the_sentence_names_the_change_that_drove_it() {
        let recommendation = Recommendation::build(
            "shapes",
            counts(&[Class::Breaking, Class::Additive]),
            Some(driver(Reason::RequiredParameterAdded)),
            false,
            None,
        );
        assert_eq!(recommendation.bump, Bump::Major);
        assert!(
            recommendation.why.contains("required_parameter_added"),
            "{}",
            recommendation.why
        );
        assert!(
            recommendation.why.contains("src/lib.rs:7"),
            "{}",
            recommendation.why
        );
        assert!(
            recommendation.why.contains("driven by f"),
            "{}",
            recommendation.why
        );
        assert!(
            recommendation.why.contains("would have been smaller if"),
            "a required parameter has a cheaper spelling and the sentence should say so: {}",
            recommendation.why
        );
        assert!(recommendation.smaller.is_some());
        // A removal has no cheaper spelling: it is only avoidable by not doing it, and the sentence must
        // not invent advice that does not exist.
        let removal = Recommendation::build(
            "shapes",
            counts(&[Class::Breaking]),
            Some(driver(Reason::ItemRemoved)),
            false,
            Some("1 file could not be read".to_string()),
        );
        assert!(removal.why.contains("note: 1 file could not be read"));
        assert_eq!(removal.bump, Bump::Major);
    }

    /// Every breaking reason a fixture can produce either has advice or has it on purpose; the reason a
    /// caller is told about must be the reason the advice column knows about.
    #[test]
    fn the_advice_column_covers_the_reasons_it_cites() {
        for reason in [
            Reason::RequiredParameterAdded,
            Reason::ParameterTypeNarrowed,
            Reason::ParameterTypeChanged,
            Reason::ParameterRenamed,
            Reason::ItemRemoved,
            Reason::ExportRemoved,
            Reason::VisibilityNarrowed,
            Reason::MemberRemoved,
            Reason::MemberTightened,
            Reason::ParameterTightened,
            Reason::ReturnTypeChanged,
            Reason::ReceiverChanged,
            Reason::AliasChanged,
            Reason::MemberTypeChanged,
            Reason::ValueTypeChanged,
            Reason::SupertypeRemoved,
        ] {
            assert!(
                smaller_than(reason).is_some(),
                "{reason:?} is cited as a driver and has no advice"
            );
        }
        assert!(smaller_than(Reason::ItemAdded).is_none());
    }

    /// The driver is the worst-class reason in the change, and the details are searched in the order
    /// they were written, so the first worst one wins — deterministically.
    #[test]
    fn the_driver_detail_is_the_worst_reason_of_the_set() {
        let details = vec![
            Detail {
                reason: Reason::ItemAdded,
                note: String::new(),
            },
            Detail {
                reason: Reason::MemberTightened,
                note: String::new(),
            },
            Detail {
                reason: Reason::MemberAdded,
                note: String::new(),
            },
        ];
        assert_eq!(
            driver_detail(&details).map(|d| d.reason),
            Some(Reason::MemberTightened)
        );
        assert_eq!(driver_detail(&[]).map(|d| d.reason), None);
    }
}
