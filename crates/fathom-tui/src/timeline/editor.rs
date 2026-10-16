//! `fathom_tui::timeline::editor` — the `F` surface: [`fathom_tui::keymap`]'s `filter.edit`, as a
//! chooser you can drive without a mouse.
//!
//! Why a chooser and not a grammar. `F` is one keystroke and the ADR's label for it is "Edit filters
//! (author, area, state, date)" — four dimensions, and a reviewer who has to remember that the area
//! filter wants `area=src/store` while the state one wants `state=unread` has been handed a shell
//! language, not a filter. So the surface is the same mechanism as the palette, which the shell already
//! implements: **a prompt that filters a list of what is actually available**, `<C-n>`/`<C-p>` (or
//! `<Up>`/`<Down>`/`<Tab>`) to walk it, `<Enter>` to choose, `<Esc>` to leave.
//!
//! That choice is deliberate and it buys three things:
//!
//! * **No new keys.** `prompt` already owns the keyboard exclusively (ADR-006 §1.1), so the editor
//!   needs no bindings of its own — nothing to conflict with (K2), nothing that could shadow a reserved
//!   key (K4), and the help overlay describes it correctly on the day it ships.
//! * **Only values that exist.** The candidates are counted out of the loaded rows ([`super::Facets`]),
//!   so a reviewer cannot type a filter for an author this repository has never seen and then wonder
//!   why the rail is empty. Free text is still available where free text is the right tool — a path
//!   fragment, an unfamiliar author, a date — and it is labelled as such.
//! * **A visible current value.** Every row prints what is set *now*, and the state of the filter is
//!   never only in the buffer: `state=unread ✓` is on screen, so a filter you forgot about is a filter
//!   you can see.
//!
//! Two stages: dimensions (what to change) and values (what to change it to). `<Enter>` on a
//! multi-valued dimension (author, kind) toggles and stays, because "agent *and* stahl *and* unread"
//! is built one choice at a time; single-valued dimensions go back to the dimension list.

use fathom_core::time::Timestamp;

use super::episode::AuthorKind;
use super::filter::{Dimension, Facets, Filter, GroupBy};

/// Where the editor is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Stage {
    /// Which dimension to change.
    #[default]
    Dimensions,
    /// Which value to change it to.
    Value(Dimension),
}

/// What choosing a row does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Go to a dimension's value list, or open its text field.
    Dimension(Dimension),
    /// Choose a value. `toggle` means add-or-remove (author, kind) rather than replace.
    Choose {
        /// Which dimension.
        dimension: Dimension,
        /// The value text, empty for "any".
        value: String,
        /// Add/remove rather than replace.
        toggle: bool,
    },
    /// Commit whatever is typed in the field.
    Keep(Dimension),
    /// Clear one dimension.
    Clear(Dimension),
    /// `filter.clear`.
    ClearAll,
    /// Leave the overlay; the rail keeps whatever is applied.
    Close,
}

/// One row of the chooser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The row's text, `label` padded and the current value after it.
    pub label: String,
    /// The right-hand hint: `Enter to choose`, `✓`, `12 episodes`.
    pub hint: String,
    /// What choosing it does.
    pub target: Target,
}

/// What choosing a row caused, so the shell can carry it out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    /// Stay in the chooser (a toggle, or a value list still open).
    Stay,
    /// Back to the dimension list.
    Back,
    /// Close the overlay.
    Close,
}

/// The editor's own state. The text field itself is the shell's prompt buffer — one field, whichever
/// overlay is open — so `<Esc>`/`<C-u>`/`<C-w>` mean the same thing in every prompt in the app.
#[derive(Debug, Default)]
pub struct Editor {
    /// Is the overlay open at all.
    pub open: bool,
    /// Which stage is showing.
    pub stage: Stage,
    /// The row under the cursor, as the shell's `prompt.result_*` keys move it.
    pub row: usize,
    /// The last thing the editor said, kept here so the panel can repeat it under the field.
    pub message: Option<String>,
}

impl Editor {
    /// A closed editor.
    #[must_use]
    pub const fn closed() -> Editor {
        Editor {
            open: false,
            stage: Stage::Dimensions,
            row: 0,
            message: None,
        }
    }

    /// Open on the dimension list.
    pub fn open(&mut self) {
        self.open = true;
        self.stage = Stage::Dimensions;
        self.row = 0;
    }

    /// Close it, forgetting the message but not what it applied.
    pub fn close(&mut self) {
        self.open = false;
        self.message = None;
        self.stage = Stage::Dimensions;
        self.row = 0;
    }

    /// Is a value stage showing?
    #[must_use]
    pub const fn in_value(&self) -> bool {
        matches!(self.stage, Stage::Value(_))
    }

