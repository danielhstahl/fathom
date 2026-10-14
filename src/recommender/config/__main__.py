"""`python3 -m recommender.config …` — same surface as `fathom config …` in the product."""

import sys

from .cli import main

if __name__ == "__main__":
    sys.exit(main())
