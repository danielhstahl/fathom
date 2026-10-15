//! `source.rs` — where the watcher's "something happened" signals come from, and how it notices that they
//! are not arriving.
//!
//! There are two backends and one rule. The rule is that **a notification backend that does not notify is
//! the common case, not the rare one**: inotify does not fire across an NFS or SMB mount, does not cross a
//! Docker bind mount from macOS (virtiofs/fuse), and refuses outright once a desktop has spent its inotify
//! watch budget. A tool that silently watches nothing in those environments is worse than one that polls,
//! because it looks alive — so this crate does not ask the user which backend to trust, it *checks*: a
//! change discovered by the debounced poll that the notification backend never mentioned is evidence, and
//! two pieces of evidence switch the backend over with the reason recorded ([`Probe::reason`], printed by
//! `fathom doctor`).
//!
//! The polling backend is a `stat` loop over [`crate::watch::observe::Fingerprint`], deliberately *not*
//! `notify`'s own `Poller`: the poller re-`stat`s every file under what it watches, which for `.git` means
//! every loose object — tens of thousands of syscalls per second on an old repository, and the exact CPU
//! cost this ticket has to stay under. Watching git's *bookkeeping* instead of git's object store is what
//! makes a second-by-second poll cost about a dozen syscalls.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Which kind of signal a backend produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    /// The OS said so (inotify, FSEvents, ReadDirectoryChangesW, kqueue).
    Notification,
    /// We looked, on a timer.
    Polling,
}

impl BackendKind {
    /// The stable spelling for `--json` and `fathom doctor`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            BackendKind::Notification => "notification",
            BackendKind::Polling => "polling",
        }
    }
}

/// What backend is in use, and why.
///
/// The `reason` is not decoration: "polling" on its own reads like a defect, while "polling, because
/// `.git` is on a `virtiofs` mount and inotify does not cross it" is an explanation a user can act on or
/// ignore knowingly. Every field is printed by `fathom doctor`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendInfo {
    /// The kind.
    pub kind: BackendKind,
    /// The backend's own name: `inotify`, `fsevents`, `read-directory-changes`, `kqueue`, `poll`.
    pub name: String,
    /// Why this backend rather than the default one, if there was a choice to make.
    pub reason: Option<String>,
}

impl BackendInfo {
    /// The one-line form: `notification(inotify)` or `poll — <reason>`.
    #[must_use]
    pub fn summary(&self) -> String {
        match &self.reason {
            Some(reason) => format!("{}({}) — {reason}", self.kind.as_str(), self.name),
            None => format!("{}({})", self.kind.as_str(), self.name),
        }
    }
}

/// What one wait produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sign {
    /// The timeout passed with nothing to report.
    Nothing,
    /// Something under the watched paths moved.
    Changed,
    /// The backend is degrading or has died. The payload is its own words, and the watcher treats this as
    /// evidence against the backend rather than as an error to show.
    Degraded(String),
}

impl Sign {
    /// Did anything arrive?
    #[must_use]
    pub fn arrived(&self) -> bool {
        matches!(self, Sign::Changed)
    }
}

/// A source of change signals. `Send` because the daemon of `reco-kvk` will own one on its own thread.
pub trait Source: Send {
    /// Block up to `timeout` for a sign of change. Must return in roughly `timeout`; the watcher's own
    /// deadlines (settle windows, poll ticks, the stop flag) are its business, not the source's.
    fn wait(&mut self, timeout: Duration) -> Sign;

    /// What this backend is, and why it was chosen.
    fn info(&self) -> BackendInfo;

    /// Adjust the polling cadence. Only the polling backend honours this; stretching it is how the CPU
    /// budget in [`crate::watch::Options::cpu_share`] gets spent.
    fn set_interval(&mut self, _interval: Duration) {}
}

