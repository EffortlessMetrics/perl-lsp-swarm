# PERLLSP-SPEC-0015 — Framework accessor and DBI facts

## Framework accessor facts
Moo/Moose `has ... isa => 'Pkg'` accessor-return facts remain fallback until source-backed exactness receipts exist.

## DBI migration
Move DBI receiver typing from text heuristics into TypeFact/ReceiverFact substrate:
- `DBI->connect` => `DBI::db`
- `prepare/prepare_cached` => `DBI::st`

## Release posture
Keep text heuristics as fallback during transition while fact-backed behavior proves out.
