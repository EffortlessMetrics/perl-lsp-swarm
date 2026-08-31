# ADR-0048: LF-Delimited Source-Line Contract

- **Status**: Accepted
- **Date**: 2026-08-29
- **Decides**: #4973 (define one newline/BOM contract for exact UTF-8 source snapshots)
- **Implemented by**: #10574 / PR #12227 (`LineRecordTable`, `SOURCE_LINE_POLICY_ID`)
- **Constrains**: #8687 (legacy constructor reconciliation), #8707 (BOM ingress and source relations), #7881 (indexed strict LSP mapper), #8716 / #8259 (legacy API classification and recurrence block), #9526 (Tree-sitter source map), #10524 (DAP source geometry), #8657 (retire the Ropey-based incremental LSP converter)
- **Related**: [ADR-0013](0013-utf16-position-tracking.md) (UTF-16 column units — a distinct concern), #8172 (raw-byte fixtures), #9830 (strict reference disposition vocabulary), #2298 (position programme)

## Context

The repository indexes the same UTF-8 source bytes under more than one notion of
"line". Because parser, LSP, Tree-sitter compatibility, and DAP paths each reach
for whichever index is nearest, the same byte offset can receive different row
identities depending on which surface asked.

Measured on current `main` (`e78f2a2`), the row models actually present are:

| Surface | LF | CRLF | bare CR | VT / FF / NEL / LS / PS |
|---|---|---|---|---|
| `LineStartsCache::new` (`line_index.rs`) | breaks | breaks | **breaks** | content |
| `LineStartsCache::new_rope` (Ropey 1.6.1) | breaks | breaks | **breaks** | **breaks** |
| `LineIndex::new` (`perl-position-tracking`) | breaks | breaks | **breaks** | content |
| `PositionMapper::byte_to_lsp_pos` (`mapper.rs`) | breaks | breaks | **breaks** | **breaks** |
| `offset_to_utf16_line_col` (`convert.rs`) | breaks | breaks | content | content |
| `perl-line-index::LineIndex::new` | breaks | breaks | content | content |

That is **three** distinct models, not two. Ropey 1.6.1 resolves with default
features, which enable `unicode_lines`, so every Rope-backed row query silently
recognizes eight break forms. A `U+2028` inside a Perl string literal or comment
therefore shifts row identity for Rope-backed consumers and for nobody else.

`PositionMapper` matters most of these: it is the provider-facing mapper, and
`byte_to_lsp_pos` resolves rows with `Rope::byte_to_line`, so the Ropey model
reaches LSP positions that editors actually consume.

The divergence was not merely undetected — it was *masked*. The committed
property `prop_text_and_rope_offsets_agree`
(`tests/line_starts_cache_fuzz.rs`) asserts that `new` and `new_rope` produce
identical positions, and passes only because its generator never emits VT, FF,
NEL, LS, or PS. Adding `U+000B` to that corpus fails immediately:

```text
minimal failing input: content = "\u{b}", offset = 1
  left:  (0, 1)   # LineStartsCache::new  — VT is content
  right: (1, 0)   # new_rope              — VT breaks the row
```

This ADR records the ruling those surfaces must converge on. It does not itself
migrate any of them.

## Decision

For exact UTF-8 source snapshots consumed by the parser, LSP mapping,
Tree-sitter compatibility, DAP, and source-bound services:

```text
LF                 = the only logical source-line terminator
CRLF               = one two-byte separator whose LF terminates the row
bare CR            = ordinary source content
VT / FF / NEL      = ordinary source content
LS / PS            = ordinary source content
mixed LF / CRLF    = supported without normalization
```

Each row records three exact byte boundaries plus its separator kind:

```text
start_byte  <=  content_end_byte  <=  separator_end_byte  <=  source length
```

For `"abc\r\ndef"`:

```text
row 0: start 0, content_end 3, separator_end 5, CrLf
row 1: start 5, content_end 8, separator_end 8, None
```

For `"abc\rdef"` there is one row, and the bare CR is addressable content.

Further rules:

- exact source bytes are never silently normalized by a line index or mapper;
- empty source has one row starting at byte zero;
- a final LF creates a terminal empty row;
- separator-interior byte positions stay representable in source geometry, while
  protocol adapters apply their own strict validity policy;
- Rope may store, slice, and edit bytes, but `len_lines`, `line`, `line_to_byte`,
  and `byte_to_line` are **not** source-line authority unless a separately named
  internal domain explicitly adopts Ropey's Unicode-line semantics.

The ruling changes only if evidence establishes that an admitted product surface
must treat bare CR or the Unicode separator set as logical Perl source lines end
to end. Dependency behavior alone is not that evidence.

### Policy identity

The accepted policy is named `lf-source-lines/v1`, exposed as
`perl_position_tracking::SOURCE_LINE_POLICY_ID`. The identity travels with every
`LineRecordTable`, so a stored table can never be silently reinterpreted under a
different ruling. Changing the ruling changes this constant, which fails
`source_line_policy_authority.rs` and forces this ADR to be revisited.

