# PERLLSP-ADR-0003: Impossible Perl is bounded, not silently claimed

Decision:

For Perl constructs requiring runtime execution or compile-time side effects,
perl-lsp will prove bounded parse/degradation rather than claiming complete
static correctness.

Consequence:

- source filters are never claimed as statically applied;
- regex code blocks are opaque unless a future rail explicitly parses them;
- runtime prototypes and BEGIN effects are semantic/runtime boundaries;
- fixtures assert no crash/no hang/preserved recoverable structure.
