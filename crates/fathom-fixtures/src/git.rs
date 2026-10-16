//! `git.rs` — writing git objects, refs and an index, in-process and byte-exactly.
//!
//! Why this exists instead of a `git` subprocess: three reasons, and the third is the one that decides it.
//!
//! 1. **Speed.** `ten_k_commits` writes 10 000 commits. Ten thousand `git commit` invocations is minutes of
//!    process spawns; the corpus has a two-minute CI budget for *everything*.
//! 2. **Reach.** A gitlink (mode `160000`), a shallow clone's `.git/shallow`, and a history whose parent
//!    object is deliberately absent are all things `git` refuses to create for you. The corpus has to be
//!    able to express the shapes that break tools, not only the shapes one tool produces.
//! 3. **Determinism.** A subprocess inherits whatever `~/.gitconfig`, `core.autocrlf`, `core.fileMode`,
//!    `i18n.*` and `commit.gpgsign` the runner has. The object bytes written here are a pure function of
//!    the arguments passed in, which is the whole acceptance criterion: *"reproduces identical SHAs on all
//!    three OSes"*.
//!
//! What this is *not* is a claim that the bytes are git-valid on assertion alone. `tests/agrees_with_git.rs`
//! hands a generated repository to the real `git` binary and requires `git fsck`, `git log`, `git cat-file`
//! and a clean `git status` to agree with it — the same double-check from the other side that
//! `fathom-ingest`'s fixtures use.
//!
//! Only loose objects are written. A fixture is read by `gix`, which does not care, and packfiles would add
//! a writer (and a window policy, and a thread count) whose only output would be nondeterminism.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use gix::ObjectId;
use gix::objs::Tree;
use gix::objs::tree::{Entry as GixEntry, EntryKind, EntryMode};

use crate::policy::Identity;

/// What kind of object a tree entry names, and therefore what mode byte it carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// `100644`.
    Blob,
    /// `100755`. Never emitted by the corpus today (`core.fileMode = false`), but a scenario may want it.
    BlobExecutable,
    /// `120000`. Never emitted: a symlink is a portability trap on Windows for no analytic gain.
    Symlink,
    /// `160000` — a gitlink, the submodule case (`Scenario::Submodule`).
    Gitlink,
}

impl Mode {
    /// The git mode number.
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        match self {
            Mode::Blob => 0o100_644,
            Mode::BlobExecutable => 0o100_755,
            Mode::Symlink => 0o120_000,
            Mode::Gitlink => 0o160_000,
        }
    }
}

/// One entry in a tree, keyed by its repository-relative path with `/` separators.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    /// See [`Mode`]. For a gitlink, [`File::oid`] is the submodule's commit id and [`File::blob`] is empty.
    pub mode: Mode,
    /// The file bytes.
    pub blob: Vec<u8>,
    /// The precomputed object id, for a gitlink only.
    pub oid: Option<String>,
}

impl File {
    /// A normal file.
    #[must_use]
    pub fn text(blob: impl Into<Vec<u8>>) -> Self {
        Self {
            mode: Mode::Blob,
            blob: blob.into(),
            oid: None,
        }
    }

    /// A gitlink pointing at submodule commit `oid`.
    #[must_use]
    pub fn gitlink(oid: impl Into<String>) -> Self {
        Self {
            mode: Mode::Gitlink,
            blob: Vec::new(),
            oid: Some(oid.into()),
        }
    }

    /// Whether the bytes look binary, by git's own test: no NUL in the first 8 000 bytes.
    ///
    /// This is a fixture-side statement about content, not an analysis: the corpus needs to know whether
    /// the file it wrote is the binary file the scenario claims to contain.
    #[must_use]
    pub fn looks_binary(&self) -> bool {
        self.blob[..self.blob.len().min(8_000)].contains(&0)
    }
}

/// A commit to write. Every field here lands in the object bytes, so every field is part of the SHA.
#[derive(Debug, Clone)]
pub struct CommitSpec<'a> {
    /// The tree's hex id.
    pub tree: String,
    /// Parent commit hex ids, first parent first.
    pub parents: Vec<String>,
    /// The author *and* committer: one identity per commit keeps "who wrote it" and "who committed it"
    /// equal, which is what an agent's own commits are, and keeps the identity table the only place a name
    /// is defined.
    pub identity: &'a Identity,
    /// Unix seconds; the same value goes in both signatures.
    pub at: i64,
    /// The raw message, trailer block included. A trailing newline is added if missing, because
    /// `git commit -m` adds one and a golden file must not depend on that being remembered.
    pub message: String,
}

