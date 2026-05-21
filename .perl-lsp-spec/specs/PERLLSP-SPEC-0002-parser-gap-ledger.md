# PERLLSP-SPEC-0002 — Parser Gap Ledger

## Requirement

Parser gap claims must be fixture-backed and tracked in a policy ledger (`policy/parser-gap-ledger.toml`).

Each gap entry must include:
- fixture path,
- expected behavior contract,
- status,
- proof command(s),
- claim boundary.

## Claim boundary

A fixture-backed gap demonstrates behavior for tested inputs only; it does not imply universal language coverage.
