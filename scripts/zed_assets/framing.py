"""Strict LSP framing helpers for the managed perllsp smoke."""

from __future__ import annotations

import json
from typing import Any

from .common import ReceiptError


def lsp_frame(message: dict[str, Any]) -> bytes:
    body = json.dumps(message, separators=(",", ":"), ensure_ascii=False).encode("utf-8")
    return f"Content-Length: {len(body)}\r\n\r\n".encode("ascii") + body


def parse_lsp_frames(data: bytes) -> list[dict[str, Any]]:
    frames: list[dict[str, Any]] = []
    cursor = 0
    while cursor < len(data):
        header_end = data.find(b"\r\n\r\n", cursor)
        if header_end < 0:
            raise ReceiptError("protocol stdout contains an incomplete or stray header")
        header = data[cursor:header_end].decode("ascii", errors="strict")
        length: int | None = None
        for line in header.split("\r\n"):
            name, separator, value = line.partition(":")
            if separator and name.strip().lower() == "content-length":
                length = int(value.strip())
        if length is None or length < 0:
            raise ReceiptError("protocol stdout frame lacks a valid Content-Length")
        body_start = header_end + 4
        body_end = body_start + length
        if body_end > len(data):
            raise ReceiptError("protocol stdout frame is truncated")
        value = json.loads(data[body_start:body_end])
        if not isinstance(value, dict):
            raise ReceiptError("protocol stdout frame is not a JSON object")
        frames.append(value)
        cursor = body_end
    return frames
