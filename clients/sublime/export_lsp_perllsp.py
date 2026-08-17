from __future__ import annotations

import argparse
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent
PACKAGE = ROOT / "LSP-perllsp"
INCLUDED = [
    ".python-version",
    "LICENSE-APACHE",
    "LICENSE-MIT",
    "LSP-perllsp.sublime-settings",
    "README.md",
    "messages.json",
    "messages/install.txt",
    "plugin.py",
    "release.py",
    "server-manifest.json",
    "sublime-package.json",
]


def build(output: Path) -> None:
    missing = [path for path in INCLUDED if not (PACKAGE / path).is_file()]
    if missing:
        raise SystemExit(f"missing package files: {missing}")
    output.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for relative in sorted(INCLUDED):
            data = (PACKAGE / relative).read_bytes()
            info = zipfile.ZipInfo(relative, date_time=(1980, 1, 1, 0, 0, 0))
            info.external_attr = 0o644 << 16
            archive.writestr(info, data)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    build(args.output)


if __name__ == "__main__":
    main()