/// The OS notification backend, when this build has one.
#[cfg(feature = "watch")]
pub struct NotifySource {
    receiver: std::sync::mpsc::Receiver<notify::Result<notify::Event>>,
    // Held, not used: dropping the watcher unsubscribes, and the watcher's worker thread is what fills the
    // channel. There is no join handle to keep and no handle to poll.
    _watcher: notify::RecommendedWatcher,
    name: &'static str,
}

#[cfg(feature = "watch")]
impl NotifySource {
    /// Subscribe to `paths`, recursively. The error is a human-readable reason rather than a typed error
    /// because the caller's only action is "fall back and say why" — `notify`'s own error enum describes
    /// OS-level failures that have no fathom-side remedy beyond polling.
    ///
    /// Note what a *successful* subscription does not prove: that events will arrive. macOS and NFS mounts
    /// accept the subscription and stay silent forever, which is why the machine keeps a reactive fallback
    /// no matter how this call went.
    pub fn new(paths: &[PathBuf]) -> Result<NotifySource, String> {
        use notify::Watcher as _;

        let (sender, receiver) = std::sync::mpsc::channel();
        let mut watcher = notify::recommended_watcher(sender).map_err(|err| err.to_string())?;
        let mut subscribed = Vec::new();
        let mut failures: Vec<String> = Vec::new();
        for path in paths {
            match watcher.watch(path, notify::RecursiveMode::Recursive) {
                Ok(()) => subscribed.push(path.clone()),
                Err(err) => failures.push(format!("{}: {err}", path.display())),
            }
        }
        if subscribed.is_empty() {
            return Err(format!(
                "could not watch any path ({})",
                failures.join("; ")
            ));
        }
        let _ = failures;
        Ok(NotifySource {
            receiver,
            _watcher: watcher,
            name: NOTIFICATION_BACKEND,
        })
    }
}

#[cfg(feature = "watch")]
impl Source for NotifySource {
    fn wait(&mut self, timeout: Duration) -> Sign {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Sign::Nothing;
            }
            match self.receiver.recv_timeout(remaining) {
                Ok(Ok(event)) => {
                    if matters(&event) {
                        // Return on the first relevant event: the debounce window, not this loop, is what
                        // batches a commit's dozen events into one run, and returning early is what keeps
                        // detection latency at the debounce window instead of the full timeout.
                        return Sign::Changed;
                    }
                }
                Ok(Err(err)) => return Sign::Degraded(err.to_string()),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => return Sign::Nothing,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Sign::Degraded("the notification backend stopped sending".to_owned());
                }
            }
        }
    }

    fn info(&self) -> BackendInfo {
        BackendInfo {
            kind: BackendKind::Notification,
            name: self.name.to_owned(),
            reason: None,
        }
    }
}

/// The name of whatever `notify`'s recommended backend is on this platform. `notify` does not report it,
/// and the value is a compile-time fact about `recommended_watcher`, so it is stated here rather than
/// guessed at by a probe.
#[cfg(feature = "watch")]
pub const NOTIFICATION_BACKEND: &str = if cfg!(target_os = "linux") {
    "inotify"
} else if cfg!(target_os = "macos") {
    "fsevents"
} else if cfg!(target_os = "windows") {
    "read-directory-changes"
} else {
    "kqueue"
};

/// Is a notification backend compiled into this build at all? `fathom doctor` says so when it is not, which
/// is the difference between "polling by design" and "polling because you built the wrong binary".
pub const NOTIFICATION_COMPILED_IN: bool = cfg!(feature = "watch");

/// Is this `notify` event the repository having changed?
///
/// The filter is not tidiness, it is a feedback loop: `notify`'s backends report `Access` events for
/// opening and closing a watched path, and the watcher *opens* directories inside the git dir on every look
/// (`objects/pack`, the rebase todo, the ref directories). Unfiltered, the watcher notifies itself of its own
/// reading, re-arms its debounce window, looks again, and never goes idle — measured at about two
/// notifications per poll tick on a completely quiet repository before this was fixed. Only creation,
/// removal, modification and metadata/renames are the repository having changed.
#[cfg(feature = "watch")]
fn matters(event: &notify::Event) -> bool {
    !matches!(
        event.kind,
        notify::event::EventKind::Access(_)
            | notify::event::EventKind::Other
            | notify::event::EventKind::Any
    )
}

