# Parser Limitations — compatibility pointer

This page is retained for older links, but its former resolved-issue inventory and aggregate coverage language were historical snapshots rather than current evidence.

Current parser behavior and boundaries are maintained in:

- [Parser status](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/status/parser.md);
- [Current status](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/CURRENT_STATUS.md);
- [perl-parser README](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/crates/perl-parser/README.md);
- [perl-parser-core README](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/crates/perl-parser-core/README.md);
- [Parser feature policy](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/features.toml).

Static parsing cannot establish runtime-generated source, compile-time side effects, dynamic symbol-table effects, or general semantic correctness without separate evidence. Do not turn those boundaries into a parser completeness percentage.

For a new limitation, attach a minimal source example, the parser subject and revision, the observed result, and the narrowest claim the evidence supports.
## Retired limitation anchors

The former limitation sections remain addressable for older links; their details are now maintained in the current parser status and feature-policy authorities above.

<a id="11-source-filters"></a>
### 11. Source filters

Use the current parser status and fixture evidence for this subject.

<a id="12-eval-string"></a>
### 12. eval STRING

Runtime compilation and execution remain outside static parser evidence.

<a id="13-dynamic-symbol-table-manipulation"></a>
### 13. Dynamic symbol-table manipulation

Runtime symbol-table mutation remains outside static parser evidence.

<a id="14-begin-block-side-effects"></a>
### 14. BEGIN block side effects

Compile-time execution effects remain outside static parser evidence.
