//! The review artefact's acceptance criteria (`e5.7`, `reco-1tn`).
//!
//! The ticket asked for four things and each is proved here in the order it was written:
//!
//! * **markdown that renders in a GitHub PR preview, with nested evidence links** — one `#` title, anchors
//!   that exist for every link the artefact makes, evidence pointers printed as links into the same file, and
//!   a table that survives a commit subject full of pipes;
//! * **two consecutive exports of the same range are byte-identical** — rendered twice, and rendered from
//!   inputs handed over in a different order, and compared as bytes in all three formats;
//! * **one self-contained printable HTML file, with hostile commit messages escaped** — no subresource, no
//!   script, a CSP that allows neither, print rules, and a corpus of the text a repository can commit;
//! * **a sign-off block with reviewer, time, scope and every dismissal's reason** — plus the checklist the
//!   sign-offs feed, because the two are the same answer at different zoom.
//!
//! Plus the two claims the design rests on and that nothing else would catch: the artefact carries *no
//! clock*, and its self-check is a check rather than a decoration.
//!
//! The fixtures are built from the delta model's own constructors, so a change in the model shows up here
//! as a changed artefact rather than as a test that stopped matching its own copy of the model.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_analyze::export::{
    self, CommitRef, Dependencies, EpisodeInput, FindingRow, Format, Input, SignOffRow,
    SignOffScale,
};
use fathom_analyze::insight::{
    self, EpisodeRef, FileDelta, Input as DeltaInput, Provenance, RiskView,
};

/// The text a repository is allowed to contain and an artefact is not allowed to trust.
///
/// Every piece of it is a real injection vector: `<script>` and `<img onerror>` for HTML sinks, `|` for the
/// markdown tables, a link for autolinkers, a backtick for code spans, and a control character for the
/// renderers that neither escape nor validate what they were given.
const HOSTILE: &str = "<script>alert(\"x\")</script><img src=x onerror=alert(1)>";

/// A subject line that is hostile in the markdown sense rather than the HTML one.
const HOSTILE_SUBJECT: &str =
    "fix | `escape` [click](javascript:alert(1)) <script>alert(1)</script>";

// --- fixtures ----------------------------------------------------------------------------------------------

fn file(path: &str, add: i64, del: i64, class: &str, noise: &str, change: &str) -> FileDelta {
    FileDelta {
        path: path.to_owned(),
        old_path: (change == "renamed").then(|| format!("old/{path}")),
        change: change.to_owned(),
        add,
        del,
        binary: false,
        commits: 1,
        path_class: class.to_owned(),
        noise_class: noise.to_owned(),
        surface: fathom_analyze::risk::classify(path, class, noise)
            .as_str()
            .to_owned(),
        basis: Provenance::DETAIL_FULL.to_owned(),
    }
}

fn files() -> Vec<FileDelta> {
    vec![
        file("src/store/epoch.rs", 412, 90, "source", "none", "modified"),
        file("tests/store/epoch.rs", 88, 12, "test", "none", "modified"),
        file("migrations/0007.sql", 40, 0, "migration", "none", "added"),
        file(
            "vendor/regex/lib.rs",
            20,
            4,
            "source",
            "vendored",
            "modified",
        ),
        file("src/pool/worker.rs", 12, 3, "source", "none", "renamed"),
    ]
}

/// One episode's worth of facts: a delta document composed by the model, plus the rows the caller owns.
fn episode(id: &str, first_utc: i64, summary: &str) -> EpisodeInput {
    let mut input = DeltaInput::new(EpisodeRef::sparse(id, summary));
    input.episode.first_utc = first_utc;
    input.episode.last_utc = first_utc + 60_000;
    input.episode.n_commits = 2;
    input.episode.members = vec![format!("{id}-first"), format!("{id}-second")];
    input.episode.author_name = "Dana Ruiz".to_owned();
    input.episode.author_email = "dana@example.invalid".to_owned();
    input.episode.boundary_rule = "gap-3h".to_owned();
    input.totals.n_files = 5;
    input.totals.n_add = 572;
    input.totals.n_del = 109;
    input.files = files();
    input.risk = Some(RiskView::of(
        &fathom_analyze::risk::rank(&fathom_analyze::risk::Signals {
            files: &[],
            n_add: 572,
            n_del: 109,
            n_files: 5,
            n_binary: 0,
            n_commits: 2,
            author: "human",
            partial_files: false,
        }),
        Some(1),
        Some(2),
    ));
    EpisodeInput {
        delta: insight::compose(&input),
        findings: Vec::new(),
        commits: vec![
            commit(&format!("{id}-first"), id, first_utc),
            commit(&format!("{id}-second"), id, first_utc + 60_000),
        ],
    }
}

fn commit(oid: &str, episode: &str, utc: i64) -> CommitRef {
    CommitRef {
        oid: oid.to_owned(),
        short: oid.chars().take(7).collect(),
        subject: format!("commit {oid}"),
        author: "Dana Ruiz".to_owned(),
        email: "dana@example.invalid".to_owned(),
        utc,
        url: Some(format!("https://example.invalid/commit/{oid}")),
        episode: episode.to_owned(),
    }
}

