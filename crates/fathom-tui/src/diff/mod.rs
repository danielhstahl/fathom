//! `fathom_tui::diff` — the diff viewer: the file tree, the hunk pane, and the three ways out.
//!
//! This is `e1.7`'s screen: the one a reviewer lives in. The shell owns the frame, the focus ring and
//! the keystrokes (ADR-006 §1, §5); this module owns what the two `diff` panes mean, and it is
//! deliberately a *viewer* — it is handed patch bytes through [`source::PatchSource`] and never opens a
//! repository, because `fathom-tui` has no store handle and no ingest edge (ADR-002 §5.2). `e1.8`'s
//! lazy materialisation implements that trait against the index; nothing in this build hands it a
//! patch yet, so the golden tests — and every review of this pane today — read the empty state.
//!
//! Three properties the rest of the module is organised around:
//!
//! * **Bounded.** Everything is read through [`lines`] in [`budget::Budget::chunk_bytes`] pieces and
//!   everything retained is counted — [`scan::Scan::retained_bytes`] for the index,
//!   [`budget::Budget::window_worst_bytes`] for a pane. The 50 000-line criterion is an assertion
//!   against those two numbers, not a vibe.
//! * **Honest about what it is not showing.** A binary file, a mode-only change, a file over the size
//!   cap, a diff that is not available offline, a window the budget cut short, a pair the whitespace
//!   rule hid: all of them are rows and footer lines with reasons, never silence (ADR-001 §7.2).
//! * **One arithmetic.** Hunk row counts, hunk anchors and drawn rows all come from [`plan`], so a jump
//!   lands where the drawing is.

mod budget;
mod doc;
mod handoff;
mod highlight;
mod lines;
mod materialize;
mod model;
mod plan;
mod scan;
mod source;
mod tree;
mod word;

// The surface, in one place (ADR-002 §8: a crate's modules are declared in `lib.rs`, and a subsystem
// re-exports what it wants the shell to see). Tests and `fathom-cli` build against these names, so a
// rename inside the subsystem is not a rename outside it.
pub use budget::Budget;
pub use doc::{Doc, Hunks, Live, Plan, Request, build_from, decode, height_of};
pub use handoff::{Command, Copy, Editor, Env, Handoff, Os, copy_plan, editor_plan};
pub use materialize::{FromCache, Key, Materializer, Warm};
pub use model::{
    FileEntry, FileKind, Hunk as HunkRecord, Layout, Row, SPLIT_MIN_COLS, Shape, Side, Sign, slot,
};
pub use scan::Scan;
pub use source::{BytesSource, NoSource, PatchReader, PatchSource, Unavailable};
pub use tree::{Group, Target as TreeTarget, Tree, TreeRow, TreeShape};

use std::cell::RefCell;
use std::fmt;
use std::sync::Arc;

use crate::event::Bus;
use crate::style::{Face, StyledLine};

/// Which way `[`/`]` move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hunk {
    /// The previous hunk's header row.
    Previous,
    /// The next hunk's header row.
    Next,
}

