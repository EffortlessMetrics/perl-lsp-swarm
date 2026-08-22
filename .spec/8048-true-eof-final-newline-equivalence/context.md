# Context: #8048 — true-EOF whole-document edits, exact final-newline policy

## Problem

Whole-document edit geometry and final-newline handling disagree across
production authorities, and the historical behavior corrupts documents:

```text
source:  "x;\n"
edit:    (0,0)..(0,2) -> replacement ending in "\n"
result:  doubled terminal newlines
```

`str::lines()` drops the trailing empty logical line, so `str::lines()`-derived
EOF ends at the last content character and leaves the original terminal
separator outside the replacement. Independently, native `FinalNewline::Insert`
strips all trailing CR/LF bytes and appends LF while `FinalNewline::Trim`
removes every final newline; the LSP projection strips LF bytes separately.
This splits CRLF pairs, leaves bare-CR residue, converts conventions, collapses
existing sequences under insert-only policy, removes the one final newline
under trim-only policy, and can bind evidence to an intermediate value.

## Governing ruling (issue #8048, 2026-08-20/21 reviews)

The complete production closeout is sequenced behind the byte-native train:

```text
#9283 → #10237 byte-native source/target/edit-plan contracts
      → #10239 native API/caller cutover
      → #10242 canonical generation-owned LSP projection
      → #8048 closeout → #7138 localized plans
```

A direct production patch that reuses current native/LSP DTOs does not satisfy
the issue. Sanctioned now, without production authority:

1. a pure terminal-sequence policy module consuming/returning exact bytes or
   strings without LSP positions;
2. independent application proof with typed rejection of reversed,
   out-of-bounds, overlapping, and mid-code-point edits, with distinct
   same-position insertions preserving the oracle's input order;
3. offline fixtures whose old `text.lines()` helper cannot distinguish true
   EOF, plus mutation controls for every historical false-pass path.

No other mapper or canonical edit DTO may be published while blocked.

## This bundle

- `crates/perl-lsp-perltidy/src/native/terminal_sequence.rs`: atomic
  LF/CRLF/bare-CR run analysis, the two independent LSP booleans as
  [`FinalNewlinePolicy`], sequence-aware apply, and evidence computed after the
  final bytes exist. Integration owner: #10239.
- `crates/perl-lsp-perltidy/src/native/edit_application.rs`: fallible
  independent applicator over `(line, character)` edits in UTF-16 or UTF-8-byte
  encodings; rejects invalid sets instead of clamping. Proof/receipt owner:
  #8048/#10239/#10242 adoption.
- `crates/perl-lsp-perltidy/tests/final_newline_policy_tests.rs` and
  `edit_application_equivalence_tests.rs`: the matrix, negative controls, and
  mutations bound row-by-row in `acceptance.md`.

Out of scope here: production wiring of any formatter route, byte-native
contract publication, mapper/DTO publication, #10220 endpoint convergence,
#7138 localized-edit algorithms, installed-product receipts.

## Sibling lane

PR #11873 corrects `perl-lsp-perltidy::TextRange::whole_document` to true EOF
and fixes one defect-pinning expectation. The remaining defect-pinning
expectation (`perl-lsp-rs/tests/lsp_formatting_tests.rs`, doubled final
newline) is coupled to that cutover and must be corrected by its lane or
immediately after merge; it cannot be green on this candidate's base.
