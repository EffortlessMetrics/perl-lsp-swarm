# ADR-0013: Source-text newline, BOM, and position domains

**Status**: Accepted, amended 2026-08-15  
**Original date**: 2025-01-20  
**Decision issue**: #4973  
**Fixture authority**: #8172  
**Position programme**: #1814

## Decision

Live UTF-8 text documents use one newline and BOM contract:

1. `LF` is the canonical line boundary.
2. `CRLF` is one line ending because its `LF` byte terminates the line; the preceding `CR` is part of the separator, not line content.
3. A bare `CR` is ordinary source content. It does not create a new line.
4. Mixed `LF` and `CRLF` input is supported under the same LF-delimited model.
5. A leading UTF-8 BOM is decoded and preserved as `U+FEFF` in the source snapshot. It contributes three UTF-8 bytes, one Unicode scalar, and one UTF-16 code unit unless a future ingress contract strips it and supplies an explicit offset map.
6. No constructor may silently normalize source bytes.

This is **Option A** from #4973: support LF and CRLF while declining bare-CR-as-newline semantics. Bare CR is neither rejected nor normalized by the position layer; it remains addressable content.

## Coordinate domains

The repository keeps these propositions distinct:

| Domain | Column unit | Primary consumers |
|---|---|---|
| Source byte offset | UTF-8 bytes from snapshot start | parser, ranges, edits |
| Parser byte point | zero-based UTF-8 bytes from LF-delimited line start | parser and Tree-sitter-shaped surfaces |
| LSP UTF-8 position | zero-based UTF-8 bytes from line-content start | negotiated LSP sessions |
| LSP UTF-16 position | zero-based UTF-16 code units from line-content start | default LSP sessions |
| Unicode-scalar column | Unicode scalar values | only explicitly named internal consumers |

A type or method name must make the unit and source ownership recoverable. Parser byte points do not become LSP UTF-16 positions, and an LSP mapper does not become parser source identity.

## Line records

For each LF-delimited line, implementations distinguish:

```text
line_start
line_content_end
line_separator_end
```

For `"abc\r\ndef"`:

```text
line 0 start          = 0
line 0 content end    = 3
line 0 separator end  = 5
line 1 start          = 5
```

For `"abc\rdef"`, there is one line. The bare CR is content between `c` and `d`.

A final LF creates a terminal empty line. Empty input has one line starting at byte zero. These facts are recorded explicitly by #8172 rather than inferred differently by each consumer.

## Constructor authority

String-, Rope-, and retained byte-index constructors representing the same text-document domain must derive line starts from the same LF-delimited rule.

At the current implementation boundary:

- `LineStartsCache::new(&str)` and `LineStartsCache::new_rope(&Rope)` must agree;
- `perl_position_tracking::LineIndex` follows the same text-document rule;
- parser-only indexes may retain a different byte-coordinate domain only when their names, callers, and #8245 disposition make that distinction explicit;
- tolerant converters remain compatibility helpers and cannot authorize edits, exact outgoing ranges, or compatibility proof.

## Strictness and invalid boundaries

Strict mappings reject or return a typed non-exact disposition for:

- a byte offset inside a UTF-8 code point;
- a UTF-16 column inside a surrogate pair;
- an invalid line;
- a reversed range;
- a point past line content where the owning API does not permit separator addressing;
- a stale or wrong source generation.

Legacy tolerant helpers may clamp only when their API names and result types disclose that behavior. #8245 owns their terminal disposition.

## BOM policy

Editor-supplied text, filesystem text, parser input, and indexed text preserve the decoded leading `U+FEFF` unless one documented ingress authority strips it.

Random providers, line indexes, and renderers must not add or subtract three bytes to “handle BOM.” A stripping ingress must instead publish an explicit source relation and offset map consumed by every downstream coordinate authority.

## Required proof

#8172 provides raw-byte fixtures for:

- empty input and terminal empty lines;
- ASCII, BMP, astral, and combining text;
- LF, CRLF, bare CR, and mixed endings;
- BOM and no-BOM controls;
- invalid UTF-8 and UTF-16 interior positions;
- line start, content end, separator end, and EOF.

The independent strict reference, indexed mapper, legacy constructors, text transactions, provider projections, and client journeys consume the same fixture identities.

## Consequences

### Positive

- String and Rope constructors cannot assign different line numbers to the same supported source.
- CRLF remains fully supported without inventing a separate line model.
- Bare CR behavior is explicit instead of constructor-dependent.
- BOM handling is source-owned rather than provider arithmetic.
- Tree-sitter UTF-8 byte points and LSP UTF-16 positions can share source facts without sharing a coordinate type.

### Cost

- Callers that relied on bare CR creating lines must migrate or classify that behavior as a distinct internal domain.
- Some tolerant APIs require renaming or retirement under #8245.
- Exact separator-interior behavior remains API-specific and must be stated rather than hidden by broad “position conversion” names.

## Non-goals

- No automatic newline normalization.
- No non-UTF-8 source decoding policy.
- No requirement that every parser byte index implement LSP semantics.
- No provider-specific repair without a failing exact range round trip.
- No claim that constructor parity alone completes the strict indexed mapper in #7881.
