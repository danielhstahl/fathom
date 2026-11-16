//! `fathom open` and `fathom difftool` — leaving the reader on purpose (`e5.8`, `reco-dmw`).
//!
//! Fathom is a reader, not an editor (ADR-001 §8 non-goal 2), and the ticket this module exists for is
//! the sentence that follows it: *make the exits good*. Two exits are built here, both of them the
//! shell's, because both need things `fathom-tui` may not touch — a store read, a working tree, a
//! subprocess, and a terminal to hand over.
//!
//! * **`fathom open <episode>`** jumps to a file at a line. The episode handle grammar is the one every
//!   other verb accepts (id, rev, oid, prefix); the target is a file the episode actually touched, and
//!   the line is the first `@@` header the index recorded for it — the same header the diff pane counts
//!   hunks with, so a `+42` in `$EDITOR` and a `[` in the pane agree (ADR-001 §7.10). Nothing runs
//!   before it is printed: `--print` shows the exact command and the sentence for why *that* editor got
//!   *that* argument, and `--list` shows every jump the episode would offer.
//! * **`fathom difftool <episode>`** writes one file's two sides into a directory of their own and
//!   hands the pair to a heavyweight diff tool — difftastic, meld, kdiff3, `vim -d`, `code --diff`.
//!   The reviewer's own configuration chooses the tool; a clone's configuration does not (see
//!   [`crate::handoff`] on who may name a program this process runs), and a tool that is not installed
//!   is a stated fallback with the pair already written, not a dead end.
//!
//! # Why the plan is printed before it is run
//!
//! Both verbs go through [`fathom_core::handoff`], which returns a [`Command`] carrying a `why` instead
//! of running it. That buys the two acceptance criteria outright: a machine with no editor and no
//! difftool is an ordinary, well-explained place to be (`--print` works with nothing installed), and a
//! run that chose for you says which choice it made — so "it opened at the top" is a question with an
//! answer in the transcript rather than a mystery.
//!
//! # Links
//!
//! Paths and commits are printed through [`crate::links`]: on a terminal that advertises OSC-8 the file
//! and the commit are clickable, and on one that does not they are the same seven characters and the
//! same path, unlinked. Nothing else on the line moves either way, which is what makes it safe to wire
//! into a printer at all.

use std::path::{Path, PathBuf};

use eyre::Result;
use fathom_core::exit::ExitCode;
use fathom_core::handoff::{Command, Difftool, Editor, Os, difftool_plan, editor_plan};

use crate::handoff::Settings;
use crate::links::Printer;
use crate::output;

/// A file an episode touched, with the line worth jumping to.
#[derive(Debug, Clone)]
struct Target {
    /// The path as the index names it, in the work tree.
    path: String,
    /// The rename/copy origin, when a row recorded one.
    old_path: Option<String>,
    /// `add`, `modify`, `delete`, `rename` or `copy`, as the spine stored it.
    kind: String,
    /// The first line the file's first hunk touches on the new side, when it has one.
    line: Option<u32>,
    /// Lines added and removed, for the listing a reviewer chooses from.
    add: i64,
    del: i64,
    /// The row says the content is binary, so no line argument means anything.
    binary: bool,
    /// What kind of file the repository's own path taxonomy says this is — `source`, `test`, `docs`,
    /// `dependency`, `generated`, `vendored`, `binary`, and so on. Printed in `--list` and used to rank
    /// the default, because being offered a jump into a lockfile is a thing a reviewer should be able
    /// to see the reason for.
    ///
    /// `risk::classify` is asked rather than the stored columns alone: today's indexer writes
    /// `path_class = unknown` for every row, and `classify` is the one place that already says an
    /// unclassified `Cargo.lock` is a dependency by name (docs/concepts/paths.md §1). Re-deriving it
    /// here is not the re-derivation ADR-004 §4.2 forbids — the stored answer is consulted first, and
    /// the name table is the documented fallback for the rows that have none.
    surface: &'static str,
    /// The blob pair, for `fathom difftool`. The old side is `None` on a creation.
    blob_old: Option<String>,
    blob_new: Option<String>,
}

impl Target {
    /// Can this file be jumped into? A deletion has no new side to open, and a binary has no line.
    fn jumpable(&self) -> bool {
        self.kind != "delete" && !self.binary
    }
}