/// The diff view's own state: what is loaded, how the pane is shaped, and what the reviewer has asked
/// for that the shell has to carry out (a copy, an editor).
pub struct DiffView {
    /// The ceilings.
    pub budget: Budget,
    /// The decode engine: the bounded cache, the bounded queue, the sources, and the workers if
    /// anyone started any (`e1.8`).
    materializer: Materializer,
    /// The origin of the patch on screen — the name its source gave, which is also its cache lane.
    origin: Option<String>,
    /// The index built from it. An `Arc` so a tree row can point into it without a borrow.
    index: Option<Arc<Scan>>,
    /// Which load the index came from, so the tree cache can tell a re-scan from a repeat.
    scan_id: u64,
    /// The tree's structure, cached per `(load, noise)`: the O(files) pass happens once, not once per
    /// pane per frame (`e1.8`).
    tree_cache: RefCell<Option<(u64, bool, Arc<TreeShape>)>>,
    /// Why there is no index, in the source's own words.
    pub unavailable: Option<String>,
    /// Unified or split.
    pub layout: Layout,
    /// Hide whitespace-only differences.
    pub ignore_ws: bool,
    /// Emphasise what changed inside a changed line.
    pub word_diff: bool,
    /// Show the files the noise rule hides.
    pub show_noise: bool,
    /// The file the pane is reading.
    pub file: Option<usize>,
    /// The hunk the pane was last showing, for `Y`'s "copy this hunk".
    pub hunk: Option<usize>,
    /// Work the loop owes us outside a frame: run the clipboard or `$EDITOR` with the terminal handed
    /// back, or write an OSC 52 escape. One at a time, because a queue of editors is a queue of
    /// surprises.
    pub handoff: Option<Handoff>,
    /// The environment the copy and editor plans read: variables and `PATH`, supplied by the host,
    /// never read here (ADR-002 §3). Empty until a host installs one, which is a stated answer: the
    /// plan falls through to OSC 52 and says so.
    pub env: Env<'static>,
    /// The OS the plans are written for. A parameter so one test suite covers three platforms; the
    /// default is the build's own.
    pub os: handoff::Os,
    /// The last thing the view decided, for a toast that wants to say what happened.
    pub notice: Option<String>,
}

impl Default for DiffView {
    fn default() -> Self {
        DiffView {
            budget: Budget::DEFAULT,
            materializer: Materializer::new(Budget::DEFAULT),
            origin: None,
            index: None,
            scan_id: 0,
            tree_cache: RefCell::new(None),
            unavailable: None,
            layout: Layout::Unified,
            ignore_ws: false,
            word_diff: false,
            show_noise: false,
            file: None,
            hunk: None,
            handoff: None,
            env: Env::empty(),
            os: handoff::Os::current(),
            notice: None,
        }
    }
}

impl fmt::Debug for DiffView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DiffView")
            .field("origin", &self.origin)
            .field(
                "files",
                &self.index.as_ref().map_or(0, |scan| scan.files.len()),
            )
            .field("layout", &self.layout)
            .field("ignore_ws", &self.ignore_ws)
            .field("word_diff", &self.word_diff)
            .field("show_noise", &self.show_noise)
            .field("file", &self.file)
            .field("decode", &self.materializer.stats())
            .finish()
    }
}

impl DiffView {
    /// A view with nothing loaded and the shipped budgets.
    #[must_use]
    pub fn new() -> Self {
        DiffView {
            budget: Budget::DEFAULT,
            layout: Layout::Unified,
            ..DiffView::default()
        }
    }

    /// A view with a different budget — how a test proves a cap is a cap.
    #[must_use]
    pub fn with_budget(budget: Budget) -> Self {
        DiffView {
            materializer: Materializer::new(budget),
            budget,
            ..DiffView::new()
        }
    }

    /// The decode engine, for a caller that wants its counters or wants to drive it by hand.
    #[must_use]
    pub fn materializer(&self) -> &Materializer {
        &self.materializer
    }

    /// Start the decode workers and switch the hunks pane to the threaded policy: from here a cache
    /// miss is a stated placeholder, not a read on the UI thread.
    pub fn spawn_workers(&self, bus: Arc<Bus>) {
        self.materializer.spawn(bus, 2);
    }

    /// Serve hunks from the decode cache without starting workers: the queue is this thread's to
    /// drain, which is how a test inspects the frames a background decode makes on its way to being
    /// an answer.
    pub fn serve_from_cache(&self) {
        self.materializer.serve_from_cache();
    }

    /// Run the next queued decode on this thread — the deterministic path a test drives instead of
    /// starting workers.
    #[must_use]
    pub fn run_one_decode(&self) -> bool {
        self.materializer.run_one()
    }

    /// Run every queued decode on this thread.
    pub fn drain_decodes(&self) -> usize {
        self.materializer.drain()
    }

