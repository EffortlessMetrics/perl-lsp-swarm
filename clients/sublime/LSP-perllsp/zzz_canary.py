"""Diagnostic canary: proves plugin loading runs on CI hosts.

Writes unconditionally at import time and again when the API is ready, so a
silent Sublime startup is distinguishable from a plugin-loading stall. Remove
once the host journey is green.
"""
from __future__ import annotations

import os
import sys

_CANARY_PATH = os.path.join(
    os.path.expanduser("~"), "perllsp_sublime_canary.log"
)

with open(_CANARY_PATH, "a", encoding="utf-8") as handle:
    handle.write(f"import python={sys.version.split()[0]}\n")


def plugin_loaded() -> None:
    with open(_CANARY_PATH, "a", encoding="utf-8") as handle:
        handle.write("plugin_loaded\n")
