# Provenance: `specification.md`

- Source repository: `microsoft/language-server-protocol`
- Source path: `_specifications/lsp/3.18/specification.md`
- Pinned revision: `2cbcf18d991d3564af08fcbf5eb8b8af546a3e71`
  (commit "Update 3.18.0 release date (#2268)", committed 2026-06-09)
- Vendored: 2026-08-28
- SHA-256: `67e09b5458884dad63631a4cc7f4ea72b659b023e950eb0f7fa7311355cde3d7`

This file is a byte-for-byte copy of the upstream file at the pinned revision.
It is the independent authoritative source for the official LSP 3.18 change-log
bullet inventory consumed by `xtask/tests/lsp_318_changelog_totality.rs`. The
test parses the `3.18.0 (06/04/2026)` Change Log section from this fixture and
requires every parsed bullet to be classified exactly once by the handwritten
mapping, so bullets can be neither dropped nor invented there.

To re-pin: replace `specification.md` with the new upstream revision, update
the pinned revision, date, and SHA-256 in this file, and re-derive the
handwritten mapping from the parsed bullets. Never edit `specification.md`
by hand.