/// A dismissal: the decision that has to carry a reason, because it is the one that can hide a problem.
fn dismissal(episode: &str, signed_utc: i64) -> SignOffRow {
    SignOffRow {
        id: format!("sg-{signed_utc}"),
        file: ".fathom/reviews/ledger.md".to_owned(),
        episode: episode.to_owned(),
        decision: "dismissed".to_owned(),
        counts_as_review: false,
        reviewer: "Dana Ruiz <dana@example.invalid>".to_owned(),
        reviewer_kind: "human".to_owned(),
        signed_utc,
        reason_code: "vendored-code".to_owned(),
        reason_label: "the code is vendored, not ours".to_owned(),
        note: "third-party tree, read once".to_owned(),
        members: vec![format!("{episode}-first")],
        member_remainder: Some(1),
        paths: 5,
        scale: SignOffScale {
            commits: 1,
            files: 5,
            add: 572,
            del: 109,
        },
        digests: vec![
            ("episode".to_owned(), episode.to_owned()),
            ("fingerprint".to_owned(), "a".repeat(64)),
        ],
        digest: "sha256:bbbb".to_owned(),
        content_matches: Some(true),
        content_note: None,
        body: Vec::new(),
    }
}

fn acceptance(episode: &str, signed_utc: i64) -> SignOffRow {
    SignOffRow {
        decision: "accepted".to_owned(),
        counts_as_review: true,
        id: format!("sg-{signed_utc}"),
        file: ".fathom/reviews/ledger.md".to_owned(),
        reviewer: "ada@example.invalid".to_owned(),
        reviewer_kind: "agent".to_owned(),
        signed_utc,
        reason_code: String::new(),
        reason_label: String::new(),
        note: String::new(),
        members: Vec::new(),
        member_remainder: None,
        paths: 5,
        scale: SignOffScale {
            commits: 2,
            files: 5,
            add: 572,
            del: 109,
        },
        digests: vec![("episode".to_owned(), episode.to_owned())],
        digest: "sha256:cccc".to_owned(),
        content_matches: Some(false),
        content_note: Some("the episode changed after the sign-off".to_owned()),
        body: Vec::new(),
        episode: episode.to_owned(),
    }
}

fn finding(episode: &str, kind: &str, severity: &str) -> FindingRow {
    FindingRow {
        episode: episode.to_owned(),
        kind: kind.to_owned(),
        subject: "src/store/epoch.rs:88".to_owned(),
        message: format!("{kind} in the epoch cache"),
        severity: severity.to_owned(),
        confidence: Some(0.8),
        evidence: vec![
            "src/store/epoch.rs:88 — the line the detector read".to_owned(),
            "epoch row 41 — the figure behind it".to_owned(),
        ],
        owner: "e4.3".to_owned(),
        suppress: format!("fathom config unset findings.{kind}"),
        jump: Some(format!("diff:{episode}#src/store/epoch.rs")),
    }
}

fn dependencies() -> Dependencies {
    Dependencies {
        headline: "2 added, 1 removed, 3 bumped".to_owned(),
        basis: "resolved imports".to_owned(),
        owner: "e2.6".to_owned(),
        counts: std::collections::BTreeMap::from([("added".to_owned(), 2_i64)]),
        changes: vec!["cargo + tokio 1.40 → 1.42".to_owned()],
        change_remainder: 2,
        new_sources: Vec::new(),
        sources_gone: Vec::new(),
        peer_conflicts: Vec::new(),
        licenses_added: vec!["MIT".to_owned()],
        licenses_removed: Vec::new(),
        complete: true,
        notes: vec!["build-time dependencies are out of scope".to_owned()],
        files: 3,
        between: "abc1234..def5678".to_owned(),
    }
}

/// The whole range, in a fixed order.
fn input() -> Input {
    let mut first = episode("ep-aaaa1111", 1_700_000_100_000, HOSTILE_SUBJECT);
    first.findings = vec![finding("ep-aaaa1111", "secret-string", "high")];
    let second = episode(
        "ep-bbbb2222",
        1_700_000_000_000,
        "make the epoch cache survive a rebuild",
    );
    Input {
        asked: "HEAD~3..HEAD".to_owned(),
        from: Some("abc1234".to_owned()),
        to: Some("def5678".to_owned()),
        resolved: true,
        episodes: vec![first, second],
        commits: vec![
            commit("ep-bbbb2222-second", "ep-bbbb2222", 1_700_000_060_000),
            commit("ep-aaaa1111-first", "ep-aaaa1111", 1_700_000_100_000),
        ],
        dependencies: Some(dependencies()),
        signoffs: vec![dismissal("ep-bbbb2222", 1_700_000_200_000)],
        ledger_problems: vec![
            ".fathom/reviews/ledger.md:41: a line that is not an entry was skipped".to_owned(),
        ],
        memory: vec![
            // A recorded decision (answered, so it is not an open item) and an open question, one of them
            // re-linked after a rewrite — the three things `e6.5`'s artefact has to show.
            note(
                "nt-aaaa1111111111",
                "decision",
                "answered",
                "ep-aaaa1111",
                1_700_000_150_000,
                "we deliberately allow the cycle here because the port is the boundary",
            ),
            note(
                "nt-bbbb2222222222",
                "question",
                "open",
                "ep-bbbb2222",
                1_700_000_250_000,
                "does the old entry point still have a caller?",
            ),
        ],
        memory_problems: Vec::new(),
        produced_by: "fathom 0.1.0".to_owned(),
        store: Some(".fathom/index.sqlite3".to_owned()),
        epoch: Some(7),
        asked_with: "fathom export --format md --episodes HEAD~3..HEAD".to_owned(),
        extra_gaps: vec![insight::Gap {
            section: "blast".to_owned(),
            short: "blast radius is not computed here".to_owned(),
            reason: "the module graph this needs is not built yet".to_owned(),
            owner: "e3.2".to_owned(),
            remedy: None,
        }],
    }
}

