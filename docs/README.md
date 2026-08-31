# perl-lsp Documentation

Use this page as the short docs route map. It tells you where to go next
without making you learn the workspace layout first. For a broader curated
map of the docs tree, use [INDEX.md](INDEX.md).

## Repository context

This docs tree is checked in with the active development repository,
`EffortlessMetrics/perl-lsp-swarm` on `main`.

- Clone `perl-lsp-swarm`, open ordinary development issues here, and target pull
  requests here.
- `EffortlessMetrics/perl-lsp` on `master` owns public release lineage and
  published artifacts, so installation and release links may point there
  intentionally.
- A merge to `perl-lsp-swarm/main` is development state. It does not prove that
  the change has completed publication, entered a release, or reached any
  package or editor channel.

The current distinction is defined by
[product identity](reference/product-identity.md) and the
[publication sync protocol](swarm/sync-protocol.md). The landed
contributor-topology projection derives the repository, branch, and publication
relationships from local authorities without requiring network access. The bare
command intentionally leaves live stage and channel status `NOT_PROVEN`; a captured
`--observation` is required to project observed status:

```bash
cargo run --locked -p xtask --bin contributor-topology
```

## Diataxis in This Repository

When adding or moving docs, choose the content type first, then the file:

| Content intent | Place it under | Writing focus |
| --- | --- | --- |
| Teach by doing | `docs/tutorials/` | step-by-step learning journey |
| Solve a concrete task | `docs/how-to/` | shortest reliable path to an outcome |
| Describe the contract | `docs/reference/` | exact behavior, options, and constraints |
| Explain rationale | `docs/explanation/` | design tradeoffs and mental models |

If a doc starts mixing multiple intents, split it and cross-link the parts.

## Canonical Sources

| Topic | Source | Verified By |
| --- | --- | --- |
| Current release line | [`../Cargo.toml`](../Cargo.toml) | Workspace manifest |
| Status overview and subsystem map | [project/status/index.md](project/status/index.md) and its subsystem links | The index `Owner` column is a summary; follow each linked page or section's ownership and update instructions |
| Roadmap and active milestone | [project/ROADMAP.md](project/ROADMAP.md) | Human review |
| Distribution and install-channel matrix | [project/DISTRIBUTION_MATRIX.md](project/DISTRIBUTION_MATRIX.md) | Release status + channel receipts |
| Capability catalog | [`../features.toml`](../features.toml) | `just ci-gate` |
| Local validation flow | [project/CI_LOCAL_VALIDATION.md](project/CI_LOCAL_VALIDATION.md) | `just ci-gate` |

Rule: keep the project overview narrative in
[project/status/index.md](project/status/index.md). Its `Owner` column summarizes each
subsystem; it is not complete edit authority for every linked page or mixed-ownership
section. Follow the ownership, generator, and check instructions declared by the
specific linked page or section, and do not copy computed metrics into the index.

## Compatibility posture

The current product line is public beta, not stable or GA. Published crate compatibility follows [the stability policy](reference/STABILITY.md): patch releases preserve the public API, while pre-1.0 minor releases may contain intentional breaking changes with migration guidance. CLI flags, advertised LSP capabilities, DAP preview boundaries, and distribution-channel status are separate claims; verify each against its owning source before treating it as supported.


## Repository Map (Code + Docs)

Start here if you need to orient quickly before diving into a crate:

| Path | What lives here | When to go there |
| --- | --- | --- |
| `crates/perllsp/` | Thin top-level CLI binary (`perllsp`) | Entry point wiring, CLI flags, process startup |
| `crates/perl-lsp-rs/` | LSP server integration crate | User-visible LSP behavior, request plumbing |
| `crates/perl-lsp-rs-core/` | Shared LSP runtime, protocol, providers, governance | Core provider/runtime implementation |
| `crates/perl-dap/` | Debug Adapter Protocol server | Breakpoints, stepping, debugger transport |
| `crates/perl-parser/` | Native recursive-descent parser | Syntax parsing and parser behavior changes |
| `crates/perl-lexer/` | Context-aware tokenizer | Lexing/token stream changes |
| `crates/perl-parser-core/` | Parser shared infrastructure | Low-level parser utilities and common primitives |
| `crates/perl-semantic-analyzer/` | Scope/symbol resolution | Name resolution, cross-reference semantics |
| `crates/perl-workspace/` | Cross-file indexing and symbol lookup | Workspace search/refactor surfaces |
| `crates/tree-sitter-perl-c/` | C tree-sitter grammar binding | Compatibility for tree-sitter consumers |
| `crates/tree-sitter-perl-rs/` | Rust-native tree-sitter-style facade over v3 parser | Tree-sitter ergonomics on native parser stack |
| `docs/project/` | Status, roadmap, process, governance docs | "What is true now?" and "what ships next?" |
| `docs/reference/` | Contract-style reference docs | Command/config/API behavior lookups |
| `docs/how-to/`, `docs/tutorials/`, `docs/explanation/` | Task guides, walkthroughs, rationale | Learning and operational guidance |

