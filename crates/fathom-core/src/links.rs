//! `fathom_core::links` — OSC-8 hyperlinks, emitted only where the terminal asked for them (`e5.8`).
//!
//! A terminal hyperlink is an escape sequence, and an escape sequence is a claim about the thing it is
//! written into. Written into a terminal that speaks OSC-8, `src/store.rs` becomes a path you can
//! cmd-click; written into tmux 3.2 or a `dumb` terminal or a pipe, it is at best invisible and at worst
//! a strip of `8;;file:///…` litter across the line the reviewer was trying to read. So the rule this
//! module enforces is the acceptance criterion it was written for: **emit when the terminal advertises
//! support, print the plain text otherwise, and say which happened.** Silent is the one forbidden
//! outcome — a reviewer who cannot tell whether a path was clickable is going to type it out either way.
//!
//! Nothing here reads the environment (ADR-002 §3, lints `W1`/`W2`): [`Hyperlinks::from_env`] is handed
//! a lookup, the same shape [`crate::handoff::Env`] uses, so "does this terminal advertise support" is a
//! table of fixtures rather than a request for a rack of real terminals.
//!
//! ## What counts as advertising
//!
//! There is no terminfo capability for OSC-8, so a terminal advertises by *naming itself* — the variables
//! in [`ADVERTISERS`] and [`NAMED`] are what terminals set when they start. A `TERM` of
//! `xterm-256color` is not an advertisement: it says how to position a cursor, not what escape
//! sequences the emulator understands, which is why an unnamed `xterm` gets no links and a message about
//! the one variable that changes the answer.
//!
//! A multiplexer is a second veto. tmux passes hyperlinks through from 3.3; GNU screen does not pass
//! them at all. An advertisement from the terminal on the far side of either one is only half the route,
//! so both are checked before anything is emitted.

use std::fmt;

/// The variable a reviewer sets to overrule the probe. `auto` (or anything unrecognised) is "ask the
/// terminal"; `always` and `never` are the two answers a person can give better than any probe.
pub const KNOB: &str = "FATHOM_HYPERLINKS";

/// A terminal that names itself, and the version its OSC-8 support starts at, if it ever did.
///
/// `(TERM_PROGRAM value, name to print, first version that speaks OSC-8)`; an empty version means the
/// name is the whole answer.
pub const NAMED: &[(&str, &str, &str)] = &[
    ("wezterm", "WezTerm", ""),
    ("ghostty", "ghostty", ""),
    ("contour", "contour", ""),
    ("hyper", "Hyper", ""),
    // VS Code's integrated terminal renders OSC-8 from 1.57 (May 2021); an older one gets the text.
    ("vscode", "VS Code's terminal", "1.57"),
    ("code", "VS Code's terminal", "1.57"),
    // iTerm2 speaks them from 3.0 (2016). The 2.x line is the only one that does not.
    ("iterm.app", "iTerm2", "3.0"),
    ("org.kde.konsole", "Konsole", "22.12"),
];

/// A variable whose mere presence names the terminal, with the name to print.
///
/// These are the emulators that do not set `TERM_PROGRAM`, or that set a marker of their own alongside
/// it. Every one of them has shipped OSC-8 for years; the version, where there is one, is printed for the
/// message rather than checked.
pub const ADVERTISERS: &[(&str, &str)] = &[
    ("KITTY_WINDOW_ID", "kitty"),
    ("ALACRITTY_WINDOW_ID", "Alacritty"),
    ("WEZTERM_PANE", "WezTerm"),
    ("GHOSTTY_RESOURCES_DIR", "ghostty"),
    ("WT_SESSION", "Windows Terminal"),
];

/// The minimum `VTE_VERSION` that understands OSC-8 — 0.36, from 2014.
pub const VTE_MIN: u32 = 3600;
/// The minimum tmux that forwards hyperlinks: 3.3, from 2021.
pub const TMUX_MIN: (u32, u32) = (3, 3);