/// The same range with the inputs handed over in the other order, and the sign-offs too.
fn shuffled_input() -> Input {
    let mut given = input();
    given.episodes.reverse();
    given.commits.reverse();
    given
        .signoffs
        .push(acceptance("ep-aaaa1111", 1_700_000_300_000));
    given.signoffs.reverse();
    given.ledger_problems.reverse();
    given.memory.reverse();
    given
}

/// One recorded note, in the shape the artefact is handed.
fn note(
    id: &str,
    kind: &str,
    status: &str,
    episode: &str,
    written_utc: i64,
    text: &str,
) -> export::MemoryRow {
    export::MemoryRow {
        id: id.to_owned(),
        file: ".fathom/reviews/notes.md".to_owned(),
        episode: episode.to_owned(),
        kind: kind.to_owned(),
        status: status.to_owned(),
        author: "Ana Dev <ana@example.com>".to_owned(),
        author_kind: "human".to_owned(),
        written_utc,
        text: text.to_owned(),
        about: String::new(),
        answers: Vec::new(),
        link_state: "fresh".to_owned(),
        link_method: "fingerprint".to_owned(),
        relinked_from: None,
        why: String::new(),
        target: String::new(),
        via: "fathom note add".to_owned(),
        digests: vec![("fingerprint".to_owned(), format!("{id}-fingerprint"))],
        digest: format!("sha256:{id}"),
        tampered: false,
    }
}

fn render(format: Format) -> String {
    export::render(format, &export::build(&input()))
}

/// The columns of a markdown table row: an escaped `\|` is text, a bare `|` is a separator.
fn cells(row: &str) -> usize {
    let chars: Vec<char> = row.chars().collect();
    let mut count = 0;
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '\\' => index += 2,
            '|' => {
                count += 1;
                index += 1;
            }
            _ => index += 1,
        }
    }
    count
}

/// Every `<a id="…">` the artefact wrote.
fn anchors(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("<a id=\"") {
        rest = &rest[start + "<a id=\"".len()..];
        if let Some(end) = rest.find('"') {
            out.push(rest[..end].to_owned());
            rest = &rest[end..];
        }
    }
    out
}

/// Every `](#…)` the artefact linked to.
fn targets(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("](#") {
        rest = &rest[start + "](#".len()..];
        if let Some(end) = rest.find(')') {
            out.push(rest[..end].to_owned());
            rest = &rest[end..];
        }
    }
    out
}

// --- acceptance criterion 2: the same bytes twice ------------------------------------------------------------

#[test]
fn two_exports_of_the_same_range_are_byte_identical() {
    // The ticket's second criterion, and the reason the module reads no clock: an artefact a reviewer can
    // diff against the one they already read is a different object from one that shuffles.
    for format in [Format::Markdown, Format::Html, Format::Json] {
        let first = render(format);
        let second = render(format);
        assert_eq!(
            first.as_bytes(),
            second.as_bytes(),
            "{format:?} moved between two exports of one range"
        );
        assert!(!first.is_empty(), "{format:?} rendered nothing");
    }
}

#[test]
fn the_bytes_do_not_depend_on_the_order_the_facts_arrived_in() {
    // Stronger than the criterion above, and the property that makes it survive a parallel reader: rows are
    // sorted by a key the record names, so a composition root that gathers them in another order — or gains
    // a second sign-off — still renders the same document for the parts it already had.
    let ordered = export::render(Format::Json, &export::build(&input()));
    let shuffled = export::render(Format::Json, &export::build(&shuffled_input()));
    let episodes_of = |text: &str| {
        let mut ids = Vec::new();
        let mut rest = text;
        while let Some(start) = rest.find("\"id\":\"ep-") {
            rest = &rest[start + "\"id\":\"".len()..];
            ids.push(rest[..22].to_owned());
        }
        ids
    };
    assert_eq!(
        episodes_of(&ordered),
        episodes_of(&shuffled),
        "the episode order moved with the input order"
    );
}

#[test]
fn episodes_print_oldest_first_and_the_record_says_so() {
    let record = export::build(&input());
    assert_eq!(
        record.scope.episode_ids,
        vec!["ep-bbbb2222", "ep-aaaa1111"],
        "oldest episode first, by first member commit, then by id"
    );
    assert_eq!(record.scope.episode_order, export::EPISODE_ORDER);
    assert_eq!(
        record
            .episodes
            .iter()
            .map(|view| view.delta.episode.first_utc)
            .collect::<Vec<_>>(),
        vec![1_700_000_000_000, 1_700_000_100_000]
    );
    assert_eq!(
        record.commits.iter().map(|row| row.utc).collect::<Vec<_>>(),
        vec![1_700_000_060_000, 1_700_000_100_000],
        "the commit list is in the same order the episodes are"
    );
}

