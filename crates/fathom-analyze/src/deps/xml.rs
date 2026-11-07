//! The XML subset `pom.xml` and `*.csproj` need — a reader, not a parser (e2.5 / reco-j83).
//!
//! Maven's POM and MSBuild's project file are XML with a namespace, a doctype and the occasional comment,
//! and everything fathom needs from them is *element names, attributes and short text*: `<dependency>`
//! rows, `PackageReference` items, `<module>` lists. That is a small reader, and small is the point:
//!
//! * It never resolves entities, never follows a `<!DOCTYPE` to a URL and never fetches an external
//!   DTD. A dependency manifest that could make a reader open a socket is a supply-chain story, and fathom
//!   does not do that (ADR-001 §8.2).
//! * Namespace prefixes are kept verbatim and matched on the local name, because the same `csproj` element
//!   appears as `PackageReference` and as `x:PackageReference` depending on the SDK's `ToolsVersion`, and
//!   the difference is noise a reviewer should never see.
//! * Anything outside the subset — a mismatched close tag, an unterminated attribute, a tag closed by
//!   end-of-file — is an error with a line number, which becomes a typed `unparsed` rather than a partial
//!   element tree that looks complete.
//!
//! Truncation is therefore *not* detected here: a cut-off POM is an unclosed tag, and the caller knows
//! whether it read the whole blob, which is where [`crate::deps::text::toml_reason`]-style classification
//! would be the wrong tool.

/// The deepest element nesting [`parse`] will build.
///
/// [`parse`] recurses once per open element, so nesting is the one axis of a hostile XML file that the
/// parser itself can lose a stack over: 5 000 `<a>` tags are 5 000 frames, and the loss is a `SIGSEGV`
/// rather than a reportable error. No build file a human maintains nests anywhere near this; the depth is
/// here so that "hostile" has an answer (`e7.8`, threat model F-5).
const MAX_DEPTH: usize = 100;

/// One attribute.
pub type Attr = (String, String);

/// An XML element with its children.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    /// The tag name exactly as written, prefix included.
    pub name: String,
    /// Attributes, in document order.
    pub attrs: Vec<Attr>,
    /// Children: elements and text, in document order.
    pub children: Vec<Element>,
    /// Text directly inside this element, with surrounding whitespace collapsed.
    pub text: String,
}

impl Element {
    /// The tag name without its namespace prefix.
    #[must_use]
    pub fn local(&self) -> &str {
        self.name.rsplit(':').next().unwrap_or(&self.name)
    }

    /// Whether this element's local name is `name`.
    #[must_use]
    pub fn is(&self, name: &str) -> bool {
        self.local() == name
    }

    /// An attribute value.
    #[must_use]
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// The first child whose local name matches.
    #[must_use]
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|child| child.is(name))
    }

    /// Every child whose local name matches.
    pub fn all(&self, name: &str) -> impl Iterator<Item = &Element> {
        self.children.iter().filter(move |child| child.is(name))
    }

    /// The first child's text, trimmed.
    #[must_use]
    pub fn child_text(&self, name: &str) -> Option<&str> {
        self.child(name).map(Element::content)
    }

    /// This element's own text content.
    #[must_use]
    pub fn content(&self) -> &str {
        self.text.trim()
    }
}

/// Parse one XML document, returning its root element.
///
/// The error is a sentence with a line number, exactly as [`super::yaml::parse`]'s is.
pub fn parse(text: &str) -> Result<Element, String> {
    let mut cursor = Scanner {
        text,
        at: 0,
        line: 1,
    };
    cursor.skip_prolog_and_text()?;
    let Some((name, attrs, closed)) = cursor.open_tag()? else {
        return Err("no element found".to_owned());
    };
    let mut root = Element {
        name,
        attrs,
        children: Vec::new(),
        text: String::new(),
    };
    if !closed {
        cursor.children(&mut root, 1)?;
    }
    Ok(root)
}

