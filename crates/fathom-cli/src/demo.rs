//! `--demo` — the miniature repository that ships inside the binary (`e5.9`).
//!
//! The ticket's first acceptance criterion is the whole product decision: *from a clean machine,
//! `fathom --demo` shows meaningful content in under a minute with no config*. Everything else in the
//! first-run experience assumes a repository the reviewer owns, which is the one thing a person deciding
//! whether to bother does not have configured yet. So the review they see first is of a repository that
//! comes with the binary.
//!
//! Three things this module owns, and nothing else:
//!
//! * **the pack.** [`PACK`] is `assets/fathom-demo.pack`, a miniature git repository written by
//!   `tools/gen_demo_repo.py` — 21 commits with a rename, a dependency bump in two ecosystems, a test
//!   deletion, a CI job, a cycle and a merge, by two people and one agent. The manifest beside it is the
//!   contract; `tools/gen_demo_repo.py check` is what keeps the two honest. Loose git objects are already
//!   deflate-compressed, so the pack is embedded raw and read by [`unpack`] with no decompressor in the
//!   binary.
//! * **the destination.** The pack is expanded once into the user's cache directory, under a directory
//!   named from the manifest's digest — which is what makes "already unpacked" a stat instead of a hash,
//!   and what makes an old binary's unpacked copy simply unused rather than stale. Nothing is ever written
//!   into the repository the reviewer is standing in, and nothing outside the cache directory is touched.
//! * **the handover.** The index runs here and nowhere else (committed-only: the demo's worktree mtimes
//!   belong to whoever ran the generator, not to this reviewer), the guide lines go to stderr, and the
//!   process's working directory becomes the miniature — so every verb, `--plain` and the shell alike,
//!   then reads the demo through exactly the code path it uses for a real repository. There is no
//!   demo-shaped exception anywhere below this file, which is the point: the demo *is* the product, on
//!   somebody else's history.
//!
//! The guide lines name only keys and verbs this build has, because an empty state that sends a reviewer
//! to a flag that does not exist is the defect this ticket was filed against elsewhere (`reco-1nu`).

use std::path::{Path, PathBuf};

use eyre::Result;
use fathom_core::exit::ExitCode;

use crate::output;
use crate::verbs::{IndexArgs, Pending};

/// The miniature repository, packed by `tools/gen_demo_repo.py`.
const PACK: &[u8] = include_bytes!("../assets/fathom-demo.pack");
/// The manifest that describes it: the digest the cache directory is named from, and the counts the guide
/// prints. Read at runtime rather than baked into prose so the copy cannot drift from the artefact.
const MANIFEST: &str = include_str!("../assets/fathom-demo.json");
/// The pack's magic, shared with the generator.
const MAGIC: &[u8] = b"FATHOMDEMO";

/// One unpacked miniature, ready to be indexed and reviewed.
pub struct Demo {
    /// The repository's directory, which becomes the process's working directory.
    pub root: PathBuf,
    /// Commits the miniature holds, from the manifest.
    pub commits: u64,
    /// The manifest's digest, so a bug report can name the exact miniature that was seen.
    pub digest: String,
}

/// Put the miniature where it belongs and hand it over.
///
/// Unpacking is skipped when the cache already holds this exact digest, so the second `--demo` costs a
/// stat and an incremental index rather than a hundred-odd file writes.
///
/// # Errors
///
/// [`Pending`] with [`ExitCode::Error`] when the pack and manifest disagree or the cache directory cannot
/// be written, each with the remedy in the message: a demo that cannot unpack is a broken binary as far as
/// the reviewer is concerned, so the refusal has to say which of the two it was.
pub fn prepare() -> Result<Demo> {
    let manifest = manifest().map_err(|why| Pending::new(ExitCode::Error, why, None))?;
    let destination = cache_root().join(&manifest.digest[..12]).join("signal");
    if unpacked_at(&destination, &manifest.digest) {
        // Nothing to write: the miniature is already here, from this build of the binary.
    } else {
        unpack(&destination, &manifest.digest).map_err(|why| {
            Pending::new(
                ExitCode::Error,
                format!("the demo repository could not be written to {}: {why}", destination.display()),
                Some(format!(
                    "`{}` is Fathom's cache directory; point `$XDG_CACHE_HOME` at a writable place and \
                     try again",
                    cache_root().display()
                )),
            )
        })?;
    }
    Ok(Demo {
        root: destination,
        commits: manifest.commits,
        digest: manifest.digest,
    })
}