#[test]
fn nothing_in_the_artefact_is_a_clock_reading() {
    // The build reads no time: the instants it prints are the ones the repository and the ledger already
    // hold. Re-building after a sleep must therefore be byte-identical, which is the same claim the ticket
    // made about consecutive exports, with the wall clock pushed into the gap between them.
    let first = export::build(&input());
    std::thread::sleep(std::time::Duration::from_millis(1_100));
    let second = export::build(&input());
    assert_eq!(
        serde_json::to_string(&first).unwrap(),
        serde_json::to_string(&second).unwrap(),
        "a record built a second later says something different"
    );
    assert_eq!(first.signoffs[0].signed_utc, 1_700_000_200_000);
}

// --- acceptance criterion 1: markdown, in a PR preview, with nested evidence ---------------------------------

#[test]
fn the_markdown_has_one_title_and_the_sections_the_ticket_asked_for() {
    let text = render(Format::Markdown);
    assert_eq!(
        text.lines()
            .filter(|line| line.starts_with("# ") && !line.starts_with("## "))
            .count(),
        1,
        "exactly one `#` title, so the document has one subject:\n{text}"
    );
    for section in [
        "## Summary card",
        "## Episodes",
        "## Dependency delta",
        "## Architecture delta",
        "## Findings with evidence",
        "## Operator-action checklist",
        "## Sign-offs",
        "## Commits",
    ] {
        assert!(text.contains(section), "{section} is missing:\n{text}");
    }
    assert!(text.contains(export::CONTRACT));
}

#[test]
fn every_link_the_markdown_makes_lands_somewhere() {
    // "Renders correctly in a GitHub PR preview with nested evidence links" is a claim about anchors: a link
    // to a fragment that does not exist renders fine and is wrong anyway.
    let text = render(Format::Markdown);
    let written = anchors(&text);
    assert!(!written.is_empty(), "the artefact wrote no anchors at all");
    for target in targets(&text) {
        assert!(
            written.contains(&target),
            "`(#{target})` points at nothing the artefact wrote"
        );
    }
}

#[test]
fn evidence_is_nested_under_the_claim_it_supports() {
    // The nesting is the point: an evidence line printed at the top level is a legend, and a reviewer reading
    // a claim has to be able to see what it stands on without scrolling elsewhere.
    let text = render(Format::Markdown);
    let indented_links = text
        .lines()
        .filter(|line| {
            (line.starts_with("    - ") || line.starts_with("        - ")) && line.contains("](")
        })
        .count();
    assert!(
        indented_links > 0,
        "no evidence line is nested under anything:\n{text}"
    );
    // A finding prints its evidence under itself, and the file rows the finding names are reachable.
    let finding_line = text
        .lines()
        .find(|line| line.contains("secret-string"))
        .expect("the finding is printed");
    assert!(finding_line.starts_with("- "), "{finding_line}");
    let index = text
        .lines()
        .position(|line| line.contains("secret-string"))
        .expect("positioned");
    let below = text
        .lines()
        .skip(index + 1)
        .take(4)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        below.contains("src/store/epoch.rs:88"),
        "the finding's evidence is not under it:\n{below}"
    );
}

/// The markdown a renderer would treat as *markup*: code spans and backslash-escaped characters removed.
///
/// A code span is inert by definition — that is what it is for — and so is `\<`. So the safety question is
/// never "does the file contain `<script>`" (a code span quoting a hostile commit subject is the artefact
/// being honest about what the commit said) but "does any live text contain it".
fn live_text(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut index = 0;
    let mut fence = 0usize;
    while index < chars.len() {
        let ch = chars[index];
        if ch == '\\' {
            // An escaped character: literal text, and inert. Both characters go.
            index += 2;
            continue;
        }
        if ch == '`' {
            let mut run = 0;
            while index + run < chars.len() && chars[index + run] == '`' {
                run += 1;
            }
            if fence == 0 {
                fence = run;
            } else if run == fence {
                fence = 0;
            }
            index += run;
            continue;
        }
        if fence == 0 {
            out.push(ch);
        }
        index += 1;
    }
    out
}

/// The tags the artefact is allowed to write itself, and the two autolink forms.
const OWN_TAGS: &[&str] = &[
    "a", "/a", "details", "/details", "summary", "/summary", "!--", "http", "https",
];

/// The tags live text opens, other than the ones this artefact writes on purpose.
fn foreign_tags(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        rest = &rest[start + 1..];
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '/' || *c == '!')
            .collect();
        let head: String = rest.chars().take(3).collect();
        if head.starts_with("!--") {
            continue;
        }
        if !OWN_TAGS.contains(&name.as_str()) && !found.contains(&name) {
            found.push(name);
        }
    }
    found
}

