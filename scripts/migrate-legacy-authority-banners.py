#!/usr/bin/env python3
"""Insert local status banners into the legacy agent/control-plane document set.

This is a one-shot migration helper for #4555. It preserves each document body and
changes only explicit top-level status metadata plus one banner after the title.
"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MARKER = "<!-- authority-status:v1 -->"


@dataclass(frozen=True)
class Document:
    path: str
    status: str
    successor: str
    successor_label: str
    authority_index: str
    replace: tuple[str, str] | None = None


DOCUMENTS = (
    Document(
        "docs/reference/ORCHESTRATION_DOCTRINE.md",
        "superseded",
        "../agents/DEVELOPMENT_METHOD.md",
        "Development method",
        "../agents/AUTHORITY_STATUS.md",
    ),
    Document(
        "docs/reference/PIPELINE_GATES.md",
        "superseded",
        "../agents/DEVELOPMENT_METHOD.md",
        "Development method",
        "../agents/AUTHORITY_STATUS.md",
        (
            "**Status**: Active doctrine (introduced 2026-04-27; authority model reconciled 2026-07-13 for the #4005 subtraction)",
            "**Status**: Superseded historical doctrine (introduced 2026-04-27; superseded by the provider-native method)",
        ),
    ),
    Document(
        "docs/reference/OCTOPUS_CLUSTER.md",
        "historical",
        "../agents/DEVELOPMENT_METHOD.md",
        "Development method",
        "../agents/AUTHORITY_STATUS.md",
    ),
    Document(
        "docs/reference/GLOSSARY.md",
        "superseded",
        "../agents/AUTHORITY_STATUS.md",
        "Agent and maintainer authority status",
        "../agents/AUTHORITY_STATUS.md",
    ),
    Document(
        "docs/reference/LIVE_SIGNALS_VS_LABELS.md",
        "historical",
        "../agents/GITHUB_SURFACES.md",
        "GitHub surfaces",
        "../agents/AUTHORITY_STATUS.md",
    ),
    Document(
        "docs/adr/0044-octopus-cluster-orchestration.md",
        "superseded",
        "../agents/DEVELOPMENT_METHOD.md",
        "Development method",
        "../agents/AUTHORITY_STATUS.md",
        ("**Status**: Accepted", "**Status**: Superseded"),
    ),
    Document(
        "docs/articles/PIPELINE_STATE_MACHINE.md",
        "historical",
        "../agents/GITHUB_SURFACES.md",
        "GitHub surfaces",
        "../agents/AUTHORITY_STATUS.md",
    ),
    Document(
        "docs/handoff/SWARM_DESIGN.md",
        "historical",
        "../agents/DEVELOPMENT_METHOD.md",
        "Development method",
        "../agents/AUTHORITY_STATUS.md",
    ),
    Document(
        ".spec/3988-merge-readiness/spec.md",
        "historical",
        "../../docs/agents/REVIEW_CURRENTNESS.md",
        "Review and proof currentness",
        "../../docs/agents/AUTHORITY_STATUS.md",
    ),
)


def banner(document: Document) -> str:
    return (
        f"\n\n{MARKER}\n"
        f"> **Status: {document.status}.** Current authority: "
        f"[{document.successor_label}]({document.successor}).\n"
        "> Retained as historical design or mechanism evidence. Internal wording below "
        "that calls this document accepted, active doctrine, a north star, current "
        "instruction, or lifecycle authority is historical and must not route current "
        f"work. See [Agent and maintainer authority status]({document.authority_index})."
    )


def migrate(document: Document) -> bool:
    path = ROOT / document.path
    text = path.read_text(encoding="utf-8")
    if MARKER in text:
        return False

    if document.replace is not None:
        old, new = document.replace
        if old not in text:
            raise RuntimeError(f"{document.path}: expected explicit status text is missing")
        text = text.replace(old, new, 1)

    first_newline = text.find("\n")
    if first_newline < 0 or not text.startswith("#"):
        raise RuntimeError(f"{document.path}: expected a Markdown title on the first line")

    text = text[:first_newline] + banner(document) + text[first_newline:]
    path.write_text(text, encoding="utf-8")
    return True


def validate(document: Document) -> None:
    text = (ROOT / document.path).read_text(encoding="utf-8")
    head = "\n".join(text.splitlines()[:24])
    required = (
        MARKER,
        f"Status: {document.status}.",
        document.successor,
        document.successor_label,
        document.authority_index,
    )
    missing = [item for item in required if item not in head]
    if missing:
        raise RuntimeError(f"{document.path}: migrated banner missing {missing!r}")

    if document.replace is not None:
        old, new = document.replace
        if old in head or new not in head:
            raise RuntimeError(f"{document.path}: explicit status replacement did not hold")


def main() -> int:
    changed: list[str] = []
    for document in DOCUMENTS:
        if migrate(document):
            changed.append(document.path)
        validate(document)

    for path in changed:
        print(path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