/// One episode's worth of jump targets, oldest member first.
#[cfg(feature = "sqlite-bundled")]
struct Episode {
    /// `episode` or `commit`: which half of the handle grammar the argument was.
    kind: &'static str,
    /// The handle as the reviewer wrote it, shortened the way every other surface shortens it.
    handle: String,
    /// The member commits, oldest first — the newest is what a difftool pair's new side comes from.
    members: Vec<String>,
    targets: Vec<Target>,
    /// Members the index has no row for — a stated gap, never a silently shorter episode.
    missing: Vec<String>,
}

#[cfg(feature = "sqlite-bundled")]
impl Episode {
    /// The handle in the words an error message uses: `episode 4f2b9c1`, `commit b999c15`.
    fn name(&self) -> String {
        format!("{} {}", self.kind, short(&self.handle))
    }

    /// The newest member's oid.
    fn oid(&self) -> String {
        self.members.last().cloned().unwrap_or_default()
    }

    /// The line both verbs open with: what was asked for, how big it is, and — linked, where the
    /// terminal asked for links — the commit the change lands on.
    fn heading(&self, links: &Printer) -> String {
        match self.kind {
            "episode" => format!(
                "episode {} · {} commit(s) · {}",
                short(&self.handle),
                self.members.len(),
                links.commit(&self.oid())
            ),
            _ => format!("commit {}", links.commit(&self.oid())),
        }
    }
}

/// `fathom open <EPISODE>`: open the file an episode touched, at the line it touched.
///
/// # Errors
///
/// A `Pending` for every user-visible refusal — no index, no such episode, no such path, the path is not
/// in the working tree, no editor — each with the remedy in the hint. An editor that was found and run
/// leaves with *its* exit status, because `:cq` is a cancelled edit and a wrapper that reports success
/// after that is a wrapper that lies.
/// The signature is the flag list, one argument per flag, so that the verb's contract is
/// readable without a struct in the way (`drift` does the same).
#[allow(clippy::too_many_arguments)]
pub fn open(
    root: &str,
    episode: Option<&str>,
    path: Option<&str>,
    line: Option<u32>,
    editor: Option<&str>,
    print: bool,
    list: bool,
    as_json: bool,
) -> Result<ExitCode> {
    #[cfg(not(feature = "sqlite-bundled"))]
    {
        let _ = (root, episode, path, line, editor, print, list, as_json);
        return Err(crate::verbs::feature_absent(
            "fathom open",
            "sqlite-bundled",
        ));
    }

    #[cfg(feature = "sqlite-bundled")]
    {
        let root = Path::new(root);
        let settings = Settings::from_config(root);
        for notice in &settings.notices {
            output::problem(format_args!("fathom: {notice}"));
        }
        // No handle is not a question — it is "what did I just change", which is `HEAD`'s episode. The
        // heading the verb prints says which episode that turned out to be, so the default is never
        // guessed at by the reader.
        let asked = episode.unwrap_or("HEAD");
        let opened = load(root, asked)?;
        let links = Printer::for_root(root);
        if let Some(notice) = &links.notice {
            output::problem(format_args!("fathom: {notice}"));
        }
        if list {
            return list_targets(&opened, &links, as_json);
        }
        let target = choose(&opened, path, line)?;
        if !target.jumpable() {
            return Err(crate::verbs::pending(
                ExitCode::Error,
                format!(
                    "`{}` cannot be opened: it is {} in this episode",
                    target.path,
                    if target.binary { "binary" } else { "deleted" }
                ),
                Some(format!(
                    "`fathom open {} --list` shows what else this episode touched",
                    opened.name()
                )),
            ));
        }
        let absolute = work_tree(root).join(&target.path);
        let in_work_tree = absolute.is_file();
        let shown = absolute.to_string_lossy().into_owned();
        // `--editor` is asked for by name rather than smuggled into a variable the planner would then
        // report as having found: a plan that says "from $EDITOR" about a value nobody exported is the
        // beginning of a confusing bug report.
        let plan = editor_plan(
            Os::current(),
            &shown,
            target.line.unwrap_or(0),
            editor.filter(|value| !value.trim().is_empty()),
            &crate::handoff::environment_with(&settings),
        );
        let command = match plan {
            Editor::Open(command) => command,
            Editor::Unavailable { why } => {
                return Err(crate::verbs::pending(
                    ExitCode::Error,
                    format!("no editor to open it with: {why}"),
                    Some(
                        "set $EDITOR (or $FATHOM_EDITOR for fathom alone), or `[ui] editor` in \
                         ~/.config/fathom/config.toml; `--editor vim` names one for this run only"
                            .to_owned(),
                    ),
                ));
            }
        };
        report_open(
            &opened,
            &target,
            &command,
            &links,
            print,
            as_json,
            line,
            in_work_tree,
        );
        if print {
            return Ok(ExitCode::Ok);
        }
        // Only now: a plan is a plan whatever is checked out, and `--print` exists to be run where
        // nothing is. Launching an editor at a path that is not there would be the editor's problem
        // rather than ours, and the reviewer would learn about it from the editor.
        if !in_work_tree {
            return Err(crate::verbs::pending(
                ExitCode::Error,
                format!(
                    "`{}` is not in the working tree, so there is nothing to open there",
                    target.path
                ),
                Some(
                    "check the branch out, or name a file that is there with --path; the plan above is \
                     what would have run, and `--print` prints it without needing the file at all"
                        .to_owned(),
                ),
            ));
        }
        match crate::handoff::run(&command) {
            Ok(status) => Ok(exit_of(status, &command)),
            Err(why) => Err(crate::verbs::pending(
                ExitCode::Error,
                why,
                Some(
                    "the plan above is the exact command; `--print` reprints it without running it"
                        .to_owned(),
                ),
            )),
        }
    }
}