/// The polling backend: sleep, and report a tick when the cadence is due.
pub struct PollSource {
    interval: Duration,
    due: Instant,
}

impl PollSource {
    /// Poll every `interval`, starting immediately — a watcher that waits a whole interval before its first
    /// look would miss whatever happened between the caller's last read and now.
    #[must_use]
    pub fn new(interval: Duration) -> PollSource {
        PollSource {
            interval,
            due: Instant::now(),
        }
    }
}

impl Source for PollSource {
    fn wait(&mut self, timeout: Duration) -> Sign {
        let now = Instant::now();
        if now >= self.due {
            self.due = now + self.interval;
            return Sign::Changed;
        }
        // Sleep the shorter of "the caller's patience" and "until the next tick", so a stop flag is
        // noticed inside one tick instead of one interval.
        let until_due = self.due.duration_since(now);
        std::thread::sleep(timeout.min(until_due));
        let now = Instant::now();
        if now >= self.due {
            self.due = now + self.interval;
            Sign::Changed
        } else {
            Sign::Nothing
        }
    }

    fn info(&self) -> BackendInfo {
        BackendInfo {
            kind: BackendKind::Polling,
            name: "poll".to_owned(),
            reason: None,
        }
    }

    fn set_interval(&mut self, interval: Duration) {
        self.interval = interval;
    }
}

/// Which backend the caller asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackendChoice {
    /// Notifications where they can be trusted, polling where they cannot, with the reasoning reported.
    #[default]
    Auto,
    /// Notifications or a refusal. No silent polling: `--backend notify` is how a user says "I want to know
    /// if my filesystem cannot do this".
    Notify,
    /// Always the poll loop, at `Options::poll_interval`.
    Poll,
}

impl BackendChoice {
    /// Parse `--backend`. Unknown values are the caller's error to phrase, so `None`.
    #[must_use]
    pub fn parse(text: &str) -> Option<BackendChoice> {
        match text.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(BackendChoice::Auto),
            "notify" | "notification" | "inotify" | "fsevents" => Some(BackendChoice::Notify),
            "poll" | "polling" => Some(BackendChoice::Poll),
            _ => None,
        }
    }

    /// The spelling, for echoing back.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            BackendChoice::Auto => "auto",
            BackendChoice::Notify => "notify",
            BackendChoice::Poll => "poll",
        }
    }
}

/// The answer to "what should I use, and what did you decide about my filesystem".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    /// The chosen source and its reason.
    pub backend: BackendInfo,
    /// The filesystem type under the git dir, when this platform says (`/proc/mounts` on Linux).
    pub filesystem: Option<String>,
    /// Whether notifications are expected to work here at all — `false` is a *prediction*, and the reactive
    /// fallback exists for the cases the prediction gets wrong.
    pub notifications_trusted: bool,
    /// What `--backend` asked for, so `doctor` can show a request being overridden rather than hiding it.
    pub requested: BackendChoice,
}

