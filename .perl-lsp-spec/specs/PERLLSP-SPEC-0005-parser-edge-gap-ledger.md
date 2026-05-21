# PERLLSP-SPEC-0005: Parser edge-gap ledger

## Contract

Every known parser edge gap must be represented in:

```text
policy/parser-edge-gap-ledger.toml
```

## Ledger shape

```toml
schema_version = "1.0"
owner = "parser-edge-gap-closure"

[[gap]]
id = "PARSER-GAP-0001"
slug = "continue-redo-statements"
category = "ga-missing-coverage"
priority = "P0"
status = "fixture-needed"
source = "docs/issues/corpus/gaps/ga-feature-missing-coverage/continue-redo-statements.md"
fixture = ""
expected = ""
proof = []
claim_boundary = "No GA parser claim for continue/redo coverage until fixture-backed."
```

## Valid statuses

```text
untriaged
fixture-needed
design-needed
boundedness-needed
fixture-backed
fixed-v3
fixed-all
bounded-degradation
accepted-impossible
superseded
regressed
```

## Required validator

```bash
cargo xtask check-parser-edge-gaps
```

Validation rules:

- every `gap.id` is unique;
- every `gap.slug` is unique;
- every non-`accepted-impossible` P0/P1 gap has a fixture or explicit `design-needed`;
- every timeout/hang gap has a boundedness budget;
- every `fixed-v3` gap has proof commands;
- every `accepted-impossible` gap has a rationale and non-goal;
- every fixture path exists once fixture status is claimed;
- every proof command is non-empty;
- no gap is silently removed from the ledger.
