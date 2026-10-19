//! `repo.rs` — finding a repository and asking it who it is (e1.1 / reco-cey).
//!
//! Everything here is a *read* of repository state: where the repository is, what `HEAD` says, what refs
//! exist, whether the history is whole, which objects are missing and why. Discovery is upward from a
//! starting directory (with the same ceiling and filesystem-boundary rules `git` uses), and it never
//! shells out — ADR-001 §8.2's "fathom is not a git client" is enforced by the dependency list (ADR-002
//! §4.2) and asserted by a test that watches for process spawns.
//!
//! Two things are deliberately separated, because they have different costs and different staleness:
//! [`identity`](Repository::identity), which is a handful of paths read at open time, and
//! [`health`](Repository::health), which reads the shallow file, the config, the ref store and the
//! worktree/index existence and is computed once, lazily, and cached.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use fathom_core::time::Timestamp;

use crate::error::{Error, Unavailable};
use crate::graph::{CommitHeader, CommitLog, Detail, Signature, WalkOptions};
use crate::oid::Oid;
use crate::paths::RepoPath;
use crate::revspec;
use crate::state::{ScanOptions, WorkingState};

/// A repository, open for reading.
///
/// Cloning this handle is cheap and is the intended way to hand a repository to another thread: `gix`'s
/// object store and ref store are shared behind an `Arc`, so a clone re-uses the loaded pack index
/// instead of reading it again. The handle is `Send` and *not* `Sync` — its decode caches are
/// `RefCell`s — so a thread owns its handle rather than sharing one behind a lock (ADR-002 §10).
#[derive(Clone)]
pub struct Repository {
    inner: gix::Repository,
    identity: RepoIdentity,
    health: OnceLock<Result<RepoHealth, String>>,
}

impl Repository {
    /// Find the repository containing `from`, searching upward, and open it.
    ///
    /// This is what `fathom` does when a verb is run with no `--repository`: same search `git` does,
    /// including stopping at a filesystem boundary and honouring ceiling directories.
    ///
    /// # Errors
    ///
    /// [`Error::NotARepository`] if there is no repository at or above `from`, [`Error::RepositoryUntrusted`]
    /// if one was found and discarded for ownership reasons, [`Error::RepositoryUnreadable`] if one was
    /// found and could not be opened.
    pub fn discover(from: &Path) -> Result<Self, Error> {
        Self::discover_with(from, &DiscoverOptions::default())
    }

    /// [`discover`](Self::discover) with explicit options.
    pub fn discover_with(from: &Path, options: &DiscoverOptions) -> Result<Self, Error> {
        let trust = options.trust_map();
        let opened = if options.follow_environment {
            gix::ThreadSafeRepository::discover_with_environment_overrides_opts(
                from,
                options.upwards_options(),
                trust,
            )
        } else {
            gix::ThreadSafeRepository::discover_opts(from, options.upwards_options(), trust)
        };
        match opened {
            Ok(thread_safe) => Self::finish(thread_safe, options),
            Err(err) => Err(classify_discovery(from, err)),
        }
    }

    /// Open the repository *at* `path` — a `.git` directory, a bare repository, or a working tree — with
    /// no upward search.
    ///
    /// This is the door `--repository <path>` and the daemon's per-repository registry use: discovering
    /// upward from an explicit path would silently answer a question about a different repository.
    pub fn open_at(path: &Path, options: &DiscoverOptions) -> Result<Self, Error> {
        let trust = options.trust_map();
        let opened = if options.follow_environment {
            gix::ThreadSafeRepository::open_with_environment_overrides(path, trust)
        } else {
            gix::ThreadSafeRepository::open_opts(path, options.open_options())
        };
        match opened {
            Ok(thread_safe) => Self::finish(thread_safe, options),
            Err(err) => Err(Error::RepositoryUnreadable {
                root: display(path),
                reason: err.to_string(),
            }),
        }
    }

    fn finish(
        thread_safe: gix::ThreadSafeRepository,
        options: &DiscoverOptions,
    ) -> Result<Self, Error> {
        let mut inner = thread_safe.to_thread_local();
        if options.object_cache_bytes > 0 {
            // A bounded decode cache: a walk that reads a commit then its tree hits the cache instead of
            // inflating twice. Bounded, because an unbounded cache over a 100k-commit repository is how a
            // review tool becomes a memory problem (budget: `rss.cold_index.peak_mib`, ADR-004 §11).
            inner.object_cache_size(options.object_cache_bytes);
        }
        let identity = RepoIdentity::read(&inner);
        Ok(Repository {
            inner,
            identity,
            health: OnceLock::new(),
        })
    }

    /// The paths and shape established when the repository was opened.
    #[must_use]
    pub fn identity(&self) -> &RepoIdentity {
        &self.identity
    }

    /// What kind of repository this is, as one value a CLI can print.
    #[must_use]
    pub fn kind(&self) -> RepositoryKind {
        self.identity.kind.clone()
    }

    /// Is this a bare repository (no working tree)?
    #[must_use]
    pub fn is_bare(&self) -> bool {
        self.identity.kind.is_bare()
    }

    /// The working tree, if there is one.
    #[must_use]
    pub fn work_dir(&self) -> Option<&Path> {
        self.identity.work_dir.as_deref()
    }

    /// The `.git` directory (or the bare repository's own directory).
    #[must_use]
    pub fn git_dir(&self) -> &Path {
        &self.identity.git_dir
    }

    /// The shared directory: the main repository's `.git` for a linked worktree, else the git dir.
    #[must_use]
    pub fn common_dir(&self) -> &Path {
        &self.identity.common_dir
    }

    /// Where this repository's index file lives, whether or not it exists yet.
    ///
    /// A linked worktree's index is under `common/worktrees/<name>/`, not beside `.git`, so a caller that
    /// assumed `git_dir/index` would watch and lock a file git never touches. `e1.9`'s watcher stats it
    /// (`watch::Stat`), which is the same fact `working_state` depends on.
    #[must_use]
    pub fn index_path(&self) -> PathBuf {
        self.inner.index_path()
    }

    /// The object hash this repository uses.
    #[must_use]
    pub fn object_hash(&self) -> ObjectHash {
        self.identity.object_hash.clone()
    }

    /// `HEAD`, in all three of its states.
    ///
    /// # Errors
    ///
    /// [`Error::RepositoryUnreadable`] if `HEAD` itself cannot be read (it is a file; files fail).
    pub fn head(&self) -> Result<Head, Error> {
        let head = self
            .inner
            .head()
            .map_err(|err| self.unreadable("reading HEAD", &err))?;
        Ok(Head::from_gix(&head))
    }

    /// The commit `HEAD` points at, or `None` when `HEAD` is unborn.
    pub fn head_oid(&self) -> Result<Option<Oid>, Error> {
        Ok(self.head()?.oid())
    }