/// Decide, without opening the repository, which backend a watcher there should use.
///
/// This is what `fathom doctor` runs: it costs a filesystem read and (on Linux) one `/proc/mounts` parse,
/// and it makes the automatic fallback in acceptance criterion four *inspectable* rather than something
/// the user only discovers when their commits arrive late.
#[must_use]
pub fn probe(choice: BackendChoice, git_dir: &Path, poll_interval: Duration) -> Probe {
    let filesystem = filesystem_fstype(git_dir);
    let untrusted_kind = filesystem
        .as_deref()
        .filter(|fstype| !notifications_work(fstype));
    let filesystem_reason = untrusted_kind.map(|fstype| {
        format!(
            "this filesystem is `{fstype}`; the OS notification backends do not report changes made \
             through such a mount, so polling is the backend that actually works here"
        )
    });

    let (backend, trusted) = match choice {
        BackendChoice::Poll => (
            BackendInfo {
                kind: BackendKind::Polling,
                name: "poll".to_owned(),
                reason: Some("asked for with `--backend poll`".to_owned()),
            },
            true,
        ),
        BackendChoice::Notify if !NOTIFICATION_COMPILED_IN => (
            BackendInfo {
                kind: BackendKind::Polling,
                name: "poll".to_owned(),
                reason: Some(
                    "this binary has no notification backend; rebuild with `--features watch` \
                     to get one"
                        .to_owned(),
                ),
            },
            false,
        ),
        BackendChoice::Notify => (
            BackendInfo {
                kind: BackendKind::Notification,
                name: notification_name().to_owned(),
                reason: filesystem_reason
                    .clone()
                    .map(|reason| format!("notifications were asked for even though {reason}")),
            },
            filesystem_reason.is_none(),
        ),
        BackendChoice::Auto if !NOTIFICATION_COMPILED_IN => (
            BackendInfo {
                kind: BackendKind::Polling,
                name: "poll".to_owned(),
                reason: Some(
                    "this binary has no notification backend; rebuild with `--features watch` \
                     to get one"
                        .to_owned(),
                ),
            },
            false,
        ),
        BackendChoice::Auto => match &filesystem_reason {
            Some(reason) => (
                BackendInfo {
                    kind: BackendKind::Polling,
                    name: "poll".to_owned(),
                    reason: Some(reason.clone()),
                },
                false,
            ),
            None => (
                BackendInfo {
                    kind: BackendKind::Notification,
                    name: notification_name().to_owned(),
                    reason: None,
                },
                true,
            ),
        },
    };
    let _ = poll_interval;
    Probe {
        backend,
        filesystem,
        notifications_trusted: trusted,
        requested: choice,
    }
}

/// Open the source the probe chose. On `Auto`, a notification backend that cannot even subscribe (inotify's
/// watch budget exhausted, an unsupported mount) degrades to polling here, with the OS's reason kept.
#[must_use]
pub fn open(
    probe: &Probe,
    paths: &[PathBuf],
    poll_interval: Duration,
) -> (Box<dyn Source>, BackendInfo) {
    match probe.backend.kind {
        BackendKind::Polling => {
            let mut source = PollSource::new(poll_interval);
            source.set_interval(poll_interval);
            (Box::new(source), probe.backend.clone())
        }
        BackendKind::Notification => open_notification(paths, poll_interval),
    }
}

/// Build the notification source, or the polling source that replaces it when the OS refuses.
fn open_notification(paths: &[PathBuf], poll_interval: Duration) -> (Box<dyn Source>, BackendInfo) {
    #[cfg(feature = "watch")]
    {
        match NotifySource::new(paths) {
            Ok(source) => {
                let info = source.info();
                (Box::new(source), info)
            }
            Err(reason) => (
                Box::new(PollSource::new(poll_interval)),
                BackendInfo {
                    kind: BackendKind::Polling,
                    name: "poll".to_owned(),
                    reason: Some(format!(
                        "the notification backend could not watch this repository ({reason}), so \
                         polling took over"
                    )),
                },
            ),
        }
    }
    #[cfg(not(feature = "watch"))]
    {
        let _ = paths;
        (
            Box::new(PollSource::new(poll_interval)),
            BackendInfo {
                kind: BackendKind::Polling,
                name: "poll".to_owned(),
                reason: Some("no notification backend in this build".to_owned()),
            },
        )
    }
}

/// The name to show for the notification backend when it is not compiled in — the probe must still be able
/// to say what it *would* have used.
#[must_use]
pub fn notification_name() -> &'static str {
    #[cfg(feature = "watch")]
    {
        NOTIFICATION_BACKEND
    }
    #[cfg(not(feature = "watch"))]
    {
        // Without the crate there is no backend to name, and `doctor` says so under `reason`.
        "none"
    }
}