For complete workspace membership and canonical crate/version truth, use [`../Cargo.toml`](../Cargo.toml).

## Common Routes

| If you need to... | Read this |
| --- | --- |
| get working fast | [tutorials/GETTING_STARTED.md](tutorials/GETTING_STARTED.md) |
| set up continuous testing | [how-to/CONTINUOUS_TESTING.md](how-to/CONTINUOUS_TESTING.md) |
| set up pre-commit hooks | [how-to/PRE_COMMIT.md](how-to/PRE_COMMIT.md) |
| install or upgrade | [how-to/INSTALLATION.md](how-to/INSTALLATION.md), [how-to/UPGRADING.md](how-to/UPGRADING.md) |
| set up `perllsp` in GitHub Actions | [how-to/GITHUB_ACTIONS.md](how-to/GITHUB_ACTIONS.md) |
| configure an editor | [how-to/EDITOR_SETUP.md](how-to/EDITOR_SETUP.md) |
| troubleshoot a broken setup | [how-to/TROUBLESHOOTING.md](how-to/TROUBLESHOOTING.md) |
| understand editor trust, fallbacks, and copyable receipts | [how-to/EDITOR_TRUST.md](how-to/EDITOR_TRUST.md) |
| review and improve public API documentation | [reference/MISSING_DOCUMENTATION_GUIDE.md](reference/MISSING_DOCUMENTATION_GUIDE.md) |
| learn API docs writing standards | [reference/API_DOCUMENTATION_STANDARDS.md](reference/API_DOCUMENTATION_STANDARDS.md) |
| choose the right Diátaxis doc type before writing | [reference/DOCUMENTATION_GUIDE.md](reference/DOCUMENTATION_GUIDE.md) |
| tune performance or threading | [how-to/PERFORMANCE_TUNING.md](how-to/PERFORMANCE_TUNING.md), [how-to/THREADING_CONFIGURATION_GUIDE.md](how-to/THREADING_CONFIGURATION_GUIDE.md) |
| work with DAP workflows | [tutorials/DAP_USER_GUIDE.md](tutorials/DAP_USER_GUIDE.md) |
| understand project architecture | [reference/ARCHITECTURE.md](reference/ARCHITECTURE.md) |
| understand measured editor trust and the long-term Rust Perl path | [explanation/MEASURED_PERL_EDITOR_TRUST.md](explanation/MEASURED_PERL_EDITOR_TRUST.md) |
| check known limitations and parser support | [reference/KNOWN_LIMITATIONS.md](reference/KNOWN_LIMITATIONS.md), [reference/PARSER_FEATURE_MATRIX.md](reference/PARSER_FEATURE_MATRIX.md) |
| see what is true now | [project/status/index.md](project/status/index.md) |
| see the current release plan | [project/ROADMAP.md](project/ROADMAP.md) |
| understand the compiler-backed LSP direction | [project/COMPILER_BACKED_LSP_ROADMAP.md](project/COMPILER_BACKED_LSP_ROADMAP.md) |
| inspect the workflow UX scorecard contract | [project/metrics/WORKFLOW_SCORECARDS.md](project/metrics/WORKFLOW_SCORECARDS.md), [reference/UX_TESTING.md](reference/UX_TESTING.md) |
| verify badges and PR evidence boundaries | [VERIFICATION.md](VERIFICATION.md) |
| work on the codebase | [../CONTRIBUTING.md](../CONTRIBUTING.md) |
| browse the curated docs map | [INDEX.md](INDEX.md) |
| classify or author docs by Diataxis type | [reference/DIATAXIS_GUIDE.md](reference/DIATAXIS_GUIDE.md) |