    /// Offer a neighbour episode — the next or the previous one — to warm in the background: its patch
    /// is scanned and its first hunks decoded before the reviewer moves to it.
    ///
    /// The caller owns the bytes; this only says where they are. A refusal comes back as
    /// [`Unavailable`] so the shell can print it, because "the next episode's diff is not local" is a
    /// fact about the next episode and not about this pane.
    pub fn prewarm(&self, slot: &str, source: Box<dyn PatchSource>) -> Result<String, Unavailable> {
        self.materializer.prewarm(slot, source, None)
    }

    /// What a warm-up has to say about one origin, if it was ever offered one.
    #[must_use]
    pub fn warm_words(&self, origin: &str) -> Option<String> {
        self.materializer.warm_words(origin)
    }

    /// Take a source and build the index. The scan is a pass over the bytes and nothing else: no line
    /// text is kept.
    ///
    /// When this source was offered as a neighbour a moment earlier, the warm-up's index is adopted
    /// rather than rebuilt and its decoded hunks are kept — that is the whole reason pre-warming is
    /// worth a thread.
    pub fn load(&mut self, source: Box<dyn PatchSource>) -> Result<(), Unavailable> {
        self.materializer.set_budget(self.budget);
        let adopting = self.materializer.claim_prewarm(&source.origin());
        let origin = if adopting {
            self.materializer.adopt(source)?
        } else {
            self.materializer.install(source)?
        };
        let scan = match self.materializer.take_warm(&origin) {
            Some(scan) => scan,
            None => {
                let mut reader = self.materializer.open(&origin)?;
                scan::scan(&origin, reader.as_mut(), &self.budget)
            }
        };
        self.index = Some(Arc::new(scan));
        self.scan_id += 1;
        self.origin = Some(origin);
        self.file = None;
        self.hunk = None;
        self.unavailable = None;
        Ok(())
    }

    /// Read the same source again — after a `fathom index`, or because the working tree moved.
    ///
    /// A failure here leaves nothing loaded: the source that failed is gone with the error, and the
    /// pane prints the reason rather than a stale index pretending to be current.
    ///
    /// Every decoded hunk for these bytes is dropped first: a reload says the bytes moved, and an
    /// offset into the old bytes is the one thing a viewer must not draw.
    pub fn reload(&mut self) -> Result<(), Unavailable> {
        let Some(origin) = self.origin.clone() else {
            return Err(Unavailable::new(
                "no patch is loaded, so there is nothing to reload",
            ));
        };
        // Where the reviewer was reading, by path: a file index means nothing across two scans, and
        // dropping them back to the first file after every `<C-r>` is its own small rudeness.
        let was = self
            .file
            .and_then(|index| self.file_at(index))
            .map(|entry| entry.path.clone());
        self.materializer.invalidate(&origin);
        let mut reader = self.materializer.open(&origin)?;
        self.index = Some(Arc::new(scan::scan(&origin, reader.as_mut(), &self.budget)));
        self.scan_id += 1;
        self.hunk = None;
        self.unavailable = None;
        if let Some(path) = was
            && let Some(index) = self
                .index
                .as_ref()
                .and_then(|scan| scan.files.iter().position(|file| file.path == path))
        {
            self.file = Some(index);
        }
        Ok(())
    }

    /// Forget everything, with the reason a pane should print.
    pub fn clear(&mut self, why: impl Into<String>) {
        self.index = None;
        self.scan_id += 1;
        self.origin = None;
        self.file = None;
        self.hunk = None;
        self.unavailable = Some(why.into());
    }

    /// Is there an index to draw?
    #[must_use]
    pub fn is_loaded(&self) -> bool {
        self.index.is_some()
    }

    /// The index, if there is one.
    #[must_use]
    pub fn index(&self) -> Option<&Scan> {
        self.index.as_deref()
    }

    /// The index as a shared handle, for a tree that wants to point into it.
    #[must_use]
    pub fn index_shared(&self) -> Option<&Arc<Scan>> {
        self.index.as_ref()
    }

    /// Where the bytes came from.
    #[must_use]
    pub fn origin(&self) -> Option<&str> {
        self.origin.as_deref()
    }

    /// One file's index entry.
    #[must_use]
    pub fn file_at(&self, index: usize) -> Option<&FileEntry> {
        self.index.as_ref()?.files.get(index)
    }