/// What this stream was told about hyperlinks, and the sentence that explains it.
///
/// The verdict is a value, not a side effect: a caller prints [`Hyperlinks::why`] next to the first link
/// (or next to none) and the reviewer knows which world they are in.
#[derive(Clone, PartialEq, Eq)]
pub struct Hyperlinks {
    emit: bool,
    why: String,
    /// Whether a person said so (`$FATHOM_HYPERLINKS`) rather than the probe inferring it. A stream
    /// that is not a terminal declines the probe's answer; it does not overrule a person's.
    forced: bool,
    /// Whether the terminal advertised OSC-8 at all. The difference between "this terminal does not do
    /// that" and "that got eaten on the way here" is worth a sentence, and only the second one is news.
    advertised: bool,
}

impl fmt::Debug for Hyperlinks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Hyperlinks")
            .field("emit", &self.emit)
            .field("forced", &self.forced)
            .field("why", &self.why)
            .finish()
    }
}

impl Hyperlinks {
    /// Ask the environment, through `look` — which is a closure rather than a map so this crate never
    /// reads a variable itself (ADR-002 §3). Assumes the bytes are going where a terminal is reading
    /// them; [`Hyperlinks::from_stream`] is the version that knows when they are not.
    #[must_use]
    pub fn from_env(look: impl FnMut(&str) -> Option<String>) -> Self {
        Self::from_stream(true, look)
    }

    /// The same question, plus the one only the caller can answer: is the stream these strings are
    /// about to be written to a terminal?
    ///
    /// The environment describes the terminal a session has; it does not say that *this* output is
    /// reaching it. `fathom show | fzf`, `fathom show > file`, and a CI runner's log all sit inside a
    /// terminal while their stdout is not one, and in each of those an introducer becomes somebody
    /// else's payload. So a stream that is not a terminal gets plain text whatever it advertises —
    /// except under the explicit knob, because a reviewer who types `FATHOM_HYPERLINKS=always` has
    /// said they know where the bytes are going.
    #[must_use]
    pub fn from_stream(tty: bool, mut look: impl FnMut(&str) -> Option<String>) -> Self {
        let mut reader = move |key: &str| look(key).filter(|value| !value.trim().is_empty());
        if let Some(knob) = reader(KNOB) {
            match knob.trim().to_ascii_lowercase().as_str() {
                "always" | "on" | "yes" | "1" => {
                    return Hyperlinks {
                        emit: true,
                        why: format!(
                            "`{KNOB}={knob}`, so links are emitted whatever the terminal said"
                        ),
                        forced: true,
                        advertised: false,
                    };
                }
                "never" | "off" | "no" | "0" => {
                    return Hyperlinks {
                        emit: false,
                        why: format!("`{KNOB}={knob}` says not to, whatever the terminal is"),
                        forced: true,
                        advertised: false,
                    };
                }
                "auto" => {}
                other => {
                    // An unrecognised value is reported rather than silently read as `auto`: a reviewer
                    // who typed `FATHOM_HYPERLINKS=yes-please` deserves to learn the vocabulary.
                    let probe = Self::probe(&mut reader);
                    let mut why = format!("`{KNOB}={other}` is not `always`, `never` or `auto`");
                    if !tty {
                        why.push_str(" · and this is not a terminal");
                    }
                    why.push(' ');
                    why.push_str(&probe.why);
                    return Hyperlinks {
                        emit: probe.emit && tty,
                        why,
                        forced: false,
                        advertised: probe.advertised,
                    };
                }
            }
        }
        if !tty {
            // Both facts in one sentence. A pipe inside a terminal that advertises is exactly the case
            // where the terminal's answer and the stream's answer disagree, and the stream's is the one
            // that decides — but the reviewer still wants to know the terminal was not the problem.
            let advertised = advertise(&mut reader).is_some();
            return Hyperlinks {
                emit: false,
                forced: false,
                advertised,
                why: if advertised {
                    format!(
                        "the terminal advertises OSC-8, but this is not a terminal, so nothing is \
                         linked \
                         — the sequences would land in a file or a pipe where nothing reads them \
                         (ADR-006 §6.7). `{KNOB}=always` overrides this"
                    )
                } else {
                    format!(
                        "this is not a terminal and nothing advertised OSC-8, so nothing is linked \
                         (ADR-006 §6.7). `{KNOB}=always` emits into a pipe as well, for the pipe a \
                         person is reading"
                    )
                }
                .to_owned(),
            };
        }
        Self::probe(&mut reader)
    }

