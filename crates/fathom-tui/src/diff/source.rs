//! `fathom_tui::diff::source` — where patch bytes come from, and what "not available" means.
//!
//! The viewer never opens a repository: `fathom-tui` has no `fathom-ingest` edge and no store handle
//! (ADR-002 §5.2, lint `M02`/`E01`), so the bytes arrive through this trait and the ticket that owns
//! materialisation — `e1.8`'s lazy decoder — implements it. Two rules fall out of that:
//!
//! * **A reader is positioned, not slurped.** [`PatchReader::read_at`] is the only way in, so the
//!   scanner can read a window of a 50 000-line patch without ever holding the rest of it. A source
//!   that hands over a `Vec<u8>` is allowed, but it is then the *source's* documented cost, not the
//!   viewer's.
//! * **Unavailable is a value.** A blob that is not in the local object store is a stated answer with
//!   a next step ([`Unavailable`]), not an error and not an empty pane — ADR-001 §7.6 again, and the
//!   reason the trait has no `io::Result` in its name.
//!
//! There is deliberately no file-backed source in this crate. `fathom-tui` is IO-free (ADR-002 §3): it
//! opens no files, reads no environment and spawns nothing. Whoever has the bytes — `fathom-cli` with a
//! `--patch` file, `e1.8` with a decoded blob, a future `git diff` adapter — implements [`PatchSource`]
//! and [`PatchReader`] here and hands the view the trait object. The whole job of an implementation is:
//! name yourself, report the length you know, and copy `want` bytes from `at` into the caller's buffer.

use std::io;

/// The stated reason a patch (or a file's half of one) cannot be shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unavailable {
    /// The source's own words, printed verbatim in the pane.
    pub reason: String,
    /// What would make it available, if anything would.
    pub hint: Option<String>,
}

impl Unavailable {
    /// A refusal with no advice attached.
    #[must_use]
    pub fn new(reason: impl Into<String>) -> Self {
        Unavailable {
            reason: reason.into(),
            hint: None,
        }
    }

    /// A refusal that says what to run.
    #[must_use]
    pub fn with_hint(reason: impl Into<String>, hint: impl Into<String>) -> Self {
        Unavailable {
            reason: reason.into(),
            hint: Some(hint.into()),
        }
    }
}

impl std::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.reason)?;
        if let Some(hint) = &self.hint {
            write!(f, " — {hint}")?;
        }
        Ok(())
    }
}

/// A positioned, bounded reader over patch bytes.
///
/// [`crate::diff::lines`] is the only caller that matters: it asks for at most `chunk_bytes` at a
/// time, which is what the memory cap is measured against.
pub trait PatchReader: Send {
    /// Read up to `buf.len()` bytes starting at byte `at`. A short read means the end.
    ///
    /// # Errors
    ///
    /// The underlying reader's error, unchanged.
    fn read_at(&mut self, at: u64, buf: &mut [u8]) -> io::Result<usize>;

    /// Total size, if the source knows it. `None` means the scanner stops on a short read.
    fn len(&self) -> Option<u64>;

    /// Is the size known to be zero?
    fn is_empty(&self) -> bool {
        self.len() == Some(0)
    }

    /// Bytes handed out so far. Instrumentation, not bookkeeping: the memory-cap test asserts on it.
    #[must_use]
    fn bytes_read(&self) -> u64 {
        0
    }

    /// How many reads it took, ditto.
    #[must_use]
    fn reads(&self) -> u64 {
        0
    }
}

/// Something that can produce the patch for one scope.
pub trait PatchSource: Send {
    /// Where the bytes came from, as the pane prints it (`working tree`, `episode 01J…`, a path).
    fn origin(&self) -> String;

    /// Open the patch text.
    ///
    /// # Errors
    ///
    /// [`Unavailable`] when the text is not there — which the pane prints instead of an empty diff.
    fn open(&self) -> Result<Box<dyn PatchReader>, Unavailable>;
}