    /// The file the pane is reading: the one it was last on, else the first that is not hidden.
    pub fn ensure_file(&mut self) -> Option<usize> {
        let index = self.index.as_ref()?;
        if let Some(file) = self.file
            && index.files.get(file).is_some()
        {
            return Some(file);
        }
        // The first *tree* row that names a file, not the first file in patch order: the cursor and
        // the pane have to be looking at the same file, and the tree reorders by taxonomy.
        // The first *tree* row that names a file, not the first file in patch order: the cursor and
        // the pane have to be looking at the same file, and the tree reorders by taxonomy.
        let first = self.tree().current(0);
        self.file = first;
        first
    }

    /// Point the pane at a file, if it exists.
    pub fn select_file(&mut self, index: usize) -> bool {
        if self.file_at(index).is_some() {
            self.file = Some(index);
            self.hunk = None;
            return true;
        }
        false
    }

    /// The whole-patch statement, if the source or the patch header made one.
    ///
    /// "This episode's diff is not in the local packfile" is a fact about the patch, not about a file,
    /// and it has to be on screen — otherwise an empty pane reads as a bug in the viewer.
    #[must_use]
    pub fn patch_statement(&self) -> Option<String> {
        self.index
            .as_ref()
            .and_then(|scan| scan.patch_skip.as_ref())
            .map(|skip| skip.to_string())
            .or_else(|| self.unavailable.clone())
    }

    /// Which file the tree row `selection` names, if it names one.
    #[must_use]
    pub fn file_at_row(&self, selection: usize) -> Option<usize> {
        self.tree().current(selection)
    }

    /// The row of the tree that names the file the pane is showing, if any.
    #[must_use]
    pub fn file_row(&self) -> Option<usize> {
        let file = self.file?;
        self.tree().row_of(file)
    }

    /// What both panes say when there is nothing to walk, longest fact first.
    ///
    /// The tree prints the first line and the hunk pane prints all of them: one sentence is enough to
    /// explain an empty tree, while the hunk pane has the room to give the whole reason and the command
    /// that would fix it.
    #[must_use]
    pub fn empty_statements(&self) -> Vec<String> {
        if let Some(said) = self.patch_statements() {
            return said;
        }
        match self.index.as_ref() {
            None => vec![
                self.unavailable
                    .clone()
                    .unwrap_or_else(|| "no patch is loaded".to_owned()),
            ],
            Some(scan) if scan.lines == 0 => vec!["the patch is empty — 0 bytes".to_owned()],
            Some(scan) if !scan.saw_headers => vec![format!(
                "not a unified diff: no file headers in these {} lines",
                scan.lines
            )],
            Some(_) => vec!["this patch changes no files".to_owned()],
        }
    }

    /// The whole-patch statement, split into lines the panes can print one per row.
    #[must_use]
    pub fn patch_statements(&self) -> Option<Vec<String>> {
        self.index
            .as_ref()
            .and_then(|scan| scan.patch_skip.as_ref())
            .map(|skip| skip.statements())
            .or_else(|| self.unavailable.clone().map(|why| vec![why]))
    }

    /// The tree at its natural width: every row whole, nothing padded.
    ///
    /// Cheap by construction: the structure is cached per `(load, noise)` and a tree is two `Arc`
    /// clones, so the strip, the clamp and the pane can each ask without one more O(files) pass.
    #[must_use]
    pub fn tree(&self) -> Tree {
        self.tree_at(0)
    }

    /// The tree padded to `width`. Width does not change the structure — the tree clips a row, it
    /// never wraps one — so every width shares one cached shape. Zero is the "do not pad" width; see
    /// [`crate::style::StyledLine::padded`].
    #[must_use]
    pub fn tree_at(&self, width: usize) -> Tree {
        let Some(index) = &self.index else {
            return Tree::default();
        };
        Tree::from_shape(self.tree_shape(), Arc::clone(index), width)
    }

