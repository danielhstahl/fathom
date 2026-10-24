//! The event bus and its wire format, as contract (`e6.7`, `reco-raw`).
//!
//! Three claims, one per rule in the module header: a subscriber may not break the analysis; a reader
//! ignores what it does not know and says so; and the whole thing is values in and values out — no clock,
//! no file, no environment, so every one of these tests is a function of its input.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use fathom_core::events::{
    self, CONTRACT, Event, Filter, Kind, MAX_SUBSCRIBERS, Reading, SCHEMA_VERSION, Subscriber,
};
use fathom_core::time::Timestamp;

/// A subscriber that counts, and remembers the last event it saw.
#[derive(Default)]
struct Counter {
    seen: AtomicU64,
    last: std::sync::Mutex<Option<Event>>,
}

impl Subscriber for Counter {
    fn on_event(&self, event: &Event) {
        *self.last.lock().expect("unused") = Some(event.clone());
        self.seen.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn every_event_carries_the_contract_and_the_version_it_claims() {
    let event = Event::new(events::kind::ANALYSIS_STARTED, "run-1").episode("ep-7f2a");
    let line = event.to_line().expect("serialisable");
    let value: serde_json::Value = serde_json::from_str(&line).expect("one object");
    assert_eq!(value["contract"], CONTRACT);
    assert_eq!(value["schema_version"], SCHEMA_VERSION);
    assert_eq!(value["kind"], "analysis.started");
    assert_eq!(value["episode"], "ep-7f2a");
    // The optional halves are absent, not null: a stream a human reads has no `"detector":null` on it.
    assert!(value.get("detector").is_none());
    assert!(value.get("note").is_none());
}

#[test]
fn a_bus_stamps_sequence_numbers_of_its_own_and_in_order() {
    let bus = events::Bus::new();
    let counter = Arc::new(Counter::default());
    let _kept = bus.subscribe(counter.clone());
    for name in [
        events::kind::RUN_STARTED,
        events::kind::ANALYSIS_STARTED,
        events::kind::RUN_FINISHED,
    ] {
        bus.publish(Event::new(name, "run-1"));
    }
    let seen = counter.seen.load(Ordering::Relaxed);
    assert_eq!(seen, 3);
    let stats = bus.stats();
    assert_eq!(stats.published, 3);
    assert_eq!(stats.delivered, 3);
    assert_eq!(stats.panics, 0);
    // The event handed to the subscriber is stamped; the one the producer built is not, and never claims
    // a position it was not given.
    assert_eq!(
        counter
            .last
            .lock()
            .expect("unused")
            .as_ref()
            .expect("delivered")
            .seq,
        3
    );
}

#[test]
fn a_panicking_subscriber_is_counted_and_the_rest_still_get_the_event() {
    // The rule the whole module is built on: a subscriber is somebody else's code, and its panic is not
    // this analysis' failure.
    let bus = events::Bus::new();
    let before = Arc::new(Counter::default());
    let _keep_before = bus.subscribe(before.clone());
    let boom = Arc::new(|_event: &Event| panic!("a subscriber that panics"));
    let subscription = bus.subscribe(boom);
    let after = Arc::new(Counter::default());
    let _keep_after = bus.subscribe(after.clone());

    // The panic the bus swallows still prints through the default hook; that noise is the test's, not
    // the product's, so the hook is quieted for the duration.
    let hook = |_: &std::panic::PanicHookInfo<'_>| {};
    std::panic::set_hook(Box::new(hook));
    bus.publish(Event::new(events::kind::NOTICE, "run-1"));
    let _previous = std::panic::take_hook();

    assert_eq!(before.seen.load(Ordering::Relaxed), 1);
    assert_eq!(after.seen.load(Ordering::Relaxed), 1);
    let stats = bus.stats();
    assert_eq!(stats.panics, 1, "the panic is counted, not swallowed");
    assert_eq!(stats.delivered, 2);
    assert_eq!(
        stats.subscribers, 3,
        "the offender stays listed: dropping it silently makes the \
                                       next mystery harder"
    );
    drop(subscription);
    assert_eq!(bus.stats().subscribers, 2);
}

#[test]
fn dropping_a_subscription_unsubscribes() {
    let bus = events::Bus::new();
    let counter = Arc::new(Counter::default());
    let subscription = bus.subscribe(counter.clone());
    bus.publish(Event::new(events::kind::NOTICE, "run-1"));
    drop(subscription);
    bus.publish(Event::new(events::kind::NOTICE, "run-1"));
    assert_eq!(counter.seen.load(Ordering::Relaxed), 1);
    assert!(bus.is_quiet());
}

#[test]
fn the_sixteenth_subscription_is_refused_with_a_number_attached() {
    let bus = events::Bus::new();
    let mut kept = Vec::new();
    for _ in 0..MAX_SUBSCRIBERS {
        kept.push(bus.subscribe(Arc::new(Counter::default())));
    }
    assert!(bus.subscribe(Arc::new(Counter::default())).is_none());
    assert_eq!(bus.stats().refused, 1);
    assert_eq!(bus.stats().subscribers, MAX_SUBSCRIBERS);
}

#[test]
fn a_recorder_keeps_its_cap_and_counts_what_it_dropped() {
    let recorder = events::Recorder::new(2);
    let bus = events::Bus::new();
    let subscription = bus.subscribe(Arc::clone(&recorder) as Arc<dyn Subscriber>);
    for index in 0..5 {
        bus.publish(Event::new(events::kind::NOTICE, "run").count("i", index));
    }
    drop(subscription);
    assert_eq!(recorder.len(), 2);
    assert_eq!(recorder.dropped(), 3);
    let kept = recorder.events();
    assert_eq!(kept[0].counts.get("i").copied(), Some(3));
    assert_eq!(kept[1].counts.get("i").copied(), Some(4));
    assert_eq!(
        recorder.kinds(),
        vec!["notice.raised".to_owned(), "notice.raised".to_owned()]
    );
    assert!(!recorder.is_empty());
}

#[test]
fn a_line_round_trips_and_an_unknown_kind_survives_as_a_kind() {
    let event = Event::new(Kind::new("merge.requested"), "run-9")
        .subject("refs/heads/main")
        .at(Timestamp::FIXED_FIXTURE);
    let line = event.to_line().expect("serialisable");
    let reading = Event::from_line(&line);
    let Some(read) = reading.event() else {
        panic!("a line this build wrote must read back: {line}");
    };
    assert_eq!(read, &event);
    assert!(
        !read.kind.is_declared(),
        "a new producer's kind is unknown here, and kept"
    );
    assert_eq!(read.kind.family(), "merge");
    assert_eq!(read.ts, Some(Timestamp::FIXED_FIXTURE));
}

#[test]
fn a_version_this_build_cannot_read_is_refused_by_name() {
    let line = r#"{"contract":"fathom-analysis-events","schema_version":99,"kind":"notice","seq":1,"run":"r"}"#;
    let Reading::Ignored { reason } = Event::from_line(line) else {
        panic!("a document from the future must be refused, not partially understood");
    };
    assert!(reason.contains("99") && reason.contains("1"), "{reason}");
}

#[test]
fn a_stream_reports_its_bad_lines_rather_than_failing() {
    // The realistic case: a run killed mid-write leaves a half line at the end, and every event before it
    // is still worth having.
    let text = concat!(
        r#"{"contract":"fathom-analysis-events","schema_version":1,"kind":"run.started","seq":1,"run":"r"}"#,
        "\n",
        "not json at all\n",
        "\n",
        r#"{"contract":"fathom-analysis-events","schema_version":1,"kind":"analysis.finished","seq":2}"#,
        "\n",
        r#"{"contract":"fathom-hooks","schema_version":1,"kind":"tool_call","seq":3,"run":"r"}"#,
        "\n",
        r#"{"contract":"fathom-analysis-events","schema_version":1,"kind":"run.finished","seq":4,"run":"r"}"#,
    );
    let stream = events::read_stream(text);
    assert_eq!(
        stream.events.len(),
        2,
        "the two lines that are ours and readable"
    );
    assert_eq!(stream.ignored.len(), 3, "one per refused line: {text}");
    assert!(stream.ignored[0].contains("not JSON"));
    assert!(
        stream.ignored[1].contains("missing field"),
        "{:?}",
        stream.ignored[1]
    );
    assert!(
        stream.ignored[2].contains("fathom-hooks"),
        "{:?}",
        stream.ignored[2]
    );
}

#[test]
fn an_unknown_field_is_ignored_because_additive_must_stay_additive() {
    let line = r#"{"contract":"fathom-analysis-events","schema_version":1,"kind":"notice","seq":1,"run":"r","note":"hello","a_field_from_2027":{"nested":true}}"#;
    let reading = Event::from_line(line);
    let event = reading
        .event()
        .expect("unknown fields are the point of additive versioning");
    assert_eq!(event.note.as_deref(), Some("hello"));
}

#[test]
fn a_filter_selects_a_family_or_an_exact_kind() {
    let (filter, notices) = Filter::parse(["analysis.*", "notice.raised", ""]);
    assert_eq!(
        notices.len(),
        1,
        "the empty pattern is refused with a sentence"
    );
    assert!(filter.allows(&Kind::new(events::kind::ANALYSIS_STARTED)));
    assert!(filter.allows(&Kind::new(events::kind::ANALYSIS_FINISHED)));
    assert!(filter.allows(&Kind::new(events::kind::NOTICE)));
    assert!(!filter.allows(&Kind::new(events::kind::RUN_STARTED)));
    assert!(!filter.allows(&Kind::new("merge.requested")));
    let (all, _) = Filter::parse(["*"]);
    assert!(all.allows(&Kind::new("whatever.a.newer.build.sends")));
    // `events.on = []` is not "nothing": a filter that selects nothing is a run that says nothing, which
    // is a worse failure than a notice.
    let (fallback, notes) = Filter::parse(Vec::<String>::new());
    assert!(fallback.allows(&Kind::new(events::kind::RUN_FINISHED)));
    assert!(notes.is_empty());
}

#[test]
fn the_declared_kinds_are_named_in_one_family_order_and_match_their_spelling() {
    for name in events::kind::ALL {
        let kind = Kind::new(*name);
        assert!(
            kind.is_declared(),
            "{name} is declared and must read as declared"
        );
        let (family, fact) = name.split_once('.').expect("every kind is family.fact");
        assert!(!family.is_empty() && !fact.is_empty(), "{name}");
        assert!(
            name.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_'),
            "{name} is lowercase dotted"
        );
    }
    assert!(!Kind::new("nonsense").is_declared());
}

/// The published schema for one line of the stream is generated and blessed, not hand-kept (ADR-002 §4.3):
/// the types in `events` own the shape, the generator writes the document, and this test is what notices when
/// they disagree. `FATHOM_BLESS=1` rewrites the file — and the diff that comes out of that is the diff of the
/// contract, which is the point of generating it.
#[test]
#[cfg(feature = "json-schema")]
fn the_checked_in_event_schema_is_the_generated_one() {
    use std::path::{Path, PathBuf};

    fn workspace_root() -> PathBuf {
        let mut dir = Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf();
        loop {
            if dir.join("Cargo.lock").is_file() {
                return dir;
            }
            match dir.parent() {
                Some(parent) => dir = parent.to_path_buf(),
                None => panic!("no Cargo.lock above the crate directory"),
            }
        }
    }

    let path = workspace_root().join(fathom_core::events::schema::SCHEMA_FILE);
    let generated = fathom_core::events::schema::to_text();
    if std::env::var_os("FATHOM_BLESS").is_some_and(|v| v == "1") {
        std::fs::create_dir_all(path.parent().unwrap_or(Path::new("."))).expect("create schemas/");
        std::fs::write(&path, &generated).expect("bless the event schema");
    }
    let checked_in = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("{}: {err} — write it with FATHOM_BLESS=1", path.display()));
    assert_eq!(
        generated,
        checked_in,
        "event schema drifted from {}; regenerate with FATHOM_BLESS=1",
        path.display()
    );
}
