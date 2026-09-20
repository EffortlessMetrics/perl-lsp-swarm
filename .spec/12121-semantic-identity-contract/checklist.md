# SEM-ID-01 checklist

- [x] Closed versioned scope identity exists (`SemanticScopeIdentity` + schema tag).
- [x] Contribution/owner/dependency/fact-family identity vocabulary exists.
- [x] Identity binds exact source/parser/profile generation; never traversal order, names, offsets, pointers, or paths alone.
- [x] Scope, file/global, and source-order ownership remain distinct kinds.
- [x] Recovered/dynamic/unsupported/stale/instrument states explicit (`SemanticSubjectStatus`).
- [x] Source-identical generations, close/reopen, and multi-root subjects remain distinct (fixtures).
- [x] Deterministic fixtures prove unrelated source movement does not churn unaffected identities.
- [x] Architecture fence: no LSP/provider/parser/traversal-order types in the lower model.
- [x] No AST traversal, semantic output, edit-impact, incremental, provider, or release behavior changes.