/// Something that can say *where* the patch for a named scope lives.
///
/// The viewer takes bytes through [`PatchSource`]; it cannot know which patch a scope *means*,
/// because scopes ("the episode under the cursor") are the shell's idea, not the pane's. A provider
/// is that lookup turned inside out: the composition root — the only party with a store handle —
/// installs one on the shell (`frame::Options::patches`), and the shell asks it when a keystroke
/// (`d`) asks the diff pane about an episode. The provider stays a trait object for the same reason
/// [`PatchSource`] is: `fathom-tui` opens nothing itself (ADR-002 §3), so the party that opens
/// things hands over a verb instead of a handle.
pub trait PatchProvider: Send + Sync {
    /// Produce the patch for the scope named by `scope` (the rail row's episode id).
    ///
    /// A refusal is a stated [`Unavailable`], which the pane prints — "no episode with that id in
    /// the index" is a fact the reviewer can act on, not an empty pane.
    ///
    /// # Errors
    ///
    /// [`Unavailable`] when there is no patch to show for that scope, with the reason.
    fn patch_for(&self, scope: &str) -> Result<Box<dyn PatchSource>, Unavailable>;
}

/// A provider in the shape `Options` and `App` can hold: `Arc`-cloned, and `Debug`-able without the
/// trait opting into `Debug` (a provider holds store and repository handles; whatever they can print
/// about themselves is theirs to print, not the frame's).
#[derive(Clone)]
pub struct Provider(pub std::sync::Arc<dyn PatchProvider>);

impl Provider {
    /// Wrap a provider for the frame.
    #[must_use]
    pub fn new(provider: impl PatchProvider + 'static) -> Self {
        Provider(std::sync::Arc::new(provider))
    }
}

impl std::fmt::Debug for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Provider")
            .field(&"<patch provider>")
            .finish()
    }
}

/// A patch already in memory/// A patch already in memory — what a test, a pipe, or `e1.8`'s decode cache hands over.
#[derive(Debug, Clone)]
pub struct BytesSource {
    label: String,
    bytes: std::sync::Arc<Vec<u8>>,
    unavailable: Option<Unavailable>,
}

impl BytesSource {
    /// A named in-memory patch.
    #[must_use]
    pub fn new(label: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        BytesSource {
            label: label.into(),
            bytes: std::sync::Arc::new(bytes.into()),
            unavailable: None,
        }
    }

    /// An in-memory patch that is not there.
    #[must_use]
    pub fn unavailable(label: impl Into<String>, why: Unavailable) -> Self {
        BytesSource {
            label: label.into(),
            bytes: std::sync::Arc::new(Vec::new()),
            unavailable: Some(why),
        }
    }
}

impl PatchSource for BytesSource {
    fn origin(&self) -> String {
        self.label.clone()
    }

    fn open(&self) -> Result<Box<dyn PatchReader>, Unavailable> {
        if let Some(why) = &self.unavailable {
            return Err(why.clone());
        }
        Ok(Box::new(BytesReader {
            bytes: std::sync::Arc::clone(&self.bytes),
            at: 0,
        }))
    }
}

struct BytesReader {
    bytes: std::sync::Arc<Vec<u8>>,
    at: u64,
}

impl PatchReader for BytesReader {
    fn read_at(&mut self, at: u64, buf: &mut [u8]) -> io::Result<usize> {
        let start = usize::try_from(at)
            .unwrap_or(usize::MAX)
            .min(self.bytes.len());
        let got = (self.bytes.len() - start).min(buf.len());
        buf[..got].copy_from_slice(&self.bytes[start..start + got]);
        self.at = at + got as u64;
        Ok(got)
    }

    fn len(&self) -> Option<u64> {
        Some(self.bytes.len() as u64)
    }

    fn bytes_read(&self) -> u64 {
        self.at
    }

    fn reads(&self) -> u64 {
        // One read per chunk; the chunk size is what the cap test asserts on, not this count.
        0
    }
}

/// A source that refuses everything, for "we know nothing yet".
#[derive(Debug, Clone)]
pub struct NoSource(pub String);

impl PatchSource for NoSource {
    fn origin(&self) -> String {
        self.0.clone()
    }

    fn open(&self) -> Result<Box<dyn PatchReader>, Unavailable> {
        Err(Unavailable::with_hint(
            self.0.clone(),
            // No flag is named here: `fathom tui --patch <file>` used to be, and no build of this
            // binary has ever had it (ADR-006 §10.3: a sentence may not name a command the binary does
            // not have). Since `reco-0kw` the shipped shell hands this pane a patch itself — `d` reads
            // the selected episode out of the index — so this source is what a host *without* a
            // provider still gets, and the headless verb is the fallback that answers there.
            "no patch source is installed in this build of the pane; where the binary has an index, `d` \
             loads the selected episode's diff, and `fathom show <episode>` prints its files headlessly",
        ))
    }
}
