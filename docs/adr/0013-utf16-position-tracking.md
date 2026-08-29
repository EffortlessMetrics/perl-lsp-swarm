# ADR-0013: Source-text newline, BOM, and position domains

**Status**: Accepted, amended 2026-08-29
**Decision issue**: #4973
**Fixture authority**: #8172
**Position programme**: #1814

## Decision

Exact UTF-8 source snapshots use one LF-delimited source-line contract:

1. `LF` terminates a logical source line.
2. `CRLF` is one separator whose `LF` terminates the line; its `CR` is not content.
3. A bare `CR`, `VT`, `FF`, `NEL`, `LS`, and `PS` remain ordinary source content.
4. Mixed `LF` and `CRLF` input is supported without source normalization.
5. Empty input has one line at byte zero; a final `LF` creates a terminal empty line.

For each line, source geometry distinguishes `line_start`, `line_content_end`, and
`line_separator_end`. For `"abc\\r\\ndef"`, line 0 is `0..3` content and `0..5`
including its separator; line 1 starts at byte 5.

The `perl-position-tracking` line-index constructors and the `perl-line-index`
constructor use this contract. Rope remains storage: Ropey's broader logical-line
classification is not source-line authority. Parser byte points, Tree-sitter points,
LSP positions, and DAP positions retain separate coordinate types and units.

## BOM and source subjects

BOM handling belongs to source ingress and identity, not line indexing. A leading
decoded `U+FEFF` may be preserved or stripped only under an explicit ingress policy;
stripping creates a related source subject with an offset map. Non-leading `U+FEFF`
is ordinary content. Providers and indexes must not add or subtract three bytes
locally. Current LSP ingress stripping remains owned by #5219/#8707 and is not
overridden by this ADR.

## Coordinate domains

| Domain | Unit | Owner |
|---|---|---|
| Source/parser geometry | UTF-8 bytes | #8687 and parser consumers |
| Tree-sitter point | UTF-8 bytes from source-line start | #9526 |
| LSP position | negotiated UTF-8 bytes or UTF-16 code units | #9830/#7881 and LSP ports |
| DAP source position | DAP line/column over the same source line | #10524 |

No LSP UTF-16 type belongs in a source-line record.

## Strictness

Strict mappings reject or return a typed non-exact disposition for invalid lines,
UTF-8 scalar interiors, UTF-16 surrogate interiors, reversed ranges, disallowed
separator interiors, stale generations, and wrong source subjects. Tolerant helpers
may clamp only when their API and result type disclose that behavior; they cannot
authorize exact edits or outgoing ranges.

## Proof and ownership

#8172 owns raw-byte fixtures and independent expected rows for line endings, unusual
separators, BOM controls, Unicode, EOF, separator boundaries, and invalid positions.
The strict reference (#9830), indexed mapper (#7881), ingress relations (#8707),
legacy classification (#8716/#8259), Tree-sitter map (#9526), and DAP geometry
(#10524) consume those facts within their named domains. This ADR records the ruling;
it does not claim those downstream migrations are complete.

## Consequences

- All three position-tracking constructors now derive line starts from the same LF
  rule, including when the source is read through Rope chunks.
- Callers that relied on bare-CR or Unicode-separator line breaks must be classified
  as a separate internal domain or migrated by their owning issue.
- No provider migration, source normalization, UTF-32 policy, or indexed-mapper
  performance claim follows from this decision alone.

## References

- [Position Tracking Guide](../reference/POSITION_TRACKING_GUIDE.md)
- [Rope Integration Guide](../reference/ROPE_INTEGRATION_GUIDE.md)
- [LSP text document specification](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#textDocuments)
