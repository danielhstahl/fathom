//! The first five minutes, from the outside (`e5.9`, reco-2uo).
//!
//! The ticket's claim is narrow and checkable: on a clean machine, with nothing configured, `fathom --demo`
//! shows something worth looking at in under a minute. Every one of those words is a test below — *clean*
//! (a HOME with no cache and no configuration), *nothing configured* (no `fathom.toml`, no `FATHOM_*` in
//! the environment), *worth looking at* (the frame carries ranked episodes with figures on them, not an
//! empty state), and *under a minute* (measured, not asserted in prose).
//!
//! This file is also the only place the miniature is exercised through the binary rather than through a
//! fixture builder, which makes it the test that fails when the shipped pack and the code that reads it
//! drift apart — the failure mode a checked-in asset always carries.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Instant;

/// A throwaway "clean machine": an empty HOME, an empty cache root, and a working directory that is
/// neither. Everything the demo is allowed to touch lives under `root`; everything it is not allowed to
/// touch is `cwd`, which exists to be *not* touched.
struct Machine {
    root: PathBuf,
    cwd: PathBuf,
}

impl Machine {
    fn new(name: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "fathom-cli-demo-{}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            name
        ));
        let _ = std::fs::remove_dir_all(&root);
        let cwd = root.join("cwd");
        std::fs::create_dir_all(root.join("home")).expect("create the fake HOME");
        std::fs::create_dir_all(cwd.join("src")).expect("create the working directory");
        // A directory that looks like somebody's project, and is not a repository: the demo must not care
        // what it is run inside, and a stray `.git` here would make `--demo` index the wrong thing.
        std::fs::write(cwd.join("README.md"), "# mine\n").expect("write the decoy file");
        std::fs::write(cwd.join("src/lib.rs"), "pub fn mine() {}\n").expect("write the decoy file");
        Machine { root, cwd }
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    fn cache(&self) -> PathBuf {
        self.root.join("cache")
    }

    /// Run the built binary as this machine's user would: no inherited configuration, no `FATHOM_*`, and
    /// a HOME that started empty.
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_fathom"))
            .current_dir(&self.cwd)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", self.home())
            .env("XDG_CACHE_HOME", self.cache())
            .env("LC_CTYPE", "C.UTF-8")
            .args(args)
            .output()
            .expect("the built fathom binary is runnable")
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

fn code(out: &Output) -> i32 {
    out.status
        .code()
        .expect("fathom exits with a code, not a signal")
}

/// The claim, in one test: clean machine, no configuration, under a minute, real content.
///
/// The time bound is the ticket's, and it is measured rather than assumed because the miniature ships as
/// an asset — it has to stay small enough for the promise to stay true, and a test is the only thing that
/// notices when a regeneration makes it four times bigger.
#[test]
#[cfg(feature = "sqlite-bundled")]
fn a_clean_machine_goes_from_nothing_to_a_frame_in_under_a_minute() {
    let machine = Machine::new("cold");
    // `--plain` because a test has no terminal: the frame is then printed as text, which is the same pane
    // contents the shell would draw (ADR-006 §6.7).
    let started = Instant::now();
    let out = machine.run(&["--demo", "--plain"]);
    let took = started.elapsed();
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(
        took < std::time::Duration::from_secs(60),
        "the first-run promise is one minute; this run took {took:?}"
    );
    // …and well inside it, because the miniature exists to be immediate. A slow machine is allowed a
    // factor of ten before this stops being a regression and starts being a weather report.
    assert!(
        took < std::time::Duration::from_secs(10),
        "the miniature is meant to be instant; this run took {took:?}"
    );

    let text = stdout(&out);
    // Not an empty state. The distinction is the whole point of the pack: 21 commits that produce ranked
    // episodes, so the first frame a stranger sees is the product working.
    assert!(
        !text.contains("no index here yet"),
        "the demo indexed nothing:\n{text}"
    );
    assert!(
        text.contains("21 episodes from index epoch 1 (ready)"),
        "the frame should say what was indexed:\n{text}"
    );
    // A ranked row with figures on it — `risk`, `+`, `−` — rather than a list of subjects. The queue *is*
    // the product's answer, so the demo has to show an answer and not a placeholder for one.
    assert!(
        text.contains("risk") && text.contains('+') && text.contains('−'),
        "the queue pane is showing no ranked episode:\n{text}"
    );
    assert!(
        text.contains("agent") && text.contains("human"),
        "the miniature is supposed to be mixed agent and human work, so the queue can rank them \
         against each other:\n{text}"
    );

    let advice = stderr(&out);
    for line in [
        "two people and one agent wrote it",
        "`?` lists the keys",
        "deleting that directory is a complete undo",
        "cd <your-repository> && fathom index",
    ] {
        assert!(advice.contains(line), "the guide lost {line:?}:\n{advice}");
    }
}

