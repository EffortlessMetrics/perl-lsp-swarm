# Remaining dormant fixtures

## Issue

[#6441](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6441)

## Scope

Measure the 32 fixtures that remained dormant after #6612: declarations and
boundaries (7), span geometry (10), error recovery (10), and
providers/negative/incremental (5). Manifest fixture coverage goes 18 → 50 of 52.

All 32 report `strict_parse_ok: true`, including every deliberately malformed
one, so the existing harness is sufficient and no recovery mode was needed. An
earlier note on #6441 claiming otherwise was corrected before this work.

`Signature`, `MandatoryParameter`, `Do`, `StatementModifier`, and `Eval` join
the metrics scored-node set. Each is the only node naming the construct its
fixture exists to guard, and none has a scored substitute.

## Gold-label completeness

Adding a scored kind is not free. `score_manifest_ast` collects every scored
node on a line carrying any expectation, and an unmatched prediction counts as a
false positive — so an unlabelled node of a newly scored kind degrades precision
against correct parser output.

Measured against `origin/main` as baseline: main carries 5 unlabelled scored
nodes on scored lines (all in `slash_ambiguity`); this candidate introduces 0.
Twenty-one labels were added to reach that, found by sweeping every scored line
rather than by inspection. The 5 on main are untouched — they predate this work.

## Claim boundary

This measures the AST the parser produces for these source shapes. It does not
establish signature semantics, format output, postfix-deref resolution, eval
boundaries, AUTOLOAD dispatch, export behavior, incremental reparse, or any LSP
provider behavior — only parse shape.

Two limits are load-bearing and stated rather than papered over:

**The span-geometry fixtures do not test what their names claim.** `.gitattributes`
declares `* text eol=lf` with a single exemption for `span_coordinates.pl`, so
`span_crlf.pl`, `span_mixed_newlines.pl`, `span_cross_line.pl`, `span_tabs.pl`,
`span_empty_at_eof.pl`, `span_utf8_multibyte.pl`, and `span_emoji.pl` contain
**zero CR bytes** in the committed blob. Their expectations here prove LF line
arithmetic only. `span_coordinates` is the sole fixture in the group that
genuinely exercises BOM, CR, tab, emoji, and accented bytes. Reported in #6630.

**Three recovery fixtures assert current behavior, not desired behavior.**
`unclosed_quote_like_operator`, `partial_sub_body`, and
`nested_malformed_delimiters` produce no `Error` or `UnknownRest` node at all —
the parser accepts the malformed input with a clean tree. `malformed_heredoc_recovery`
is named for recovery that does not happen: its `UnknownRest` swallows the
well-formed subroutine that follows. These expectations pin what the parser does
today so the behavior is visible and any change is deliberate; they are not an
endorsement. Reported in #6629.

## Observations recorded, not asserted

No expectation here depends on the span defects in #6565 — empty
`ExpressionStatement` spans, `VariableDeclaration` extents that exclude a literal
initializer, or `qw` element spans covering the whole literal. Where a node was
the only carrier of a fixture's claim, a substring valid under either extent was
used instead.