    /// Every reference, or a filtered subset.
    ///
    /// The list is a snapshot: a long-lived `fathom` watching a repository re-reads it (ADR-004 §7.8's
    /// "reload, do not cache the world").
    pub fn refs(&self, filter: RefFilter) -> Result<Vec<Ref>, Error> {
        let platform = self
            .inner
            .references()
            .map_err(|err| self.unreadable("opening the ref store", &err))?;
        let iter = match filter {
            RefFilter::All => platform.all(),
            RefFilter::LocalBranches => platform.local_branches(),
            RefFilter::RemoteBranches => platform.remote_branches(),
            RefFilter::Tags => platform.tags(),
        }
        .map_err(|err| self.unreadable("iterating refs", &err))?;
        let mut out = Vec::new();
        for reference in iter {
            let reference = reference.map_err(|err| self.unreadable("reading a ref", &err))?;
            let ref_name: &gix::bstr::BStr = reference.name().into();
            let name = crate::paths::to_str_lossy(ref_name).into_owned();
            let target = match reference.target() {
                gix::refs::TargetRef::Symbolic(symbolic) => {
                    let symbolic: &gix::bstr::BStr = symbolic.into();
                    RefTarget::Symbolic {
                        to: crate::paths::to_str_lossy(symbolic).into_owned(),
                    }
                }
                gix::refs::TargetRef::Object(oid) => RefTarget::Direct { oid: from_gix(oid) },
            };
            let mut peeled = reference.detach().peeled.map(|oid| from_gix(oid.as_ref()));
            let kind = RefKind::classify(&name);
            // A loose annotated tag records no peel, so compute the one thing a caller always wants:
            // the commit the tag names. Bounded, because a tag pointing at a tag is a mistake, not a
            // data structure.
            if peeled.is_none()
                && kind == RefKind::Tag
                && let RefTarget::Direct { oid } = target
            {
                peeled = peel_to_non_tag(self, oid);
            }
            out.push(Ref {
                name,
                kind,
                target,
                peeled,
            });
        }
        out.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(out)
    }

    /// Resolve a revision expression to a commit.
    ///
    /// The supported grammar is documented on [`crate::revspec`]; it is the subset that a review tool
    /// needs and no more, and anything outside it is a typed error naming what fathom understands.
    pub fn resolve(&self, spec: &str) -> Result<Oid, Error> {
        revspec::resolve(self, spec)
    }

    /// The repository's health: whole history? shallow? filtered? worktree present? refs consistent?
    ///
    /// Computed once and cached, because it reads several files and nothing in a single `fathom` run
    /// changes them.
    pub fn health(&self) -> Result<&RepoHealth, Error> {
        if let Some(cached) = self.health.get() {
            return cached
                .as_ref()
                .map_err(|reason| Error::RepositoryUnreadable {
                    root: self.identity.display_name.clone(),
                    reason: reason.clone(),
                });
        }
        let computed = self.compute_health();
        let stored = computed.map_err(|err| err.to_string());
        let _ = self.health.set(stored);
        self.health()
    }

    /// The commits listed in `.git/shallow`, or `None` if the repository is not shallow.
    ///
    /// `gix` snapshots that file and re-reads it when it changes, so a `git fetch --unshallow` in another
    /// terminal is picked up by the next call rather than frozen into this handle.
    pub fn shallow_boundary(&self) -> Result<Option<Vec<Oid>>, Error> {
        self.inner
            .shallow_commits()
            .map(|snapshot| {
                snapshot.map(|commits| commits.iter().map(|oid| from_gix(oid.as_ref())).collect())
            })
            .map_err(|err| self.unreadable("reading the shallow file", &err))
    }

    /// Is the repository shallow? (Cheaper than [`shallow_boundary`](Self::shallow_boundary).)
    pub fn is_shallow(&self) -> Result<bool, Error> {
        self.inner
            .is_shallow()
            .map_err(|err| self.unreadable("reading the shallow file", &err))
    }

    /// What the clone filter says, if there is one.
    ///
    /// git records a filtered clone in the remote's own section — `remote.<name>.promisor = true` and
    /// `remote.<name>.partialclonefilter = <filter>` — plus `extensions.partialclone = <name>` to say
    /// which remote to ask. All three are read, because a repository cloned by an older git, or by a
    /// tool that mimics git, may have set only some of them.
    #[must_use]
    pub fn partial_clone(&self) -> PartialClone {
        let mut filter = None;
        let mut promisor_remotes = Vec::new();
        let snapshot = self.inner.config_snapshot();
        if let Some(sections) = snapshot.sections_by_name("remote") {
            for section in sections {
                let promisor = section
                    .value("promisor")
                    .is_some_and(|value| is_true(value.as_ref()));
                if !promisor {
                    continue;
                }
                if let Some(name) = section.header().subsection_name() {
                    promisor_remotes.push(crate::paths::to_str_lossy(name).into_owned());
                }
                if filter.is_none() {
                    filter = section
                        .value("partialclonefilter")
                        .map(|value| crate::paths::to_str_lossy(value.as_ref()).into_owned());
                }
            }
        }
        let remote = self
            .config_string("extensions.partialclone")
            // A single promisor remote needs no `extensions` entry to name it, and a caller that wants
            // to tell a human *which remote would answer* needs an answer anyway.
            .or_else(|| (promisor_remotes.len() == 1).then(|| promisor_remotes[0].clone()));
        let promisor = !promisor_remotes.is_empty() || remote.is_some();
        PartialClone {
            is_partial: promisor || filter.is_some(),
            filter,
            remote,
            promisor_remotes,
        }
    }

    /// Why is `oid` not in the object database?
    ///
    /// This is the classification that turns "object not found" into something a human can act on: a
    /// shallow graft, a filter clone with a named remote, or genuine damage. The order matters: a
    /// repository can be both shallow *and* filtered, and the shallow file is the sharper claim.
    pub fn why_unavailable(&self, oid: Oid) -> Unavailable {
        let boundary = self.shallow_boundary().ok().flatten().unwrap_or_default();
        if boundary.contains(&oid) {
            return Unavailable::ShallowBoundary { at: oid };
        }
        let partial = self.partial_clone();
        if partial.is_partial {
            return Unavailable::PartialClone {
                filter: partial.filter,
                remote: partial
                    .remote
                    .or_else(|| partial.promisor_remotes.first().cloned()),
            };
        }
        // Not a boundary itself, but the clone is shallow: every object behind a graft is behind it, so
        // name the graft we do know about rather than pretending the object vanished on its own.
        if let Some(first) = boundary.first() {
            return Unavailable::ShallowBoundary { at: *first };
        }
        Unavailable::Missing
    }

    /// Is this object in the database?
    #[must_use]
    pub fn has_object(&self, oid: Oid) -> bool {
        self.inner.has_object(to_gix(oid, self.inner.object_hash()))
    }

    /// The kind and size of an object, or `None` if it is not present.
    ///
    /// Cheaper than reading it: a header read does not inflate a delta chain to hand back the bytes.
    pub fn object_header(&self, oid: Oid) -> Result<Option<ObjectHeader>, Error> {
        let id = to_gix(oid, self.inner.object_hash());
        let header = self
            .inner
            .try_find_header(id)
            .map_err(|err| self.unreadable("reading an object header", &err))?;
        Ok(header.map(|header| ObjectHeader {
            oid,
            kind: ObjectKind::from(header.kind()),
            size: header.size(),
        }))
    }

