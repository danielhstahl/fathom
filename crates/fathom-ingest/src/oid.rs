//! `oid.rs` — an object id that is not a `gix` type (ADR-002 §4.2, §4.3).
//!
//! `fathom-analyze` and `fathom-tui` never name `gix`: the workspace model gives them no such dependency
//! (lint `E02`), and a signature like `fn head() -> gix::ObjectId` would quietly make every crate above
//! the door a git client too. So the door translates. What leaves it is this type — twenty bytes of
//! SHA-1, displayed as forty lowercase hex characters, ordered by bytes, serialised as hex.
//!
//! SHA-1 only, for the same reason the build is SHA-1 only: `gix` is compiled with `sha1` and not `sha256`
//! (ADR-002 §4.2), so a SHA-256 repository fails to open with
//! [`Error::RepositoryUnsupported`](crate::error::Error::RepositoryUnsupported) rather than arriving here
//! as an id of the wrong length. When SHA-256 repositories become common enough to matter, the fix is a
//! `Kind` field on this type and a `sha256` feature on `gix` — not a second id type.

use std::fmt;
use std::str::FromStr;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A git object name: the raw 20-byte SHA-1 digest.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Oid([u8; Oid::LEN]);

impl Oid {
    /// The digest length in bytes.
    pub const LEN: usize = 20;

    /// The zero id. It is not a valid object name; it is what "there is no object here" is stored as,
    /// which is a real state in a repository (a deleted branch points at nothing).
    pub const ZERO: Oid = Oid([0u8; Oid::LEN]);

    /// Wrap a digest.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; Oid::LEN]) -> Self {
        Oid(bytes)
    }

    /// The raw digest.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; Oid::LEN] {
        &self.0
    }

    /// Is this the zero id (no object)?
    #[must_use]
    pub const fn is_zero(&self) -> bool {
        let bytes = &self.0;
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] != 0 {
                return false;
            }
            index += 1;
        }
        true
    }

    /// Lowercase hex, 40 characters.
    #[must_use]
    pub fn hex(&self) -> String {
        let mut out = String::with_capacity(Self::LEN * 2);
        self.write_hex(&mut out);
        out
    }

    /// Lowercase hex appended to `out`.
    ///
    /// The allocation-free form of [`Self::hex`]: a renderer that formats a thousand rows should not
    /// allocate a `String` per row just to slice it again.
    pub fn write_hex(&self, out: &mut String) {
        use std::fmt::Write as _;
        for byte in &self.0 {
            // Writing into a `String` cannot fail; `let _` is the honest spelling of that, where
            // `unwrap` in a library is `clippy::unwrap_used` (ADR-002 §7.5).
            let _ = write!(out, "{byte:02x}");
        }
    }

    /// Does this id start with the given hex prefix (case-insensitive; odd lengths allowed)?
    ///
    /// Abbreviated ids are a display concern, but *matching* on them is a lookup: `--grep`, the TUI's
    /// filter box and `fathom show 3f2a` all need it, and doing it here keeps byte-ordering the only
    /// ordering in play.
    #[must_use]
    pub fn starts_with_hex(&self, prefix: &str) -> bool {
        let bytes = prefix.as_bytes();
        if bytes.len() > Self::LEN * 2 {
            return false;
        }
        for (index, ch) in bytes.iter().enumerate() {
            let nibble = match ch {
                b'0'..=b'9' => ch - b'0',
                b'a'..=b'f' => ch - b'a' + 10,
                b'A'..=b'F' => ch - b'A' + 10,
                _ => return false,
            };
            let byte = self.0[index / 2];
            let actual = if index % 2 == 0 {
                byte >> 4
            } else {
                byte & 0x0f
            };
            if actual != nibble {
                return false;
            }
        }
        true
    }
}

/// Why a string is not an object id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OidParseError {
    /// The string was not 40 hex characters.
    WrongLength {
        /// How many characters were given.
        given: usize,
    },
    /// A character that is not a hex digit, with its 1-based position.
    NotHex {
        /// 1-based character position of the offender.
        position: usize,
    },
}

impl fmt::Display for OidParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OidParseError::WrongLength { given } => {
                write!(f, "expected 40 hex characters, got {given}")
            }
            OidParseError::NotHex { position } => {
                write!(f, "character {position} is not a hex digit")
            }
        }
    }
}

impl std::error::Error for OidParseError {}

impl FromStr for Oid {
    type Err = OidParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = s.as_bytes();
        if bytes.len() != Self::LEN * 2 {
            return Err(OidParseError::WrongLength { given: bytes.len() });
        }
        let mut out = [0u8; Self::LEN];
        // `as_chunks`, not `chunks_exact(2)`: clippy's `chunks_exact_to_as_chunks` (rust 1.99) says a
        // constant chunk size should say so in the type, and the length check above already guarantees
        // there is no remainder to ignore. `as_chunks` is in the pinned MSRV (1.97), so both the floor and
        // stable build this — which is the only reason the lint can be satisfied rather than allowed.
        for (index, pair) in bytes.as_chunks::<2>().0.iter().enumerate() {
            let hi = hex_value(pair[0]).ok_or(OidParseError::NotHex {
                position: index * 2 + 1,
            })?;
            let lo = hex_value(pair[1]).ok_or(OidParseError::NotHex {
                position: index * 2 + 2,
            })?;
            out[index] = hi << 4 | lo;
        }
        Ok(Oid(out))
    }
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

impl fmt::Display for Oid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Oid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Oid({self})")
    }
}

impl Serialize for Oid {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.hex())
    }
}

impl<'de> Deserialize<'de> for Oid {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Oid::from_str(&text).map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::str::FromStr;

    use super::Oid;

    const EMPTY_BLOB: &str = "da39a3ee5e6b4b0d3255bfef95601890afd80709";

    #[test]
    fn round_trips_through_hex() {
        let oid = EMPTY_BLOB.parse::<Oid>().expect("valid");
        assert_eq!(oid.hex(), EMPTY_BLOB);
        assert_eq!(oid.to_string(), EMPTY_BLOB);
        let json = serde_json::to_string(&oid).expect("serialisable");
        assert_eq!(json, format!("\"{EMPTY_BLOB}\""));
        let back: Oid = serde_json::from_str(&json).expect("deserialisable");
        assert_eq!(back, oid);
    }

    #[test]
    fn rejects_the_wrong_shape_rather_than_guessing() {
        assert!(Oid::from_str("deadbeef").is_err());
        assert!(Oid::from_str(&"z".repeat(40)).is_err());
        assert!(Oid::from_str(&"a".repeat(39)).is_err());
    }

    #[test]
    fn hex_prefix_matching_agrees_with_the_text_form() {
        let oid = EMPTY_BLOB.parse::<Oid>().expect("valid");
        for len in [1usize, 2, 3, 7, 40] {
            assert!(
                oid.starts_with_hex(&EMPTY_BLOB[..len]),
                "prefix of length {len} must match"
            );
        }
        assert!(oid.starts_with_hex("DA39")); // git's own prefixes are case-insensitive
        assert!(!oid.starts_with_hex("da39b"));
        assert!(!oid.starts_with_hex(&"f".repeat(41)));
        assert!(!oid.starts_with_hex("nope"));
    }

    #[test]
    fn zero_is_the_absence_of_an_object() {
        assert!(Oid::ZERO.is_zero());
        assert!(!EMPTY_BLOB.parse::<Oid>().expect("valid").is_zero());
    }
}