/// Index the miniature the way a first repository gets indexed, and say what happens next.
///
/// This is the guided first index: the progress rows come from `fathom index` itself (the same verb, the
/// same sink), so what a reviewer watches on the demo is what they will watch on their own history.
pub fn index(demo: &Demo, globals: &crate::verbs::Globals) -> Result<()> {
    let args = IndexArgs {
        root: demo.root.display().to_string(),
        stats: false,
        json: false,
        refresh: false,
        rebuild: false,
        watch: false,
        // The packed worktree's stat data belongs to the machine that built the pack, so the demo never
        // claims to have uncommitted work: what is under review is the history, which is also what a
        // reviewer gets when they run this on their own repository an hour later.
        committed_only: true,
        // No progress flags of its own: `--progress` and `--no-progress` are globals now, so the indexing
        // the demo does on the reviewer's behalf obeys the same override as a real `fathom index` — rows on
        // a terminal, silence on a pipe, and `fathom --demo --progress` forces them either way.
        // stdout is about to carry the frame, so the run report goes to stderr.
        quiet: true,
    };
    crate::verbs::index(&args, globals)?;
    Ok(())
}

/// What to say before the index starts: what is about to be indexed, and where it came from.
///
/// On stderr, always: stdout may be the frame (`--plain`) or an artefact and must stay clean, and the same
/// six lines are read on a terminal and in a log.
pub fn intro(demo: &Demo) {
    output::problem(format_args!(
        "fathom demo · a miniature repository of {} commits, unpacked to {} ({} @ {})",
        demo.commits,
        demo.root.display(),
        "pack",
        &demo.digest[..12]
    ));
    output::problem(format_args!(
        "  two people and one agent wrote it, so the queue ranks agent work against human work; its \
         history has a rename, a dependency bump in two ecosystems, a deleted test, a CI job, a cycle \
         and a merge"
    ));
    output::problem(format_args!(
        "  indexing it now, with `fathom index` exactly as you would run it on your own history"
    ));
}

/// What to say once the miniature is indexed: which key does what, and how to leave.
pub fn handoff() {
    // Printed whether or not the shell is what comes next: headless, this is the reviewer's first sight of
    // what they can do once they are at a terminal, and the note the CLI already prints about text output
    // covers the fact that they are not at one.
    output::problem(format_args!(
        "  `?` lists the keys · `q` queue · `t` timeline · `d` diff · `s` structure · `g` graph · `p` \
         dependencies · `o` export"
    ));
    output::problem(format_args!(
        "  nothing here is yours: it is a copy under your cache directory, and deleting that directory \
         is a complete undo"
    ));
    // The subject of this sentence is the reviewer's own repository, not the demo: the miniature is what
    // they are looking at now, and the next thing they will do is point the same tool at their own history.
    output::problem(format_args!(
        "  to review your own work: cd <your-repository> && fathom index, then run fathom"
    ));
}

/// The manifest, read once, with the two facts the guide and the cache path need.
struct Manifest {
    digest: String,
    commits: u64,
    entries: u64,
}

fn manifest() -> std::result::Result<Manifest, String> {
    let value: serde_json::Value = serde_json::from_str(MANIFEST)
        .map_err(|err| format!("the demo manifest does not parse: {err}"))?;
    let digest = value
        .get("digest")
        .and_then(serde_json::Value::as_str)
        .filter(|digest| digest.len() >= 12)
        .ok_or_else(|| "the demo manifest has no digest".to_owned())?
        .to_owned();
    if value.get("pack").and_then(serde_json::Value::as_str) != Some("fathom-demo.pack") {
        return Err(format!(
            "the demo manifest describes {}, not the embedded pack",
            value
                .get("pack")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("nothing")
        ));
    }
    // The cheapest possible integrity check on the embedded pair: a pack truncated in transit, or a
    // manifest regenerated without its pack, is a corrupt binary and must not half-run.
    let bytes = value.get("bytes").and_then(serde_json::Value::as_u64);
    if bytes != Some(PACK.len() as u64) {
        return Err(format!(
            "the embedded demo pack is {} bytes and the manifest says {}",
            PACK.len(),
            bytes.map_or_else(|| "nothing".to_owned(), |bytes| bytes.to_string())
        ));
    }
    Ok(Manifest {
        digest,
        commits: value
            .get("commits")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default(),
        entries: value
            .get("entries")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default(),
    })
}