    /// The kind and size of an object, or `None` if it is not present.
    /// Read a commit header, or `None` if the object is absent or is not a commit.
    pub fn commit(&self, oid: Oid) -> Result<Option<CommitHeader>, Error> {
        self.read_commit(oid, Detail::Headers)
            .map(|ready| ready.and_then(|ready| ready.detail))
    }

    /// Read a blob's bytes, up to `cap` bytes.
    ///
    /// The cap is not politeness: `benchmarks/budgets.json`'s `ingest.file_size_cap.mib` exists because a
    /// review tool that slurps a 500 MB blob is a review tool that dies on someone's vendor directory.
    /// `Err` with [`Error::ObjectUnavailable`] when the blob is missing and classified as such.
    ///
    /// The header is read before the data, and that order *is* the guarantee: `try_find_header` reports a
    /// blob's kind and length without inflating it, so a blob over the cap is turned away with its bytes
    /// still in the pack. Checking the length after `try_find_object` would inflate a 500 MB blob to
    /// discover it was too big to keep — the exact failure the cap exists to prevent. The second lookup on
    /// the accepted path is the price of that guarantee, and `benchmarks/reports/e1-2-ingest.json` records
    /// what it costs.
    pub fn blob(&self, oid: Oid, cap: usize) -> Result<BlobRead, Error> {
        let id = to_gix(oid, self.inner.object_hash());
        let header = self
            .inner
            .try_find_header(id)
            .map_err(|err| self.unreadable("reading a blob header", &err))?;
        let Some(header) = header else {
            return Err(Error::ObjectUnavailable {
                oid,
                why: self.why_unavailable(oid),
            });
        };
        if header.kind() != gix::objs::Kind::Blob {
            return Ok(BlobRead::WrongKind {
                kind: ObjectKind::from(header.kind()),
            });
        }
        let size = usize::try_from(header.size()).unwrap_or(usize::MAX);
        if size > cap {
            return Ok(BlobRead::Oversized { size });
        }
        let object = self
            .inner
            .try_find_object(id)
            .map_err(|err| self.unreadable("reading a blob", &err))?;
        let Some(object) = object else {
            return Err(Error::ObjectUnavailable {
                oid,
                why: self.why_unavailable(oid),
            });
        };
        debug_assert_eq!(object.data.len(), size, "the header and the data agree");
        Ok(BlobRead::Present {
            oid,
            // `detach` rather than a field move: `gix::Object` borrows the repository, so it cannot be
            // destructured; detaching hands the buffer over with the borrow dropped.
            bytes: object.detach().data,
        })
    }

    /// The entries of a tree, in the tree's own order (which is byte order over names).
    pub fn tree_entries(&self, oid: Oid) -> Result<Vec<TreeEntry>, Error> {
        let id = to_gix(oid, self.inner.object_hash());
        let Some(object) = self
            .inner
            .try_find_object(id)
            .map_err(|err| self.unreadable("reading a tree", &err))?
        else {
            return Err(Error::ObjectUnavailable {
                oid,
                why: self.why_unavailable(oid),
            });
        };
        if object.kind != gix::objs::Kind::Tree {
            return Err(Error::RepositoryUnsupported {
                detail: format!(
                    "{oid} is a {}, not a tree",
                    ObjectKind::from(object.kind).as_str()
                ),
            });
        }
        let tree = object
            .try_into_tree()
            .map_err(|err| self.unreadable("decoding a tree", &err))?;
        let mut out = Vec::new();
        for entry in tree.iter() {
            let entry = entry.map_err(|err| self.unreadable("decoding a tree entry", &err))?;
            out.push(TreeEntry {
                // The name stays bytes: a repository can contain a file name that is not valid UTF-8,
                // and a tree entry whose name got replaced with `?` cannot be opened again.
                name: RepoPath::new(entry.filename().to_vec()),
                mode: entry.mode().value(),
                kind: TreeEntryKind::from_mode_value(u32::from(entry.mode().value())),
                oid: from_gix(entry.object_id().as_ref()),
            });
        }
        Ok(out)
    }

    /// Walk the commit graph.
    ///
    /// The returned iterator is lazy: nothing is read until it is pulled. See [`crate::graph`] for the
    /// orders, the pruning rules and how truncation is reported.
    #[must_use]
    pub fn log(&self, options: WalkOptions) -> CommitLog<'_> {
        CommitLog::new(self, options)
    }

    /// The working tree's staged and unstaged state.
    pub fn working_state(&self, options: &ScanOptions) -> Result<WorkingState, Error> {
        crate::state::working_state(self, options)
    }

    /// The submodules declared by this repository, with each one's state.
    pub fn submodules(&self) -> Result<Vec<Submodule>, Error> {
        crate::state::submodules(self)
    }

    /// If this repository is a submodule, the git directory of its superproject.
    #[must_use]
    pub fn superproject(&self) -> Option<&Path> {
        self.identity.superproject.as_deref()
    }

    /// The linked worktrees of this repository (`git worktree list`), main worktree included.
    pub fn linked_worktrees(&self) -> Result<Vec<LinkedWorktree>, Error> {
        crate::state::linked_worktrees(self)
    }

    /// The `gix` hash kind, for callers that have to name it (hashing, prefix lookups).
    #[must_use]
    pub(crate) fn object_hash_kind(&self) -> gix::hash::Kind {
        self.inner.object_hash()
    }

    /// Read one commit for the walk: `Ok(None)` means "exists but is not a commit".
    ///
    /// An absent object is an `Err(ObjectUnavailable)` here rather than an `Ok(None)` *because* the walk
    /// has to distinguish "not a commit, skip quietly" from "commit we cannot read, report loudly"; the
    /// `why` is computed at the point of failure, where the shallow and filter state is freshest.
    pub(crate) fn read_commit(
        &self,
        oid: Oid,
        detail: Detail,
    ) -> Result<Option<ReadyCommit>, Error> {
        let id = to_gix(oid, self.inner.object_hash());
        let object = self
            .inner
            .try_find_object(id)
            .map_err(|err| self.unreadable("reading a commit", &err))?;
        let Some(object) = object else {
            return Err(Error::ObjectUnavailable {
                oid,
                why: self.why_unavailable(oid),
            });
        };
        if object.kind != gix::objs::Kind::Commit {
            return Ok(None);
        }
        let commit = object
            .try_into_commit()
            .map_err(|err| self.unreadable("decoding a commit", &err))?;
        let parents: Vec<Oid> = commit
            .parent_ids()
            .map(|id| from_gix(id.as_ref()))
            .collect();
        let tree = from_gix(
            commit
                .tree_id()
                .map_err(|err| self.unreadable("decoding a commit tree", &err))?
                .as_ref(),
        );
        let committer = signature(
            commit
                .committer()
                .map_err(|err| self.unreadable("decoding a commit committer", &err))?,
        )?;
        if detail == Detail::Ids {
            return Ok(Some(ReadyCommit {
                oid,
                tree,
                parents,
                when: committer.when,
                detail: None,
            }));
        }
        let author = signature(
            commit
                .author()
                .map_err(|err| self.unreadable("decoding a commit author", &err))?,
        )?;
        let message = commit
            .message_raw()
            .map_err(|err| self.unreadable("decoding a commit message", &err))?
            .to_vec();
        let subject = commit
            .message()
            .map(|message| crate::paths::to_str_lossy(message.summary().as_ref()).into_owned())
            .unwrap_or_default();
        let extra_header_names = commit
            .decode()
            .map(|decoded| {
                decoded
                    .extra_headers
                    .iter()
                    .map(|(name, _)| crate::paths::to_str_lossy(name).into_owned())
                    .collect()
            })
            .unwrap_or_default();
        Ok(Some(ReadyCommit {
            oid,
            tree,
            parents: parents.clone(),
            when: committer.when,
            detail: Some(CommitHeader {
                oid,
                tree,
                parents,
                author,
                committer,
                subject,
                message,
                extra_header_names,
            }),
        }))
    }

    /// A config string, lossy-converted. `None` if unset.
    #[must_use]
    pub fn config_string(&self, key: &str) -> Option<String> {
        self.inner
            .config_snapshot()
            .string(key)
            .map(|value| crate::paths::to_str_lossy(value.as_ref()).into_owned())
    }

    /// A boolean config value, if present and parseable.
    #[must_use]
    pub fn config_bool(&self, key: &str) -> Option<bool> {
        self.inner.config_snapshot().boolean(key)
    }

    /// The `gix` handle, for this crate's own modules and for the backend seam. Not public: a caller
    /// upstream of the door must not be able to reach `gix` by accident (ADR-002 §4.2).
    pub(crate) fn gix(&self) -> &gix::Repository {
        &self.inner
    }

    pub(crate) fn unreadable(&self, what: &str, err: &dyn std::error::Error) -> Error {
        Error::RepositoryUnreadable {
            root: self.identity.display_name.clone(),
            reason: format!("{what}: {err}"),
        }
    }
}