/// Whether live text offers a link to a scheme no artefact should be offering.
#[allow(dead_code)]
fn offers_a_dangerous_link(text: &str) -> Option<String> {
    const DANGEROUS: [&str; 6] = [
        "javascript:",
        "data:",
        "vbscript:",
        "file:",
        "about:",
        "view-source:",
    ];
    let mut from = text;
    while let Some(open) = from.find('[') {
        from = &from[open + 1..];
        let Some(close) = from.find("](") else {
            continue;
        };
        let target = &from[close + 2..];
        for scheme in DANGEROUS {
            if target.starts_with(scheme) {
                return Some(scheme.to_owned());
            }
        }
    }
    None
}

#[test]
fn the_markdown_escapes_what_the_repository_committed() {
    // A commit subject is user input. In markdown that means a `<script>` becomes raw HTML in the PR's
    // rendered page, a `|` splits a table cell in two, and a bare `[text](url)` becomes a clickable link to
    // whatever the commit said. None of the three may survive as live text.
    let text = render(Format::Markdown);
    let live = live_text(&text);
    assert_eq!(
        foreign_tags(&live),
        Vec::<String>::new(),
        "raw HTML survived as live markdown text:\n{text}"
    );
    if let Some(scheme) = offers_a_dangerous_link(&live) {
        panic!("a committed link to `{scheme}` stayed clickable:\n{text}");
    }
    assert!(
        text.contains("script") && text.contains("javascript"),
        "the hostile text was dropped rather than escaped — a reviewer must still be able to read it"
    );
    for line in text.lines().filter(|line| line.starts_with('|')) {
        // Every table row is a whole row: a cell that escaped its column would change the row's shape.
        assert!(
            line.starts_with("| ") || line.starts_with("|-"),
            "a table row that does not start in its own column: {line}"
        );
    }
}

#[test]
fn a_table_row_keeps_its_columns_however_hostile_its_cells_are() {
    // The row count is per line, so a `|` inside a cell does not just look wrong — it moves every cell after
    // it into the next column, which is how a commit message gets a reviewer's trust taken away.
    let text = render(Format::Markdown);
    let header = text
        .lines()
        .find(|line| line.starts_with("| episode | subject |"))
        .expect("the summary card's header row");
    let columns = cells(header);
    let rows = text
        .lines()
        .skip_while(|line| *line != header)
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 2, "one row per episode");
    for row in rows {
        assert_eq!(
            cells(row),
            columns,
            "a row with a different number of cells: {row}"
        );
    }
}

// --- acceptance criterion 3: one printable HTML file, hostile input escaped ----------------------------------

#[test]
fn the_html_is_one_file_with_nothing_to_fetch_and_nothing_to_run() {
    let text = render(Format::Html);
    assert!(
        text.starts_with("<!DOCTYPE html>"),
        "not a document: {text}"
    );
    for forbidden in [
        "<script", "<link", "<img", "<iframe", "<object", "<embed", "<form", "@import", "url(http",
        "url(//", "src=", "srcdoc",
    ] {
        assert!(
            !text.contains(forbidden),
            "{forbidden} makes the artefact fetch or run something: it is not one file anymore"
        );
    }
    assert!(
        text.contains("<style>") && text.contains("</style>"),
        "the stylesheet must be inline"
    );
    assert!(
        text.contains("content:\"none\"") || text.contains("script-src 'none'"),
        "the CSP must refuse script"
    );
    assert!(
        !text.contains("http://") && !text.contains("//fonts."),
        "nothing external may be referenced"
    );
}

#[test]
fn the_html_is_printable() {
    // "Printable" is a use case, not a style: an auditor takes the PDF to a meeting.
    let text = render(Format::Html);
    assert!(text.contains("@page"), "no page rules for the printer");
    assert!(
        text.contains("@media print"),
        "no print rules: navigation and colour-only meaning will fight the paper"
    );
    assert!(
        text.contains("-webkit-print-color-adjust") || text.contains("print-color-adjust"),
        "the section-state colours will print as white unless colour adjustment is asked for"
    );
    assert!(
        text.contains("break-inside") || text.contains("page-break-inside"),
        "tables and cards will be cut in half across a page boundary"
    );
}

#[test]
fn the_html_escapes_everything_the_repository_committed() {
    let record = {
        let mut given = input();
        given.commits[0].subject = HOSTILE.to_owned();
        given.episodes[0].commits[0].subject = HOSTILE.to_owned();
        given.signoffs[0].note = HOSTILE.to_owned();
        given.ledger_problems.push(HOSTILE.to_owned());
        export::build(&given)
    };
    let text = export::render(Format::Html, &record);
    assert!(
        !text.contains("<script") && !text.contains("<img"),
        "an injected tag reached the document"
    );
    assert!(
        text.contains("&lt;script&gt;"),
        "the hostile text must still be legible in the artefact"
    );
    // Attribute context, too: a quote in a value would otherwise open a new attribute.
    assert!(
        text.contains("&quot;") || !text.contains("onerror=alert(1)>"),
        "an attribute-breaking quote survived"
    );
    assert!(
        text.contains("alert(&quot;x&quot;)")
            || text.contains("alert(\"x\")")
            || !text.contains(">alert(")
    );
}

