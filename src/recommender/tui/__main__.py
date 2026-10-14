"""Entry point for `python3 -m recommender.tui …` (see `cli.py` for the verbs)."""
import sys

from .cli import main

if __name__ == "__main__":
    sys.exit(main())
