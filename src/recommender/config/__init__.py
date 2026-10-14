"""The layered configuration system (`e0.8` / `reco-dq3`): types, discovery, layering,
explanation, and the JSON Schema — as an executable contract.

This package is the Python form of the split ADR-002 §4.5 mandates, and the split is the
point:

| module | Rust home | may do IO? |
|---|---|---|
| `schema.py` | `fathom-core::config` | **no** — types, defaults, coercion, suggestions only |
| `globs.py` | `fathom-core::paths` | no — pure pattern matching |
| `json_schema.py` | `fathom-core::schema_gen` (+ `fathom-cli`'s `schema-gen` feature) | read-only (drift check) |
| `sources.py` | `fathom-cli::config` | **yes** — files, `FATHOM_*`, `git` probes |
| `layer.py` | `fathom-cli::config::resolve` | via `sources` only |
| `explain.py` | `fathom-cli::verbs::config` | no — renders a resolved `Config` |
| `cli.py` | `fathom-cli::verbs::config` | the verb surface, exit codes |

ADR-002 §4.5's rule — *"no crate below `fathom-cli` reads configuration, environment
variables or files to find out how it should behave"* — is enforced by
`tests/test_config_contract.py`, which greps `schema.py` and `explain.py` for `open(`,
`os.environ` and `subprocess`. A contract with no failing test behind it is a preference.

Public surface:

    from recommender.config import load, Config
    config = load(cwd="/repo", sets=["risk.tau=80"])
    config.get("risk.tau")            # 80
    config.decision("risk.tau").winner  # 'flag'
    print(explain.explain_path_text(config, "src/foo.rs"))
"""

from __future__ import annotations

from .errors import (ConfigError, ConfigInvalid, ConfigVersionRejected,
                     NetworkConsentRequired, UsageError, Warning)
from .explain import explain_path_text, explain_text, privacy_matrix, privacy_text
from .layer import Config, Decision, Step, load, merge
from .schema import CONFIG_VERSION, KEYS, KEY_LIST, Layer, Key

__all__ = ["CONFIG_VERSION", "Config", "ConfigError", "ConfigInvalid",
           "ConfigVersionRejected", "Decision", "KEYS", "KEY_LIST", "Layer", "Key",
           "NetworkConsentRequired", "Step", "UsageError", "Warning",
           "explain_path_text", "explain_text", "load", "merge", "privacy_matrix",
           "privacy_text"]
