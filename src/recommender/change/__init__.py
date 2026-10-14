"""The change classifier (`e2.1` / `reco-dsh`): what every changed path in an episode *is*.

One question per changed file, one answer, with receipts:

    from recommender.change import ChangeClassifier, FileChange

    clf = ChangeClassifier.default()
    d = clf.classify(FileChange(path="src/api.pb.go", content=...))
    d.cls          # "generated"
    d.confidence   # 0.94
    d.band         # "confirmed"
    d.reason       # "content marker gen.protoc … ; path rule gen.protobuf agrees"

`e0.9` (`reco-5vz`) answers "what does the **path** say". This package answers "what is
the file, given the path *and* the bytes", which is what the ticket asks for and what
everything downstream of it — `e2.2`'s noise suppression, `e2.3`'s folder delta,
`e2.8`'s non-code surfaces, `e2.9`'s test surface — consumes. The three rules that shape
the design:

1. **The path table is still the first word, and a configured assertion is the last.**
   [docs/concepts/paths.md](../../../docs/concepts/paths.md) §9 refuses content-sniffing
   *inside the rule table*, because a table entry has to stay overrulable by a user's
   `fathom.toml`. That refusal is honoured, not overturned: the table is consulted first
   and untouched here; content evidence is layered on top of it, and it can never beat a
   rule that came from a config layer (§policy). A repository that says
   `generated = ["src/protos/**"]` still gets `generated`, whatever the bytes look like.
2. **Content evidence never guesses.** Path rules have a fallback and it is `other`; the
   classifier has the same rule with a stronger reason. Two claims of similar strength,
   or no claim at all, and the answer is `other` — never `source`, which is the guess that
   makes a noise-share number a lie. See AC 3 in
   [docs/concepts/change-classification.md](../../../docs/concepts/change-classification.md).
3. **Every answer carries a confidence and a sentence.** A label without both is a
   regression waiting to be trusted. `as_dict()` gives the TUI structure (it must not
   parse prose); `explain.py` gives the reviewer prose, and the same text is what the
   diff tree pane renders (ADR-006 §6.2: the tree pane is grouped by taxonomy class).

Modules, and the Rust home ADR-002 §9 gives them (team 5, `fathom-analyze`):

| module | Rust home | may do IO? |
|---|---|---|
| `markers.py`   | `fathom-analyze::change::markers` | no — the content-marker table, versioned |
| `evidence.py`  | `fathom-analyze::change::evidence` | no — bytes → facts, build-config index |
| `policy.py`    | `fathom-analyze::change::policy` | no — weights, precedence, confidence bands |
| `engine.py`    | `fathom-analyze::change` | no — the classifier itself |
| `episode.py`   | `fathom-analyze::change::episode` | no — per-episode aggregation |
| `cache.py`     | `fathom-analyze::change::cache` | no — per-blob keying and encoding |
| `corpus.py`    | `fathom-analyze::change::corpus` (tests) | no — the labelled fixture set |
| `eval.py`      | `fathom-cli::verbs::classify::score` | no — accuracy scoring |
| `explain.py`   | `fathom-cli::verbs::classify` | no — renders a decision |
| `cli.py`       | `fathom-cli::verbs::classify` | **yes** — reads blobs and `fathom.toml` |

`tests/test_change_classifier_contract.py` greps the analysis half for `open(`,
`os.environ` and `subprocess` the way the taxonomy contract suite greps its own.
"""

from __future__ import annotations

from .cache import (CACHE_VERSION, BlobCache, blob_oid, classification_key, input_hash)
from .engine import (CLASSIFIER_VERSION, Change, ChangeClassifier, Classification,
                     default_classifier)
from .episode import Episode, EpisodeClassification, classify_episode
from .evidence import BuildIndex, FileFacts, inspect_bytes, references_for
from .markers import MARKERS_VERSION, Marker, scan
from .policy import (BAND_AMBIGUOUS, BAND_CONFIRMED, BAND_LIKELY, BAND_WEAK, BANDS,
                     Vote, combine)

__all__ = ["BuildIndex", "BANDS", "BAND_AMBIGUOUS", "BAND_CONFIRMED", "BAND_LIKELY", "BAND_WEAK",
           "CACHE_VERSION", "CLASSIFIER_VERSION", "MARKERS_VERSION", "BlobCache",
           "Change", "ChangeClassifier", "Classification", "Decision", "Episode",
           "EpisodeClassification", "FileFacts", "Marker", "Vote",
           "blob_oid", "classification_key", "classify_episode", "combine", "path_weight",
           "default_classifier", "inspect_bytes", "input_hash", "references_for", "scan"]
