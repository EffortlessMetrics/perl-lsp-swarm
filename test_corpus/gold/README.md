# Gold Corpus

`test_corpus/gold/` is the hand-verified regression corpus for Perl LSP diagnostics and editor intelligence. A fixture directory owns one Perl document and one or more assertion sidecars. The same source can therefore prove several consumer surfaces without duplicating the case.

## Fixture contract

Every direct child directory must contain:

- `fixture.pl` — the UTF-8 Perl document opened by the test harness
- at least one recognized assertion sidecar

The repository contract rejects symbolic links, non-regular required files, malformed JSON, empty assertion arrays, mismatched fixture identities, unsupported sidecar versions, unknown `expected*.json` files, and population regressions.

Fixture roots may contain only the allowlisted source and sidecar files plus an optional `lib/` payload. The `lib/` subtree may be nested, but every leaf must be a regular `.pm` file; symbolic links and other unlisted assets are rejected.

Editor and module sidecars use this common envelope:

```json
{
  "version": 1,
  "fixture": "rename_subroutine",
  "assertions": [
    {
      "kind": "rename_succeeds",
      "line": 4,
      "character": 4,
      "new_name": "sum_values"
    }
  ]
}
```

`fixture` must exactly match the containing directory name. Named sidecars are
closed-world contracts: their envelope and typed assertion members reject unknown
fields. LSP `line` and `character` values are zero-based.

Rename assertions use `expected_edits` as follows:

- Omit `expected_edits` for the legacy count-only contract; the scorecard still requires a well-formed response.
- Provide an array to require the exact edited ranges and replacement text. An empty array therefore requires no edits.
- `null` is invalid and rejected by the corpus parser. It never downgrades an assertion to count-only mode.

## Recognized sidecars

| Sidecar | Surface | Current floor |
|---|---|---:|
| `expected.json` | Diagnostics | 28 |
| `expected_hover.json` | Hover | 8 |
| `expected_goto.json` | Goto definition | 3 |
| `expected_completion.json` | Completion | 4 |
| `expected_symbols.json` | Document symbols | 2 |
| `expected_rename.json` | Rename | 2 |
| `expected_module.json` | Contract metadata for `@INC` and module-resolution cases | 5 |

The corpus currently contains at least 34 fixture directories. Sidecar counts overlap because one fixture may exercise several surfaces.

## Diagnostics assertions

`expected.json` contains a `diagnostics` array:

```json
{
  "diagnostics": [
    {
      "assertion": "diagnostic_present",
      "code": "PL001",
      "message_contains": "parse"
    }
  ]
}
```

| Assertion | Fields | Contract |
|---|---|---|
| `no_diagnostics` | — | No diagnostic may be emitted. |
| `no_diagnostic` | `code` | The named diagnostic code must be absent. |
| `diagnostic_present` | `code`, optional `message_contains`, optional `byte_offset` | A diagnostic with the code must be present; the message substring is matched when supplied. Declared byte offsets must be within the UTF-8 source length. |
| `diagnostic_count` | `code`, `count` | Exactly `count` diagnostics with the code must be emitted. |

`byte_offset` is retained as source-location metadata and is bounds-checked by the repository contract. The current editor-intelligence diagnostics runner selects diagnostics by code and optional message; it does not yet use the offset to disambiguate equal-coded diagnostics.

## Verification

Run the repository topology, schema, identity, and population gate:

```bash
cargo test -p perl-corpus --test gold_repository_contract
```

Run all scored diagnostics and editor-intelligence assertions:

```bash
RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs --test editor_intelligence_scorecard -- --nocapture
```

Run the module-resolution consumer-consistency harness:

```bash
cargo test -p perl-lsp-ux-tests --test ux_scenario_14_inc_conformance -- --nocapture
```

The generated editor scorecard is produced with:

```bash
cargo xtask ux-scorecard
```

## Adding a fixture

1. Create `test_corpus/gold/<descriptive_identity>/fixture.pl`.
2. Add at least one recognized sidecar. Prefer adding `expected.json` as well when the document has a stable diagnostics expectation.
3. For editor or module sidecars, set `version` to `1`, set `fixture` to the directory name, and add at least one assertion with a rationale.
4. Use zero-based LSP positions and keep assertions narrow enough to identify the intended behavior.
5. Raise the relevant population floor in `crates/perl-corpus/tests/gold_repository_contract.rs` when the case is accepted as durable corpus evidence.
6. Run the repository contract and every consumer named by the new sidecars.

Do not add a fixture solely to increase a count. A gold case should preserve a behavior we intend to support or make a known failure explicit.

## Module-resolution fixtures

The five `expected_module.json` fixtures exercise the same module-resolution decision across diagnostics, goto definition, and hover:

| Fixture | Resolution mode |
|---|---|
| `inc_relative_include_path` | Workspace-relative include path |
| `inc_use_lib_lexical` | Lexical `use lib` |
| `inc_no_lib_cancellation` | `use lib` followed by `no lib` |
| `inc_findbin_relative` | `FindBin`-relative library path |
| `inc_system_inc` | Injected system `@INC` entry |

The repository contract validates these sidecars. The production
`ux_scenario_14_inc_conformance` harness currently uses its own inline fixtures and
does not consume `expected_module.json`; its results therefore establish the
module-resolution behavior, not sidecar-to-harness wiring. Keep the sidecars as
contract metadata until a dedicated loader/adapter makes that connection explicit.

## Tracking

- #4065 — bootstrap gold corpus and diagnostics scorecard
- #4067 — module-resolution consumer consistency