/// Everything that can go wrong while materialising a fixture.
///
/// Hand-rolled rather than `thiserror` on purpose: `thiserror` is not in this crate's published dependency
/// set (ADR-002 §4.4), and a fixture crate that grows a dependency to report its own errors is a fixture
/// crate that has started costing the tree something.
#[derive(Debug)]
pub enum Error {
    /// The filesystem said no.
    Io {
        /// What was being written, e.g. the path.
        context: String,
        /// The OS error.
        source: std::io::Error,
    },
    /// `gix` said no, or a byte buffer was not what it should have been.
    Other {
        /// What was being attempted.
        context: String,
        /// The underlying message, kept as text so this type stays dependency-free.
        detail: String,
    },
    /// The script asked for something that cannot mean what it says — a parent that does not exist, a path
    /// that is both a file and a directory. A fixture that cannot mean what it says is a bug in the
    /// scenario, not a runtime condition, and it says so instead of producing a repository full of lies.
    Contract(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io { context, source } => write!(f, "writing {context}: {source}"),
            Error::Other { context, detail } => write!(f, "{context}: {detail}"),
            Error::Contract(what) => write!(f, "impossible fixture: {what}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(source: std::io::Error) -> Self {
        Error::Io {
            context: "fixture".to_owned(),
            source,
        }
    }
}

/// The result type every fallible call in this crate returns.
pub type Result<T> = std::result::Result<T, Error>;

/// A git repository being written.
///
/// The work tree and the git dir are separate arguments because a submodule's git dir lives inside its
/// parent's (`.git/modules/<name>`) while its work tree does not, and because a bare repository (the
/// upstream of a shallow clone) has no work tree at all.
pub struct GitDir {
    work: PathBuf,
    git: PathBuf,
    repo: gix::Repository,
}

impl GitDir {
    /// Create a repository whose git dir is `git` and whose work tree is `work` (the same path when the
    /// repository is not bare), with `HEAD` pointing at `head_ref` and `extra_config` lines appended to
    /// `.git/config`.
    ///
    /// The config is written by hand rather than by `gix::init` for the reason this module exists:
    /// `gix::init` reads the runner's global config, and a fixture must not. Everything `gix` needs in
    /// order to discover this repository is here and nowhere else.
    pub fn create(
        work: &Path,
        git: &Path,
        head_ref: &str,
        extra_config: &[String],
    ) -> Result<Self> {
        for dir in [
            git.to_path_buf(),
            git.join("objects"),
            git.join("refs").join("heads"),
            git.join("refs").join("tags"),
            git.join("info"),
        ] {
            std::fs::create_dir_all(&dir).map_err(|source| Error::Io {
                context: dir.display().to_string(),
                source,
            })?;
        }
        let bare = extra_config
            .iter()
            .any(|line| line.trim().replace(' ', "") == "bare=true");
        let mut config = String::from("[core]\n");
        config.push_str("\trepositoryformatversion = 0\n");
        // `fileMode = false` and `symlinks = false` are what make the same tree come out of a Windows
        // runner and a Linux one: without them the index records the OS's opinion about the exec bit.
        config.push_str("\tfilemode = false\n");
        config.push_str("\tsymlinks = false\n");
        config.push_str(&format!(
            "\tbare = {}\n",
            if bare { "true" } else { "false" }
        ));
        config.push_str("\tlogallrefupdates = false\n");
        config.push_str("\tignorecase = false\n");
        config.push_str("\tprecomposeunicode = false\n");
        config.push_str("\tquotepath = false\n");
        for line in extra_config {
            config.push_str(line);
            if !line.ends_with('\n') {
                config.push('\n');
            }
        }
        write(git.join("config"), config.as_bytes())?;
        write(git.join("HEAD"), format!("ref: {head_ref}\n").as_bytes())?;
        write(
            git.join("description"),
            b"fathom-fixtures corpus repository; not a real project\n",
        )?;
        write(git.join("info").join("exclude"), DEFAULT_EXCLUDE.as_bytes())?;
        let repo =
            gix::open_opts(git, gix::open::Options::isolated()).map_err(|source| Error::Other {
                context: format!("opening {}", git.display()),
                detail: source.to_string(),
            })?;
        Ok(Self {
            work: work.to_path_buf(),
            git: git.to_path_buf(),
            repo,
        })
    }

    /// Open a repository that [`GitDir::create`] wrote earlier in the same run, to keep writing to it: the
    /// shallow clone appends refs and a shallow file after its commits, and the checkout happens after that.
    pub fn open(work: &Path, git: &Path) -> Result<Self> {
        let repo =
            gix::open_opts(git, gix::open::Options::isolated()).map_err(|source| Error::Other {
                context: format!("opening {}", git.display()),
                detail: source.to_string(),
            })?;
        Ok(Self {
            work: work.to_path_buf(),
            git: git.to_path_buf(),
            repo,
        })
    }