    /// The tree's structure: built once per `(load, noise)` pair and shared after that. This is the
    /// one place the tree is O(files), and `e1.8` is what made it the only one.
    #[must_use]
    pub fn tree_shape(&self) -> Arc<TreeShape> {
        let id = self.scan_id;
        let noise = self.show_noise;
        if let Some((cached_id, cached_noise, shape)) = self.tree_cache.borrow().as_ref()
            && *cached_id == id
            && *cached_noise == noise
        {
            return Arc::clone(shape);
        }
        let shape = Arc::new(match &self.index {
            Some(scan) => TreeShape::build(scan, noise),
            None => TreeShape::empty(),
        });
        *self.tree_cache.borrow_mut() = Some((id, noise, Arc::clone(&shape)));
        shape
    }

    /// Build the hunk pane's window.
    ///
    /// Two policies, one function. With a worker running ([`DiffView::spawn_workers`]) the window is
    /// drawn from the decode cache and whatever is missing is queued at `Visible` priority and drawn
    /// as a stated placeholder — the frame never waits on a read. Without one (a `--plain` run, a
    /// golden test, any host that started no threads) the same window is decoded inline, which is
    /// what `e1.7` did and what keeps the two paths producing the same rows.
    #[must_use]
    pub fn build(&self, file: usize, request: &Request) -> doc::Doc {
        let Some(entry) = self.file_at(file) else {
            return doc::Doc::stated(request.width, request.height, &["no such file".to_owned()]);
        };
        let entry = entry.clone();
        let Some(origin) = self.origin.clone() else {
            return doc::Doc::stated(
                request.width,
                request.height,
                &["no patch source is attached".to_owned()],
            );
        };
        self.materializer.set_budget(self.budget);
        if self.materializer.is_threaded() {
            let shape = Shape::of(request.layout, request.ignore_ws);
            let window = doc::window_hunks(&entry, request);
            self.materializer
                .request(&origin, &entry, shape, window, self.budget.prefetch_hunks);
            let mut cached = FromCache::new(&self.materializer, &entry, &origin, shape);
            doc::build_from(&entry, request, &self.budget, &mut cached)
        } else {
            let mut reader = match self.materializer.open(&origin) {
                Ok(reader) => reader,
                Err(why) => {
                    return doc::Doc::stated(
                        request.width,
                        request.height,
                        &[format!("`{origin}`: {why}")],
                    );
                }
            };
            doc::build(&entry, reader.as_mut(), request, &self.budget)
        }
    }

    /// Which hunks the request's viewport intersects — `(first, one past last)`. The queue asks this
    /// before it submits, so the visible window is what gets served first.
    #[must_use]
    pub fn window(&self, file: usize, request: &Request) -> (usize, usize) {
        self.file_at(file)
            .map_or((0, 0), |entry| doc::window_hunks(entry, request))
    }

    /// Which hunk the pane is looking at, given its scroll: the last one whose header row is at or
    /// above the top of the window. This is what `Y` copies and what the strip names.
    #[must_use]
    pub fn hunk_at(&self, file: usize, scroll: usize) -> Option<usize> {
        let entry = self.file_at(file)?;
        if entry.hunks.is_empty() {
            return None;
        }
        let slot = model::slot(self.layout, self.ignore_ws);
        let mut found: Option<usize> = None;
        for (index, record) in entry.hunks.iter().enumerate() {
            if record.anchors[slot] <= scroll {
                found = Some(index);
            } else {
                break;
            }
        }
        Some(found.unwrap_or(0))
    }

    /// The hunk the pane was last showing, for `Y`'s "copy this hunk".
    pub fn set_hunk(&mut self, hunk: Option<usize>) {
        self.hunk = hunk;
    }