/// Warm is not cold. The second run must not re-unpack or re-index: a demo that re-did its work on every
/// invocation would be indistinguishable from one that indexes in the foreground forever, and `0 new` is
/// the same report a real repository gives on a second `fathom index`.
#[test]
#[cfg(feature = "sqlite-bundled")]
fn a_second_demo_run_reuses_the_index_and_says_so_in_the_same_words_as_any_reindex() {
    let machine = Machine::new("warm");
    machine.run(&["--demo", "--plain"]);
    let out = machine.run(&["--demo", "--plain"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(
        stderr(&out).contains("0 new"),
        "the second run indexed again:\n{}",
        stderr(&out)
    );
}

/// The demo is allowed to write one place: its own directory under the cache root.
///
/// The promise in the guide — "deleting that directory is a complete undo" — is a promise about the
/// reviewer's own files, and this is the test that keeps it true. The directory the demo was run from is
/// checked for *any* change: a `.fathom/` appearing there would mean the miniature had been unpacked into
/// somebody's project, which is the opposite of what it is for.
#[test]
#[cfg(feature = "sqlite-bundled")]
fn the_demo_writes_only_under_the_cache_directory() {
    let machine = Machine::new("writes");
    let before = listing(&machine.cwd);
    let out = machine.run(&["--demo", "--plain"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(
        listing(&machine.cwd),
        before,
        "the demo changed the directory it was run in"
    );
    assert!(
        !machine.home().join(".fathom").exists(),
        "the demo wrote into HOME"
    );
    assert!(!machine.home().join(".config").exists());

    let cache = machine.cache().join("fathom");
    assert!(cache.is_dir(), "nothing was unpacked under {cache:?}");
    // `…/fathom/<digest>/signal`, the digest directory holding the repository it unpacked — named after
    // the miniature rather than after whatever the demo happened to be called, so two packs can coexist.
    let unpacked = std::fs::read_dir(&cache)
        .expect("the cache directory is readable")
        .filter_map(Result::ok)
        .flat_map(|entry| {
            std::fs::read_dir(entry.path())
                .into_iter()
                .flat_map(|inner| inner.filter_map(Result::ok))
                .map(|inner| inner.path())
                .collect::<Vec<PathBuf>>()
        })
        .find(|path| path.join(".fathom-demo").is_file())
        .expect("the unpacked miniature is marked with .fathom-demo");
    assert!(
        unpacked.join(".fathom/index.sqlite3").is_file(),
        "the miniature was unpacked but not indexed at {unpacked:?}"
    );
    // The marker is the whole undo: it holds the digest it is named after, which is what makes "already
    // unpacked" a stat rather than a hash, and what makes a stale binary's copy unused instead of trusted.
    let marker = std::fs::read_to_string(unpacked.join(".fathom-demo")).expect("read the marker");
    let book = manifest();
    let digest = book["digest"].as_str().expect("a digest");
    assert_eq!(
        marker.trim(),
        digest,
        "the cache directory is named from the digest and the marker must agree"
    );
    assert!(
        unpacked
            .parent()
            .is_some_and(|name| name.file_name().is_some_and(|name| name == &digest[..12])),
        "the unpacked directory is {unpacked:?}, which is not under a directory named from the digest"
    );
}

/// `--demo` is a modifier, not a substitute for the verb: `fathom --demo stats` reports the miniature.
///
/// This is why the demo is entered by changing the process's working directory rather than by
/// special-casing each verb — the miniature then *is* the repository under review, and every verb
/// exercises it through the code path it uses for a real one.
#[test]
#[cfg(feature = "sqlite-bundled")]
fn a_verb_run_under_the_demo_reports_the_miniature() {
    let machine = Machine::new("verb");
    let out = machine.run(&["--demo", "stats", "--json"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let parsed: serde_json::Value =
        serde_json::from_str(stdout(&out).trim()).expect("`--demo stats --json` is JSON");
    assert_eq!(
        parsed["index"]["commits"].as_u64(),
        Some(21),
        "the miniature is 21 commits: {parsed}"
    );
    assert_eq!(
        parsed["index"]["epoch_status"].as_str(),
        Some("ready"),
        "the demo's index is finished, not building: {parsed}"
    );
    assert_eq!(
        parsed["index"]["scale"]["authors"].as_u64(),
        Some(2),
        "two people wrote it, which is what makes the queue's ranking legible: {parsed}"
    );
}

/// The pack is versioned by its own digest, and the manifest beside it describes the bytes the binary
/// actually embedded — so a regeneration that forgot to rewrite the manifest is caught here rather than by
/// a reviewer silently serving a stale miniature out of a cache directory keyed to the old digest.
#[test]
fn the_shipped_pack_and_its_manifest_agree() {
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
    let manifest = manifest();
    let pack = std::fs::read(assets.join("fathom-demo.pack")).expect("the pack asset is installed");
    assert!(
        pack.starts_with(b"FATHOMDEMO"),
        "the pack's magic word is FATHOMDEMO"
    );
    assert_eq!(pack[10], 1, "this binary reads version 1 of the pack");
    assert_eq!(
        manifest["bytes"].as_u64(),
        Some(pack.len() as u64),
        "the manifest describes a different pack than the one installed"
    );
    let entries = u32::from_le_bytes([pack[11], pack[12], pack[13], pack[14]]) as u64;
    assert_eq!(
        manifest["entries"].as_u64(),
        Some(entries),
        "the manifest's entry count and the pack's header disagree"
    );
    let digest = manifest["digest"]
        .as_str()
        .expect("the manifest has a digest");
    assert_eq!(digest.len(), 64, "the digest is a hex sha256");
    assert_eq!(manifest["commits"].as_u64(), Some(21));
}

fn manifest() -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("fathom-demo.json");
    serde_json::from_slice(&std::fs::read(&path).expect("the manifest asset is installed"))
        .expect("the manifest is JSON")
}

/// Every path under `dir`, sorted, for the "nothing changed here" assertions.
fn listing(dir: &Path) -> Vec<String> {
    let mut entries = walkdir(dir);
    entries.sort();
    entries
}

fn walkdir(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(read) = std::fs::read_dir(dir) {
        for entry in read.filter_map(Result::ok) {
            let path = entry.path();
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if path.is_dir() {
                out.extend(
                    walkdir(&path)
                        .into_iter()
                        .map(|child| format!("{name}/{child}")),
                );
            } else {
                out.push(name);
            }
        }
    }
    out
}
