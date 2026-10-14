"""Make the contract fixtures importable without an install step — the same reason
the ADR requires no compiler: a check that needs an install is a check that gets
skipped."""
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "src"))
