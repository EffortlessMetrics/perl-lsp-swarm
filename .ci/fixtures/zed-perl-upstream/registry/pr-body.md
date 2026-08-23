## Summary

Update the existing official `perl` extension entry to the merged
`tree-sitter-perl/zed-perl` release that adds EffortlessMetrics `perllsp` as a
separate provider while preserving Perl Navigator and tree-sitter-perl's
`perl-lsp`.

## Registry change

[BLOCKED: insert the exact merged upstream commit, manifest version, branch
reachability, and current registry base.]

The intended registry diff changes only:

```text
extensions/perl   submodule commit
extensions.toml   [perl].version
```

The extension ID remains `perl`, the path remains `extensions/perl`, and the
submodule remote remains the existing HTTPS upstream URL.

## Tested scope

[BLOCKED: insert the exact Zed host/platform receipt, public perllsp managed
target boundary, and companion Zed-default-selection state.]

## Validation

[BLOCKED: insert `pnpm sort-extensions` and current registry validation results,
plus the final diff digest.]

This registry update publishes metadata and a submodule commit. It does not by
itself prove a clean public-registry installation or runtime behavior; that
receipt remains separate.

> This body must not be submitted with a `[BLOCKED: ...]` marker remaining.
