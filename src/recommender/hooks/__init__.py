"""Harness adapters (ADR-005 §2). `all_adapters()` is what `harness status` reports,
and the `provenance` it carries is the difference between a protocol we verified and
a protocol we guessed."""

from __future__ import annotations

from typing import List

from .base import Adapter
from .claude_code import ClaudeCodeAdapter
from .generic_jsonl import GenericJsonlAdapter, install_git_hook, replay_git_history
from .pi_json import PiJsonAdapter
from .vendor_adapters import CodexCliAdapter, OpenCodeAdapter

__all__ = ["Adapter", "PiJsonAdapter", "ClaudeCodeAdapter", "CodexCliAdapter", "OpenCodeAdapter",
           "GenericJsonlAdapter", "all_adapters", "install_git_hook", "replay_git_history"]


def all_adapters() -> List[Adapter]:
    """Order matters only for display stability. The git-hook fallback is last: it
    is the one that always works, and the least specific."""
    return [PiJsonAdapter(), ClaudeCodeAdapter(), CodexCliAdapter(), OpenCodeAdapter(),
            GenericJsonlAdapter()]