struct Scanner<'a> {
    text: &'a str,
    at: usize,
    line: usize,
}

impl<'a> Scanner<'a> {
    fn byte(&self) -> Option<u8> {
        self.text.as_bytes().get(self.at).copied()
    }

    fn rest(&self) -> &'a str {
        &self.text[self.at..]
    }

    fn starts(&self, needle: &str) -> bool {
        self.rest().starts_with(needle)
    }

    /// Move forward `bytes` bytes, counting the newlines crossed.
    ///
    /// Every index this scanner keeps is a *byte* index, and every caller counts bytes: the distance a
    /// `find` returned, the width of a `<` or a `</`, and — in the text loop of `children` below — a step
    /// of one byte at a time across the text between two tags. None of those is a character boundary in a
    /// file that holds a character wider than one byte, which is every `pom.xml` with an accented author
    /// name and every manifest whose bytes were not valid UTF-8 in the first place (the caller hands this
    /// reader a lossy `String`, so the bad bytes arrived as three-byte U+FFFD). Slicing at such an `end` is
    /// a panic, and a panic in a reader is a review tool that died on somebody's clone (ADR-004 §7.7). So
    /// `end` is snapped up to the next boundary instead: an ASCII needle — `>`, `"`, `</` — never matches
    /// *inside* a character, so the searchers land where they always did, and the byte-stepping text loop
    /// now skips a whole character rather than walking into one.
    fn advance(&mut self, bytes: usize) {
        let mut end = self.at.saturating_add(bytes).min(self.text.len());
        while !self.text.is_char_boundary(end) {
            end += 1;
        }
        self.line += self.text[self.at..end].matches('\n').count();
        self.at = end;
    }

    fn skip_to(&mut self, needle: &str) -> Result<usize, String> {
        let offset = self
            .rest()
            .find(needle)
            .ok_or_else(|| format!("line {}: `{needle}` never closed", self.line))?;
        let start = self.at;
        self.advance(offset + needle.len());
        Ok(start + offset)
    }

    /// Consume processing instructions, comments, doctypes and inter-tag text.
    fn skip_prolog_and_text(&mut self) -> Result<(), String> {
        loop {
            self.skip_whitespace();
            if self.starts("<!--") {
                self.skip_to("-->")?;
                continue;
            }
            if self.starts("<?") {
                self.skip_to("?>")?;
                continue;
            }
            if self.starts("<!") {
                self.skip_to(">")?;
                continue;
            }
            return Ok(());
        }
    }

    fn skip_whitespace(&mut self) {
        while let Some(byte) = self.byte() {
            if byte.is_ascii_whitespace() {
                self.advance(1);
            } else {
                break;
            }
        }
    }

    /// Read `<name …>` or `<name … />`; `None` at end of input.
    fn open_tag(&mut self) -> Result<Option<(String, Vec<Attr>, bool)>, String> {
        if self.byte() != Some(b'<') {
            return Ok(None);
        }
        self.advance(1);
        let name = self.read_name()?;
        let mut attrs = Vec::new();
        loop {
            self.skip_whitespace();
            match self.byte() {
                Some(b'>') => {
                    self.advance(1);
                    return Ok(Some((name, attrs, false)));
                }
                Some(b'/') if self.starts("/>") => {
                    self.advance(2);
                    return Ok(Some((name, attrs, true)));
                }
                None => {
                    return Err(format!("line {}: `<{name}>` was never closed", self.line));
                }
                _ => {
                    let key = self.read_name()?;
                    self.skip_whitespace();
                    if self.byte() != Some(b'=') {
                        return Err(format!(
                            "line {}: attribute `{key}` has no value",
                            self.line
                        ));
                    }
                    self.advance(1);
                    self.skip_whitespace();
                    let value = self.read_quoted()?;
                    attrs.push((key, value));
                }
            }
        }
    }

    fn read_name(&mut self) -> Result<String, String> {
        let start = self.at;
        while let Some(byte) = self.byte() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':') {
                self.advance(1);
            } else {
                break;
            }
        }
        if start == self.at {
            return Err(format!("line {}: expected a name", self.line));
        }
        Ok(self.text[start..self.at].to_owned())
    }

    fn read_quoted(&mut self) -> Result<String, String> {
        let Some(quote) = self.byte() else {
            return Err(format!("line {}: expected a quoted value", self.line));
        };
        if quote != b'"' && quote != b'\'' {
            return Err(format!("line {}: expected a quoted value", self.line));
        }
        self.advance(1);
        let end = self
            .rest()
            .find(if quote == b'"' { "\"" } else { "'" })
            .ok_or_else(|| format!("line {}: unterminated attribute value", self.line))?;
        let value = self.text[self.at..self.at + end].to_owned();
        self.advance(end + 1);
        Ok(unescape(&value))
    }

    /// Fill `element`'s children until its close tag.
    fn children(&mut self, element: &mut Element, depth: usize) -> Result<(), String> {
        let openers = element.name.clone();
        let mut text = String::new();
        loop {
            match self.byte() {
                None => {
                    return Err(format!("line {}: `</{openers}>` never appeared", self.line));
                }
                Some(b'<') if self.starts("</") => {
                    self.advance(2);
                    let name = self.read_name()?;
                    self.skip_whitespace();
                    if self.byte() != Some(b'>') {
                        return Err(format!("line {}: malformed close tag", self.line));
                    }
                    self.advance(1);
                    if name != openers {
                        return Err(format!(
                            "line {}: `</{name}>` closes `<{openers}>`",
                            self.line
                        ));
                    }
                    element.text.push_str(&collapse(&text));
                    return Ok(());
                }
                Some(b'<') if self.starts("<!--") || self.starts("<?") || self.starts("<!") => {
                    self.skip_prolog_and_text()?;
                }
                Some(b'<') => {
                    let (name, attrs, closed) = self
                        .open_tag()?
                        .ok_or_else(|| format!("line {}: unexpected end of input", self.line))?;
                    let mut child = Element {
                        name,
                        attrs,
                        children: Vec::new(),
                        text: String::new(),
                    };
                    if !closed {
                        if depth >= MAX_DEPTH {
                            return Err(format!(
                                "line {}: nested more than {MAX_DEPTH} elements deep, which is not a \
                                 build file",
                                self.line
                            ));
                        }
                        self.children(&mut child, depth + 1)?;
                    }
                    element.children.push(child);
                }
                Some(_) => {
                    let start = self.at;
                    while self.byte().is_some_and(|byte| byte != b'<') {
                        self.advance(1);
                    }
                    text.push_str(&unescape(&self.text[start..self.at]));
                }
            }
        }
    }
}

/// Collapse whitespace in an element's text.
fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The five XML built-ins, plus `&#NN;`. No entities at all is the safe default; these are the five that
/// appear in a real `<description>` or `<summary>`.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find('&') {
        out.push_str(&rest[..index]);
        let tail = &rest[index + 1..];
        let Some(end) = tail.find(';') else {
            out.push('&');
            rest = tail;
            continue;
        };
        let entity = &tail[..end];
        match entity {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            "nbsp" => out.push('\u{a0}'),
            other => {
                if let Some(hex) = other.strip_prefix("#x").or(other.strip_prefix("#X")) {
                    if let Some(ch) = u32::from_str_radix(hex, 16).ok().and_then(char::from_u32) {
                        out.push(ch);
                        rest = &tail[end + 1..];
                        continue;
                    }
                } else if let Some(decimal) = other.strip_prefix('#')
                    && let Some(ch) = decimal.parse::<u32>().ok().and_then(char::from_u32)
                {
                    out.push(ch);
                    rest = &tail[end + 1..];
                    continue;
                }
                // Anything else stays exactly as written: an unresolved entity in a version string is a
                // fact about the file, not something to invent.
                out.push('&');
                out.push_str(other);
                out.push(';');
            }
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}