    /// The row a hunk jump lands on, given where the pane is now.
    #[must_use]
    pub fn jump(&self, file: usize, scroll: usize, direction: Hunk) -> usize {
        let Some(entry) = self.file_at(file) else {
            return 0;
        };
        let anchors = doc::anchors(entry, self.layout, self.ignore_ws);
        if anchors.is_empty() {
            return 0;
        }
        // The unit is the hunk the pane is *looking at*, not the first header below its top: at the
        // top of a file the pane is already inside hunk 1, so `]` has to mean "the next one" and not
        // "move down one row". `[` then walks back the same way, stopping at the header of the hunk
        // you are in before it goes to the one before that — which is what makes `[` usable from the
        // middle of a long hunk.
        let current = self.hunk_at(file, scroll).unwrap_or(0);
        let head = anchors[current].1;
        match direction {
            Hunk::Next => anchors.get(current + 1).map_or(head, |(_hunk, row)| *row),
            Hunk::Previous => {
                if scroll > head {
                    head
                } else if current == 0 {
                    // One more `[` from the first hunk's header shows the file header: the pane's
                    // "top", and the row that names which file this is.
                    0
                } else {
                    anchors.get(current - 1).map_or(0, |(_hunk, row)| *row)
                }
            }
        }
    }

    /// `hunk 3 of 12 · @@ -12,7 +12,9 @@`, for the strip and for the toast a jump leaves behind.
    #[must_use]
    pub fn hunk_label(&self, file: usize, hunk: Option<usize>) -> String {
        let Some(entry) = self.file_at(file) else {
            return "no file".to_owned();
        };
        match hunk.or(self.hunk).and_then(|index| entry.hunks.get(index)) {
            Some(record) => format!(
                "hunk {} of {} · {}",
                entry
                    .hunks
                    .iter()
                    .position(|other| std::ptr::eq(other, record))
                    .map_or(1, |index| index + 1),
                entry.hunks.len(),
                record.short_header()
            ),
            None => format!(
                "{} · {}",
                if entry.hunks.is_empty() {
                    "no hunks".to_owned()
                } else {
                    format!("{} hunks", entry.hunks.len())
                },
                entry.describe()
            ),
        }
    }

    /// The text a `y` on the current row copies: a file path, or a directory for a header row.
    #[must_use]
    pub fn path_text(&self, selection: usize) -> Option<String> {
        let tree = self.tree();
        tree.path_of(selection)
    }

    /// The raw text of a hunk — header line and all — which is what `Y` copies. Read through the same
    /// chunked reader as everything else, so copying a huge hunk is not an unbounded allocation.
    pub fn hunk_text(&self, file: usize, hunk: Option<usize>) -> Result<String, String> {
        let entry = self.file_at(file).ok_or("no file is selected")?;
        let origin = self.origin.clone().ok_or("no patch is loaded")?;
        let index = hunk
            .or(self.hunk)
            .or(if entry.hunks.is_empty() {
                None
            } else {
                Some(0)
            })
            .ok_or("this file has no hunks")?;
        let record = entry
            .hunks
            .get(index)
            .ok_or("that hunk is not in this file")?;
        let mut reader = self
            .materializer
            .open(&origin)
            .map_err(|why| format!("the patch is not readable: {why}"))?;
        let mut out = format!(
            "diff --git a/{path} b/{path}\n{}\n",
            record.header,
            path = entry.path
        );
        let mut lines = lines::LineReader::new(
            reader.as_mut(),
            record.byte_start,
            record.byte_end,
            &self.budget,
        );
        let mut count = 0_usize;
        while count < self.budget.window_lines {
            match lines.next_line() {
                Ok(Some(line)) => {
                    out.push_str(&line.text);
                    out.push('\n');
                    count += 1;
                }
                Ok(None) => break,
                Err(err) => {
                    out.push_str(&format!("… the patch could not be read: {err}\n"));
                    break;
                }
            }
        }
        if count >= self.budget.window_lines {
            out.push_str(&format!(
                "… {} more lines than the {}-line window budget\n",
                record.rows[0].saturating_sub(count),
                self.budget.window_lines
            ));
        }
        Ok(out)
    }

    /// Decide how to copy `text`, without running anything.
    #[must_use]
    pub fn copy_plan(&self, text: &str) -> Copy {
        handoff::copy_plan(self.os, text, &self.env)
    }

    /// Decide how to open a file at a line, without running anything.
    #[must_use]
    pub fn editor_plan(&self, path: &str, line: u32) -> Editor {
        handoff::editor_plan(self.os, path, line, &self.env)
    }