/// `fathom difftool <EPISODE>`: one file's two sides, in the diff tool the reviewer configured.
///
/// # Errors
///
/// A `Pending` per refusal, including "no difftool is installed" — which is a normal outcome with a
/// remedy (and a pair of written files) rather than a fault.
/// The signature is the flag list, one argument per flag, so that the verb's contract is
/// readable without a struct in the way (`drift` does the same).
#[allow(clippy::too_many_arguments)]
pub fn difftool(
    root: &str,
    episode: Option<&str>,
    path: Option<&str>,
    tool: Option<&str>,
    print: bool,
    list: bool,
    keep: bool,
    as_json: bool,
) -> Result<ExitCode> {
    #[cfg(not(feature = "sqlite-bundled"))]
    {
        let _ = (root, episode, path, tool, print, list, keep, as_json);
        return Err(crate::verbs::feature_absent(
            "fathom difftool",
            "sqlite-bundled",
        ));
    }

    #[cfg(feature = "sqlite-bundled")]
    {
        let root = Path::new(root);
        let settings = Settings::from_config(root);
        for notice in &settings.notices {
            output::problem(format_args!("fathom: {notice}"));
        }
        let asked = episode.unwrap_or("HEAD");
        let opened = load(root, asked)?;
        let links = Printer::for_root(root);
        if let Some(notice) = &links.notice {
            output::problem(format_args!("fathom: {notice}"));
        }
        if list {
            return list_targets(&opened, &links, as_json);
        }
        let target = choose(&opened, path, None)?;
        let (request, source) = settings.difftool_request(tool);
        let mut staged = stage(root, &opened, &target)?;
        let plan = difftool_plan(
            Os::current(),
            request.as_deref(),
            &staged.left.display().to_string(),
            &staged.right.display().to_string(),
            &crate::handoff::environment_with(&settings),
        );
        let command = match plan {
            Difftool::Open(command) => command,
            Difftool::Unavailable { why, tried } => {
                // The files are already written, so the refusal is not the end of the review: name the
                // tools that were looked for, the keys that choose one, and where the pair is.
                staged.keep();
                return Err(crate::verbs::pending(
                    ExitCode::Error,
                    format!("no difftool to show it with: {why}"),
                    Some(format!(
                        "looked for: {}; `--tool NAME`, $FATHOM_DIFFTOOL or `[diff] difftool` in \
                         ~/.config/fathom/config.toml chooses one. The two files are written either \
                         way, so any diff tool at all will do — {}",
                        tried.join(", "),
                        staged.describe()
                    )),
                ));
            }
        };
        report_difftool(&opened, &target, &command, &source, &links, print, as_json);
        if print {
            // `--print` is the "show me, run nothing" door, so the pair outlives the run by design and
            // the message says where it is.
            staged.keep();
            return Ok(ExitCode::Ok);
        }
        let outcome = crate::handoff::run(&command);
        if keep {
            staged.keep();
            output::problem(format_args!(
                "fathom: kept the two files — {}",
                staged.describe()
            ));
        }
        match outcome {
            Ok(status) => Ok(match status.code() {
                // Several difftools — `diff` itself foremost — use a non-zero exit for "the files
                // differ", which is not a failure of the handoff. Say what the number was; keep the
                // verb's own code clean so a script can tell "no difftool" (1) from "ran, differs" (0).
                Some(0) => ExitCode::Ok,
                Some(code) => {
                    output::problem(format_args!(
                        "fathom: `{}` exited {code}; several difftools use a non-zero exit for \
                         \"the files differ\", so the handoff counts as done",
                        command.program
                    ));
                    ExitCode::Ok
                }
                None => {
                    output::problem(format_args!(
                        "fathom: `{}` was killed by a signal",
                        command.program
                    ));
                    ExitCode::Error
                }
            }),
            Err(why) => Err(crate::verbs::pending(
                ExitCode::Error,
                why,
                Some(
                    "the plan above is the exact command; `--print` reprints it and `--keep` leaves \
                     the two files behind to diff by hand"
                        .to_owned(),
                ),
            )),
        }
    }
}