## Docs by Type

- Tutorials: [tutorials/GETTING_STARTED.md](tutorials/GETTING_STARTED.md), [tutorials/LSP_DEVELOPMENT_GUIDE.md](tutorials/LSP_DEVELOPMENT_GUIDE.md), [tutorials/DAP_USER_GUIDE.md](tutorials/DAP_USER_GUIDE.md), [tutorials/COMPREHENSIVE_TESTING_GUIDE.md](tutorials/COMPREHENSIVE_TESTING_GUIDE.md)
- How-to: [how-to/INSTALLATION.md](how-to/INSTALLATION.md), [how-to/GITHUB_ACTIONS.md](how-to/GITHUB_ACTIONS.md), [how-to/EDITOR_SETUP.md](how-to/EDITOR_SETUP.md), [how-to/TROUBLESHOOTING.md](how-to/TROUBLESHOOTING.md), [how-to/EDITOR_TRUST.md](how-to/EDITOR_TRUST.md), [how-to/CONTINUOUS_TESTING.md](how-to/CONTINUOUS_TESTING.md), [how-to/UPGRADING.md](how-to/UPGRADING.md), [how-to/PRE_COMMIT.md](how-to/PRE_COMMIT.md), [how-to/PERFORMANCE_TUNING.md](how-to/PERFORMANCE_TUNING.md), [how-to/THREADING_CONFIGURATION_GUIDE.md](how-to/THREADING_CONFIGURATION_GUIDE.md), [how-to/SECURITY_DEVELOPMENT_GUIDE.md](how-to/SECURITY_DEVELOPMENT_GUIDE.md)
- Reference: [reference/COMMANDS_REFERENCE.md](reference/COMMANDS_REFERENCE.md), [reference/CONFIG.md](reference/CONFIG.md), [reference/LSP_FEATURES.md](reference/LSP_FEATURES.md), [reference/ARCHITECTURE.md](reference/ARCHITECTURE.md), [reference/KNOWN_LIMITATIONS.md](reference/KNOWN_LIMITATIONS.md), [reference/PARSER_FEATURE_MATRIX.md](reference/PARSER_FEATURE_MATRIX.md), [reference/MISSING_DOCUMENTATION_GUIDE.md](reference/MISSING_DOCUMENTATION_GUIDE.md), [reference/API_DOCUMENTATION_STANDARDS.md](reference/API_DOCUMENTATION_STANDARDS.md), [reference/DIATAXIS_GUIDE.md](reference/DIATAXIS_GUIDE.md), [reference/DOCUMENTATION_GUIDE.md](reference/DOCUMENTATION_GUIDE.md), [reference/FAQ.md](reference/FAQ.md)
- Project, specs, and explanations: [INDEX.md](INDEX.md), [project/status/index.md](project/status/index.md), [project/ROADMAP.md](project/ROADMAP.md), [project/COMPILER_BACKED_LSP_ROADMAP.md](project/COMPILER_BACKED_LSP_ROADMAP.md), [project/CI.md](project/CI.md), [project/FEATURE_GOVERNANCE.md](project/FEATURE_GOVERNANCE.md), [explanation/MEASURED_PERL_EDITOR_TRUST.md](explanation/MEASURED_PERL_EDITOR_TRUST.md), [explanation/LSP_DOCUMENTATION.md](explanation/LSP_DOCUMENTATION.md)

## Maintenance

For the generated pages and sections owned by `cargo xtask update-status`:

```bash
nix develop -c just ci-gate
just status-update
just status-check
```

- Keep overview narrative in [project/status/index.md](project/status/index.md). For
  each linked subsystem page or section, follow its own human/generated ownership and
  its declared generation and verification commands; treat the index `Owner` column
  as a subsystem summary, not complete edit authority.
- Update [project/ROADMAP.md](project/ROADMAP.md) when the active milestone or release framing changes.
- Keep top-level summary docs short and link back to the canonical project docs.
- Keep each doc in the correct Diataxis category; prefer cross-links over hybrid docs that try to do everything.
