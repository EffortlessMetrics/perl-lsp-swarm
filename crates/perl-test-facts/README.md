# perl-test-facts

Pure, dependency-free TAP result facts for native Perl test intelligence.

`perl-test-facts` parses TAP text into stable Rust values describing plans,
assertions, TODO/SKIP directives, bailouts, diagnostics, runner-reported source
locations, and malformed or unknown protocol evidence.

## Boundary

This crate reads **result-time TAP only**. It does not:

- execute `perl`, `prove`, `yath`, or project commands;
- inspect the filesystem or discover test files;
- parse Perl source or model Test2/Test::More imports;
- depend on LSP, DAP, VS Code, or an async runtime;
- implement Test2 or Test::More.

Source-time test facts and test discovery belong to compiler/project-fact
layers. Process execution and runner selection belong to runtime adapters.
Product surfaces may project a `TapReport` into editor diagnostics or test
results without redefining TAP semantics.

## Example

```rust
use perl_test_facts::{TapAssertionStatus, parse_tap};

let report = parse_tap(
    "TAP version 13\n\
     ok 1 - loads\n\
     not ok 2 - later # TODO known issue\n\
     1..2\n",
);

assert!(report.is_success());
assert_eq!(report.count(TapAssertionStatus::Todo), 1);
assert_eq!(report.passed_count(), 1);
```

## Evidence retained

The parser keeps distinctions that downstream consumers commonly need:

- raw `ok` / `not ok` outcome versus TODO/SKIP classification;
- TAP stream line versus runner-reported source file and line;
- top-level and nested assertion depth;
- plan and bailout state;
- ordinary and YAML diagnostic records;
- first reported `got` and `expected` values;
- structural diagnostics and unknown raw lines.

Raw evidence remains available when the TAP stream is partial, malformed, or
uses protocol forms the current parser does not classify.

## License

MIT OR Apache-2.0