    /// The advertisement question, without the override on top of it.
    fn probe(look: &mut impl FnMut(&str) -> Option<String>) -> Self {
        let advertised = advertise(look);
        let route = middleman(look);
        match (advertised, route) {
            (Some(name), None) => Hyperlinks {
                emit: true,
                why: format!("{name} advertises OSC-8, so paths and commits are links"),
                forced: false,
                advertised: true,
            },
            (Some(name), Some(Route::Passes(via))) => Hyperlinks {
                emit: true,
                why: format!("{name} advertises OSC-8 and {via}, so paths and commits are links"),
                forced: false,
                advertised: true,
            },
            (Some(name), Some(Route::Drops(via))) => Hyperlinks {
                emit: false,
                forced: false,
                advertised: true,
                why: format!(
                    "{name} advertises OSC-8, but {via} — set `{KNOB}=always` if you know the route \\
                     carries them"
                ),
            },
            (None, Some(Route::Drops(via))) => Hyperlinks {
                emit: false,
                forced: false,
                advertised: false,
                why: format!(
                    "nothing advertised OSC-8 and {via}; `{KNOB}=always` emits them regardless"
                ),
            },
            (None, Some(Route::Passes(_))) | (None, None) => Hyperlinks {
                emit: false,
                forced: false,
                advertised: false,
                why: format!(
                    "nothing advertised OSC-8 (no `TERM_PROGRAM`, `VTE_VERSION`, `KITTY_WINDOW_ID`, \
                     `WT_SESSION` or `ALACRITTY_WINDOW_ID` was set) — `{KNOB}=always` emits links \
                     anyway, and a terminal that does not speak them prints the text untouched"
                ),
            },
        }
    }

    /// Links on, whoever asked.
    #[must_use]
    pub fn always(why: impl Into<String>) -> Self {
        Hyperlinks {
            emit: true,
            why: why.into(),
            forced: true,
            advertised: true,
        }
    }

    /// Links off, with the reason — a pipe, a `dumb` terminal, a caller that never got an answer.
    #[must_use]
    pub fn never(why: impl Into<String>) -> Self {
        Hyperlinks {
            emit: false,
            why: why.into(),
            forced: true,
            advertised: false,
        }
    }

    /// Emit, or not.
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.emit
    }

    /// Did the terminal advertise OSC-8, whether or not the route to it is there?
    #[must_use]
    pub fn advertised(&self) -> bool {
        self.advertised
    }

    /// Did a person say so, through [`KNOB`]?
    ///
    /// The one caller that cares is the one about to write into something that is not a terminal: a
    /// probe's "this looks like a pipe" is a guess that should yield to a pipe, and a person's
    /// `FATHOM_HYPERLINKS=always > notes.txt` is an instruction that should not.
    #[must_use]
    pub fn forced(&self) -> bool {
        self.forced
    }

    /// The one-line answer to "why are these not clickable".
    #[must_use]
    pub fn why(&self) -> &str {
        &self.why
    }

    /// `text` as a link to `uri` when this stream was told to link, and `text` alone when it was not.
    ///
    /// The text is never replaced by the target: a link that fails to render still reads exactly like the
    /// unlinked line, which is what makes this safe to wrap around every path a verb prints rather than
    /// only the ones a caller feels confident about. An empty `uri` is not a link and never was.
    #[must_use]
    pub fn wrap(&self, uri: &str, text: &str) -> String {
        if !self.emit || uri.is_empty() {
            return text.to_owned();
        }
        osc8(uri, text)
    }
}

/// What sits between this process and the terminal that advertised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// Something is in the middle and it forwards OSC-8.
    Passes(String),
    /// Something is in the middle and it does not — the sentence says which and why.
    Drops(String),
}

