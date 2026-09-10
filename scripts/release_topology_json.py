"""Admit an exact top-level release-topology schema before ordinary JSON decoding."""

from decimal import Decimal, InvalidOperation
import json
from typing import Any


class _ObjectPairs(list):
    """Keep object identity and duplicate fields during the admission pass."""


class _NumberToken(str):
    """Retain a JSON-owned numeric token without converting unrelated fields."""


def load_topology_json(raw: bytes | str) -> dict[str, Any]:
    """Preserve ordinary decoded values and original evidence bytes after admission.

    Only the root schema field has this exact-number contract. JSON owns key
    escaping and nesting; unrelated fields retain the ordinary decoder behavior.
    """
    exact = json.loads(
        raw, parse_int=_NumberToken, parse_float=_NumberToken, object_pairs_hook=_ObjectPairs
    )
    if not isinstance(exact, _ObjectPairs):
        raise ValueError("release topology schema requires a JSON object")
    versions = [value for key, value in exact if key == "schema"]
    if len(versions) != 1 or type(versions[0]) is not _NumberToken:
        raise ValueError("release topology schema must appear once and be exactly 1 or 2")
    try:
        version = Decimal(versions[0])
    except InvalidOperation as error:
        raise ValueError("release topology schema has an unsupported numeric representation") from error
    if version not in (1, 2):
        raise ValueError("release topology schema must appear once and be exactly 1 or 2")
    return json.loads(raw)
