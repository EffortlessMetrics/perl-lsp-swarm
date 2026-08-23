# Gate enforcement app bindings

The Gate Enforcement Contract keeps three identities separate:

1. **Producer identity** names the workflow job or external integration that emits a status context.
2. **Classic app identity** names the GitHub App to which classic branch protection binds that context.
3. **Ruleset integration identity** names the integration to which a ruleset binds that context.

These facts may coincide, but none is inferred from another. A repository-owned job does not, by itself, prove that classic protection or a ruleset requires the context from GitHub Actions.

## Static policy fields

A `[[checks]]` row may declare either source-specific binding:

```toml
classic_app_id = 15368
ruleset_integration_id = 15368
```

`classic_app_id` is valid only when `enforcement` includes `github-branch-protection`. `ruleset_integration_id` is valid only when `enforcement` includes `github-ruleset`. Each present value must be a positive integer; booleans, strings, floats, null, zero, and negative values fail the static contract.

Absence is meaningful. It means the checked-in policy does not require or prove an app binding for that enforcement source. The validator does not synthesize an identity from `producer`, workflow path, job id, or context name.

## Current checked-in state

Classic protection currently declares explicit GitHub Actions app bindings for:

```text
Perl LSP Rust Small Result  classic_app_id = 15368
ripr+ New Gap Gate          classic_app_id = 15368
```

The current ruleset-required contexts remain intentionally unbound in static policy because the live ruleset does not declare an integration id:

```text
Compile All Targets (bit-rot guard)
Conflict marker check
validate-title
```

That distinction prevents the live-union reconciler from turning a repository-job producer into a fabricated ruleset integration requirement.

## Receipt behavior

Both optional fields are retained in `subjects.contexts` and therefore participate in the schema-v2 `subject_sha256`. Changing a declared binding changes the exact static subject consumed by downstream reconciliation.

This contract remains static and read-only. It does not query GitHub, collect live rulesets or branch protection, evaluate check conclusions, mutate repository settings, or authorize promotion. Issue #9152 consumes the explicit binding fields; issue #9154 owns trusted live observation.

## Closed row vocabulary

The `[[checks]]` table is a closed contract. Unknown fields—including likely binding aliases such as `app_id`, `classic_appid`, and `ruleset_app_id`—block validation with the context and offending key named. Canonicalization never turns malformed intent into an honestly absent field.

## Semantic identity and exact-source attestation

The receipt retains two different digests:

- `subject_sha256` hashes the versioned canonical policy meaning: policy identity plus name-sorted canonical contexts. Reordering `[[checks]]` tables without changing their meaning leaves this digest unchanged.
- `exact_source_sha256` hashes the exact repository, raw policy-file, and workflow-file attestations. Byte movement, including an order-only policy edit, remains visible here.

Binding, enforcement-source, role, applicability, producer, workflow, job, result, or event movement changes the semantic subject. Raw file or repository movement cannot masquerade as semantic movement, and semantic normalization does not discard exact-source evidence.
