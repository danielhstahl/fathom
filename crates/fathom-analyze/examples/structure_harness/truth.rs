//! The corpus annotation format, and the loader that refuses to guess about it.
//!
//! Shape inherited from spike `s1` (`crates/fathom-analyze/spike/s1/corpus/<language>/truth.json`),
//! with two additions this ticket needs and the spike did not have a shipped extractor for:
//!
//! * `bases` — the declared supertype relation, `{"n": <of>, "b": <base>, "k": <kind>}`, where `kind`
//!   is `base`, `trait`, `interface` or `metaclass`. This is what `supertypes` in the report scores.
//! * `synthetic` — the scopes that cannot be enumerated, `{"scope": <dotted, "" for file>, "t":
//!   [<reasons>]}`. The reasons are the same words the extractor uses, so a disagreement between the
//!   two is visible as a mismatch rather than hidden.
//!
//! Everything is strict: an unknown key is an error, because the failure mode of a truth file nobody
//! validates is a 100 % score built on a key nobody spelled right.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Truth {
    pub language: String,
    #[serde(default)]
    pub root: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    pub files: BTreeMap<String, FileTruth>,
    /// Why the `bases` rows are what they are, in the annotator's words. Provenance, not data: a
    /// relation this strict needs its vocabulary written down next to the numbers.
    #[serde(default)]
    pub bases_notes: Option<String>,
    #[serde(default)]
    pub synthetic_notes: Option<String>,
    /// The `who depends on this` question the wildcard check asks, spelled out rather than derived,
    /// so the report says which question it answered. Absent means: the first wildcard import in the
    /// corpus, asked about the file it points at.
    #[serde(default)]
    pub wildcard_question: Option<Question>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Question {
    pub file: String,
    #[serde(default)]
    pub symbol: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileTruth {
    #[serde(default)]
    pub definitions: Vec<Definition>,
    #[serde(default)]
    pub exports: Vec<String>,
    #[serde(default)]
    pub imports: Vec<Import>,
    /// A dependency on something outside the corpus. Not scored — the extractor's edge for it is
    /// `unresolved` by construction, and scoring that would require annotating every stdlib name in
    /// existence. Counted, and visible in the report, so a file whose externals vanished is noticed.
    #[serde(default)]
    pub external: Vec<String>,
    #[serde(default)]
    pub bases: Vec<Base>,
    #[serde(default)]
    pub synthetic: Vec<SyntheticScope>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub k: String,
    pub n: String,
    #[serde(default)]
    pub t: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Import {
    pub f: String,
    pub s: String,
    #[serde(default)]
    pub t: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Base {
    pub n: String,
    pub b: String,
    #[serde(default)]
    pub k: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyntheticScope {
    pub scope: String,
    #[serde(default)]
    pub t: Vec<String>,
}

impl Truth {
    pub fn load(dir: &Path) -> Result<Truth, String> {
        let path = dir.join("truth.json");
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let truth: Truth =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if truth.language
            != dir
                .file_name()
                .map(|n| n.to_string_lossy())
                .unwrap_or_default()
        {
            return Err(format!(
                "{}: says language {:?}, the directory says {:?}",
                path.display(),
                truth.language,
                dir.file_name().map(|n| n.to_string_lossy().into_owned())
            ));
        }
        // A truth file that names a file the corpus does not contain is a lie, and it is the most
        // dangerous kind: it scores as a miss the extractor cannot fix, forever.
        let mut missing = Vec::new();
        for name in truth.files.keys() {
            if !dir.join(name).exists() {
                missing.push(name.clone());
            }
        }
        if !missing.is_empty() {
            return Err(format!(
                "{} names files the corpus does not have: {}",
                path.display(),
                missing.join(", ")
            ));
        }
        Ok(truth)
    }

    /// `(definitions, exports, imports, bases, synthetic)` row counts, for the report header.
    pub fn rows(&self) -> (usize, usize, usize, usize, usize) {
        let mut counts = [0usize; 5];
        for file in self.files.values() {
            counts[0] += file.definitions.len();
            counts[1] += file.exports.len();
            counts[2] += file.imports.len();
            counts[3] += file.bases.len();
            counts[4] += file.synthetic.len();
        }
        (counts[0], counts[1], counts[2], counts[3], counts[4])
    }

    pub fn wildcard_question(&self) -> Option<(String, Option<String>)> {
        if let Some(question) = &self.wildcard_question {
            return Some((question.file.clone(), question.symbol.clone()));
        }
        self.files.iter().find_map(|(path, file)| {
            file.imports
                .iter()
                .find(|import| import.s == "*")
                .map(|import| (import.f.clone(), None))
                .or_else(|| Some((path.clone(), None)))
        })
    }
}