/// An editor's exit status becomes the verb's, with a word when it was not zero.
fn exit_of(status: std::process::ExitStatus, command: &Command) -> ExitCode {
    match status.code() {
        Some(0) => ExitCode::Ok,
        Some(code) => {
            output::problem(format_args!("fathom: `{}` exited {code}", command.program));
            ExitCode::Error
        }
        None => {
            output::problem(format_args!(
                "fathom: `{}` was killed by a signal",
                command.program
            ));
            ExitCode::Error
        }
    }
}

/// The work tree the editor will be handed, absolute.
fn work_tree(root: &Path) -> PathBuf {
    let base = match fathom_ingest::repo::Repository::discover(root) {
        Ok(repo) => repo
            .work_dir()
            .map_or_else(|| root.to_path_buf(), |path| path.to_path_buf()),
        Err(_) => root.to_path_buf(),
    };
    base.canonicalize().unwrap_or(base)
}

/// Read the episode and its file rows out of the index.
#[cfg(feature = "sqlite-bundled")]
fn load(root: &Path, asked: &str) -> Result<Episode> {
    use crate::state::REMEDY_INDEX;
    use crate::verbs::{Pending, failed};
    use fathom_ingest::repo::Repository;
    use fathom_store::spine::Store;

    let store_path = fathom_analyze::index::default_store_path(root);
    if !store_path.is_file() {
        return Err(Pending::new(
            ExitCode::IndexDegraded,
            format!(
                "no index at {} yet, so nothing can be jumped to",
                crate::sanitize::Redactor::for_root(root).path(&store_path)
            ),
            Some(REMEDY_INDEX.to_owned()),
        )
        .into());
    }
    let store = Store::open(&store_path).map_err(failed)?;
    let repository = Repository::discover(root).map_err(failed)?;
    // The same two questions `fathom analyze` asks in the same order: an episode id first, because it is
    // cheaper and cannot be confused with a rev, then the revspec grammar every other verb uses.
    let (kind, handle, members) = match store.episode(asked) {
        Ok(Some(row)) => ("episode", row.id.clone(), row.members.clone()),
        _ => {
            let oid = repository.resolve(asked).map_err(|err| {
                Pending::new(
                    ExitCode::Error,
                    format!("`{asked}` names nothing in this repository: {err}"),
                    Some(
                        "the grammar is `HEAD`, `@`, `main~3`, `HEAD^2`, a full oid or a hex prefix; \
                         an episode id works too"
                            .to_owned(),
                    ),
                )
            })?;
            let hex = oid.hex();
            ("commit", hex.clone(), vec![hex])
        }
    };
    let mut targets: Vec<Target> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    for member in &members {
        let row = match store.read_commit(member) {
            Ok(row) => row,
            Err(_) => {
                missing.push(member.clone());
                continue;
            }
        };
        for file in &row.files {
            let target = Target {
                path: file.new_path.clone(),
                old_path: file.old_path.clone(),
                kind: file.change_kind.clone(),
                line: first_hunk_line(&file.hunk_headers),
                add: file.n_add,
                del: file.n_del,
                binary: file.is_binary,
                surface: fathom_analyze::risk::classify(
                    &file.new_path,
                    &file.path_class,
                    &file.noise_class,
                )
                .as_str(),
                blob_old: file.blob_old.clone(),
                blob_new: file.blob_new.clone(),
            };
            // A path a later member touched has the last word on it: the line and the blobs come from
            // that last touch, which is the version worth jumping to. The size does not — an episode's
            // 40-line file is 40 lines however it was spread over its members, and `episode_files`
            // sums the same way for the same reason.
            match targets
                .iter_mut()
                .find(|existing| existing.path == target.path)
            {
                Some(existing) => {
                    let (add, del) = (existing.add + target.add, existing.del + target.del);
                    *existing = target;
                    existing.add = add;
                    existing.del = del;
                }
                None => targets.push(target),
            }
        }
    }
    if targets.is_empty() {
        let why = if missing.is_empty() {
            "the index has no file rows for it".to_owned()
        } else {
            format!(
                "the index has no row for {} of its {} commit(s)",
                missing.len(),
                members.len()
            )
        };
        return Err(Pending::new(
            ExitCode::IndexDegraded,
            format!("`{}` cannot be jumped into: {why}", handle),
            Some(REMEDY_INDEX.to_owned()),
        )
        .into());
    }
    Ok(Episode {
        kind,
        handle,
        members,
        targets,
        missing,
    })
}

