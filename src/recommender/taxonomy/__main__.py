"""`python3 -m recommender.taxonomy …` — the executable form of `fathom explain <path>`."""

import sys

from .cli import main

if __name__ == "__main__":
    sys.exit(main())