    /// Install the host's environment — variables and `PATH` — that the copy and editor plans read.
    /// `fathom-cli` does this at startup; nothing in this crate reads the environment itself.
    pub fn install_env(&mut self, env: Env<'static>, os: handoff::Os) {
        self.env = env;
        self.os = os;
    }

    /// Copy `text`: queue the delivery and return the sentence for the toast. The loop does the
    /// running, because only the loop owns the screen; its own toast then replaces this one with the
    /// outcome, so what a reviewer reads last is what actually happened.
    pub fn copy(&mut self, text: &str) -> Result<String, String> {
        let plan = self.copy_plan(text);
        let (delivery, said) = plan.deliver(text);
        match delivery {
            Handoff::Refused => Err(said),
            work => {
                self.handoff = Some(work);
                Ok(said)
            }
        }
    }

    /// Ask for `path` to be opened at `line`. The command is queued for the loop, because running an
    /// editor means handing the terminal back and only the loop may do that.
    pub fn open_in_editor(&mut self, path: &str, line: u32) -> String {
        match self.editor_plan(path, line) {
            Editor::Open(command) => {
                let why = command.why.clone();
                self.handoff = Some(Handoff::Run(command));
                format!("queued `{path}:{line}` — {why}")
            }
            Editor::Unavailable { why } => format!("no editor: {why}"),
        }
    }

    /// Take the queued handoff, if any.
    pub fn take_handoff(&mut self) -> Option<Handoff> {
        self.handoff.take()
    }

    /// Toggle the layout, and say whether it actually took — a split that would clip a side is
    /// refused out loud (ADR-006 §6.3).
    #[must_use]
    pub fn toggle_layout(&mut self, pane_cols: usize) -> String {
        match self.layout {
            Layout::Unified => {
                if pane_cols < model::SPLIT_MIN_COLS {
                    return format!(
                        "split needs {cols} columns of diff, this pane has {have} — `\\` will ask \
                         again when the terminal grows (ADR-006 §6.3, `SPLIT_MIN_COLS`)",
                        cols = model::SPLIT_MIN_COLS,
                        have = pane_cols
                    );
                }
                self.layout = Layout::Split;
                "split — old on the left, new on the right (`\\` back to unified)".to_owned()
            }
            Layout::Split => {
                self.layout = Layout::Unified;
                "unified — one column, `\\` for split".to_owned()
            }
        }
    }

    /// Toggle word-diff.
    #[must_use]
    pub fn toggle_word(&mut self) -> String {
        self.word_diff = !self.word_diff;
        if self.word_diff {
            "word-diff on — what changed inside each line is lit (W off)".to_owned()
        } else {
            "word-diff off (W on)".to_owned()
        }
    }

    /// Toggle the whitespace rule.
    #[must_use]
    pub fn toggle_whitespace(&mut self) -> String {
        self.ignore_ws = !self.ignore_ws;
        if self.ignore_ws {
            "whitespace ignored — pairs that differ only in space or CRLF are folded into one line \
             and counted (W shows them)"
                .to_owned()
        } else {
            "whitespace shown — every CRLF change is on screen (W ignores it)".to_owned()
        }
    }

    /// Toggle the noise rule, and name the rule that was hiding things.
    #[must_use]
    pub fn toggle_noise(&mut self) -> String {
        self.show_noise = !self.show_noise;
        let rule = self
            .index
            .as_deref()
            .and_then(Scan::noise_rule)
            .unwrap_or_else(|| "no noise rule applied to this patch".to_owned());
        if self.show_noise {
            format!("noise shown — {rule}")
        } else {
            format!("noise hidden again — {rule}")
        }
    }

    /// The tree pane's footer strip: what a keystroke would act on, and how the pane is shaped.
    #[must_use]
    pub fn tree_strip(&self, selection: usize) -> String {
        let Some(index) = &self.index else {
            return self
                .unavailable
                .clone()
                .unwrap_or_else(|| "no patch is loaded".to_owned());
        };
        let tree = self.tree();
        let path = tree
            .current(selection)
            .and_then(|file| self.file_at(file))
            .map_or_else(
                || format!("{} files", index.files.len()),
                |entry| entry.path.clone(),
            );
        format!(
            "{path} · {} · {} · {}",
            self.layout.name(),
            if self.word_diff { "word-diff" } else { "plain" },
            if self.ignore_ws {
                "ws-ignored"
            } else {
                "ws-shown"
            }
        )
    }

