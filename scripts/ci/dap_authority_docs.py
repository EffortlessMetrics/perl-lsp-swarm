"""Documentation/manifest reconciliation for the DAP authority gate."""

from __future__ import annotations

from pathlib import Path
from typing import Any, Mapping, Sequence

from dap_authority_common import (
    DOC_PATHS,
    FORBIDDEN_DOC_PHRASES,
    AuthorityError,
    manifest_rows,
    object_value,
    read_text,
    string_value,
)


def _markdown_code_row(values: Sequence[str]) -> str:
    return "| " + " | ".join(f"`{value}`" for value in values) + " |"


def validate_docs(root: Path, manifest: Mapping[str, Any]) -> None:
    upstream = object_value(manifest.get("upstream"), "manifest.upstream")
    commit = string_value(upstream.get("commit"), "manifest.upstream.commit")
    blob = string_value(upstream.get("git_blob_sha1"), "manifest.upstream.git_blob_sha1")

    extension_rows: list[str] = []
    for index, extension in enumerate(manifest_rows(manifest, "project_extensions")):
        extension_rows.append(
            _markdown_code_row(
                (
                    string_value(
                        extension.get("wire_name"), f"project_extensions[{index}].wire_name"
                    ),
                    string_value(extension.get("kind"), f"project_extensions[{index}].kind"),
                    string_value(
                        extension.get("classification"),
                        f"project_extensions[{index}].classification",
                    ),
                    string_value(
                        extension.get("version"), f"project_extensions[{index}].version"
                    ),
                    string_value(extension.get("owner"), f"project_extensions[{index}].owner"),
                )
            )
        )

    configuration_rows: list[str] = []
    for index, configuration in enumerate(manifest_rows(manifest, "project_configuration")):
        configuration_rows.append(
            _markdown_code_row(
                (
                    string_value(
                        configuration.get("surface"),
                        f"project_configuration[{index}].surface",
                    ),
                    string_value(
                        configuration.get("classification"),
                        f"project_configuration[{index}].classification",
                    ),
                    string_value(
                        configuration.get("owner"),
                        f"project_configuration[{index}].owner",
                    ),
                )
            )
        )

    documents: list[str] = []
    for relative in DOC_PATHS:
        text = read_text(root / relative, "protocol authority document")
        documents.append(text)

        for phrase in FORBIDDEN_DOC_PHRASES:
            if phrase in text:
                raise AuthorityError(f"{relative} retains forbidden stale claim {phrase!r}")
        required_markers = (
            "Content-Length framed JSON",
            "not JSON-RPC",
            "standard DAP",
            "project extension",
            "inlineValues",
            commit,
            blob,
            "#6737",
            '<a id="4-breakpoint-requests"></a>',
            *extension_rows,
            *configuration_rows,
        )
        for required in required_markers:
            if required not in text:
                raise AuthorityError(f"{relative} is missing authority marker {required!r}")

    if documents[0] != documents[1]:
        raise AuthorityError(
            "canonical DAP authority doc and committed book copy differ; run the documentation sync"
        )
