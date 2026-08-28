# Gold Corpus — Editor Intelligence Validation

The gold corpus is a curated set of Perl fixtures with explicit expected editor behavior. The headless scorecard opens each `fixture.pl` through the real LSP server, replays the requests declared by its sidecars, and fails CI when any assertion regresses.

This corpus is behavior evidence. It is separate from the static UX measurement fixture and from the broader workflow receipt dashboard: adding a sidecar here adds a replayable protocol assertion, not a manually reported score.

## Directory structure

Each scenario lives in its own directory:

```text
test_corpus/gold/
└── completion_scope_sibling/
    ├── fixture.pl
    └── expected_completion.json
```

A directory may carry more than one expectation file when the same Perl source is useful across editor surfaces.

| Sidecar | Surface | Harness behavior |
|---|---|---|
| `expected.json` | Diagnostics | Checks diagnostic presence, absence, and exact per-code counts |
| `expected_hover.json` | Hover | Checks nullability and required or forbidden content |
| `expected_goto.json` | Go to definition | Checks nullability and the first target line |
| `expected_completion.json` | Completion | Checks non-empty results, Top-1/Top-5 relevance, presence, and forbidden noise |
| `expected_symbols.json` | Document symbols | Checks symbol presence, absence, and total count |
| `expected_rename.json` | Rename | Checks success, rejection, and minimum edit count |
| `expected_module.json` | Module resolution consistency | Declares the expected result shared by diagnostics, hover, and go to definition |

## Run the editor-intelligence scorecard

```bash
RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs --test editor_intelligence_scorecard -- --nocapture
```

The test discovers supported sidecars automatically, prints per-surface pass rates, and fails if any assertion fails.

The diagnostics-specific suite remains available separately:

```bash
cargo test -p perl-lsp-diagnostics --test diagnostics_gold_suite -- --nocapture
```

## Completion expectations

A completion sidecar names the fixture and places each assertion at a zero-based LSP position:

```json
{
  "version": 1,
  "fixture": "completion_scope_sibling",
  "assertions": [
    {
      "kind": "completion_present",
      "line": 5,
      "character": 11,
      "expected_label": "$sib_level_top",
      "rationale": "the ancestor binding remains visible"
    },
    {
      "kind": "completion_noise_absent",
      "line": 5,
      "character": 11,
      "forbidden_label": "$sib_left",
      "rationale": "an ended sibling binding is not visible"
    }
  ]
}
```

Supported completion assertions:

| Assertion | Required field | Meaning |
|---|---|---|
| `completion_non_empty` | — | At least one completion item is returned |
| `completion_top1` | `expected_label` | The expected label is first |
| `completion_top5` | `expected_label` | The expected label appears in the first five items |
| `completion_present` | `expected_label` | The expected label appears anywhere |
| `completion_noise_absent` | `forbidden_label` | The forbidden label does not appear at any rank |

### Scope controls

`completion_scope_sibling` proves lexical admission rather than an empty response: a file-scope binding sharing the prefix must remain present while the binding from an ended sibling block must be absent.

`completion_scope_ranking` proves relevance among two valid candidates: the immediate-scope lexical must be Top-1, while the file-scope ancestor must remain within the Top-5.

## Diagnostics expectations

The diagnostics sidecar uses the `assertion` discriminator:

```json
{
  "diagnostics": [
    {
      "assertion": "diagnostic_present",
      "code": "PL100",
      "message_contains": "strict"
    }
  ]
}
```

| Assertion | Fields | Meaning |
|---|---|---|
| `no_diagnostics` | — | No diagnostics are emitted |
| `no_diagnostic` | `code` | The specified diagnostic code is absent |
| `diagnostic_present` | `code`, optional `byte_offset`, optional `message_contains` | A matching diagnostic is present |
| `diagnostic_count` | `code`, `count` | Exactly `count` diagnostics with the code are emitted |

## Module-resolution consistency

Module-resolution fixtures are consumed by `ux_scenario_14_inc_conformance`. They declare the module, the `@INC` resolution mode, and the expected agreement among PL701 diagnostics, go to definition, and hover.

```json
{
  "module": "Greet",
  "resolution_mode": "workspace_config",
  "consumers": {
    "PL701_diagnostic": "no_error",
    "goto_definition": "resolves",
    "hover": "resolves"
  }
}
```

Current resolution modes include workspace `includePaths`, lexical `use lib`, `no lib` cancellation, `FindBin`-relative paths, and injected system `@INC` paths.

Run that harness with:

```bash
cargo test -p perl-lsp-ux-tests --test ux_scenario_14_inc_conformance -- --nocapture
```

## Add a fixture

1. Create `test_corpus/gold/<descriptive-name>/fixture.pl`.
2. Add one or more supported expectation sidecars.
3. Use zero-based LSP line and character positions.
4. Include a positive control when asserting absence so an empty or broken response cannot pass.
5. Run the focused scorecard and the relevant provider tests.

## Related issues

- #4065 — Bootstrap gold corpus seed fixtures and diagnostics scorecard
- #4066 — Headless editor-intelligence scorecard
- #4067 — Module-resolution consumer-consistency harness
- #8941 — Cursor lexical-visibility admission for completion