    /// The hunks pane's footer strip.
    #[must_use]
    pub fn hunk_strip(&self, file: usize, hunk: Option<usize>) -> String {
        format!(
            "{} · {} · {}",
            self.hunk_label(file, hunk),
            self.layout.name(),
            match (self.word_diff, self.ignore_ws) {
                (true, true) => "word-diff · ws-ignored",
                (true, false) => "word-diff",
                (false, true) => "ws-ignored",
                (false, false) => "as written",
            }
        )
    }

    /// How many rows the tree has, whatever the pane's width.
    ///
    /// The tree clips a long path and it never wraps one, so the row count does not depend on the
    /// width — which is what lets a selection be clamped without drawing anything.
    #[must_use]
    pub fn tree_len(&self) -> usize {
        self.index.as_ref().map_or(0, |_| self.tree().len())
    }

    /// The rows of the tree pane, already clipped to `width`.
    #[must_use]
    pub fn tree_rows(&self, width: usize) -> Vec<StyledLine> {
        let width = width.clamp(12, 4096);
        if self
            .index
            .as_ref()
            .is_some_and(|scan| scan.files.is_empty())
        {
            // Nothing to walk, and the reason why: the patch-level skip, or the plain "no files".
            let said = self.patch_statement().unwrap_or_else(|| {
                let scan = self.index.as_ref();
                match scan {
                    Some(scan) if scan.lines == 0 => "the patch is empty — 0 bytes".to_owned(),
                    Some(scan) if !scan.saw_headers => format!(
                        "no file headers in these {} lines — that is not a unified diff",
                        scan.lines
                    ),
                    _ => "this patch changes no files".to_owned(),
                }
            });
            return vec![StyledLine::with_face(said, Face::Warning).padded(width)];
        }
        if self.index.is_none() {
            return empty_state(
                self.unavailable
                    .as_deref()
                    .unwrap_or("no episode is in scope"),
            )
            .into_iter()
            .map(|line| StyledLine::with_face(line, Face::Muted).padded(width))
            .collect();
        };
        self.tree_at(width)
            .rows()
            .into_iter()
            .map(|row| row.line.padded(width))
            .collect()
    }

    /// The rows of the tree pane for the window `scroll..scroll + height`, already clipped to
    /// `width`, padded to `height`.
    ///
    /// This is the virtualised path: a pane 24 rows tall over 5 000 files draws 24 rows and nothing
    /// else, which is the same claim `e1.8` makes about hunks applied to a list.
    #[must_use]
    pub fn tree_window(&self, scroll: usize, height: usize, width: usize) -> Vec<StyledLine> {
        let width = width.clamp(12, 4096);
        if self
            .index
            .as_ref()
            .is_some_and(|scan| scan.files.is_empty())
        {
            return vec![];
        }
        let tree = self.tree_at(width);
        let start = scroll.min(tree.len());
        let mut out: Vec<StyledLine> = tree
            .window(start..start.saturating_add(height))
            .into_iter()
            .map(|row| row.line.padded(width))
            .collect();
        while out.len() < height {
            out.push(StyledLine::plain("").padded(width));
        }
        out
    }
}

/// The empty state of the tree pane: what to run, and who owns the thing that is missing.
///
/// The remedy is the headless verb, not a flag: `fathom tui --patch <file>` used to be named here, and
/// no build of this binary has ever had that flag (the rule ADR-006 §10.3 now states — a sentence may
/// not name a command the binary does not have).
fn empty_state(why: &str) -> Vec<String> {
    vec![
        format!("no diff — {why}"),
        "nothing hands this pane a patch in this build; `fathom show <episode>` prints its files \
         headlessly today"
            .to_owned(),
    ]
}