/// The line to jump at: the new-side start of the first `@@` header, which is where the pane's `[`
/// takes a reviewer too. A file with no headers — a pure rename, a mode change — has no line, and the
/// editor opens at the top and says so.
fn first_hunk_line(headers: &[String]) -> Option<u32> {
    headers.iter().find_map(|header| {
        // `@@ -12,7 +12,9 @@ fn parse` — the new side is the `+` half, whose first number is the hunk's
        // first line. A one-line hunk omits the count (`+12`), which parses the same way.
        let middle = header.split('+').nth(1)?;
        let digits: String = middle.chars().take_while(char::is_ascii_digit).collect();
        digits.parse::<u32>().ok().filter(|line| *line > 0)
    })
}

/// Pick the file to act on: the exact path, then a unique suffix, then the one worth reading.
#[cfg(feature = "sqlite-bundled")]
fn choose(episode: &Episode, want: Option<&str>, line: Option<u32>) -> Result<Target> {
    let mut target = match want {
        Some(want) => {
            let wanted = want.trim_start_matches("./");
            if let Some(exact) = episode.targets.iter().find(|target| target.path == wanted) {
                (*exact).clone()
            } else {
                let matches: Vec<&Target> = episode
                    .targets
                    .iter()
                    .filter(|target| target.path.ends_with(wanted))
                    .collect();
                match matches.len() {
                    1 => (*matches[0]).clone(),
                    0 => {
                        return Err(crate::verbs::pending(
                            ExitCode::Error,
                            format!("`{}` did not touch `{want}`", episode.name()),
                            Some(format!(
                                "`fathom open {} --list` lists its {} file(s)",
                                episode.name(),
                                episode.targets.len()
                            )),
                        ));
                    }
                    many => {
                        // More than one file ends with what was typed: say which, rather than choosing
                        // the first and hoping the reviewer notices the path in the line under it.
                        return Err(crate::verbs::pending(
                            ExitCode::Usage,
                            format!(
                                "`{want}` matches {many} of the files `{}` touched",
                                episode.name()
                            ),
                            Some(format!(
                                "name more of the path: {}",
                                matches
                                    .iter()
                                    .map(|target| target.path.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            )),
                        ));
                    }
                }
            }
        }
        None => best(&episode.targets),
    };
    if let Some(line) = line {
        target.line = Some(line);
    }
    Ok(target)
}

/// The file worth opening when the reviewer named none: the biggest change to a file a review would
/// read — not a deletion, not a binary, not the surface the path taxonomy has already discounted. The
/// ranking is deliberately dull, because a clever one would be a guess about what the commit was "about":
/// kind first, then size, and the surface name printed next to the result every time so a wrong guess is
/// visible instead of structural.
#[cfg(feature = "sqlite-bundled")]
fn best(targets: &[Target]) -> Target {
    let mut candidates: Vec<&Target> = targets.iter().filter(|target| target.jumpable()).collect();
    if candidates.is_empty() {
        candidates = targets.iter().collect();
    }
    let score = |target: &&Target| {
        // The kinds a review reads past, in the taxonomy's own words. `asset` stays in the running: a
        // commit whose whole point is a screenshot is a commit about a screenshot.
        let discounted = matches!(
            target.surface,
            "dependency" | "generated" | "vendored" | "binary"
        );
        (discounted, -(target.add + target.del))
    };
    (*candidates
        .iter()
        .min_by_key(|target| score(target))
        .copied()
        .unwrap_or(candidates[0]))
    .clone()
}

/// Print every jump the episode offers.
#[cfg(feature = "sqlite-bundled")]
fn list_targets(episode: &Episode, links: &Printer, as_json: bool) -> Result<ExitCode> {
    if as_json {
        let rows: Vec<_> = episode
            .targets
            .iter()
            .map(|target| {
                serde_json::json!({
                    "path": target.path,
                    "kind": target.kind,
                    "line": target.line,
                    "add": target.add,
                    "del": target.del,
                    "binary": target.binary,
                    "surface": target.surface,
                })
            })
            .collect();
        output::line(format_args!(
            "{}",
            serde_json::json!({
                "contract": "fathom.cli.open/1",
                "episode": episode.name(),
                "files": rows,
                "missing_commits": episode.missing,
            })
        ));
        return Ok(ExitCode::Ok);
    }
    output::line(format_args!(
        "{} — {} file(s)",
        episode.name(),
        episode.targets.len()
    ));
    for target in &episode.targets {
        let line = target
            .line
            .map(|line| format!(":{line}"))
            .unwrap_or_else(|| ":—".to_owned());
        output::line(format_args!(
            "  {:<8} {:>5} {:>5}  {:<11} {}{}{}{}",
            target.kind,
            format!("+{}", target.add),
            format!("-{}", target.del),
            target.surface,
            links.path(&target.path),
            line,
            target
                .old_path
                .as_ref()
                .map_or_else(String::new, |old| format!("  (from {old})")),
            if target.jumpable() {
                String::new()
            } else {
                "  (nothing to open)".to_owned()
            }
        ));
    }
    for member in &episode.missing {
        output::line(format_args!(
            "  {} has no row in the index — `fathom index`",
            short(member)
        ));
    }
    Ok(ExitCode::Ok)
}

/// The two files a difftool was handed, and the directory that holds them.
struct Staged {
    dir: PathBuf,
    left: PathBuf,
    right: PathBuf,
    /// Whether to leave them behind. `false` means this drop cleans up.
    kept: bool,
}

impl Staged {
    fn describe(&self) -> String {
        format!("{} vs {}", self.left.display(), self.right.display())
    }
    fn keep(&mut self) {
        self.kept = true;
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        if !self.kept {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

/// Write one file's two sides into a directory of their own, with the real extension kept —
/// difftastic chooses its grammar by suffix, and a `store.before` gets no grammar at all.
#[cfg(feature = "sqlite-bundled")]
fn stage(root: &Path, episode: &Episode, target: &Target) -> Result<Staged> {
    use fathom_ingest::record::Limits;
    use fathom_ingest::repo::{BlobRead, Repository};

    let repository = Repository::discover(root).map_err(|err| {
        crate::verbs::pending(
            ExitCode::NotARepository,
            format!("the object store is not reachable from here: {err}"),
            Some("run fathom inside the repository, where `fathom index` ran".to_owned()),
        )
    })?;
    let cap = usize::try_from(Limits::standard().blob_memory_cap).unwrap_or(usize::MAX);
    let read = |hex: &Option<String>| -> Result<Vec<u8>> {
        let Some(hex) = hex else {
            // No old side is a creation and no new side a deletion; an empty file is the honest
            // representation of a side that did not exist, and every tool in the table draws it as one.
            return Ok(Vec::new());
        };
        let oid = hex.parse::<fathom_ingest::oid::Oid>().map_err(|_| {
            crate::verbs::pending(
                ExitCode::IndexDegraded,
                format!("the index names blob `{hex}`, which is not an oid"),
                Some("fathom index --rebuild".to_owned()),
            )
        })?;
        match repository.blob(oid, cap).map_err(|err| {
            crate::verbs::pending(
                ExitCode::Error,
                format!("the object store would not serve {}: {err}", short(hex)),
                Some(
                    "the object may have been pruned or the clone filtered; `fathom doctor` says \
                      what is missing"
                        .to_owned(),
                ),
            )
        })? {
            BlobRead::Present { bytes, .. } => Ok(bytes),
            BlobRead::Oversized { size } => Err(crate::verbs::pending(
                ExitCode::BudgetExceeded,
                format!(
                    "`{}` is {size} bytes, past the {cap}-byte blob cap, so its contents were never \
                     read into the index",
                    target.path
                ),
                Some("raise `ingest.file_size_cap.mib`, or diff it in git".to_owned()),
            )),
            BlobRead::WrongKind { kind } => Err(crate::verbs::pending(
                ExitCode::IndexDegraded,
                format!(
                    "the index named a {} where a blob belongs for `{}`",
                    kind.as_str(),
                    target.path
                ),
                Some("fathom index --rebuild".to_owned()),
            )),
        }
    };
    let left = read(&target.blob_old)?;
    let right = read(&target.blob_new)?;
    let name = Path::new(&target.path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_owned());
    let dir = std::env::temp_dir().join(format!(
        "fathom-difftool-{}-{}",
        std::process::id(),
        short(&episode.oid())
    ));
    std::fs::create_dir_all(&dir).map_err(|err| {
        crate::verbs::pending(
            ExitCode::Error,
            format!(
                "the staging directory `{}` would not be created: {err}",
                dir.display()
            ),
            Some("$TMPDIR has room, or is not set to something else entirely".to_owned()),
        )
    })?;
    let left_path = dir.join(format!("before-{name}"));
    let right_path = dir.join(format!("after-{name}"));
    if let Err(err) =
        std::fs::write(&left_path, &left).and_then(|()| std::fs::write(&right_path, &right))
    {
        return Err(crate::verbs::pending(
            ExitCode::Error,
            format!(
                "the two sides could not be written into {}: {err}",
                dir.display()
            ),
            Some("a difftool needs two real files; $TMPDIR is where they go".to_owned()),
        ));
    }
    Ok(Staged {
        dir,
        left: left_path,
        right: right_path,
        kept: false,
    })
}

/// What `fathom open` decided, printed before anything is run.
#[cfg(feature = "sqlite-bundled")]
#[allow(clippy::too_many_arguments)]
fn report_open(
    episode: &Episode,
    target: &Target,
    command: &Command,
    links: &Printer,
    print: bool,
    as_json: bool,
    forced_line: Option<u32>,
    in_work_tree: bool,
) {
    if as_json {
        output::line(format_args!(
            "{}",
            serde_json::json!({
                "contract": "fathom.cli.open/1",
                "episode": episode.name(),
                "commit": episode.oid(),
                "path": target.path,
                "line": target.line,
                "line_from:": match (forced_line, target.line) {
                    (Some(_), _) => "--line",
                    (None, Some(_)) => "first hunk header",
                    (None, None) => "the file has no hunk header to take one from",
                },
                "editor": command.line(),
                "why": command.why,
                "hyperlinks": links.enabled(),
                "in_work_tree": in_work_tree,
                "run": !print,
            })
        ));
        return;
    }
    output::line(format_args!("  asked     {}", episode.heading(links)));
    output::line(format_args!(
        "  target    {} {}",
        links.path_line(&target.path, target.line),
        target.line.map_or_else(
            || "(no line to jump to — it opens at the top)".to_owned(),
            |line| format!("at line {line}")
        )
    ));
    output::line(format_args!("  editor    {}", command.line()));
    output::line(format_args!("  why       {}", command.why));
    if !in_work_tree {
        output::line(format_args!(
            "  note      {} is not in the working tree — nothing will be run without it",
            target.path
        ));
    }
    if print {
        output::line(format_args!(
            "  run       not run — `--print` stopped at the plan"
        ));
        output::line(format_args!(
            "  links     {} — {}",
            if links.enabled() { "on" } else { "off" },
            links.why()
        ));
    }
}

/// What `fathom difftool` decided, printed before anything is run.
#[cfg(feature = "sqlite-bundled")]
fn report_difftool(
    episode: &Episode,
    target: &Target,
    command: &Command,
    source: &str,
    links: &Printer,
    print: bool,
    as_json: bool,
) {
    if as_json {
        output::line(format_args!(
            "{}",
            serde_json::json!({
                "contract": "fathom.cli.difftool/1",
                "episode": episode.name(),
                "commit": episode.oid(),
                "path": target.path,
                "tool": command.line(),
                "tool_from": source,
                "why": command.why,
                "hyperlinks": links.enabled(),
                "run": !print,
            })
        ));
        return;
    }
    output::line(format_args!("  asked     {}", episode.heading(links)));
    output::line(format_args!("  file     {}", links.path(&target.path)));
    output::line(format_args!("  difftool {}", command.line()));
    output::line(format_args!(
        "  why      {} · tool from {source}",
        command.why
    ));
    if print {
        output::line(format_args!(
            "  run      not run — `--print` stopped at the plan, and the two files stay where they are"
        ));
        output::line(format_args!(
            "  links    {} — {}",
            if links.enabled() { "on" } else { "off" },
            links.why()
        ));
    }
}

/// Seven characters, git-style, for every line a human reads.
fn short(oid: &str) -> String {
    oid.chars().take(7).collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn the_jump_line_is_the_new_side_of_the_first_hunk() {
        // The header form the index stores, straight out of `fathom-ingest`'s own renderer.
        let headers = vec![
            "@@ -12,7 +12,9 @@ fn parse(input: &str) -> Out".to_owned(),
            "@@ -40,3 +52,4 @@".to_owned(),
        ];
        assert_eq!(first_hunk_line(&headers), Some(12));
        assert_eq!(first_hunk_line(&["@@ -1 +1,4 @@".to_owned()]), Some(1));
        assert_eq!(
            first_hunk_line(&[]),
            None,
            "no header is no line, not line zero"
        );
        assert_eq!(
            first_hunk_line(&["@@ -3,2 +0,0 @@".to_owned()]),
            None,
            "a deleted side has no new-side line"
        );
    }

    // `sqlite-bundled`-gated with the function it calls: `best` ranks the targets a store-backed `fathom
    // open` read out of the index, and §6.2 compiles none of that path into the headless binary.
    #[cfg(feature = "sqlite-bundled")]
    #[test]
    fn the_best_file_is_the_biggest_real_one_not_the_first_in_the_list() {
        let make = |path: &str, kind: &str, add: i64, surface: &'static str| Target {
            path: path.to_owned(),
            old_path: None,
            kind: kind.to_owned(),
            line: Some(1),
            add,
            del: 0,
            binary: false,
            surface,
            blob_old: None,
            blob_new: None,
        };
        let targets = vec![
            make("Cargo.lock", "modify", 9_000, "dependency"),
            make("src/deleted.rs", "delete", 0, "source"),
            make("src/small.rs", "modify", 3, "source"),
            make("src/big.rs", "modify", 400, "source"),
        ];
        assert_eq!(best(&targets).path, "src/big.rs");
        // A lockfile that dwarfs the real change is still not the real change.
        let noisy = vec![
            make("Cargo.lock", "modify", 9_000, "dependency"),
            make("src/small.rs", "modify", 3, "source"),
        ];
        assert_eq!(best(&noisy).path, "src/small.rs");
        // ...and when every file is machine-written, the biggest one is the honest answer.
        let all_machine = vec![
            make("Cargo.lock", "modify", 9_000, "dependency"),
            make("pnpm-lock.yaml", "modify", 10, "dependency"),
        ];
        assert_eq!(best(&all_machine).path, "Cargo.lock");
    }

    #[test]
    fn a_deleted_file_is_not_something_to_open() {
        let deleted = Target {
            path: "x.rs".to_owned(),
            old_path: None,
            kind: "delete".to_owned(),
            line: None,
            add: 0,
            del: 10,
            binary: false,
            surface: "source",
            blob_old: None,
            blob_new: None,
        };
        assert!(!deleted.jumpable());
    }
}
