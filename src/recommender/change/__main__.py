"""`python3 -m recommender.change …` — the executable form of `e2.1`'s verb surface."""
from __future__ import annotations

import sys

from .cli import main

if __name__ == "__main__":
    sys.exit(main())