impl std::fmt::Debug for Repository {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `gix::Repository` holds decode caches and is not `Debug`; the identity is everything a log
        // line, a test failure or a `--json` diagnostic wants to see.
        f.debug_struct("Repository")
            .field("display_name", &self.identity.display_name)
            .field("kind", &self.identity.kind)
            .field("git_dir", &self.identity.git_dir)
            .finish_non_exhaustive()
    }
}

/// What the walk read about one commit, before it decides how much to hand back.
pub(crate) struct ReadyCommit {
    pub(crate) oid: Oid,
    #[allow(dead_code)]
    pub(crate) tree: Oid,
    pub(crate) parents: Vec<Oid>,
    pub(crate) when: Timestamp,
    pub(crate) detail: Option<CommitHeader>,
}

fn signature(from: gix::actor::SignatureRef<'_>) -> Result<Signature, Error> {
    let time = from.time().map_err(|err| Error::RepositoryUnsupported {
        detail: format!("a commit signature has an unreadable date: {err}"),
    })?;
    Ok(Signature {
        name: crate::paths::to_str_lossy(from.name.as_ref()).into_owned(),
        email: crate::paths::to_str_lossy(from.email.as_ref()).into_owned(),
        when: Timestamp::from_unix_micros(time.seconds.saturating_mul(1_000_000)),
        offset_minutes: time.offset / 60,
    })
}

/// How to look for a repository.
#[derive(Debug, Clone)]
pub struct DiscoverOptions {
    /// Honour `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE` and `GIT_SHALLOW_FILE`.
    ///
    /// Off by default: `fathom` reading a repository the environment happens to name, when it was told
    /// which one to read, is a class of support ticket nobody enjoys. The CLI turns it on for the
    /// hook-event path (ADR-005 §7), where the harness *is* the one that knows.
    pub follow_environment: bool,
    /// Read the user's and the system's git config.
    ///
    /// Off by default, for determinism (ADR-001 §7.4): two machines with different `core.*` settings
    /// should get the same answer from the same repository. The repository's own `.git/config` is always
    /// read — it is part of the repository, not part of the machine.
    pub use_user_config: bool,
    /// Stop the upward search at this directory, as `GIT_CEILING_DIRECTORIES` would.
    pub ceiling: Option<PathBuf>,
    /// Refuse a repository that is not owned by the current user, rather than reading it with reduced
    /// permissions. This is `safe.directory` seen from the other side.
    pub require_trusted: bool,
    /// Let the upward search cross a filesystem boundary (git requires
    /// `GIT_DISCOVERY_ACROSS_FILESYSTEM` for this, and so do we).
    pub cross_filesystem: bool,
    /// Decode-cache budget in bytes; `0` disables the cache. Sized against
    /// `benchmarks/budgets.json`'s `rss.cold_index.peak_mib`.
    pub object_cache_bytes: usize,
    /// Extra `-c` style config overrides, applied last, e.g. `core.abbrev=12`.
    pub config_overrides: Vec<String>,
}

impl Default for DiscoverOptions {
    fn default() -> Self {
        DiscoverOptions {
            follow_environment: false,
            use_user_config: false,
            ceiling: None,
            require_trusted: false,
            cross_filesystem: false,
            object_cache_bytes: 4 * 1024 * 1024,
            config_overrides: Vec::new(),
        }
    }
}

impl DiscoverOptions {
    fn upwards_options(&self) -> gix::discover::upwards::Options<'static> {
        let mut options = gix::discover::upwards::Options {
            cross_fs: self.cross_filesystem,
            ..Default::default()
        };
        if let Some(ceiling) = &self.ceiling {
            options.ceiling_dirs = vec![ceiling.clone()];
            options.match_ceiling_dir_or_error = true;
        } else {
            // A ceiling from the environment that does not contain the start path is git's error case;
            // fathom would rather keep looking, because the CLI passes `--ceiling` as a hint, not a
            // contract.
            options.match_ceiling_dir_or_error = false;
        }
        options
    }

    fn open_options(&self) -> gix::open::Options {
        let base = if self.use_user_config {
            gix::open::Options::default()
        } else {
            gix::open::Options::isolated()
        };
        if self.config_overrides.is_empty() {
            base
        } else {
            base.clone().config_overrides(self.config_overrides.clone())
        }
    }

    fn trust_map(&self) -> gix::sec::trust::Mapping<gix::open::Options> {
        let options = self.open_options();
        gix::sec::trust::Mapping {
            full: options.clone(),
            reduced: options,
        }
    }
}