### BOM is a source-subject decision, not a line-boundary case

1. Every mapper operates inside one exact source snapshot and reports positions
   in that subject.
2. An ingress may preserve or strip a **leading** decoded `U+FEFF` only under an
   explicit normalization policy.
3. Stripping creates a distinct source subject/revision with an explicit source
   relation and offset map to the original bytes.
4. Downstream consumers never add or subtract three bytes locally.
5. A non-leading `U+FEFF` is ordinary content.

LSP ingress already strips a leading editor-provided `U+FEFF` on some paths
(#5219), so "preserve BOM everywhere" is not current truth. #8707 owns the exact
ingress matrix and relation implementation. To the line table, a BOM is content.

### Coordinate domains stay distinct

The shared line table supplies source facts; it does not collapse units.

| Domain | Column unit | Adapter |
|---|---|---|
| source / parser byte geometry | UTF-8 bytes | parser and source consumers |
| Tree-sitter `Point` | UTF-8 bytes from row start | #9526 |
| LSP position | negotiated UTF-8 bytes or UTF-16 code units over row content | #9830 / #7881 |
| DAP source position | protocol line/column base over the same row | #10524 / #2300 |

No LSP UTF-16 type belongs in the canonical source-line record.

## Considered options

**Adopt Ropey's Unicode line model everywhere.** Rejected. It would make a
storage dependency's convenience semantics the product's architectural
definition of a Perl source line, and would require the string, byte-index, and
protocol paths to adopt eight break forms — changing row identity for text that
every other Perl tool treats as one line.

**Normalize source bytes at ingress (rewrite CR / CRLF to LF).** Rejected. It
destroys the exact-bytes property that parser spans, byte ranges, and edit
application depend on, and would force every consumer to map between original
and rewritten offsets anyway.

**Reject bare CR and the Unicode separators as invalid source.** Rejected. They
are valid UTF-8 and can legitimately appear inside Perl strings, comments, and
heredocs. Refusing them would fail files that Perl itself accepts.

**Leave each surface with its own model and document the differences.**
Rejected. This is the status quo that produced the masked property-test gap
above; documentation alone gives no recurrence gate.

## Consequences

True today, in the canonical `LineRecordTable` this ADR ratifies:

- one ruling now covers parser, LSP, Tree-sitter, and DAP without rewriting
  source, so a consumer built on this table has an unambiguous contract to hold;
- unusual but valid bytes stay inspectable content rather than being rejected or
  normalized away;
- CRLF has exact content/separator ownership, which strict protocol mapping and
  byte ranges both require.

True only once the migrations below land — **not** yet:

- Unicode separators inside strings and comments will stop shifting editor,
  debugger, parser, and Tree-sitter rows differently. **Today they still do** on
  the Rope-backed surfaces: a `U+2028` in an ordinary Perl string still moves a
  row for `PositionMapper` and `LineStartsCache::new_rope`, which
  `legacy_ropey_only_separator_divergence_is_pinned` asserts as current fact.

That distinction is deliberate. This ADR accepts a ruling and pins the gap
between it and the shipped legacy surfaces; it does not close that gap, and a
downstream consumer reading only the ruling would otherwise mis-predict current
row behavior on any Rope-backed path.

Costs and follow-up obligations:

- the legacy surfaces in the table above still implement the pre-ruling models.
  They are **not** migrated by this ADR. `source_line_policy_authority.rs` pins
  their current behavior so the divergence is executable and visible rather than
  hidden by a corpus gap;
- reconciling them is #8687, with classification and recurrence blocking in
  #8716 / #8259. Migrating a surface requires updating that pinned map in the
  same change, which is the intended review checkpoint;
- `prop_text_and_rope_offsets_agree` keeps its present corpus deliberately.
  Widening it to the five Ropey-only separators would turn a known,
  owner-assigned divergence into red `main`. The divergence is asserted
  explicitly instead;
- #9830's provisional bare-CR separator behavior is reconciled under this ruling
  by #8687, preserving its strict disposition vocabulary.

## Proof

- `crates/perl-position-tracking/src/source_lines.rs` — `LineRecordTable`,
  the canonical implementation of this contract (#10574 / PR #12227).
- `crates/perl-position-tracking/tests/source_lines_chunk_stability.rs` —
  contract rows under every chunk partition, including CR|LF splits.
- `crates/perl-position-tracking/tests/source_line_policy_authority.rs` —
  binds this ADR to the code: pins `SOURCE_LINE_POLICY_ID`, the decisive
  contract rows, and the exact legacy divergence map as a recurrence gate.

```bash
cargo test -p perl-position-tracking --locked --test source_line_policy_authority
cargo test -p perl-position-tracking --locked --test source_lines_chunk_stability
```

`--test <NAME>` selects the integration target. A bare positional argument is a
*filter on test-function names*, and since no function here contains its file's
name, the positional form runs zero tests and still exits `0` — green output
proving nothing.