/// Which filesystem `path` sits on, as the mount table spells it. `None` where the platform has no cheap
/// answer (every OS without `/proc/mounts`), which the caller treats as "uncharacterised", never as "fine".
#[must_use]
pub fn filesystem_fstype(path: &Path) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let table = std::fs::read_to_string("/proc/mounts").ok()?;
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        mount_fstype(&table, &canonical)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        None
    }
}

/// The longest mount point in `table` that contains `path`, and its filesystem type.
///
/// Pure and side-effect-free so the classification is testable with a synthetic mount table — the only way
/// to test NFS handling in a container with no NFS (the same problem ADR-004 §14.2 records for the store's
/// network-lock rules).
#[must_use]
pub fn mount_fstype(table: &str, path: &Path) -> Option<String> {
    let target = path.to_string_lossy();
    let mut best: Option<(usize, String)> = None;
    for line in table.lines() {
        let mut fields = line.split_whitespace();
        let (Some(_device), Some(mount), Some(fstype)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let mount = unescape_mount(mount);
        if mount.is_empty() || !contains(&mount, &target) {
            continue;
        }
        let score = mount.len();
        if best.as_ref().is_none_or(|(len, _)| score > *len) {
            best = Some((score, fstype.to_owned()));
        }
    }
    best.map(|(_, fstype)| fstype)
}

/// Does `mount` contain `path`, by path components rather than by characters (`/repo` must not be said to
/// contain `/repos`)?
fn contains(mount: &str, path: &str) -> bool {
    if mount == "/" {
        return path.starts_with('/');
    }
    path == mount
        || (path.len() > mount.len()
            && path.starts_with(mount)
            && path.as_bytes()[mount.len()] == b'/')
}

/// Undo the octal escapes `/proc/mounts` uses for whitespace in mount points.
fn unescape_mount(mount: &str) -> String {
    let bytes = mount.as_bytes();
    let mut out = String::with_capacity(mount.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\'
            && index + 3 < bytes.len()
            && bytes[index + 1].is_ascii_digit()
            && bytes[index + 2].is_ascii_digit()
            && bytes[index + 3].is_ascii_digit()
        {
            let octal = &mount[index + 1..index + 4];
            if let Ok(byte) = u8::from_str_radix(octal, 8) {
                out.push(byte as char);
                index += 4;
                continue;
            }
        }
        let start = index;
        while index < bytes.len() && bytes[index] != b'\\' {
            index += 1;
        }
        out.push_str(&mount[start..index]);
        if start == index {
            // A lone backslash that is not an escape: keep it rather than dropping a byte.
            out.push('\\');
            index += 1;
        }
    }
    out
}

/// Do OS notifications reach changes made to this filesystem type?
///
/// The list is what the inotify man page and years of container trouble agree on: network protocols, FUSE
/// (including the virtiofs and 9p mounts that container runtimes put between a host and a container), and
/// cluster filesystems. Local filesystems and `overlayfs` over a local upper dir are trusted. A `false`
/// here is a *prediction*, and the reactive fallback in `Watch` is what catches the cases where the
/// prediction is wrong (a Linux bind mount of a local host directory looks exactly like the host
/// filesystem and is not detectable this way at all).
#[must_use]
pub fn notifications_work(fstype: &str) -> bool {
    let fstype = fstype.to_ascii_lowercase();
    if fstype.starts_with("fuse") {
        return false;
    }
    !matches!(
        fstype.as_str(),
        "nfs"
            | "nfs4"
            | "nfs3"
            | "smb"
            | "smb2"
            | "smb3"
            | "cifs"
            | "cifs.spnego"
            | "smbfs"
            | "afpfs"
            | "apple_afp"
            | "webdav"
            | "davfs"
            | "9p"
            | "virtiofs"
            | "vmhgfs"
            | "vmhgfs-fuse"
            | "prl_fs"
            | "glusterfs"
            | "lustre"
            | "ceph"
            | "cephfs"
            | "ocfs2"
            | "gfs2"
            | "gfs23"
            | "pvfs2"
            | "ncpfs"
            | "afs"
            | "anfs"
            | "nexfs"
            | "box"
            | "dropbox"
            | "sshfs"
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn a_mount_is_matched_by_component_not_by_prefix() {
        let table = "/dev/sda1 /repo ext4 rw 0 0\n/dev/sdb1 /repos xfs rw 0 0\n";
        assert_eq!(
            mount_fstype(table, Path::new("/repo/src/main.rs")).as_deref(),
            Some("ext4")
        );
        assert_eq!(
            mount_fstype(table, Path::new("/repos/other")).as_deref(),
            Some("xfs")
        );
        // The longest match wins, which is what a bind mount inside a bind mount needs.
        let nested = "/dev/sda1 /repo ext4 rw 0 0\n/dev/sdc1 /repo/work btrfs rw 0 0\n";
        assert_eq!(
            mount_fstype(nested, Path::new("/repo/work/x")).as_deref(),
            Some("btrfs")
        );
        assert_eq!(
            mount_fstype(nested, Path::new("/repo/x")).as_deref(),
            Some("ext4")
        );
    }

    #[test]
    fn mount_points_with_spaces_survive_the_octal_escapes() {
        let table = "/dev/sda1 /home/two\\040words ext4 rw 0 0\n";
        assert_eq!(
            mount_fstype(table, Path::new("/home/two words/repo")).as_deref(),
            Some("ext4")
        );
    }

    #[test]
    fn network_and_fuse_mounts_are_the_ones_notifications_lie_about() {
        for fstype in [
            "nfs",
            "nfs4",
            "cifs",
            "smb3",
            "9p",
            "virtiofs",
            "fuse.fuse-overlayfs",
            "sshfs",
        ] {
            assert!(
                !notifications_work(fstype),
                "{fstype} must not be trusted for notifications"
            );
        }
        for fstype in ["ext4", "xfs", "btrfs", "apfs", "overlay", "tmpfs", "zfs"] {
            assert!(
                notifications_work(fstype),
                "{fstype} is local; notifications should be trusted"
            );
        }
    }

    #[test]
    fn a_probe_overrides_its_request_only_when_it_says_so() {
        let poll = probe(BackendChoice::Poll, Path::new("/"), Duration::from_secs(1));
        assert_eq!(poll.backend.kind, BackendKind::Polling);
        assert!(poll.backend.reason.is_some());
        // `notify` never silently becomes polling: if it is overridden, the reason says it was asked for.
        let notify = probe(
            BackendChoice::Notify,
            Path::new("/"),
            Duration::from_secs(1),
        );
        if NOTIFICATION_COMPILED_IN {
            assert_eq!(notify.backend.kind, BackendKind::Notification);
        } else {
            assert_eq!(notify.backend.kind, BackendKind::Polling);
            assert!(notify.backend.reason.unwrap().contains("--features watch"));
        }
    }

    #[test]
    fn the_poll_source_reports_on_its_cadence_and_sleeps_between() {
        let mut source = PollSource::new(Duration::from_millis(40));
        // The first tick is immediate: whatever happened before the watcher started must still be seen.
        assert!(source.wait(Duration::from_millis(5)).arrived());
        let started = Instant::now();
        assert!(
            !source.wait(Duration::from_millis(5)).arrived(),
            "it must not report twice inside one interval"
        );
        assert!(started.elapsed() < Duration::from_millis(30));
        std::thread::sleep(Duration::from_millis(45));
        assert!(source.wait(Duration::from_millis(5)).arrived());
    }

    #[test]
    fn a_backend_choice_parses_the_spellings_a_human_will_type() {
        assert_eq!(BackendChoice::parse("auto"), Some(BackendChoice::Auto));
        assert_eq!(BackendChoice::parse("POLL"), Some(BackendChoice::Poll));
        assert_eq!(BackendChoice::parse("inotify"), Some(BackendChoice::Notify));
        assert_eq!(BackendChoice::parse("carrier-pigeon"), None);
    }
}