/// Turn a `gix` discovery failure into the right user-facing failure.
///
/// `gix` hands back an opaque error tree, but it keeps the typed discovery outcome inside it (see
/// [`gix::discover::upwards::Error`]), so the *reason* nothing was found survives: no repository at all,
/// nothing inside the ceiling, nothing on this filesystem, or a candidate rejected for ownership. That
/// typed variant is what the three cases are matched on; the classification fallback is `gix`'s own
/// `NotFound` class, for whatever a future `gix` puts in front of it.
fn classify_discovery(from: &Path, err: gix::Error) -> Error {
    use gix::discover::upwards::Error as Upwards;
    if let Some(Upwards::NoTrustedGitRepository { candidate, .. }) =
        err.downcast_any_ref::<Upwards>()
    {
        return Error::RepositoryUntrusted {
            root: display(from),
            candidate: display(candidate),
        };
    }
    if err.downcast_any_ref::<Upwards>().is_some() || err.classify().is_not_found() {
        return Error::NotARepository {
            root: display(from),
        };
    }
    Error::RepositoryUnreadable {
        root: display(from),
        reason: err.to_string(),
    }
}

/// git's own set of truthy config words.
fn is_true(value: &gix::bstr::BStr) -> bool {
    matches!(
        value.to_ascii_lowercase().as_slice(),
        b"true" | b"yes" | b"on" | b"1"
    )
}

fn display(path: &Path) -> String {
    path.display().to_string()
}

pub(crate) fn to_gix(oid: Oid, kind: gix::hash::Kind) -> gix::ObjectId {
    // `Oid` is 20 bytes by construction, which is the only length a SHA-1 repository accepts, so the
    // error arm is unreachable; `gix::ObjectId::empty_tree` stands in rather than a panic, because a
    // library that panics loses the user's terminal (ADR-002 §9 rule 2).
    <gix::ObjectId as std::convert::TryFrom<&[u8]>>::try_from(oid.as_bytes().as_slice())
        .unwrap_or_else(|_| gix::ObjectId::empty_tree(kind))
}

pub(crate) fn from_gix(oid: &gix::hash::oid) -> Oid {
    let bytes = oid.as_bytes();
    let mut out = [0u8; Oid::LEN];
    let take = bytes.len().min(Oid::LEN);
    out[..take].copy_from_slice(&bytes[..take]);
    Oid::from_bytes(out)
}

/// Follow a chain of annotated tags to the first object that is not a tag.
fn peel_to_non_tag(repo: &Repository, from: Oid) -> Option<Oid> {
    let mut current = from;
    for _ in 0..16 {
        let header = repo.object_header(current).ok()??;
        if header.kind != ObjectKind::Tag {
            return Some(current);
        }
        let id = to_gix(current, repo.object_hash_kind());
        let object = repo.inner.try_find_object(id).ok()??;
        let tag = object.try_into_tag().ok()?;
        current = from_gix(tag.target_id().ok()?.as_ref());
    }
    None
}

/// Where a repository lives and what shape it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoIdentity {
    /// A name for the repository, for titles and log lines: the working tree's directory name, else the
    /// git directory's.
    pub display_name: String,
    /// The `.git` directory (or the bare repository's directory).
    pub git_dir: PathBuf,
    /// The directory shared by all worktrees of this repository.
    pub common_dir: PathBuf,
    /// The checked-out tree, if there is one.
    pub work_dir: Option<PathBuf>,
    /// Worktree, bare, linked worktree or submodule.
    pub kind: RepositoryKind,
    /// The object hash.
    pub object_hash: ObjectHash,
    /// The superproject's git directory, if this is a submodule.
    pub superproject: Option<PathBuf>,
    /// Was the repository accepted as owned by the current user?
    pub trusted: bool,
}

impl RepoIdentity {
    fn read(repo: &gix::Repository) -> Self {
        // `gix` reports a linked worktree's common dir as `<worktree>/.git/worktrees/<name>/../..`,
        // which is a correct path and an unreadable one. Collapsing `.` and `..` lexically (no syscall:
        // the directory may legitimately not exist, and a symlink-resolved path would differ from what
        // the user typed) is what makes `common_dir` comparable and printable.
        let git_dir = normalize(repo.git_dir());
        let common_dir = normalize(repo.common_dir());
        let work_dir = repo.workdir().map(normalize);
        let object_hash = match repo.object_hash() {
            gix::hash::Kind::Sha1 => ObjectHash::Sha1,
            // A build without the `sha256` feature cannot open a SHA-256 repository at all (ADR-002
            // §4.2), so any other hash appears here only as a diagnostic string.
            other => ObjectHash::Other(other.to_string()),
        };
        let superproject = detect_superproject(&git_dir);
        let kind = if repo.is_bare() {
            RepositoryKind::Bare
        } else if let Some(parent) = &superproject {
            RepositoryKind::Submodule {
                superproject: parent.clone(),
            }
        } else if common_dir != git_dir {
            RepositoryKind::LinkedWorkTree {
                main_repository: common_dir.clone(),
            }
        } else {
            RepositoryKind::WorkTree
        };
        let display_name = work_dir
            .as_deref()
            .or(Some(git_dir.as_path()))
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| String::from("repository"));
        RepoIdentity {
            display_name,
            trusted: matches!(repo.git_dir_trust(), gix::sec::Trust::Full),
            git_dir,
            common_dir,
            work_dir,
            kind,
            object_hash,
            superproject,
        }
    }
}

/// Collapse `.` and `..` in a path without touching the filesystem.
fn normalize(path: &Path) -> PathBuf {
    let mut out = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !matches!(out.last(), Some(std::path::Component::Normal(_))) {
                    out.push(component);
                } else {
                    out.pop();
                }
            }
            other => out.push(other),
        }
    }
    if out.is_empty() && !path.as_os_str().is_empty() {
        // `.` and `./` collapse to nothing, and an empty path is not a collapsed form of them — it is the
        // path no filesystem call accepts, which a caller downstream reads as "there is no work tree here"
        // (`fathom index .` failed exactly that way). A relative "here" stays a relative "here".
        return PathBuf::from(".");
    }
    out.into_iter().collect()
}

/// Recognise `<super>/.git/modules/<name>` — the layout git uses for a submodule's objects and refs.
///
/// The submodule's *name* may contain slashes (`vendor/src`), so the `modules` directory is not always
/// the immediate parent; the search walks up a bounded number of levels and confirms the candidate by
/// the `HEAD` file that only a real git directory has.
fn detect_superproject(git_dir: &Path) -> Option<PathBuf> {
    let mut cursor = git_dir.parent()?;
    for _ in 0..6 {
        if cursor.file_name()? == std::ffi::OsStr::new("modules") {
            let parent_git = cursor.parent()?;
            if parent_git.join("HEAD").is_file() {
                return Some(parent_git.to_path_buf());
            }
            return None;
        }
        cursor = cursor.parent()?;
    }
    None
}

/// What kind of repository was opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepositoryKind {
    /// A normal repository with a working tree.
    WorkTree,
    /// A linked worktree (`git worktree add`); its objects and refs live in the main repository.
    LinkedWorkTree {
        /// The main repository's git directory.
        main_repository: PathBuf,
    },
    /// A bare repository: no working tree.
    Bare,
    /// A submodule; its superproject's git directory is named so a caller can walk up.
    Submodule {
        /// The superproject's git directory.
        superproject: PathBuf,
    },
}

