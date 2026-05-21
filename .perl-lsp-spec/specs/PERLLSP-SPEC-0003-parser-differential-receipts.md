# PERLLSP-SPEC-0003 — Parser Differential Receipts

## Requirement

Parser-comparison runs must be able to emit durable receipts (JSON and Markdown) that identify:
- commit SHA,
- parser targets and availability,
- fixture/corpus outcomes,
- disagreement summary,
- explicit claim boundary text.

## Claim boundary

Receipts report only tested parser targets and tested inputs at run time.
