# Source-aware angle terms

Issue [#16224](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/16224)
governs the shared scanner used by parser primary angle terms. Repetition admission
remains the separate responsibility of #13930.

The parser consumes the actual `<` token through its ordinary token-budget seam,
then requests `ContextualTokenOp::ScanAngleBody`. The live token stream restores
its captured post-opener boundary before scanning and clears obsolete lookahead.
The body is source text, not a concatenation of ordinary Perl tokens. Punctuation,
spaces, `#`, Unicode, and escaped delimiters therefore retain their original byte
geometry. `Glob.pattern` retains the raw body, including escapes; it is not the
evaluated argument of Perl's runtime `glob` operation.

An unescaped `>` closes the term. LF ends an unsuccessful closure search, including
LF after a backslash; bare CR remains body content. For a proven missing closer at
LF/EOF, the scanner resumes at the first recorded unescaped semicolon, comma, or
closing caller delimiter, if present. A valid closer takes precedence over all
such punctuation. The malformed diagnostic retains both opener and recovery
offsets. These are parser recovery semantics, not acceptance of malformed Perl.

## Work and checkpoint policy

`LexerConfig.max_angle_scan_bytes` and `max_angle_scan_steps` each default to
16,777,216. They are independent, finite source-operation limits without an
unlimited sentinel. One step begins inspection of one Unicode scalar; byte work
accounts for its UTF-8 extent. The scanner charges the leading byte before reading
it, derives width from that charged byte under the valid-UTF-8 input invariant,
and reserves the remaining width before crossing it. A refusal can therefore
report a partially inspected scalar while retaining a valid source anchor.

Closer checks, escape lookahead, and unsuccessful recovery searches share these
cumulative counters. Re-inspection costs work again. No body allocation occurs
during closure search. Exhaustion terminates the lexer and produces a typed
`AngleBudgetExhausted` with dimension, threshold, spent work, and refusal offset;
the parser preserves that diagnostic and reports `LexerBudgetExhausted` as its
terminal stop cause. Cached lookahead cannot escape that terminal boundary.

Both limits participate in `LexerPolicyIdentity`. Checkpoint schema 2 includes
spent counters. Fresh restored continuations inherit spent work; restoration into
an already-used lexer takes the maximum of current and captured counters. Neither
checkpoint restore nor `reset()` refunds work. Constructing a fresh lexer begins
a new source operation. Failed source/policy/schema validation mutates nothing.

Buffered token streams without a real boundary checkpoint return the existing
`ContextualFallbackReason`. The parser exposes `AngleContextFallback` in its
diagnostic and stop cause, classified as transient rather than invalid source.
A caller can rebuild from the exact source through `Parser::new`. This change
does not claim an automatic incremental rebuild policy.

## Compatibility and proof

This pre-1.0 change adds two public `LexerConfig` fields. Exhaustive struct literals
must supply them or use `..LexerConfig::default()`. `LexerError`,
`ContextualTokenOp`, and `ContextualOpResult` gain variants; exhaustive matches
must account for them. `ContextualOpResult` retains `Clone` but no longer implements
`Copy`, because it can retain a typed lexer error. `ParseError` and `ParseStopCause`
were already non-exhaustive; their new variants are additive, with changed runtime
diagnostic behavior. Schema-1 checkpoints are deliberately refused.

Focused proof lives in `perl-lexer/tests/angle_scan_contract.rs`,
`perl-parser-core/tests/angle_scanner_contract.rs`, and the core parser's
`angle_resource_tests` module. It covers source geometry, independent budget units,
cumulative malformed work, checkpoint identity/no-refund behavior, typed resource
propagation, exact offset remapping, and buffered rebuild refusal. Real Perl 5.44
is the language oracle for the punctuation, space, escape, and newline witnesses.
Neither these tests nor the scanner execute glob patterns or establish editor
support, whole-language correctness, or repetition admission.

## Filehandle classification boundary

The shared direct-primary classifier recognizes bare and simple scalar handles,
including lowercase names, leading ASCII digits, paired `::` separators and Unicode
identifier runs. Its branch order follows Perl 5.44 `S_scan_inputsymbol` and
`parse_ident`: Unicode start/continuation runs precede the ASCII word fallback.
The resolved `unicode-ident` Unicode 17 tables supply XID predicates. Perl intersects
these with Word; private finite exclusions reproduce that intersection. A test pins
the Unicode version so upgrades require regeneration. No Unicode 16 Word table is
mixed into this calculation. The
lexer’s broader emoji/apostrophe identifier helper is deliberately not reused.
Raw spelling is retained. Literal spaces, dotted names, a single colon and glob
punctuation remain patterns. UTF-8 source-file oracle controls include lambda,
qualified lambda/beta, the Unicode 17 addition U+088F and combining continuation, with emoji and leading combining
marks as opposites.

This parser does not derive effective `utf8` or apostrophe-separator feature policy
from a local pragma heuristic. Default Perl accepts an apostrophe-qualified handle,
whereas `use v5.44` treats that spelling as a glob. That feature-dependent admission
remains with the canonical effective-feature work (#8922); this change does not
claim complete feature-sensitive angle classification.

### Reproducing the Unicode 17 intersection

Perl v5.44.0 `lib/unicore/mktables` defines `_Perl_IDStart` as
`(XID_Start + underscore) & Word`, and `_Perl_IDCont` as `XID_Continue & Word`.
The following maintenance-only Python procedure derives the complete finite
exclusions from upstream UCD 17.0.0, independently of parser output. Ordinary
Rust tests are offline; this is not a test-time network dependency.

```python
import hashlib, urllib.request
files = {
    "DerivedCoreProperties.txt": "24c7fed1195c482faaefd5c1e7eb821c5ee1fb6de07ecdbaa64b56a99da22c08",
    "extracted/DerivedGeneralCategory.txt": "d62e5bab70ca74f099343f71224fa051cb1fdd61a1ab45c0488c44cfc0b6102e",
    "PropList.txt": "130dcddcaadaf071008bdfce1e7743e04fdfbc910886f017d9f9ac931d8c64dd",
}
tables = []
for path, digest in files.items():
    data = urllib.request.urlopen("https://www.unicode.org/Public/17.0.0/ucd/" + path).read()
    if hashlib.sha256(data).hexdigest() != digest:
        raise ValueError("unexpected UCD input identity")
    table = {}
    for line in data.decode("utf-8").splitlines():
        fields = line.split("#", 1)[0].strip().split(";")
        if len(fields) < 2:
            continue
        bounds = fields[0].strip().split("..")
        lo, hi = int(bounds[0], 16), int(bounds[-1], 16)
        table.setdefault(fields[1].strip(), set()).update(range(lo, hi + 1))
    tables.append(table)
d, g, p = tables
word = d["Alphabetic"] | p["Join_Control"]
for category in ("Mn", "Mc", "Me", "Nd", "Pc"):
    word |= g[category]
for prop in ("XID_Start", "XID_Continue"):
    print(prop, " ".join(f"{cp:04X}" for cp in sorted(d[prop] - word)))
```

Expected start exclusions: `2118 212E`. Expected continuation exclusions:
`00B7 0387 1369 136A 136B 136C 136D 136E 136F 1370 1371 19DA 2118 212E 30FB FF65`.
The exact property-data hashes and independent full-range enumeration support the
finite exclusions; sampled Perl deparse witnesses alone would not establish them.