impl RepositoryKind {
    /// Is there a working tree?
    #[must_use]
    pub fn has_work_tree(&self) -> bool {
        !matches!(self, RepositoryKind::Bare)
    }

    /// Is this bare?
    #[must_use]
    pub fn is_bare(&self) -> bool {
        matches!(self, RepositoryKind::Bare)
    }

    /// A short name for `--json` and `fathom doctor`.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            RepositoryKind::WorkTree => "worktree",
            RepositoryKind::LinkedWorkTree { .. } => "linked-worktree",
            RepositoryKind::Bare => "bare",
            RepositoryKind::Submodule { .. } => "submodule",
        }
    }
}

impl std::fmt::Display for RepositoryKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The object hash a repository uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectHash {
    /// SHA-1, which is all this build can read (ADR-002 §4.2).
    Sha1,
    /// SHA-256. A repository using it fails to open, so this variant appears only in a diagnostic.
    Sha256,
    /// Something else, named verbatim.
    Other(String),
}

/// `HEAD`, in each of its three states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Head {
    /// `HEAD` names a branch that has commits.
    Branch {
        /// The branch name, without `refs/heads/`.
        name: String,
        /// The commit it points at.
        oid: Oid,
    },
    /// `HEAD` names a branch that has no commits yet — a fresh repository.
    Unborn {
        /// The branch name, without `refs/heads/`.
        name: String,
    },
    /// `HEAD` holds a commit directly: `git checkout <sha>`, or a detached worktree.
    Detached {
        /// The commit.
        oid: Oid,
    },
}

impl Head {
    fn from_gix(head: &gix::Head<'_>) -> Self {
        match &head.kind {
            gix::head::Kind::Detached { target, peeled } => Head::Detached {
                oid: from_gix(peeled.as_ref().unwrap_or(target).as_ref()),
            },
            gix::head::Kind::Unborn(name) => Head::Unborn {
                name: short_ref(name.as_ref()),
            },
            gix::head::Kind::Symbolic(reference) => {
                let name = short_ref(reference.name.as_ref());
                match reference.target.try_id() {
                    Some(oid) => Head::Branch {
                        name,
                        oid: from_gix(oid),
                    },
                    None => Head::Unborn { name },
                }
            }
        }
    }

    /// The commit, if there is one.
    #[must_use]
    pub fn oid(&self) -> Option<Oid> {
        match self {
            Head::Branch { oid, .. } | Head::Detached { oid } => Some(*oid),
            Head::Unborn { .. } => None,
        }
    }

    /// The branch name, if `HEAD` is symbolic.
    #[must_use]
    pub fn branch_name(&self) -> Option<&str> {
        match self {
            Head::Branch { name, .. } | Head::Unborn { name } => Some(name),
            Head::Detached { .. } => None,
        }
    }

    /// Is `HEAD` detached?
    #[must_use]
    pub fn is_detached(&self) -> bool {
        matches!(self, Head::Detached { .. })
    }

    /// Is this the state a fresh repository starts in?
    #[must_use]
    pub fn is_unborn(&self) -> bool {
        matches!(self, Head::Unborn { .. })
    }

    /// One line for a human: `main@a1b2c3d`, `detached@a1b2c3d`, `main (no commits yet)`.
    #[must_use]
    pub fn summary(&self) -> String {
        match self {
            Head::Branch { name, oid } => {
                format!("{name}@{}", short_hex(oid))
            }
            Head::Unborn { name } => format!("{name} (no commits yet)"),
            Head::Detached { oid } => format!("detached@{}", short_hex(oid)),
        }
    }
}

pub(crate) fn short_hex(oid: &Oid) -> String {
    oid.hex()[..7].to_owned()
}

pub(crate) fn short_ref(name: &gix::refs::FullNameRef) -> String {
    let name: &gix::bstr::BStr = name.into();
    let text = crate::paths::to_str_lossy(name);
    if let Some(branch) = text.strip_prefix("refs/heads/") {
        return branch.to_owned();
    }
    text.into_owned()
}

/// A reference and where it points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ref {
    /// The full name, e.g. `refs/heads/main`.
    pub name: String,
    /// What kind of name that is.
    pub kind: RefKind,
    /// The target.
    pub target: RefTarget,
    /// The peeled id, if the ref store recorded one (annotated tags).
    pub peeled: Option<Oid>,
}

impl Ref {
    /// The name without its `refs/…` prefix, for display.
    #[must_use]
    pub fn short_name(&self) -> &str {
        for prefix in ["refs/heads/", "refs/tags/", "refs/remotes/", "refs/notes/"] {
            if let Some(stripped) = self.name.strip_prefix(prefix) {
                return stripped;
            }
        }
        &self.name
    }
}

/// What a reference is, by its name — git's own rule, which is that the namespace is the type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RefKind {
    /// `refs/heads/*`.
    Branch,
    /// `refs/tags/*`.
    Tag,
    /// `refs/remotes/*`.
    RemoteBranch,
    /// `refs/notes/*`.
    Note,
    /// `refs/stash`.
    Stash,
    /// Anything else: `refs/bisect/*`, `refs/replace/*`, hand-made namespaces.
    Other,
}

impl RefKind {
    fn classify(name: &str) -> Self {
        if name.starts_with("refs/heads/") {
            RefKind::Branch
        } else if name.starts_with("refs/tags/") {
            RefKind::Tag
        } else if name.starts_with("refs/remotes/") {
            RefKind::RemoteBranch
        } else if name.starts_with("refs/notes/") {
            RefKind::Note
        } else if name == "refs/stash" {
            RefKind::Stash
        } else {
            RefKind::Other
        }
    }
}

/// A reference target: a commit, or another ref.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefTarget {
    /// A direct target.
    Direct {
        /// The object.
        oid: Oid,
    },
    /// A symbolic target.
    Symbolic {
        /// The name it points at.
        to: String,
    },
}

/// Which refs to list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RefFilter {
    /// Everything in the ref store.
    #[default]
    All,
    /// `refs/heads/*` only.
    LocalBranches,
    /// `refs/remotes/*` only.
    RemoteBranches,
    /// `refs/tags/*` only.
    Tags,
}

/// The kind and size of an object, as far as a header read says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectHeader {
    /// The object.
    pub oid: Oid,
    /// What it is.
    pub kind: ObjectKind,
    /// Its decompressed size in bytes.
    pub size: u64,
}

/// What the repository looks like to a human asking "is this whole?".
#[derive(Debug, Clone)]
pub struct RepoHealth {
    /// `HEAD` and its state.
    pub head: Head,
    /// Shallow-clone state.
    pub shallow: Shallow,
    /// Filter-clone state.
    pub partial: PartialClone,
    /// Whether there is a working tree, and whether it is there.
    pub worktree: WorktreePresence,
    /// Whether there is an index, and where.
    pub index: IndexPresence,
    /// How many refs of each kind.
    pub refs: RefCounts,
    /// How many submodules are declared.
    pub submodules: usize,
    /// What a human should be told, in order of interest.
    pub notices: Vec<HealthNotice>,
}