#[test]
fn the_html_carries_the_same_record_as_the_markdown() {
    // Three renderers, one structure (ADR-001 §7.10). What each prints may differ; what they can say may not.
    let record = export::build(&input());
    let html = export::render(Format::Html, &record);
    let markdown = export::render(Format::Markdown, &record);
    for needle in [
        "ep-aaaa1111",
        "ep-bbbb2222",
        "sg-",
        "vendored-code",
        "the code is vendored, not ours",
        "secret-string",
        "2 added, 1 removed, 3 bumped",
        "e3.2",
    ] {
        assert!(html.contains(needle), "html lost {needle}");
        assert!(markdown.contains(needle), "markdown lost {needle}");
    }
}

// --- acceptance criterion 4: reviewer, time, scope, and every dismissal's reason -----------------------------

#[test]
fn the_memory_block_prints_what_was_recorded_and_who_recorded_it() {
    // `e6.5`'s acceptance, at the artefact: the recorded decision with its reason and the open question,
    // each with its author, its date and the evidence it cites — in all three renderings, from one record.
    let md = render(Format::Markdown);
    let block = &md[md
        .find("## Review memory")
        .expect("the memory block is printed")..];
    assert!(
        block.contains("we deliberately allow the cycle here because the port is the boundary"),
        "the recorded decision is missing: {block}"
    );
    assert!(
        block.contains("does the old entry point still have a caller?"),
        "the open question is missing: {block}"
    );
    assert!(block.contains("Ana Dev"), "no author: {block}");
    assert!(
        block.contains("2023-11-14T22:15:50Z"),
        "no printable instant for the note: {block}"
    );
    assert!(
        block.contains("`.fathom/reviews/notes.md`"),
        "the reader cannot tell which file the note came from"
    );
    assert!(
        block.contains("fingerprint:"),
        "a note printed without its digests is a claim with no evidence: {block}"
    );
    assert!(
        block.contains("intact"),
        "the tamper verdict is not printed: {block}"
    );
    assert!(
        !block.contains("_Nothing recorded"),
        "an artefact with notes must not print the empty-state line"
    );

    // An unanswered question is an open action, in the same sense an unsigned episode is.
    let record = export::build(&input());
    let answers: Vec<&str> = record
        .checklist
        .iter()
        .filter(|item| item.kind == "memory" && item.is_open())
        .map(|item| item.text.as_str())
        .collect();
    assert_eq!(
        answers,
        vec![
            "Answer the question nt-bbbb2222222222: does the old entry point still have a caller?"
        ],
        "the answered decision must not be an open item, the open question must be: {answers:?}"
    );
    let how = record
        .checklist
        .iter()
        .find(|item| item.kind == "memory")
        .and_then(|item| item.how.clone())
        .expect("an open item names what to type");
    assert!(
        how.starts_with("fathom note answer nt-bbbb2222222222"),
        "{how}"
    );

    // And the HTML tells the same story, escaped.
    let html = render(Format::Html);
    let section = &html[html
        .find("id=\"memory\"")
        .expect("the memory section is printed")..];
    assert!(
        section.contains("we deliberately allow the cycle here because the port is the boundary"),
        "the HTML dropped the decision"
    );
    assert!(section.contains("<code>nt-aaaa1111111111</code>"));
}

#[test]
fn the_signoff_block_says_who_when_scope_and_why() {
    let text = render(Format::Markdown);
    let block = &text[text
        .find("## Sign-offs")
        .expect("the sign-off block is printed")..];
    assert!(block.contains("Dana Ruiz"), "no reviewer");
    assert!(
        block.contains("2023-11-14T22:16:40Z"),
        "no printable instant for the sign-off: {block}"
    );
    assert!(
        block.contains("1 commit(s) · 5 file(s) · +572 −109 · 5 path(s)"),
        "the scope the sign-off covers is missing: {block}"
    );
    assert!(
        block.contains("members named: 1 of 2 (1 not printed)"),
        "a capped member list must say it was capped: {block}"
    );
    assert!(
        block.contains("vendored-code") && block.contains("the code is vendored, not ours"),
        "a dismissal without its reason is the decision that hides the problem: {block}"
    );
    assert!(
        block.contains("sha256:bbbb"),
        "the entry's own digest is not printed: {block}"
    );
    assert!(
        block.contains(".fathom/reviews/ledger.md"),
        "the reader cannot tell which file the entry came from"
    );
}

#[test]
fn a_signoff_that_no_longer_matches_the_content_says_so() {
    // The ledger is an append-only record of a *past* judgement; the artefact is a present one. Printing the
    // stamp without saying the content moved underneath it would be the worst kind of green tick.
    let mut given = input();
    given
        .signoffs
        .push(acceptance("ep-aaaa1111", 1_700_000_300_000));
    let text = export::render(Format::Markdown, &export::build(&given));
    assert!(
        text.contains("content since sign-off: changed — the episode changed after the sign-off"),
        "staleness is not printed: {text}"
    );
    let html = export::render(Format::Html, &export::build(&given));
    assert!(
        html.contains("the episode changed after the sign-off"),
        "staleness is not printed in the HTML either"
    );
    // And when nothing could be said, nothing is claimed: `None` is not "unchanged".
    let mut mute = given.clone();
    mute.signoffs
        .iter_mut()
        .for_each(|row| row.content_note = None);
    let unknown = export::render(Format::Markdown, &export::build(&mute));
    assert!(unknown.contains("content since sign-off: changed"));
}

