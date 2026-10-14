"""Fathom contract fixtures (ADR-005). Not the product: the product is the Rust TUI
of ADR-002 (`e0.2`, still open). These packages exist so that the harness contract
is something a program can check rather than prose two teams interpret differently."""

__all__ = ["harness", "hooks"]
