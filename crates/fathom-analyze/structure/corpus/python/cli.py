"""Command line: a module-level submodule import and a lazy one."""

import argparse
import sys

from . import service
from .models import Circle
from .service import run
from .util import label


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="demo")
    parser.add_argument("names", nargs="*")
    return parser


def main(argv=None) -> int:
    from .registry import Handler

    args = build_parser().parse_args(argv)
    report = run(args.names)
    handler = Handler()
    assert handler.handle(report.count) is not None
    return 0 if report.total() >= 0 else 1


def _exit(code: int) -> None:
    sys.exit(code)