#[test]
fn the_verdict_counts_decisions_and_says_what_is_left() {
    let record = export::build(&input());
    assert_eq!(record.summary.dismissed, 1);
    assert_eq!(record.summary.unsigned, 1);
    assert_eq!(record.summary.signed_off, 0);
    assert!(
        record.summary.verdict.contains("0 of 2"),
        "{}",
        record.summary.verdict
    );
    assert!(record.summary.verdict.contains("undecided"));
}

#[test]
fn the_checklist_puts_open_items_first_and_says_what_to_type() {
    let record = export::build(&input());
    assert!(!record.checklist.is_empty());
    let open = record
        .checklist
        .iter()
        .filter(|item| item.is_open())
        .count();
    assert_eq!(
        record
            .checklist
            .iter()
            .take(open)
            .filter(|i| i.is_open())
            .count(),
        open,
        "an open item is printed after a closed one"
    );
    let text = render(Format::Markdown);
    assert!(
        text.contains("- [ ]") && text.contains("- [x]"),
        "the checklist must be a checklist: {text}"
    );
    assert!(
        text.contains("fathom review sign"),
        "an open review item has to say what closes it"
    );
    assert!(
        text.contains("e4.6") || text.contains("e4.3"),
        "every item names the ticket that owns what is missing"
    );
    let html = render(Format::Html);
    assert!(html.contains("<input") && html.contains("checkbox"));
}

#[test]
fn a_ledger_that_reports_a_problem_becomes_a_checklist_item() {
    // A torn line in the ledger is an operator's problem, not the renderer's, and the artefact is where the
    // operator will read about it.
    let record = export::build(&input());
    assert!(
        record
            .checklist
            .iter()
            .any(|item| item.kind == "ledger" && item.is_open()),
        "{:?}",
        record.checklist
    );
    assert!(render(Format::Markdown).contains("not an entry"));
}

// --- the rest of the record ----------------------------------------------------------------------------------

#[test]
fn the_json_artefact_is_the_record_and_round_trips() {
    let record = export::build(&input());
    let text = export::render(Format::Json, &record);
    let back: export::Record =
        serde_json::from_str(&text).expect("the JSON artefact is the record");
    assert_eq!(
        back, record,
        "the artefact did not survive its own serialisation"
    );
    assert_eq!(back.contract, export::CONTRACT);
    assert_eq!(back.schema_version, export::VERSION);
    assert_eq!(back.scope.delta_contract, insight::CONTRACT);
    assert_eq!(back.integrity.problems, Vec::<String>::new());
    // The narrative travels with it: `e4.7`'s prose is the artefact's own words, not a re-telling.
    assert!(!back.episodes[0].narrative_markdown.trim().is_empty());
    assert!(text.contains("\"narrative_markdown\""));
}

#[test]
fn the_self_check_is_a_check() {
    // `integrity` is computed at build time and printed as a verdict. If nothing could ever fail it, the
    // field would be decoration — so each planted fault has to be caught by the name it deserves.
    let clean = export::build(&input());
    assert!(clean.verify().is_empty(), "{:?}", clean.verify());
    assert!(clean.integrity.verified);
    assert_eq!(
        clean.integrity.checks,
        export::CHECKS
            .iter()
            .map(|check| (*check).to_owned())
            .collect::<Vec<_>>(),
        "the artefact prints the checks it claims to have run, in the order it prints them"
    );

    let mut tampered = export::build(&input());
    tampered
        .findings
        .push(finding("ep-not-in-scope", "secret-string", "high"));
    assert!(
        tampered
            .verify()
            .iter()
            .any(|problem| problem.contains("ep-not-in-scope")),
        "a finding about an episode outside the scope passed the self-check"
    );

    let mut tampered = export::build(&input());
    tampered.summary.files = 999_999;
    assert!(
        tampered
            .verify()
            .iter()
            .any(|problem| problem.contains("summary")),
        "a summary that disagrees with the episodes passed the self-check"
    );

    let mut tampered = export::build(&input());
    tampered.signoffs[0].episode = "ep-not-in-scope".to_owned();
    assert!(
        tampered
            .verify()
            .iter()
            .any(|problem| problem.contains("sign-off")),
        "a sign-off for an episode outside the scope passed the self-check"
    );

    // The leak check watches the fields this module owns and redacts — a host path in a *quotation* can be
    // the repository's own content (a config file, a doc), and refusing to print the artefact over it would
    // be the artefact making things up again, this time about the operator's filesystem.
    let mut tampered = export::build(&input());
    tampered.provenance.store = Some("/home/dana/.fathom/index.sqlite3".to_owned());
    assert!(
        tampered
            .verify()
            .iter()
            .any(|problem| problem.contains("absolute path")),
        "a leaked absolute path passed the self-check"
    );
    let mut tampered = export::build(&input());
    tampered.signoffs[0].file = "/tmp/ledger.md".to_owned();
    assert!(
        tampered
            .verify()
            .iter()
            .any(|problem| problem.contains("repo-relative")),
        "an absolute ledger path passed the self-check"
    );
}