/// Shallow-clone state.
#[derive(Debug, Clone, Default)]
pub struct Shallow {
    /// Is the clone shallow?
    pub is_shallow: bool,
    /// The graft commits.
    pub boundary: Vec<Oid>,
}

/// Filter-clone (partial clone) state.
#[derive(Debug, Clone, Default)]
pub struct PartialClone {
    /// Was the clone filtered?
    pub is_partial: bool,
    /// The filter, e.g. `tree:0` or `blob:none`.
    pub filter: Option<String>,
    /// The remote named by `extensions.partialclone`.
    pub remote: Option<String>,
    /// Remotes marked `promisor = true`.
    pub promisor_remotes: Vec<String>,
}

/// Whether a working tree is present.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreePresence {
    /// A working tree is checked out here.
    Present {
        /// Where.
        path: PathBuf,
    },
    /// The repository is bare.
    Bare,
    /// The working tree the repository points at is not there.
    Missing {
        /// Where it should have been.
        expected: PathBuf,
    },
}

/// Whether an index is present.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexPresence {
    /// There is an index file.
    Present {
        /// Where.
        path: PathBuf,
    },
    /// There is none — a fresh repository, or a bare one.
    Absent {
        /// Where one would be.
        expected: PathBuf,
    },
}

/// How many refs there are, by kind.
#[derive(Debug, Clone, Default)]
pub struct RefCounts {
    /// Local branches.
    pub branches: usize,
    /// Remote-tracking branches.
    pub remote_branches: usize,
    /// Tags.
    pub tags: usize,
    /// Everything else.
    pub other: usize,
}

impl RefCounts {
    /// The total.
    #[must_use]
    pub fn total(&self) -> usize {
        self.branches + self.remote_branches + self.tags + self.other
    }
}

/// How many local branches earn a `ManyBranches` notice.
///
/// A number plucked from the shape of the problem rather than from a measurement: below it, the branch list
/// is something a reviewer has; above it, it is a list somebody maintains, and the rail's ref chooser is
/// where they will feel it. It is a note, not a finding — nothing about it is wrong.
const MANY_BRANCHES: usize = 100;

/// Something a human should know about a repository before trusting what fathom says about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthNotice {
    /// `HEAD` is detached, which is a real state in CI and in a worktree and is worth naming because
    /// "what changed since the branch" has no answer.
    DetachedHead {
        /// The commit.
        at: Oid,
    },
    /// The repository has no commits yet.
    UnbornHead {
        /// The branch.
        branch: String,
    },
    /// History is shallow.
    Shallow {
        /// How many grafts.
        boundary: usize,
    },
    /// The clone is filtered, so objects will be missing.
    PartialClone {
        /// The filter, if recorded.
        filter: Option<String>,
    },
    /// There is no working tree, so there is no staged/unstaged state to read.
    Bare,
    /// The working tree is missing.
    WorktreeMissing {
        /// Where it should be.
        expected: PathBuf,
    },
    /// There is no index.
    NoIndex,
    /// The repository belongs to another user and was read with reduced permissions.
    Untrusted,
    /// A symbolic ref that points at nothing.
    BrokenSymbolicRef {
        /// The ref that is broken.
        name: String,
        /// What it points at.
        to: String,
    },
    /// More local branches than a reviewer can hold in their head, which is a triage question rather than
    /// a fault. The count is free at health time (the refs are already read); the dates are not, and a
    /// "stale" verdict would need a definition of stale fathom has no business inventing.
    ManyBranches {
        /// How many local branches there are.
        branches: usize,
    },
    /// The object store is on a different filesystem from the working tree — the shape of a network mount,
    /// a `--separate-git-dir`, or a `.git` moved to another volume. Reading is fine; *watching* is the part
    /// that lies, because inotify does not report changes made through another host's mount (ADR-004 §7.8).
    SeparateFilesystemStore {
        /// Where the object store is.
        git_dir: PathBuf,
        /// Where the working tree is.
        work_dir: PathBuf,
    },
}

impl std::fmt::Display for HealthNotice {
    /// One notice, in the sentence a human reads.
    ///
    /// This is the text the `--json` `notices` array carries and the text the status line shortens; the
    /// `{:?}` of the enum was never meant to be a user-visible string, and a machine-readable report that
    /// quotes Rust is a report whose key names change when someone renames a variant.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HealthNotice::DetachedHead { at } => {
                write!(f, "HEAD is detached at {}", at.hex())
            }
            HealthNotice::UnbornHead { branch } => {
                write!(f, "branch {branch} has no commits yet")
            }
            HealthNotice::Shallow { boundary } => write!(
                f,
                "history is shallow: it stops at {boundary} graft(s), so older history is absent rather than empty"
            ),
            HealthNotice::PartialClone { filter } => match filter.as_deref() {
                Some(filter) => write!(
                    f,
                    "the clone was filtered ({filter}), so objects it never fetched are missing"
                ),
                None => {
                    f.write_str("the clone was filtered, so objects it never fetched are missing")
                }
            },
            HealthNotice::Bare => f.write_str("bare repository: there is no working tree to read"),
            HealthNotice::WorktreeMissing { expected } => {
                write!(
                    f,
                    "the working tree is missing: nothing at {}",
                    expected.display()
                )
            }
            HealthNotice::NoIndex => f.write_str("there is no index file"),
            HealthNotice::Untrusted => f.write_str(
                "the repository belongs to another user, so it was read at arm's length",
            ),
            HealthNotice::BrokenSymbolicRef { name, to } => {
                write!(f, "the ref {name} points at {to}, which does not exist")
            }
            HealthNotice::ManyBranches { branches } => write!(
                f,
                "the repository carries {branches} local branches, each one a lineage this rail can be \
                 pointed at"
            ),
            HealthNotice::SeparateFilesystemStore { git_dir, .. } => write!(
                f,
                "the object store is on a different filesystem from the working tree ({}) — a network \
                 mount, or a moved `--separate-git-dir`",
                git_dir.display()
            ),
        }
    }
}

