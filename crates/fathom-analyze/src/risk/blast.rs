//! The `blast` axis, asked of the index instead of remembered from a plan.
//!
//! `blast` is the one feature of [`super::feature`] that has been printed as missing for a *reason*
//! rather than a fact: every version of this sentence since `e4.4` said the axis is absent because "the
//! module graph is `e3.1`'s and the graph delta is `e3.2`'s", which was true when it was written and has
//! been false since epic 3 closed — `fathom index` writes the graph lane (`e3.11`), `fathom architecture`
//! measures coupling, cycles and hotspots on it, and `fathom check` runs two graph checks over it. A
//! reviewer who reads "absent until they land" in a repository whose index holds 1 338 graph revisions is
//! being told a thing is missing that is sitting in front of them, and the honest fix for that is not a
//! better-told excuse; it is to *ask the store* and print what comes back.
//!
//! # What is asked, and what is answered
//!
//! [`Availability::probe`] asks two cheap questions of the store the read is already holding:
//!
//! * does this index hold a graph lane at all (`graph_snapshot` rows — the same test
//!   [`fathom_cli`'s architecture read][0] and `fathom check` make before they judge), and
//! * how many of the paths this episode touched are module keys in its node dictionary.
//!
//! Both are answers about *this index*, so they change when the index changes and cannot rot the way a
//! remembered ticket key does. Nothing here materialises a graph: the fan-in count is a chain walk
//! (`graph.materialise_revision.ms`) and `fathom why` does not spend that budget; this module says which
//! of the three possible reasons the axis has no number, and leaves the counting to the caller that can
//! pay for it.
//!
//! [0]: ../../../fathom-cli/src/architecture.rs

/// What the index answers when the `blast` axis asks whether it can be measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    /// Nothing was asked — the shape a caller that never opened a store gets, and the one the fixture
    /// tests score with. It says *nothing was asked*, never *nothing exists*.
    NotProbed,
    /// `graph_snapshot` holds no rows: this index has no module graph at all.
    NoGraph,
    /// The lane holds `revisions` revisions, and `named` of the `touched` paths this episode changed are
    /// module keys in its node dictionary.
    Graph {
        /// Revisions with a stored graph row, which is what "this index has a graph" is measured in.
        revisions: usize,
        /// Paths this episode touched, after deduplication.
        touched: usize,
        /// How many of them the graph has a vertex for.
        named: usize,
    },
}

impl Availability {
    /// Ask the store the two questions this axis depends on.
    ///
    /// `paths` are the episode's own paths; they are deduplicated and normalised with
    /// [`module_key`], because that is the key the dictionary is interned on.
    ///
    /// # Errors
    ///
    /// [`fathom_store::error::Error`] when a read fails — a half-answer here is worse than a stated
    /// refusal, because "0 of 7 paths are modules" and "the store would not say" are different claims.
    #[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
    pub fn probe(
        store: &fathom_store::spine::Store,
        paths: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Result<Self, fathom_store::error::Error> {
        use crate::graph::delta::module_key;

        let revisions = store.graph_revisions()?.len();
        if revisions == 0 {
            return Ok(Availability::NoGraph);
        }
        let mut touched: Vec<String> = paths
            .into_iter()
            .map(|path| module_key(path.as_ref()))
            .collect();
        touched.sort();
        touched.dedup();
        let named = touched
            .iter()
            .filter(|path| store.graph_node_id(path).is_ok_and(|id| id.is_some()))
            .count();
        Ok(Availability::Graph {
            revisions,
            touched: touched.len(),
            named,
        })
    }

    /// Whether the index could answer the question at all, if a caller paid for the walk.
    ///
    /// This is the clause the ceiling prints: an index with a graph is an index where `blast` is
    /// unmeasured *by choice*, not unavailable by construction.
    #[must_use]
    pub fn can_answer(&self) -> bool {
        matches!(self, Availability::Graph { named, .. } if *named > 0)
    }

    /// The reason the axis has no number, in the words `fathom why` prints beside the `—`.
    ///
    /// Every branch names something the reader can go and check — a table row count, a path count, the
    /// door the number would arrive through — and no branch names a ticket, because a ticket is a claim
    /// about a plan and this sentence is a claim about this index.
    #[must_use]
    pub fn sentence(&self) -> String {
        match self {
            Availability::NotProbed => {
                "nothing asked this index whether it knows what depends on the touched code, and a \
                 hole in an arithmetic has to be named rather than guessed at"
                    .to_owned()
            }
            Availability::NoGraph => {
                "this index holds no module graph, so nothing here can say what depends on the code \
                 this touched; `fathom index` writes the graph lane (`graph.enabled`), and until it \
                 does the axis is absent from the arithmetic rather than guessed at"
                    .to_owned()
            }
            Availability::Graph {
                revisions,
                touched,
                named: 0,
            } => format!(
                "this index holds a module graph ({revisions} revision(s)), and none of the {touched} \
                 path(s) this episode touched is a module in it, so who depends on this change is \
                 unknown here and not zero — the extractor covers this kind of file, this graph does \
                 not",
            ),
            Availability::Graph {
                revisions,
                touched,
                named,
            } => format!(
                "this index can say what depends on {named} of the {touched} path(s) this episode \
                 touched ({revisions} graph revision(s)) — and this read did not ask: the count is a \
                 chain walk, `fathom why` does not spend that budget, and no shipped verb hands the \
                 number over yet, so the axis is absent from the arithmetic rather than guessed at",
            ),
        }
    }

    /// Who owns the number, printed after the reason.
    ///
    /// Deliberately not a ticket key: when the index already holds the graph, the thing that is missing is
    /// a caller, and saying so is the whole content of the line.
    #[must_use]
    pub const fn owner(&self) -> &'static str {
        match self {
            Availability::NotProbed => "the caller",
            Availability::NoGraph => "the graph lane",
            Availability::Graph { .. } => "the caller",
        }
    }
}