#[test]
fn gaps_are_stated_once_and_name_their_owner_and_remedy() {
    let record = export::build(&input());
    assert!(
        !record.gaps.is_empty(),
        "a record with unavailable sections and no gaps is a record claiming completeness"
    );
    let mut seen: Vec<(String, String)> = Vec::new();
    for gap in &record.gaps {
        let key = (gap.section.clone(), gap.short.clone());
        assert!(!seen.contains(&key), "the gap {key:?} is printed twice");
        seen.push(key);
        assert!(
            !gap.owner.is_empty(),
            "a gap with no owner is a gap nobody will close"
        );
        assert!(!gap.reason.is_empty());
    }
    let remedied = export::build(&Input {
        extra_gaps: vec![insight::Gap {
            section: "dependencies".to_owned(),
            short: "the range's dependency delta could not be computed".to_owned(),
            reason: "the trees could not be read".to_owned(),
            owner: "e2.6".to_owned(),
            remedy: Some("fathom index".to_owned()),
        }],
        ..input()
    });
    let text = render(Format::Markdown);
    assert!(text.contains("e3.2"), "the owner of a gap is not printed");
    let with_remedy = export::render(Format::Markdown, &remedied);
    assert!(
        with_remedy.contains("fathom index"),
        "a gap with a remedy must print what to run: {with_remedy}"
    );
}

#[test]
fn an_artefact_with_nothing_in_it_still_renders_and_still_verifies() {
    // The empty range is the common case for a first run, and every table in the artefact is a place where
    // an empty list can be printed as a lie ("no findings", "no dependencies") rather than as an absence.
    let empty = export::build(&Input {
        asked: "HEAD".to_owned(),
        from: None,
        to: None,
        resolved: false,
        episodes: Vec::new(),
        commits: Vec::new(),
        dependencies: None,
        signoffs: Vec::new(),
        ledger_problems: Vec::new(),
        memory: Vec::new(),
        memory_problems: Vec::new(),
        produced_by: "fathom 0.1.0".to_owned(),
        store: None,
        epoch: None,
        asked_with: "fathom export".to_owned(),
        extra_gaps: Vec::new(),
    });
    assert!(empty.verify().is_empty(), "{:?}", empty.verify());
    for format in [Format::Markdown, Format::Html, Format::Json] {
        let text = export::render(format, &empty);
        assert!(
            text.contains("fathom-review-record/1"),
            "{format:?} lost the contract"
        );
        assert!(
            !text.contains("null\nnull"),
            "{format:?} printed an empty record as nulls"
        );
    }
    assert_eq!(empty.summary.episodes, 0);
    assert!(empty.scope.from.is_none());
}

#[test]
fn the_unwired_sections_are_reported_as_unwired() {
    // The architecture delta is a roll-up of what the index holds, and the dependency delta is the one the
    // range is asked about; where a section has no source yet the artefact prints the refusal, not a zero.
    let record = export::build(&input());
    assert_eq!(record.scope.section_states.len(), 7);
    let states: Vec<(&str, usize, usize, usize)> = record
        .scope
        .section_states
        .iter()
        .map(|row| (row.id.as_str(), row.measured, row.partial, row.unavailable))
        .collect();
    for (_id, measured, partial, unavailable) in states {
        assert_eq!(
            measured + partial + unavailable,
            2,
            "every episode is counted in every section state"
        );
    }
    let text = render(Format::Markdown);
    assert!(
        text.contains("## What this record can and cannot answer"),
        "the artefact does not print its own can/cannot table"
    );
    assert!(
        text.contains("e3.2") && text.contains("e2.6"),
        "the tickets that own the missing answers are not printed"
    );
}

#[test]
fn a_missing_dependency_delta_is_a_refusal_and_not_a_zero() {
    let mut given = input();
    given.dependencies = None;
    let text = export::render(Format::Markdown, &export::build(&given));
    assert!(
        text.contains("Not computed for this range"),
        "the dependency section printed nothing instead of refusing: {text}"
    );
    let html = export::render(Format::Html, &export::build(&given));
    assert!(
        html.contains("Not computed for this range"),
        "the HTML refused differently from the markdown"
    );
}

#[test]
fn commit_links_are_offered_only_when_they_are_real_links() {
    let text = render(Format::Markdown);
    assert!(
        text.contains("https://example.invalid/commit/ep-aaaa1111-first"),
        "a derivable remote link is not printed"
    );
    let mut given = input();
    for row in &mut given.commits {
        row.url = None;
    }
    for episode in &mut given.episodes {
        for row in &mut episode.commits {
            row.url = None;
        }
    }
    let text = export::render(Format::Markdown, &export::build(&given));
    assert!(
        !text.contains("/commit/"),
        "a link was invented for a commit with no remote:\n{text}"
    );
    assert!(
        text.contains("no remote"),
        "the artefact should say why there is no link: {text}"
    );
    let html = export::render(Format::Html, &export::build(&given));
    assert!(
        !html.contains("/commit/"),
        "the HTML invented a link the record does not have"
    );
}