impl Repository {
    fn compute_health(&self) -> Result<RepoHealth, Error> {
        let head = self.head()?;
        let boundary = self.shallow_boundary()?.unwrap_or_default();
        let shallow = Shallow {
            is_shallow: !boundary.is_empty(),
            boundary,
        };
        let partial = self.partial_clone();
        let worktree = match self.identity.work_dir.as_ref() {
            None => WorktreePresence::Bare,
            Some(path) => {
                if path.is_dir() {
                    WorktreePresence::Present { path: path.clone() }
                } else {
                    WorktreePresence::Missing {
                        expected: path.clone(),
                    }
                }
            }
        };
        let index_path = self.inner.index_path();
        let index = if index_path.is_file() {
            IndexPresence::Present { path: index_path }
        } else {
            IndexPresence::Absent {
                expected: index_path,
            }
        };
        let refs = self.refs(RefFilter::All)?;
        let mut counts = RefCounts::default();
        let mut notices: Vec<HealthNotice> = Vec::new();
        for reference in &refs {
            match reference.kind {
                RefKind::Branch => counts.branches += 1,
                RefKind::Tag => counts.tags += 1,
                RefKind::RemoteBranch => counts.remote_branches += 1,
                RefKind::Note | RefKind::Stash | RefKind::Other => counts.other += 1,
            }
            if let RefTarget::Symbolic { to } = &reference.target
                && to != "HEAD"
                && self.resolve(to).is_err()
            {
                notices.push(HealthNotice::BrokenSymbolicRef {
                    name: reference.name.clone(),
                    to: to.clone(),
                });
            }
        }
        let submodules = self.submodules()?.len();
        if counts.branches > MANY_BRANCHES {
            notices.push(HealthNotice::ManyBranches {
                branches: counts.branches,
            });
        }
        // Only on unix, and only a fact about devices: what it says is that a watcher here must poll, and
        // the remedy is printed with it (`fathom watch --backend poll`).
        #[cfg(unix)]
        if let Some(work_dir) = self.identity.work_dir.as_ref() {
            use std::os::unix::fs::MetadataExt as _;
            let git_dir = self.git_dir();
            if let (Ok(work), Ok(git)) = (std::fs::metadata(work_dir), std::fs::metadata(git_dir))
                && work.dev() != git.dev()
            {
                notices.push(HealthNotice::SeparateFilesystemStore {
                    git_dir: git_dir.to_path_buf(),
                    work_dir: work_dir.clone(),
                });
            }
        }
        match &head {
            Head::Detached { oid } => notices.push(HealthNotice::DetachedHead { at: *oid }),
            Head::Unborn { name } => notices.push(HealthNotice::UnbornHead {
                branch: name.clone(),
            }),
            Head::Branch { .. } => {}
        }
        if shallow.is_shallow {
            notices.push(HealthNotice::Shallow {
                boundary: shallow.boundary.len(),
            });
        }
        if partial.is_partial {
            notices.push(HealthNotice::PartialClone {
                filter: partial.filter.clone(),
            });
        }
        match &worktree {
            WorktreePresence::Bare => notices.push(HealthNotice::Bare),
            WorktreePresence::Missing { expected } => notices.push(HealthNotice::WorktreeMissing {
                expected: expected.clone(),
            }),
            WorktreePresence::Present { .. } => {}
        }
        if matches!(index, IndexPresence::Absent { .. }) {
            notices.push(HealthNotice::NoIndex);
        }
        if !self.identity.trusted {
            notices.push(HealthNotice::Untrusted);
        }
        Ok(RepoHealth {
            head,
            shallow,
            partial,
            worktree,
            index,
            refs: counts,
            submodules,
            notices,
        })
    }
}

/// One entry of a tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    /// The name inside the tree (not a path: no parent directories), in git's bytes.
    pub name: RepoPath,
    /// The raw mode, e.g. `0o100644`.
    pub mode: u16,
    /// What the mode means.
    pub kind: TreeEntryKind,
    /// The object it points at.
    pub oid: Oid,
}

impl TreeEntry {
    /// The name as text, for display only; `as_bytes()` is the one that round-trips.
    #[must_use]
    pub fn name_text(&self) -> std::borrow::Cow<'_, str> {
        self.name.to_string_lossy()
    }
}

/// What a tree entry points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeEntryKind {
    /// A file.
    File,
    /// An executable file.
    Executable,
    /// A symlink.
    Symlink,
    /// A submodule (a gitlink: a commit id recorded in a tree).
    Submodule,
    /// A tree (directory).
    Tree,
}

impl TreeEntryKind {
    pub(crate) fn from_mode_value(mode: u32) -> Self {
        match mode & 0o170000 {
            0o040000 => TreeEntryKind::Tree,
            0o120000 => TreeEntryKind::Symlink,
            0o160000 => TreeEntryKind::Submodule,
            0o100755 => TreeEntryKind::Executable,
            _ => TreeEntryKind::File,
        }
    }
}

/// Whether the repository has an index, and where it is.
///
/// A missing index is a state, not an error: `git init` and then nothing leaves a repository with commits
/// and no index, and `fathom status` on it has to say "no index" rather than crash.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlobRead {
    /// The bytes.
    Present {
        /// The blob.
        oid: Oid,
        /// Its content.
        bytes: Vec<u8>,
    },
    /// Bigger than the caller's cap, so the content was not read (see `ingest.file_size_cap.mib`).
    Oversized {
        /// The actual size.
        size: usize,
    },
    /// The object exists but is not a blob.
    WrongKind {
        /// What it is.
        kind: ObjectKind,
    },
}

/// An object's kind, as fathom names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectKind {
    /// A commit.
    Commit,
    /// A tree.
    Tree,
    /// A blob.
    Blob,
    /// An annotated tag.
    Tag,
}

impl From<gix::objs::Kind> for ObjectKind {
    fn from(kind: gix::objs::Kind) -> Self {
        match kind {
            gix::objs::Kind::Commit => ObjectKind::Commit,
            gix::objs::Kind::Tree => ObjectKind::Tree,
            gix::objs::Kind::Blob => ObjectKind::Blob,
            gix::objs::Kind::Tag => ObjectKind::Tag,
        }
    }
}

impl ObjectKind {
    /// The git name: `commit`, `tree`, `blob`, `tag`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ObjectKind::Commit => "commit",
            ObjectKind::Tree => "tree",
            ObjectKind::Blob => "blob",
            ObjectKind::Tag => "tag",
        }
    }
}

/// A submodule declared by this repository, and what state it is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submodule {
    /// The name in `.gitmodules`.
    pub name: String,
    /// Where it is checked out, repository-relative.
    pub path: RepoPath,
    /// The URL as declared.
    pub url: Option<String>,
    /// Whether `[submodule.<name>] active = true` in the local config.
    pub active: bool,
    /// The commit the superproject records for it (the gitlink).
    pub recorded: Option<Oid>,
    /// What state the checkout is in.
    pub state: SubmoduleState,
}

/// Whether a submodule's checkout is present and matches what is recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmoduleState {
    /// Not initialised: no git directory for it at all.
    Uninitialized,
    /// Initialised but empty — the directory exists with nothing checked out.
    Empty,
    /// Checked out at the recorded commit.
    AtRecorded {
        /// The commit it is at.
        at: Oid,
    },
    /// Checked out at a different commit than recorded — `git status` shows this as modified content.
    AtDifference {
        /// Recorded commit.
        recorded: Oid,
        /// Actual commit.
        actual: Oid,
    },
    /// The submodule's own git directory cannot be read.
    Unreadable {
        /// Why.
        reason: String,
    },
}

/// One worktree of a repository, as `git worktree list` would print it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedWorktree {
    /// The registered name (`git worktree add`'s last argument, or the directory name).
    pub name: String,
    /// Where the working tree is.
    pub path: PathBuf,
    /// What its `HEAD` says, or `None` when it could not be read (a prunable worktree whose directory
    /// is gone, for instance).
    pub head: Option<Head>,
    /// Whether this is the main worktree.
    pub is_main: bool,
    /// Whether its directory is gone (prunable).
    pub missing: bool,
}
