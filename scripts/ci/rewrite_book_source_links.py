#!/usr/bin/env python3
"""Resolve remaining local links in copied docs against their canonical source."""

from pathlib import Path
import re
import sys
from urllib.parse import quote


LINK = re.compile(r"(?<=\]\()[^\s)]+(?=\))")


def rewrite(source: Path, destination: Path, root: Path, text: str) -> str:
    book_root = root / "book" / "src"

    def replace(match: re.Match[str]) -> str:
        target = match.group()
        if target.startswith(("#", "/", "https:", "http:", "mailto:", "data:")):
            return target
        path_part, marker, fragment = target.partition("#")
        path_part, query_marker, query = path_part.partition("?")
        if not path_part:
            return target

        # Existing book-local chapter links are intentional. The copied source
        # cannot name these generated filenames, so prefer the in-book target.
        book_target = (destination.parent / path_part).resolve()
        source_target = (source.parent / path_part).resolve()
        if book_target.is_relative_to(book_root) and book_target.is_file() and not source_target.exists():
            return target
        if not source_target.exists() or not source_target.is_relative_to(root):
            return target

        relative = source_target.relative_to(root).as_posix()
        kind = "tree" if source_target.is_dir() else "blob"
        url = f"https://github.com/EffortlessMetrics/perl-lsp-swarm/{kind}/main/{quote(relative, safe='/')}"
        if source_target.is_dir() and path_part.endswith("/"):
            url += "/"
        if query_marker:
            url += query_marker + query
        if marker:
            url += marker + fragment
        return url

    return LINK.sub(replace, text)


def main() -> None:
    source, destination, root = (Path(value).resolve() for value in sys.argv[1:])
    original = destination.read_text(encoding="utf-8")
    rewritten = rewrite(source, destination, root, original)
    if rewritten != original:
        destination.write_text(rewritten, encoding="utf-8")


if __name__ == "__main__":
    main()
