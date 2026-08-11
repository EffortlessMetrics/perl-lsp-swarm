# Provider Fact Read Inventory

> Generated from `policy/provider-fact-reads.toml`. This inventory records
> current provider fact reads, ownership assumptions, and duplicate interpretation
> seams. It does not change provider behavior, promote a producer, or authorize edits.

Owner: [#6815](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6815)

## Coverage

| Provider | Inventoried reads |
| --- | ---: |
| `completion` | 3 |
| `definition` | 1 |
| `references` | 1 |
| `hover` | 1 |
| `diagnostics` | 1 |
| `rename` | 1 |
| `safe_delete` | 1 |
| `workspace_symbols` | 1 |
| `document_symbols` | 1 |
| `semantic_tokens` | 1 |

## Reads

| ID | Provider | Request | Query / fact need | Current producer | Proof assumption | Readiness / freshness | Fallback / refusal | Duplicate interpretation seam | Migration |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `completion.current_document_snapshot` | `completion` | `textDocument/completion` | current document AST, lexical declarations, and open-buffer source | `current_document` | `mixed` | document generation and parse availability | bounded legacy completion when current exact facts are unavailable | completion composes document syntax directly instead of one provider query port | `port_candidate` → #6816/#6817 |
| `completion.workspace_candidates` | `completion` | `textDocument/completion` | workspace declarations, packages, modules, imports, and generated candidates | `workspace_index` | `mixed` | index readiness and workspace root | local and imported candidates remain available after bounded readiness handling | completion reads and ranks workspace candidates inside the request handler | `port_candidate` → #6816/#6817 |
| `completion.receiver_shadow` | `completion` | `textDocument/completion` | receiver visibility and source-backed completion shadow facts | `semantic_shadow` | `shadow` | current document and shadow budget | live completion remains authoritative | shadow comparison is completion-specific rather than using a shared provider port | `retire_after_parity` → #6818 |
| `definition.navigation_candidates` | `definition` | `textDocument/definition` | exact, imported, module, generated, and fallback definition candidates | `runtime_mixed` | `mixed` | document generation, root, and index readiness | legacy navigation for uncertain or unavailable facts | navigation composes several candidate sources and exactness rules locally | `port_candidate` → #6816/#6817 |
| `references.reference_candidates` | `references` | `textDocument/references` | lexical, package, imported, workspace, and textual reference occurrences | `runtime_mixed` | `mixed` | document generation, include-declaration mode, root, and index readiness | legacy scan cascade for unpromoted reference classes | references owns a separate occurrence and fallback composition path | `port_candidate` → #6816/#6817 |
| `hover.semantic_and_fallback` | `hover` | `textDocument/hover` | source-backed, imported, generated, dynamic, documentation, and fallback hover facts | `runtime_mixed` | `mixed` | document generation and available semantic snapshot | legacy hover and search fallback when exact facts are unavailable | hover reconstructs provenance and fallback from several local stores | `port_candidate` → #6816/#6817 |
| `diagnostics.parser_semantic` | `diagnostics` | `textDocument/publishDiagnostics` | parser, semantic analyzer, critic, and trust-boundary diagnostic facts | `runtime_mixed` | `mixed` | accepted document generation and diagnostic scheduling state | partial or delayed diagnostics preserve current source authority | diagnostic producers and suppression evidence are joined inside runtime publication | `intentional_provider_policy` → #4799 |
| `rename.lexical_and_workspace_plan` | `rename` | `textDocument/rename` | current lexical declaration edits and guarded package or workspace rename plans | `runtime_mixed` | `edit_authorizing` | current document generation, workspace readiness, and source guard | no edit or legacy fallback when exact complete proof is absent | rename combines lexical AST proof, workspace facts, and edit guards locally | `port_candidate` → #6816/#6819 |
| `safe_delete.semantic_plan` | `safe_delete` | `perl/safeDelete` | semantic safe-delete plan, blockers, and shadow comparison | `semantic_queries` | `edit_authorizing` | entity identity and current semantic query state | block deletion when the plan is unavailable, dynamic, ambiguous, or blocked | safe delete has a dedicated semantic query and receipt path outside a common provider port | `port_candidate` → #6816/#6819 |
| `workspace_symbols.index_query` | `workspace_symbols` | `workspace/symbol` | workspace declarations and package symbols | `workspace_index` | `mixed` | workspace root, query generation, and index readiness | bounded partial workspace result or explicit not-ready outcome | workspace symbols query the live index through provider-specific shaping | `port_candidate` → #6816/#6817 |
| `document_symbols.current_snapshot` | `document_symbols` | `textDocument/documentSymbol` | current document declarations, packages, generated labels, and hierarchy | `current_document` | `mixed` | current document generation and parse snapshot | bounded syntax-only result for recovered or partial source | document symbols build a provider-specific projection from AST and semantic helpers | `port_candidate` → #6816/#6817 |
| `semantic_tokens.current_snapshot` | `semantic_tokens` | `textDocument/semanticTokens/full` | current AST, semantic analyzer, reviewed compiler-token classes, and fallback token facts | `runtime_mixed` | `mixed` | current document generation and semantic-token cache identity | current syntax tokens remain authoritative for unpromoted classes | semantic tokens compose syntax, semantic, and reviewed compiler classes locally | `intentional_provider_policy` → #2035 |

## Claim boundary

- A producer name is not a proof or safety class.
- An inventory row is not a cutover decision.
- `port_candidate` means the read should move behind the canonical provider port.
- `intentional_provider_policy` means domain policy may remain provider-owned after shared facts arrive.
- `retire_after_parity` requires request-bound comparison evidence before removal.
- Generated, dynamic, stale, partial, ambiguous, or low-confidence facts do not gain edit authority from this inventory.
