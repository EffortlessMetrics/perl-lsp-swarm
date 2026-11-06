# Position fixtures v1

This private crate is the independent expected-fact authority for #8172. It is
`publish = false` because both xtask and Rust test consumers need one validated,
typed case/filter API without moving fixture-only code into a production mapper
or copying loaders across parser, position, LSP, and process tests.

`fixtures/position-fixtures.v1.json` contains literal source subjects and expected
facts. Raw bytes use hexadecimal text (`literal-hex/v1`), so Git's text newline
conversion can alter JSON whitespace but cannot alter represented CR, CRLF, BOM,
Unicode, or invalid UTF-8 bytes. The loader checks length and SHA-256 before any
semantic fact. The review projection is generated and has no authority of its own.

`fixtures/build_fixtures.py` is an independent reviewed seed. It imports no
production parser, scanner, mapper, decoder, or Ropey semantics. Its output is
committed as literal data, and Rust independently checks the data's byte identity,
LF record coverage, UTF-8 scalar boundaries, parser points, wire facts, source
relations, and refusals. Review any regenerated JSON diff, especially the pinned
`x😀y\r\nz` fact. Large boundary cases retain selected probes; small cases list
every valid scalar and wire content boundary. Invalid UTF-8 cases are ingress
negative controls and expose no text subject or coordinates.

Run, with the workspace-approved Cargo target directory:

```text
cargo xtask position-fixtures project
cargo xtask position-fixtures check
cargo xtask position-fixtures list --filter crlf
cargo xtask position-fixtures explain astral
```

Consumers call `load(repository_root)` before reading cases, then `select` and
`raw_bytes`. The validation gate rejects missing/changed expected rows. The
`perl-position-tracking` integration test independently compares accepted LF line
records and every byte cut to the production `LineRecordTable` after validation.
The corpus does not implement a mapper, change provider behavior, or prove editor
activation. ADR-0048 (`lf-source-lines/v1`) is the accepted LF and BOM source-subject
contract. Source decoding selection belongs to #13529/#13533.