    /// The rows for the current stage, filtered by what has been typed.
    #[must_use]
    pub fn rows(&self, filter: &Filter, facets: &Facets, query: &str) -> Vec<Row> {
        let query = query.trim().to_lowercase();
        let keep = |label: &str| query.is_empty() || label.to_lowercase().contains(&query);
        let mut rows: Vec<Row> = Vec::new();
        match self.stage {
            Stage::Dimensions => {
                for dimension in Dimension::ALL {
                    let value = filter.value_of(dimension);
                    let label = format!("{} · {value}", dimension.label());
                    if !keep(&label) {
                        continue;
                    }
                    let set = !matches!(value.as_str(), "any" | "start…now" | "day")
                        && value != GroupBy::Day.as_str();
                    rows.push(Row {
                        label,
                        hint: if dimension.is_text() {
                            "type, or pick below".to_owned()
                        } else {
                            "Enter to choose".to_owned()
                        },
                        target: Target::Dimension(dimension),
                    });
                    let _ = set;
                }
                if keep("clear all filters") {
                    rows.push(Row {
                        label: "clear all filters".to_owned(),
                        hint: format!("{} active", filter.active()),
                        target: Target::ClearAll,
                    });
                }
                if keep("close") {
                    rows.push(Row {
                        label: "close — the rail keeps what is set".to_owned(),
                        hint: "<Esc>".to_owned(),
                        target: Target::Close,
                    });
                }
            }
            Stage::Value(dimension) => {
                if dimension.is_text() {
                    rows.push(Row {
                        label: format!("{} = «{}»", dimension.label(), self.placeholder(&query)),
                        hint: "Enter commits what you typed".to_owned(),
                        target: Target::Keep(dimension),
                    });
                    rows.push(Row {
                        label: format!("{} = any", dimension.label()),
                        hint: "clears this one".to_owned(),
                        target: Target::Clear(dimension),
                    });
                } else {
                    rows.push(Row {
                        label: format!("{} = any", dimension.label()),
                        hint: "clears this one".to_owned(),
                        target: Target::Clear(dimension),
                    });
                }
                let toggle = matches!(dimension, Dimension::Author | Dimension::Kind);
                for (value, count) in with_always(dimension, facets) {
                    if !keep(&value) {
                        continue;
                    }
                    let current = match dimension {
                        Dimension::Author => filter.authors.contains(&value),
                        Dimension::Kind => filter
                            .kinds
                            .iter()
                            .any(|have| have.as_str() == value || have.badge() == value),
                        Dimension::State => {
                            filter.state.is_some_and(|state| state.to_string() == value)
                        }
                        Dimension::Group => filter.group.as_str() == value,
                        Dimension::Area => filter.area.as_deref() == Some(value.as_str()),
                        Dimension::Since | Dimension::Until => false,
                    };
                    let mut hint = if toggle {
                        "Enter toggles".to_owned()
                    } else {
                        "Enter sets".to_owned()
                    };
                    if count > 0 {
                        hint = format!("{count} eps · {hint}");
                    }
                    rows.push(Row {
                        label: value.clone(),
                        hint: if current { format!("✓ {hint}") } else { hint },
                        target: Target::Choose {
                            dimension,
                            value,
                            toggle,
                        },
                    });
                }
                if rows.iter().all(|row| row.target != Target::Close) {
                    rows.push(Row {
                        label: "back to the filters".to_owned(),
                        hint: "<Esc> closes the whole overlay".to_owned(),
                        target: Target::Close,
                    });
                }
            }
        }
        rows
    }

    /// The field's prompt text when nothing is typed yet.
    #[must_use]
    fn placeholder(&self, query: &str) -> String {
        if query.is_empty() {
            "type it".to_owned()
        } else {
            query.to_owned()
        }
    }

    /// The panel's lines: the field, then the rows, then the footer that names the keys.
    #[must_use]
    pub fn panel(
        &self,
        filter: &Filter,
        rows: &[Row],
        cursor: usize,
        buffer: &str,
        width: usize,
    ) -> Vec<String> {
        let title = match self.stage {
            Stage::Dimensions => "filters — which one?",
            Stage::Value(dimension) => match dimension {
                Dimension::Author => "filters — which author?",
                Dimension::Kind => "filters — agent, human or unknown?",
                Dimension::Area => "filters — which path fragment?",
                Dimension::State => "filters — which review state?",
                Dimension::Since => "filters — from which day?",
                Dimension::Until => "filters — to which day?",
                Dimension::Group => "filters — group by what?",
            },
        };
        let mut out = vec![format!("F{}▏  {title}", buffer)];
        if let Some(message) = &self.message {
            out.push(format!("   {message}"));
        }
        for (index, row) in rows.iter().enumerate() {
            let mark = if index == cursor { "▸" } else { " " };
            let room = width.saturating_sub(row.hint.len() + 4).max(8);
            let label = if row.label.chars().count() > room {
                let mut cut: String = row.label.chars().take(room.saturating_sub(1)).collect();
                cut.push('…');
                cut
            } else {
                row.label.clone()
            };
            out.push(format!("{mark} {label:<room$} {}", row.hint));
        }
        // Short enough to survive the narrowest panel it can be drawn in: a footer clipped to `…`
        // stops teaching the keys, which is the only job it has.
        let _ = &filter;
        out.push(format!(
            "{} of {} · <C-n>/<C-p> move · <Enter> choose · <Esc> close",
            (cursor + 1).min(rows.len()),
            rows.len()
        ));
        out
    }

