//! `script.rs` — the language a scenario is written in.
//!
//! A scenario is a list of commits, and each commit is: on which branch, by whom, at what offset from the
//! fixed epoch, with which message and trailers, and what it does to the files. That is the whole vocabulary,
//! and keeping it that small is what makes the corpus auditable: reading a scenario function top to bottom
//! tells you the history, and the ground-truth file next to it states what the history *means*.
//!
//! Notably absent: any notion of the answer. A scenario may not ask what episode or finding it expects,
//! because a fixture that computes its own expectations with the code under test verifies nothing
//! (ADR-002 §4.4). Expectations live in `corpus/<scenario>.json`, which a human wrote.

use crate::policy::Identity;

/// A whole scenario, as data.
#[derive(Debug, Clone)]
pub struct Script {
    /// The scenario name, which is also the ground-truth file's stem.
    pub scenario: &'static str,
    /// The branch `HEAD` ends up on.
    pub default_branch: String,
    /// The commits, in creation order — which is also the corpus's canonical walk order, because a commit is
    /// always created after its parents.
    pub steps: Vec<Step>,
    /// Repositories this one refers to: a submodule's own history, the upstream of a shallow clone.
    pub companions: Vec<Companion>,
    /// What to do after the commits: turn the destination into a shallow clone of a companion.
    pub extras: Vec<Extra>,
    /// How much of the harness event stream to write. A 10 000-commit scenario does not get a 10 000-commit
    /// event log; see [`crate::events`].
    pub events: Events,
}

impl Script {
    /// An empty script on `main`.
    #[must_use]
    pub fn new(scenario: &'static str) -> Self {
        Self {
            scenario,
            default_branch: "main".to_owned(),
            steps: Vec::new(),
            companions: Vec::new(),
            extras: Vec::new(),
            events: Events::All,
        }
    }

    /// Start a commit on the default branch, dated a fixed quarter-hour after the previous one.
    ///
    /// This is the spelling the scenarios use: a history is a sequence of commits, and having to write a
    /// timestamp for each of ten thousand of them is how a scenario ends up with a clock that means nothing.
    /// Where the gaps matter — the four-hour pause in `reformat_only`, the hours between the churn rounds —
    /// the scenario says so with [`StepBuilder::at`].
    pub fn commit(&mut self, subject: &str) -> StepBuilder<'_> {
        let at =
            STEP_SPACING * (i64::try_from(self.steps.len()).unwrap_or(i64::MAX / STEP_SPACING) + 1);
        let branch = self.default_branch.clone();
        let mut builder = self.step();
        builder.step.branch = branch;
        builder.step.at = at;
        builder.step.subject = subject.to_owned();
        builder
    }

    /// Start a commit. Finish it with [`StepBuilder::done`].
    pub fn step(&mut self) -> StepBuilder<'_> {
        StepBuilder {
            script: self,
            step: Step {
                branch: String::new(),
                parents: Vec::new(),
                fork_from: None,
                identity: crate::policy::PI.key,
                at: 0,
                subject: String::new(),
                body: Vec::new(),
                trailers: Vec::new(),
                session: None,
                turn: None,
                ops: Vec::new(),
                tag: None,
            },
        }
    }

    /// Add a companion repository.
    pub fn companion(&mut self, companion: Companion) -> &mut Self {
        self.companions.push(companion);
        self
    }

    /// Add a post-processing step.
    pub fn extra(&mut self, extra: Extra) -> &mut Self {
        self.extras.push(extra);
        self
    }
}

/// How much of the harness event stream a scenario writes next to its repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Events {
    /// One turn's worth of events per commit, for every commit.
    All,
    /// Only the first `n` commits get events, and the manifest says so. Used by the scale scenarios, where
    /// a complete stream would be the largest artifact in the corpus and the least interesting.
    First(usize),
    /// No event stream at all.
    None,
}

/// The gap between two commits that are not otherwise dated: fifteen minutes, which is longer than a turn
/// takes and shorter than any of the gaps the corpus uses to test the time-gap rule.
pub const STEP_SPACING: i64 = 900;

/// One commit, as data.
#[derive(Debug, Clone)]
pub struct Step {
    /// The branch this commit lands on.
    pub branch: String,
    /// The branches whose heads are the parents, first parent first. Empty for a root commit.
    pub parents: Vec<String>,
    /// When `branch` does not exist yet: the branch it forks from.
    pub fork_from: Option<String>,
    /// The committer, by [`crate::policy`] key.
    pub identity: &'static str,
    /// Seconds after [`crate::policy::BASE_EPOCH`].
    pub at: i64,
    /// The subject line.
    pub subject: String,
    /// Body paragraphs.
    pub body: Vec<String>,
    /// Extra trailers, appended after the session/turn ones.
    pub trailers: Vec<(String, String)>,
    /// The harness session, which becomes the `Agent-Session:` trailer and `session_id` in the event log.
    pub session: Option<String>,
    /// The harness turn, which becomes the `Agent-Turn:` trailer. Turn ids are unique within a scenario:
    /// `fathom-ingest`'s hook rule compares turn ids and nothing else, so reusing an id across two sessions
    /// would make the fixture claim two turns were one.
    pub turn: Option<String>,
    /// What the commit does.
    pub ops: Vec<Op>,
    /// A lightweight tag to point at this commit.
    pub tag: Option<String>,
}

