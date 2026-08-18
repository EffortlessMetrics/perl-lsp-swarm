#!/usr/bin/env python3
"""Retired one-shot helper used to apply #4555 legacy-authority banners.

The migration has been committed. Current enforcement is read-only through
`tests/test_legacy_authority_banners.py` and the `Legacy Authority Banners` workflow.
"""

from __future__ import annotations

import sys


MESSAGE = """RETIRED: the local legacy-authority banner migration has already been applied.

Do not rewrite historical document bodies through this helper. Update the authority
registry and the affected local banner through a reviewed pull request, then run:

  python3 -m unittest tests/test_legacy_authority_banners.py
"""


if __name__ == "__main__":
    print(MESSAGE, file=sys.stderr)
    raise SystemExit(2)