/// The middleman between here and the terminal that advertised, if there is one.
///
/// tmux forwards hyperlinks from 3.3 and GNU screen never has, so an advertisement on the far side of
/// either is not a route. tmux does not always tell us which version we are in, and an unknown version
/// is reported as unknown rather than guessed at — the same rule `fathom doctor` applies to every
/// fact it cannot read (ADR-006 §0.1).
#[must_use]
pub fn middleman(look: &mut impl FnMut(&str) -> Option<String>) -> Option<Route> {
    if look("STY").is_some() {
        return Some(Route::Drops(
            "GNU screen (STY) does not pass OSC-8 through".to_owned(),
        ));
    }
    look("TMUX")?;
    Some(match look("TMUX_VERSION") {
        Some(version) => {
            let (major, minor) = pair(&version);
            if (major, minor) >= TMUX_MIN {
                Route::Passes(format!("tmux {version} forwards them"))
            } else {
                Route::Drops(format!(
                    "tmux {version} is older than {}.{} and drops OSC-8",
                    TMUX_MIN.0, TMUX_MIN.1
                ))
            }
        }
        None => Route::Drops(
            "tmux is between the two of you and did not say its version (3.3 is the first that \
             forwards OSC-8)"
                .to_owned(),
        ),
    })
}
/// Decide what this terminal advertised, through `look`.
///
/// Returns the terminal's name (with its version when a version was answered) — the string a message
/// prints. `None` means nothing in the environment claims OSC-8, which is a fact about the terminal and
/// not a failure of the probe.
#[must_use]
pub fn advertise(look: &mut impl FnMut(&str) -> Option<String>) -> Option<String> {
    // `VTE_VERSION` is a hex-ish integer (`0x3600` printed as `3600`), and it is the one advertisement
    // that comes with a number to compare rather than a name to trust.
    if let Some(vte) = look("VTE_VERSION") {
        let digits: String = vte.chars().filter(char::is_ascii_digit).collect();
        if let Ok(number) = digits.parse::<u32>() {
            if number >= VTE_MIN {
                return Some(format!("VTE {vte}"));
            }
            return None;
        }
    }
    for (marker, name) in ADVERTISERS {
        if look(marker).is_some() {
            return Some((*name).to_owned());
        }
    }
    let program = look("TERM_PROGRAM")?.trim().to_ascii_lowercase();
    if program.is_empty() {
        return None;
    }
    let (_, name, since) = NAMED
        .iter()
        .find(|(value, _name, _since)| *value == program.as_str())?;
    if since.is_empty() {
        return Some((*name).to_owned());
    }
    // The version is `TERM_PROGRAM_VERSION` everywhere except iTerm2, which answers in
    // `LC_TERMINAL_VERSION` and spells the major the same way.
    let version = ["TERM_PROGRAM_VERSION", "LC_TERMINAL_VERSION"]
        .iter()
        .find_map(|key| look(key))
        .filter(|value| !value.trim().is_empty());
    match version {
        None => Some(format!("{name} (version not answered, assumed ≥ {since})")),
        Some(version) => {
            if at_least(&version, since) {
                Some(format!("{name} {version}"))
            } else {
                None
            }
        }
    }
}

/// The escape itself: `ESC ] 8 ; params ; URI ST`, the text, then an empty link to close it.
/// `ST` (the two-byte string terminator) rather than `BEL`, because every emulator that implements
/// OSC-8 accepts `ST` and not every one accepts `BEL` after a URI.
#[must_use]
pub fn osc8(uri: &str, text: &str) -> String {
    format!("\u{1b}]8;;{uri}\u{1b}\\{text}\u{1b}]8;;\u{1b}\\")
}