    /// The work tree path.
    #[must_use]
    pub fn work(&self) -> &Path {
        &self.work
    }

    /// The git dir path.
    #[must_use]
    pub fn git(&self) -> &Path {
        &self.git
    }

    /// The repository `gix` opened, for a caller that wants to read back what was just written.
    #[must_use]
    pub fn repository(&self) -> &gix::Repository {
        &self.repo
    }

    /// Write a blob and return its hex id.
    pub fn blob(&mut self, bytes: &[u8]) -> Result<String> {
        let id = self
            .repo
            .write_blob(bytes)
            .map_err(|source| gix_error("writing a blob", &self.git, &source))?;
        Ok(id.detach().to_hex().to_string())
    }

    /// Write the tree over `entries` and return its hex id.
    ///
    /// `entries` is keyed by repository-relative path, so a key with a `/` in it becomes a nested tree; the
    /// grouping is done here rather than by callers because a fixture should not be able to get the nesting
    /// wrong.
    ///
    /// Entries are sorted with `gix`'s tree comparator — byte order, a directory compared as if its name
    /// were followed by `/` — because that ordering is part of the object bytes: a tree written in the
    /// wrong order is a different SHA and a `git fsck` warning.
    pub fn tree(&mut self, entries: &BTreeMap<String, File>) -> Result<String> {
        self.write_tree_level(entries)
    }

    /// One level of the recursion: `entries` is keyed relative to *this* directory.
    fn write_tree_level(&mut self, entries: &BTreeMap<String, File>) -> Result<String> {
        let mut written: Vec<GixEntry> = Vec::new();
        let mut children: BTreeMap<String, BTreeMap<String, File>> = BTreeMap::new();
        for (path, file) in entries {
            match path.split_once('/') {
                Some((dir, _)) => {
                    let start = format!("{dir}/");
                    let mut inner: BTreeMap<String, File> = BTreeMap::new();
                    for (other, other_file) in entries.range(start.clone()..) {
                        if !other.starts_with(&start) {
                            break;
                        }
                        inner.insert(other[start.len()..].to_owned(), other_file.clone());
                    }
                    children.insert(dir.to_owned(), inner);
                }
                None => {
                    let oid = match file.oid.as_ref() {
                        Some(oid) => parse_oid(oid)?,
                        None => parse_oid(&self.blob(&file.blob)?)?,
                    };
                    written.push(GixEntry {
                        mode: entry_mode(file.mode),
                        filename: gix::bstr::BString::from(path.clone()),
                        oid,
                    });
                }
            }
        }
        for (name, inner) in children {
            let id = self.write_tree_level(&inner)?;
            written.push(GixEntry {
                mode: EntryMode::from(EntryKind::Tree),
                filename: gix::bstr::BString::from(name),
                oid: parse_oid(&id)?,
            });
        }
        written.sort();
        let tree = Tree { entries: written };
        let id = self
            .repo
            .write_object(&tree)
            .map_err(|source| gix_error("writing a tree", &self.git, &source))?;
        Ok(id.detach().to_hex().to_string())
    }

    /// Write a commit and return its hex id.
    pub fn commit(&mut self, spec: &CommitSpec<'_>) -> Result<String> {
        let signature = |identity: &Identity| gix::actor::Signature {
            name: gix::bstr::BString::from(identity.author.name),
            email: gix::bstr::BString::from(identity.author.email),
            time: gix::date::Time {
                seconds: spec.at,
                // `gix-date` stores an offset in *seconds*; the corpus names offsets in minutes, the unit
                // git itself prints, because that is the unit a `+0100` is written in.
                offset: identity.offset_minutes * 60,
            },
        };
        let mut message = spec.message.clone();
        if !message.ends_with('\n') {
            message.push('\n');
        }
        let commit = gix::objs::Commit {
            tree: parse_oid(&spec.tree)?,
            parents: spec
                .parents
                .iter()
                .map(|parent| parse_oid(parent))
                .collect::<Result<Vec<_>>>()?
                .into(),
            author: signature(spec.identity),
            committer: signature(spec.identity),
            encoding: None,
            message: gix::bstr::BString::from(message),
            extra_headers: Vec::new(),
        };
        let id = self
            .repo
            .write_object(&commit)
            .map_err(|source| gix_error("writing a commit", &self.git, &source))?;
        Ok(id.detach().to_hex().to_string())
    }