impl Step {
    /// The commit message as git stores it: subject, body, then the trailer block.
    ///
    /// The trailer block is the last paragraph and every line of it looks like `Key: value`, which is
    /// git's own rule (`git-interpret-trailers(1)`) and the one `fathom-ingest::record::Trailers` parses.
    #[must_use]
    pub fn message(&self) -> String {
        let mut out = self.subject.clone();
        for line in &self.body {
            out.push_str("\n\n");
            out.push_str(line);
        }
        let mut trailers: Vec<String> = Vec::new();
        if let Some(tool) = crate::policy::identity(self.identity).and_then(|id| {
            if id.tool.is_empty() {
                None
            } else {
                Some(id.tool)
            }
        }) {
            trailers.push(format!("Assisted-by: {tool}"));
        }
        if let Some(session) = &self.session {
            trailers.push(format!("Agent-Session: {session}"));
        }
        if let Some(turn) = &self.turn {
            trailers.push(format!("Agent-Turn: {turn}"));
        }
        for (key, value) in &self.trailers {
            trailers.push(format!("{key}: {value}"));
        }
        if !trailers.is_empty() {
            out.push_str("\n\n");
            out.push_str(&trailers.join("\n"));
        }
        out
    }
}

/// What a commit does to the tree.
#[derive(Debug, Clone)]
pub enum Op {
    /// Create or overwrite a file.
    Write {
        /// Repository-relative path, `/` separated.
        path: String,
        /// The whole content.
        blob: Vec<u8>,
    },
    /// Add to the end of a file, creating it if it does not exist.
    Append {
        /// Repository-relative path.
        path: String,
        /// The text to add.
        blob: String,
    },
    /// Remove a file.
    Delete {
        /// Repository-relative path.
        path: String,
    },
    /// Move a file. The content travels unchanged, which is what makes a rename a rename.
    Rename {
        /// The old path.
        from: String,
        /// The new path.
        to: String,
    },
    /// Duplicate a file.
    Copy {
        /// The source path.
        from: String,
        /// The destination path.
        to: String,
    },
    /// Take the other branch's changes into this one, the way a merge would: theirs wins where ours did not
    /// also move, and a step that follows this one is the conflict resolution.
    Merge {
        /// The branch being incorporated.
        branch: String,
    },
    /// Add or move a gitlink, and append its `.gitmodules` entry.
    Submodule {
        /// Where the submodule is mounted.
        path: String,
        /// The URL written into `.gitmodules` — relative, so the bytes do not depend on where the corpus was
        /// materialised.
        url: String,
        /// Which companion names the commit this gitlink points at.
        companion: String,
        /// Which of that companion's commits the gitlink names.
        at: Gitlink,
    },
}

/// Which commit of a companion a gitlink names.
#[derive(Debug, Clone, Copy)]
pub enum Gitlink {
    /// The companion's last commit.
    Tip,
    /// The companion's commit at this creation index.
    Index(usize),
}

/// A repository that belongs to a scenario but is not the scenario: a submodule's history, or the upstream a
/// shallow clone was cut from.
#[derive(Debug, Clone)]
pub struct Companion {
    /// The name used in `Op::Submodule` and in the manifest, e.g. `vendor/toolkit`.
    pub name: String,
    /// Its own history.
    pub script: Script,
    /// Where its git dir goes.
    pub location: Location,
}

/// Where a companion's git dir lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Location {
    /// `<repo>/.git/modules/<name>`, with `core.worktree` pointing back and a gitfile at the mount point —
    /// the layout `git submodule add` has produced since git 1.8.
    ModulesDir,
    /// A sibling of the destination: `<dest>.companion/<name>`. For a repository that is a remote, not a
    /// part of this working tree.
    Sibling,
}

/// What to do to the destination after its commits exist.
#[derive(Debug, Clone)]
pub enum Extra {
    /// Turn the destination into a clone of a companion, `depth` commits deep: the same commit ids, a
    /// `.git/shallow`, an `origin` remote, and the ancestors below the cut simply not present. That absence
    /// is the fixture — `fathom-ingest` must walk a history whose parent it cannot read.
    ShallowClone {
        /// The companion to clone from.
        companion: String,
        /// How many commits of first-parent history to carry over.
        depth: usize,
        /// The local branch name to create.
        branch: String,
    },
}

/// The fluent form of [`Step`], so a scenario reads like the history it describes.
#[must_use]
pub struct StepBuilder<'a> {
    script: &'a mut Script,
    step: Step,
}

impl<'a> StepBuilder<'a> {
    /// Land this commit on `branch`, creating the branch from its previous head if it already exists.
    pub fn on(mut self, branch: &str) -> Self {
        self.step.branch = branch.to_owned();
        if self.script.default_branch.is_empty() {
            self.script.default_branch = branch.to_owned();
        }
        self
    }

