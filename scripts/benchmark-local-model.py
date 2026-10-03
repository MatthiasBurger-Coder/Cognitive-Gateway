#!/usr/bin/env python3
"""Run the optional local model benchmark without starting any model services."""
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'services/local-model'))
from benchmark import main

if __name__ == '__main__':
    raise SystemExit(main())