    /// Write a tag *object* and its `refs/tags/` ref, and return the tag object's hex id.
    ///
    /// Annotated tags carry a tagger signature, so their bytes are as fixed as a commit's; the corpus uses
    /// lightweight tags for release markers and this one where a scenario wants a tagged object with a
    /// message of its own.
    pub fn tag(
        &mut self,
        name: &str,
        target: &str,
        identity: &Identity,
        at: i64,
        message: &str,
    ) -> Result<String> {
        let tag = gix::objs::Tag {
            target: parse_oid(target)?,
            target_kind: gix::objs::Kind::Commit,
            signature: None,
            name: gix::bstr::BString::from(name),
            tagger: Some(gix::actor::Signature {
                name: gix::bstr::BString::from(identity.author.name),
                email: gix::bstr::BString::from(identity.author.email),
                time: gix::date::Time {
                    seconds: at,
                    offset: identity.offset_minutes * 60,
                },
            }),
            message: gix::bstr::BString::from(message),
        };
        let id = self
            .repo
            .write_object(&tag)
            .map_err(|source| gix_error("writing a tag", &self.git, &source))?;
        let hex = id.detach().to_hex().to_string();
        self.set_ref(&format!("refs/tags/{name}"), &hex)?;
        Ok(hex)
    }

    /// Point `name` (e.g. `refs/heads/main`) at `sha`, as a loose ref.
    pub fn set_ref(&mut self, name: &str, sha: &str) -> Result<()> {
        parse_oid(sha)?;
        write(self.git.join(name), format!("{sha}\n").as_bytes())
    }

    /// Make `HEAD` a symbolic ref to `name`.
    pub fn set_head(&mut self, name: &str) -> Result<()> {
        write(self.git.join("HEAD"), format!("ref: {name}\n").as_bytes())
    }

    /// Write `.git/shallow`, the one file that makes a clone shallow — the shape `fathom-ingest` has to
    /// survive, where a commit's parent is simply not in this repository.
    pub fn set_shallow(&mut self, tips: &[String]) -> Result<()> {
        let mut body = String::new();
        for tip in tips {
            body.push_str(tip);
            body.push('\n');
        }
        write(self.git.join("shallow"), body.as_bytes())
    }

    /// Append text to `.git/config` (the remote section of a clone, `core.worktree` of a submodule).
    pub fn append_config(&mut self, text: &str) -> Result<()> {
        let path = self.git.join("config");
        let mut body = read(&path)?;
        body.extend_from_slice(text.as_bytes());
        write(path, &body)
    }

    /// Append to `.git/info/exclude`, which is how a fixture keeps its own sidecar files (`fixture.json`,
    /// `.fathom/`) out of `git status` without touching the tracked tree.
    pub fn append_exclude(&mut self, text: &str) -> Result<()> {
        let path = self.git.join("info").join("exclude");
        let mut body = std::fs::read(&path).unwrap_or_default();
        body.extend_from_slice(text.as_bytes());
        write(path, &body)
    }

    /// Check out `files` into the work tree and write an index that matches, so that `git status` in a
    /// generated repository says nothing.
    ///
    /// The index carries invented stat data (`at` for both timestamps, `0` for device and inode), which is
    /// what makes it portable: a real `stat` would be a machine-dependent number inside a byte-exact
    /// artifact. git notices the mismatch, re-hashes the file, finds it matches, and reports the tree clean
    /// — the slow path, taken once, and the price of a fixture identical on three operating systems.
    pub fn checkout(&mut self, files: &BTreeMap<String, File>, at: i64) -> Result<()> {
        let mut entries: Vec<IndexEntry> = Vec::new();
        for (path, file) in files {
            let joined = self.work.join(path);
            if let Some(parent) = joined.parent() {
                std::fs::create_dir_all(parent).map_err(|source| Error::Io {
                    context: parent.display().to_string(),
                    source,
                })?;
            }
            if file.mode == Mode::Gitlink {
                // A submodule's mount point is an empty directory: the content a gitlink names is a commit in
                // another repository, not bytes in this work tree. Writing a file here — or leaving the path
                // out of the index, which is the other tempting shortcut — makes `git status` report a staged
                // deletion of the gitlink, because the index has to agree with the tree it was checked out from.
                std::fs::create_dir_all(&joined).map_err(|source| Error::Io {
                    context: joined.display().to_string(),
                    source,
                })?;
                let oid = file
                    .oid
                    .as_deref()
                    .ok_or_else(|| Error::Contract(format!("gitlink `{path}` names no commit")))?;
                entries.push(IndexEntry {
                    path: path.clone(),
                    mode: file.mode.as_u32(),
                    oid: parse_oid(oid)?,
                    size: 0,
                });
                continue;
            }
            write(&joined, &file.blob)?;
            let hex = self.blob(&file.blob)?;
            entries.push(IndexEntry {
                path: path.clone(),
                mode: file.mode.as_u32(),
                oid: parse_oid(&hex)?,
                size: file.blob.len() as u32,
            });
        }
        self.write_index(&entries, at)
    }