    /// Create `branch` from the head of `from`, then land here.
    pub fn fork(mut self, branch: &str, from: &str) -> Self {
        self.step.branch = branch.to_owned();
        self.step.fork_from = Some(from.to_owned());
        self.step.parents = vec![from.to_owned()];
        self
    }

    /// A merge commit on `into` whose second parent is the head of `other`.
    ///
    /// The tree is taken by incorporating `other`'s changes first (see [`Op::Merge`]), so any [`write`] the
    /// step then performs is the conflict resolution, exactly as a human would leave it.
    ///
    /// [`write`]: StepBuilder::write
    pub fn merge_of(mut self, into: &str, other: &str) -> Self {
        self.step.branch = into.to_owned();
        self.step.parents = vec![into.to_owned(), other.to_owned()];
        self.step.ops.push(Op::Merge {
            branch: other.to_owned(),
        });
        self
    }

    /// Commit as this identity.
    pub fn by(mut self, identity: &'static Identity) -> Self {
        self.step.identity = identity.key;
        self
    }

    /// Date this many seconds after the corpus epoch.
    pub fn at(mut self, seconds: i64) -> Self {
        self.step.at = seconds;
        self
    }

    /// Name the harness session and turn, which stamps the trailers and puts the events in the log.
    ///
    /// Session and turn ids are stored verbatim, and the convention the corpus follows is one session per
    /// agent sitting at the keyboard and a turn id unique within the scenario — see [`Step::turn`].
    pub fn turn(mut self, session: &str, turn: &str) -> Self {
        self.step.session = Some(session.to_owned());
        self.step.turn = Some(turn.to_owned());
        self
    }

    /// Add a trailer after the session ones.
    pub fn trailer(mut self, key: &str, value: &str) -> Self {
        self.step.trailers.push((key.to_owned(), value.to_owned()));
        self
    }

    /// Add a body paragraph.
    pub fn body(mut self, paragraph: &str) -> Self {
        self.step.body.push(paragraph.to_owned());
        self
    }

    /// Write a file in full.
    pub fn write(mut self, path: &str, blob: impl Into<Vec<u8>>) -> Self {
        self.step.ops.push(Op::Write {
            path: path.to_owned(),
            blob: blob.into(),
        });
        self
    }

    /// Append to a file.
    pub fn append(mut self, path: &str, blob: &str) -> Self {
        self.step.ops.push(Op::Append {
            path: path.to_owned(),
            blob: blob.to_owned(),
        });
        self
    }

    /// Delete a file.
    pub fn delete(mut self, path: &str) -> Self {
        self.step.ops.push(Op::Delete {
            path: path.to_owned(),
        });
        self
    }

    /// Rename a file.
    pub fn rename(mut self, from: &str, to: &str) -> Self {
        self.step.ops.push(Op::Rename {
            from: from.to_owned(),
            to: to.to_owned(),
        });
        self
    }

    /// Copy a file.
    pub fn copy(mut self, from: &str, to: &str) -> Self {
        self.step.ops.push(Op::Copy {
            from: from.to_owned(),
            to: to.to_owned(),
        });
        self
    }

    /// Incorporate another branch's changes explicitly (a merge commit does it implicitly).
    pub fn merge_branch(mut self, branch: &str) -> Self {
        self.step.ops.push(Op::Merge {
            branch: branch.to_owned(),
        });
        self
    }

    /// Add or move a submodule gitlink.
    pub fn submodule(mut self, path: &str, url: &str, companion: &str, at: Gitlink) -> Self {
        self.step.ops.push(Op::Submodule {
            path: path.to_owned(),
            url: url.to_owned(),
            companion: companion.to_owned(),
            at,
        });
        self
    }

    /// Point a lightweight tag at this commit.
    pub fn tag(mut self, name: &str) -> Self {
        self.step.tag = Some(name.to_owned());
        self
    }

    /// Set the subject and finish the commit.
    pub fn msg(mut self, subject: &str) -> Self {
        self.step.subject = subject.to_owned();
        self
    }

    /// Finish the commit and add it to the script.
    pub fn done(mut self) -> &'a mut Script {
        assert!(
            !self.step.subject.is_empty(),
            "scenario `{}`: a commit with no subject is not a commit",
            self.script.scenario
        );
        assert!(
            !self.step.branch.is_empty(),
            "scenario `{}`: commit `{}` names no branch",
            self.script.scenario,
            self.step.subject
        );
        let step = std::mem::replace(
            &mut self.step,
            Step {
                branch: String::new(),
                parents: Vec::new(),
                fork_from: None,
                identity: crate::policy::PI.key,
                at: 0,
                subject: String::new(),
                body: Vec::new(),
                trailers: Vec::new(),
                session: None,
                turn: None,
                ops: Vec::new(),
                tag: None,
            },
        );
        self.script.steps.push(step);
        self.script
    }
}