    /// Apply the chosen row, returning the filter it implies, the sentence for the toast, and what the
    /// chooser should do next.
    ///
    /// It hands back a whole [`Filter`] rather than mutating one in place: the rail owns the filter and
    /// has to know when it changed, because that is what rebuilds the order and follows the cursor.
    /// A bad typed value is printed back and changes nothing — a filter that half-applied a typo would
    /// be a filter the reviewer cannot trust, which is worse than a rejected keystroke.
    pub fn apply(
        &mut self,
        row: &Row,
        filter: &Filter,
        buffer: &str,
        now: Timestamp,
    ) -> (Option<Filter>, String, Next) {
        match &row.target {
            Target::Dimension(dimension) => {
                self.stage = Stage::Value(*dimension);
                self.row = 0;
                self.message = Some(format!(
                    "{} is currently {}",
                    dimension.label(),
                    filter.value_of(*dimension)
                ));
                (None, "choose a value".to_owned(), Next::Stay)
            }
            Target::Choose {
                dimension,
                value,
                toggle,
            } => {
                let mut next = filter.clone();
                if *toggle {
                    next.toggle(*dimension, value);
                } else if let Err(why) = next.set(*dimension, value, now) {
                    self.message = Some(why.clone());
                    return (
                        None,
                        format!("`{value}` is not a value for {}", dimension.label()),
                        Next::Stay,
                    );
                }
                let said = describe(*dimension, &next);
                if *toggle {
                    self.message = Some(said.clone());
                    return (Some(next), said, Next::Stay);
                }
                self.stage = Stage::Dimensions;
                self.row = 0;
                self.message = Some(said.clone());
                (Some(next), said, Next::Back)
            }
            Target::Keep(dimension) => {
                let mut next = filter.clone();
                match next.set(*dimension, buffer.trim(), now) {
                    Ok(said) => {
                        self.stage = Stage::Dimensions;
                        self.row = 0;
                        self.message = Some(said.clone());
                        (Some(next), said, Next::Back)
                    }
                    Err(why) => {
                        self.message = Some(why.clone());
                        (None, why, Next::Stay)
                    }
                }
            }
            Target::Clear(dimension) => {
                let mut next = filter.clone();
                match next.set(*dimension, "any", now) {
                    Ok(said) => {
                        self.stage = Stage::Dimensions;
                        self.row = 0;
                        self.message = Some(said.clone());
                        (Some(next), said, Next::Back)
                    }
                    Err(why) => (None, why, Next::Stay),
                }
            }
            Target::ClearAll => {
                let mut next = filter.clone();
                let was = next.active();
                next.clear();
                self.stage = Stage::Dimensions;
                self.row = 0;
                self.message = Some("all filters cleared".to_owned());
                (
                    Some(next),
                    format!("{was} filter(s) cleared — every loaded episode is back"),
                    Next::Back,
                )
            }
            Target::Close => (
                None,
                "filters closed — the rail kept whatever is set".to_owned(),
                Next::Close,
            ),
        }
    }

    /// Move the chooser's cursor, clamped.
    pub fn move_to(&mut self, row: usize, len: usize) {
        self.row = if len == 0 { 0 } else { row.min(len - 1) };
    }
}

/// What a dimension now holds, in words.
fn describe(dimension: Dimension, filter: &Filter) -> String {
    format!("{} = {}", dimension.label(), filter.value_of(dimension))
}

/// A facet list with the always-available values merged in, so a dimension's chooser is never empty for
/// a reason the reviewer can see.
#[must_use]
pub fn with_always(dimension: Dimension, facets: &Facets) -> Vec<(String, usize)> {
    let mut rows = facets.values(dimension);
    match dimension {
        Dimension::Kind => {
            for kind in AuthorKind::ALL {
                if !rows.iter().any(|(value, _)| value == kind.as_str()) {
                    rows.push((kind.as_str().to_owned(), 0));
                }
            }
        }
        Dimension::State => {
            for state in super::episode::ReviewState::ALL {
                if !rows.iter().any(|(value, _)| value == state.as_str()) {
                    rows.push((state.as_str().to_owned(), 0));
                }
            }
        }
        Dimension::Group => {
            for group in GroupBy::ALL {
                if !rows.iter().any(|(value, _)| value == group.as_str()) {
                    rows.push((group.as_str().to_owned(), 0));
                }
            }
        }
        _ => {}
    }
    rows
}