    /// Write a `DIRC` version-2 index over `entries`.
    ///
    /// Hand-written for the same reason as everything else here: it has to be byte-stable, and the stat
    /// data a real `git checkout` records is not byte-stable.
    fn write_index(&mut self, entries: &[IndexEntry], at: i64) -> Result<()> {
        let mut body: Vec<u8> = Vec::new();
        body.extend_from_slice(b"DIRC");
        body.extend_from_slice(&2u32.to_be_bytes());
        body.extend_from_slice(
            &u32::try_from(entries.len())
                .unwrap_or(u32::MAX)
                .to_be_bytes(),
        );
        for entry in entries {
            let start = body.len();
            // ctime(sec, nsec), mtime(sec, nsec), dev, ino, mode, uid, gid, size — ten big-endian u32s,
            // then the 20-byte id and the 2-byte flags. (Byte-for-byte against what `git` itself writes:
            // `tests/agrees_with_git.rs` would fail on the first wrong field here.) The stat values are
            // invented, and git re-hashes each file because of it, which is what makes the cleanliness
            // claim in this crate true on a machine whose real `stat` says something else.
            for value in [
                at as u32, 0, at as u32, 0, 0, 0, entry.mode, 0, 0, entry.size,
            ] {
                body.extend_from_slice(&value.to_be_bytes());
            }
            body.extend_from_slice(entry.oid.as_bytes());
            let name = entry.path.as_bytes();
            // The 12-bit name length lives in the top of the 16-bit flags word; no fixture path is long
            // enough to need the extended-flag form, and this says what happens if one ever is.
            let flags: u16 = if name.len() < 0x1_000 {
                (name.len() & 0xFFF) as u16
            } else {
                // The overflow spelling: the 12-bit field saturates and bit 12 says "look in the
                // extension". No fixture path is anywhere near this long; it is here so that one day's
                // mistake is an error message rather than a silently corrupt index.
                0x1FFF
            };
            body.extend_from_slice(&flags.to_be_bytes());
            body.extend_from_slice(name);
            body.push(0);
            while !(body.len() - start).is_multiple_of(8) {
                body.push(0);
            }
        }
        let mut hasher = gix::hash::hasher(gix::hash::Kind::Sha1);
        hasher.update(&body);
        let trailer = hasher.try_finalize().map_err(|source| Error::Other {
            context: "hashing the index".to_owned(),
            detail: source.to_string(),
        })?;
        body.extend_from_slice(trailer.as_bytes());
        write(self.git.join("index"), &body)
    }
}

/// One staged file in the index [`GitDir::checkout`] writes.
struct IndexEntry {
    path: String,
    mode: u32,
    oid: ObjectId,
    size: u32,
}

/// The default `.git/info/exclude`: every sidecar the corpus writes next to a repository, so a generated
/// fixture is `git status`-clean without anyone having to remember to index a file.
pub const DEFAULT_EXCLUDE: &str = "/fixture.json\n/.fathom/\n";

fn entry_mode(mode: Mode) -> EntryMode {
    EntryMode::from(match mode {
        Mode::Blob => EntryKind::Blob,
        Mode::BlobExecutable => EntryKind::BlobExecutable,
        Mode::Symlink => EntryKind::Link,
        Mode::Gitlink => EntryKind::Commit,
    })
}

fn parse_oid(hex: &str) -> Result<ObjectId> {
    ObjectId::from_hex(hex.as_bytes())
        .map_err(|source| Error::Contract(format!("`{hex}` is not an object id: {source}")))
}

fn gix_error(context: &str, git: &Path, source: &dyn std::error::Error) -> Error {
    Error::Other {
        context: format!("{context} in {}", git.display()),
        detail: source.to_string(),
    }
}

fn write(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| Error::Io {
            context: parent.display().to_string(),
            source,
        })?;
    }
    std::fs::write(path, bytes).map_err(|source| Error::Io {
        context: path.display().to_string(),
        source,
    })
}

fn read(path: impl AsRef<Path>) -> Result<Vec<u8>> {
    let path = path.as_ref();
    std::fs::read(path).map_err(|source| Error::Io {
        context: path.display().to_string(),
        source,
    })
}
