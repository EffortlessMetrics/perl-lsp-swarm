from __future__ import annotations

import argparse
import json
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent
PACKAGE = ROOT / "LSP-perllsp"
SOURCE_AUTHORITY = ROOT / "package-source.v1.json"


def included_files() -> list[str]:
    """The exported package contents derive from the authoritative manifest.

    The package-source manifest is the single declared authority for what
    ships; a second hard-coded list here would drift from it.
    """
    payload = json.loads(SOURCE_AUTHORITY.read_text(encoding="utf-8"))
    return list(payload["package_files"])


def build(output: Path) -> None:
    included = included_files()
    missing = [path for path in included if not (PACKAGE / path).is_file()]
    if missing:
        raise SystemExit(f"missing package files: {missing}")
    output.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for relative in sorted(included):
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
