# PERLLSP-SPEC-0008: Parser fixture bank

## Contract

Track B parser fixtures live under:

```text
crates/perl-parser-comparison/fixtures/
```

## Layout

```text
fixtures/
  README.md
  ga-coverage/
  nodekind/
  slash/
  heredoc/
  hash-vs-block/
  quote-like/
  indirect-object/
  regex/
  source-filter/
  unicode/
  boundedness/
```

## Assertion classes

```text
must_not_crash
must_not_hang
must_not_have_error_nodes
may_have_error_nodes
must_preserve_text
must_contain_any
must_not_contain
must_have_node_kind
must_have_diagnostic_kind
allowed_verdicts
max_duration_ms
max_ast_nodes
```
