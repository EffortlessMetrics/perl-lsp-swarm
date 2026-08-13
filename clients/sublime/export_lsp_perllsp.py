from __future__ import annotations

import argparse
import zipfile
from pathlib import Path

from package_source import load_manifest, validate_source_tree

ROOT = Path(__file__).resolve().parent
PACKAGE = ROOT / "LSP-perllsp"


def build(output: Path) -> None:
    manifest = load_manifest()
    validate_source_tree(manifest, PACKAGE)
    included = tuple(manifest["package_files"])
    output.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for relative in included:
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
