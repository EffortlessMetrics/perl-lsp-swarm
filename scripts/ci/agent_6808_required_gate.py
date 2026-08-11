#!/usr/bin/env python3
"""Add the focused textDocumentSync contract to the required LSP smoke gate."""

from __future__ import annotations

import hashlib
from pathlib import Path


path = Path(".ci/gate-policy.yaml")
data = path.read_bytes()
blob_sha = hashlib.sha1(f"blob {len(data)}\0".encode() + data).hexdigest()
expected_blob = "5843d9fbb79847867dde37cfc50cf128d849fab3"
if blob_sha != expected_blob:
    raise SystemExit(
        f"refusing to patch unexpected gate policy: expected {expected_blob}, got {blob_sha}"
    )

text = data.decode("utf-8")
old = '''  - name: lsp_smoke
    tier: merge_gate
    description: "Deterministic LSP scenario test (semantic definitions)"
    required: true
    # Single-threaded for determinism
    command: >-
      env -u RUSTC_WRAPPER
      RUST_TEST_THREADS=1 CARGO_BUILD_JOBS=1
      cargo test -p perl-lsp-rs --test semantic_definition --locked
      -- --test-threads=1
    timeout_seconds: 300
'''
new = '''  - name: lsp_smoke
    tier: merge_gate
    description: "Deterministic LSP semantic-definition and initialize-contract tests"
    required: true
    # Single-threaded for determinism. Keep the wire-shape regression focused:
    # this target contains many API contracts, but only textDocumentSync owns
    # the reviewed recurrence claim in #6776.
    command: >-
      env -u RUSTC_WRAPPER
      RUST_TEST_THREADS=1 CARGO_BUILD_JOBS=1
      cargo test -p perl-lsp-rs --test semantic_definition --locked
      -- --test-threads=1
      &&
      env -u RUSTC_WRAPPER
      RUST_TEST_THREADS=1 CARGO_BUILD_JOBS=1
      cargo test -p perl-lsp-rs --test lsp_api_contracts --locked
      test_text_document_sync_option_keys_use_lsp_camel_case
      -- --exact --test-threads=1
    timeout_seconds: 300
'''
if text.count(old) != 1:
    raise SystemExit("expected one canonical lsp_smoke gate block")
path.write_text(text.replace(old, new, 1), encoding="utf-8")