/// Where Fathom keeps unpacked artefacts, in the order the XDG Base Directory spec gives.
///
/// `$XDG_CACHE_HOME`, then `$HOME/.cache`, then the process's temp directory. The last one is the case a
/// container has no `HOME` in, and a demo that refuses to run there would be a demo that does not run in
/// the exactly-one place somebody is most likely to try it.
fn cache_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_CACHE_HOME") {
        let dir = PathBuf::from(dir);
        if !dir.as_os_str().is_empty() {
            return dir.join("fathom");
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        if !home.as_os_str().is_empty() {
            return home.join(".cache").join("fathom");
        }
    }
    std::env::temp_dir().join("fathom")
}

/// Is this exact digest already unpacked here?
///
/// The marker holds the digest and nothing else. A directory with the digest in its name but no marker is
/// a half-finished unpack — killed between the first file and the last — and gets written again rather
/// than trusted.
fn unpacked_at(dir: &Path, digest: &str) -> bool {
    std::fs::read_to_string(dir.join(".fathom-demo")).is_ok_and(|marker| marker.trim() == digest)
}

/// Write the pack into `dir`: every tracked file, and the object store that proves them.
///
/// The path is derived from this pack's own digest and lives under Fathom's cache directory, which is why
/// an existing directory there may be replaced: it can only ever be this binary's own work, and a
/// half-written one is worse than a rewritten one.
fn unpack(dir: &Path, digest: &str) -> std::result::Result<(), String> {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    let mut cursor = Cursor::new(PACK)?;
    let count = cursor.u32()?;
    if u64::from(count) != manifest().map(|manifest| manifest.entries).unwrap_or(0) {
        // Not fatal, and deliberately not quiet: the counts disagreeing is the same corruption the
        // manifest check exists to catch, and a pack that unpacked a different number of files than it
        // claims would be a repository with holes in it.
        return Err(format!(
            "the pack holds {count} entries and the manifest says {}",
            manifest().map_or(0, |manifest| manifest.entries)
        ));
    }
    for _ in 0..count {
        let path = cursor.path()?;
        let executable = cursor.byte()? != 0;
        let payload = cursor.bytes()?;
        let target = dir.join(&path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        std::fs::write(&target, payload).map_err(|err| err.to_string())?;
        if executable {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755));
            }
        }
    }
    std::fs::write(dir.join(".fathom-demo"), format!("{digest}\n"))
        .map_err(|err| err.to_string())?;
    Ok(())
}

/// The pack's reader: fixed-width little-endian fields, and one refusal for every overrun, because the
/// alternative is a slice panic in a binary the reviewer is trusting with their terminal.
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> std::result::Result<Self, String> {
        if bytes.starts_with(MAGIC) && bytes.len() > MAGIC.len() + 5 && bytes[MAGIC.len()] == 1 {
            Ok(Cursor {
                bytes,
                at: MAGIC.len() + 1,
            })
        } else {
            Err(if bytes.starts_with(MAGIC) {
                format!(
                    "the embedded demo pack is version {} and this binary reads version 1",
                    bytes.get(MAGIC.len()).copied().unwrap_or(0)
                )
            } else {
                "the embedded demo pack is not a fathom demo pack".to_owned()
            })
        }
    }

    fn take(&mut self, n: usize) -> std::result::Result<&'a [u8], String> {
        if self.bytes.len() - self.at < n {
            return Err("the demo pack ends in the middle of an entry".to_owned());
        }
        let slice = &self.bytes[self.at..self.at + n];
        self.at += n;
        Ok(slice)
    }

    fn byte(&mut self) -> std::result::Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> std::result::Result<u32, String> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn u64(&mut self) -> std::result::Result<u64, String> {
        let bytes = self.take(8)?;
        let mut wide = [0_u8; 8];
        wide.copy_from_slice(bytes);
        Ok(u64::from_le_bytes(wide))
    }

    fn path(&mut self) -> std::result::Result<PathBuf, String> {
        let wide = self.u16()?;
        let raw = self.take(usize::from(wide))?;
        let text = std::str::from_utf8(raw).map_err(|_| "a packed path is not UTF-8".to_owned())?;
        // A packed path that escapes the destination would be the generator writing something it should
        // not have; refusing it here is cheaper than reasoning about whether it did.
        if text.is_empty()
            || text.starts_with('/')
            || text.split('/').any(|part| part == ".." || part.is_empty())
        {
            return Err(format!("the pack names an unusable path: {text:?}"));
        }
        Ok(PathBuf::from(text))
    }

    fn u16(&mut self) -> std::result::Result<u16, String> {
        let bytes = self.take(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn bytes(&mut self) -> std::result::Result<&'a [u8], String> {
        let wide = self.u64()?;
        let wide = usize::try_from(wide).map_err(|_| "a packed file is too large".to_owned())?;
        self.take(wide)
    }
}