/// Strip OSC-8 sequences out of `text`.
///
/// For a test that wants to assert on the visible text of a line that may or may not be linked, and for
/// anything that has to count the columns a string will occupy.
#[must_use]
pub fn strip(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("\u{1b}]8;") {
        out.push_str(&rest[..start]);
        match rest[start..].find("\u{1b}\\") {
            Some(end) => rest = &rest[start + end + 2..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A `file://` URL for an absolute path, optionally pointing at a line.
///
/// A relative path gets `None`: a link whose target depends on somebody's current directory is a link
/// that breaks the moment the terminal's cwd does, so the caller resolves first and links after.
/// Percent-encoding covers the characters a real repository path actually contains — spaces, `#`, `%`,
/// CJK — which is most of what a naive `format!` would corrupt.
#[must_use]
pub fn file_url(path: &str, line: Option<u32>) -> Option<String> {
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    let drive = looks_like_drive(path);
    if !drive && !path.starts_with('/') {
        return None;
    }
    let encoded = encode_path(path);
    // A unix absolute path already carries the slash that separates the (empty) authority from the
    // path, so `file://` + `/src/x.rs` is `file:///src/x.rs`. A Windows path needs the third slash
    // added, not stripped: `file:///C:/src/x.rs` is the form that resolves.
    let body = if drive {
        format!("file:///{encoded}")
    } else {
        format!("file://{encoded}")
    };
    match line {
        Some(line) if line > 0 => Some(format!("{body}#L{line}")),
        _ => Some(body),
    }
}

/// Is `path` shaped like a Windows absolute path (`C:\` or `C:/`)?
fn looks_like_drive(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/'
}

/// Percent-encode one path, leaving the characters a path is allowed to contain unescaped.
fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => {
                out.push(char::from(byte));
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// The web page for one commit, from a git remote's URL.
///
/// `git@github.com:org/repo.git`, `https://host/org/repo.git`, `ssh://git@host/org/repo` and
/// `file:///srv/git/repo` all land on the shape each forge serves — GitHub and Gitea at `/commit/`,
/// GitLab at `/-/commit/`, Bitbucket at `/commits/`. A remote that names no host (`file://`, a bare
/// local path, an unusual `remote.url`) gets `None`, which is the honest answer and the common one in a
/// CI checkout with no origin: the reviewer's own host may serve commits at a path this table has never
/// seen, and a guessed URL that 404s is worse than no URL.
#[must_use]
pub fn commit_url(remote: &str, oid: &str) -> Option<String> {
    let remote = remote.trim().trim_end_matches('/');
    if remote.is_empty() || oid.is_empty() {
        return None;
    }
    let after_scheme = remote.split_once("://").map(|(_scheme, rest)| rest);
    // Two shapes reach here. `scheme://[user[:pass]@]host[:port]/path` is a URL, and
    // `git@host:org/repo.git` is scp syntax, where the colon after the host is a path separator.
    let (host, path) = match after_scheme {
        Some(rest) => {
            let split = rest.find('/').unwrap_or(rest.len());
            let (authority, path) = (&rest[..split], &rest[split..]);
            (host_of(authority)?, path.to_owned())
        }
        None => {
            if remote.starts_with('/') || remote.starts_with('.') {
                return None;
            }
            let (head, path) = remote.split_once(':')?;
            (host_of(head)?, path.to_owned())
        }
    };
    let path = path.trim_matches('/').trim_end_matches(".git");
    if path.is_empty() || host.is_empty() {
        return None;
    }
    // A local or private path remote (`file:///srv/git/repo`, `/srv/git/repo`, a bare directory) has no
    // web UI to link to; declining is the honest answer and the common one in a CI checkout with no
    // origin. So is a host with no dot and no port — that is a name from /etc/hosts at best.
    if host == "file" || (!host.contains('.') && host != "localhost") {
        return None;
    }
    let lower = host.to_ascii_lowercase();
    let shape = if lower.contains("gitlab") {
        "/-/commit/"
    } else if lower.contains("bitbucket") {
        "/commits/"
    } else {
        "/commit/"
    };
    Some(format!("https://{host}/{path}{shape}{oid}"))
}

/// The host of an authority, with any `user[:pass]@` and `:port` removed.
///
/// The credential half is dropped rather than rewritten: a remote that carries a token in its URL is
/// not about to have that token copied into a URL a terminal may log, and a link with nothing in it is
/// worse than no link.
fn host_of(authority: &str) -> Option<String> {
    let without_credentials = authority.rsplit('@').next()?.trim();
    if without_credentials.is_empty() {
        return None;
    }
    let host = match without_credentials.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|ch| ch.is_ascii_digit()) => host,
        _ => without_credentials,
    };
    Some(host.to_owned())
}

/// Compare a dotted version (`3.3a`, `1.57`, `22.12`) against a dotted floor, component by component.
fn at_least(version: &str, floor: &str) -> bool {
    let (a, b) = (pair(version), pair(floor));
    (a.0, a.1) >= (b.0, b.1)
}

/// The first two dotted numbers of a version, with a missing minor read as 0.
fn pair(version: &str) -> (u32, u32) {
    let mut parts = version.split(['.', '-']).map(|part| {
        let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
        digits.parse::<u32>().unwrap_or(0)
    });
    (parts.next().unwrap_or(0), parts.next().unwrap_or(0))
}
