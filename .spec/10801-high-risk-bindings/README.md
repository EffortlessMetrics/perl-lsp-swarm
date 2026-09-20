# CA02B high-risk configuration bindings

Contract: [#10801](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10801).
This projection consumes the canonical configuration authority catalog and the
CA02A capability validator. It is not a second configuration catalog, accepted
settings store, source-authority grant, or runtime security fix.

`fixtures/configuration_authority/high_risk_bindings.v1.json` joins 59 canonical
high-risk fields, four removed runner settings, and the derived folder-scoped
`ProjectPerlConfig.version` field. Its explicit 19-ID remainder belongs to #10807.
The core check requires that active bindings and the remainder partition the real
catalog; a high-risk field cannot be relabeled low-risk to escape the check.

## Exact source evidence

Each Rust witness names a source file and production function and carries a
complete parsed expression. The checker compares syntax trees, excluding comments
and test modules. It also joins each active row's canonical storage member to an
actual writer and, where supported, consumer expression. Serde field witnesses
bind the derived project field to its generated parser/storage declaration.
Removed-runner evidence includes schema absence and the production parser's
absence of the removed key.

The TypeScript witnesses compare complete function bodies and key-to-property
tables using the existing
TypeScript 7 compiler API. They bind configuration reads to constructed payloads,
including machine-only external-root inspection and the generic AI transport.
A machine-scoped editor preference is not trusted server authority: the generic
AI channel remains rejected and #10817 owns trusted adapter admission.
The extension's local-only `perl-lsp.formatOnSave` is not evidence that the
server's separately stored `format_on_save` field drives an automatic trigger.

AST snapshots deliberately make changes to the selected expression reviewable.
They are source identity checks, not a Rust/TypeScript type checker, whole-program
dataflow proof, runtime containment test, or proof that a referenced issue landed.
Compiler/parser failure is missing evidence and fails the command.
Test-dependent attributes conservatively exclude expressions, local initializers
and let-else branches, match arms, and struct field values from production
witness traversal; this is not compiler-resolved conditional compilation.
Documented high-risk field headings require an existing binding. Known low-risk
headings must remain in the explicit remainder, and retired runner headings must
name a removed row and carry the explicit `(removed)` suffix.

## Unresolved obligations

Every result/currentness obligation is explicitly unresolved here. No row may
claim that a consumer read proves configuration-generation result identity.
#7943 owns the consumer effect/result graph, following #7057 generation assembly.
First-effect and
hard-envelope owners are likewise ownership evidence, not executed proof.
`pending:#<owner>:<canonical-id>` names a required future first-effect control;
it is not the name of an existing test or an execution receipt. The checker joins
that requirement's owner to the explicitly unresolved first-effect obligation.

- #16176 owns implementation/removal of six parsed limit fields without a proven
  LSP_LIMITS consumer: cache entry/TTL, symbol-cache entry, index-file and total
  symbol limits, and workspace scan deadline. The similarly named IndexLimits
  fields are a different owner and do not establish this connection.
- #7479 owns hard limits. This PR neither clamps nor rejects new runtime values.
- #16182 owns the distinct workspace resolution-timeout envelope; its real
  context-to-consumer chain is bound, but its bounds are not proved here.
- #10134 owns the rejected workspace interpreter/argv contract.
- #8092 owns the server save flag's consumer decision; #11955 owns save containment.
- #9072 owns retained legacy Critic migration state. Native accepted Critic policy
  does not acquire an external process merely because the old catalog lists a
  legacy consumer.
- #10132 owns the folder-scoped Perl target. It stays outside global authority.

## Proof commands

Use the repository Cargo wrapper where usable, the `agent` profile, two jobs, and
an isolated target directory. A broken local wrapper/cache is an instrument
failure; report a direct scoped Cargo fallback rather than editing the wrapper.

```text
cargo test -p xtask --bin high-risk-configuration-bindings --profile agent --locked
cargo run -p xtask --bin high-risk-configuration-bindings --profile agent --locked -- <repo-root> <typescript-package-root>
cargo test -p perl-lsp-rs-core --lib configuration_authority --profile agent --locked
cargo test -p perl-lsp-rs-core --lib configuration_observation --profile agent --locked
cargo test -p perl-lsp-rs-core --test perllsp_settings_schema_tests --profile agent --locked
node scripts/ci/check_high_risk_client_bindings.cjs <repo-root> <typescript-package-root>
```

The explicit TypeScript package path selects an existing TypeScript 7 install;
the checker does not install dependencies or silently substitute text matching.

Issue controls are split by claim: canonical source expansion and capability
envelopes use the real core validators; removed/derived/proof-owner mutations use
the projection model; parser/storage/consumer relocation uses AST fixtures;
schema-only additions use the published schema coverage check. The TypeScript
controls alter input keys, output values and function identity while preserving
the original text elsewhere. None performs an external effect.
