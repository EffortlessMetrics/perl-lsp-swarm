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

Classic protection currently declares no explicit GitHub Actions app bindings:
the proof-floor contexts `Perl LSP Rust Small Result` and `ripr+ New Gap Gate`
moved to ruleset enforcement (#12678), so their `classic_app_id` entries were
removed together with the classic required checks. The two migrated rows keep
their GitHub Actions origin binding as `ruleset_integration_id = 15368`,
matching the live ruleset entries' `integration_id`, so no foreign check-
writing integration can satisfy them.

The remaining ruleset-required contexts stay intentionally unbound in static
policy because the live ruleset does not declare an integration id for them:

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

## Optional reporter and coverage declaration

A `[[checks]]` row may additionally declare `reporter_binding`. This is separate
from the classic/ruleset enforcement bindings. No current production row is
populated by this schema change.

```toml
[checks.reporter_binding]
kind = "check-run"
app_id = 12345
obligation_ids = ["example-obligation-a", "example-obligation-b"]
coverage_complete = true
```

These values illustrate syntax, not a production mapping. A legacy status instead
uses `kind = "legacy-status"` and `creator_id`; it cannot borrow an App ID or be a
repository-job declaration. The row's existing context name identifies the
reporter's context. A check-run's explicit App must agree with any declared
classic/ruleset App binding. Neither producer identity nor enforcement binding
is inferred when absent.

The table requires exactly its kind, matching positive integer identity,
nonempty unique exact obligation IDs, and boolean `coverage_complete = true`.
Missing fields, competing identity fields, unknown fields, partial coverage and
conflicting declared Apps block the static contract. TOML duplicate keys also
refuse. Obligation order is normalized in the semantic subject; identity,
coverage membership and completeness remain digest-covered. Existing rows with
no table retain their old static result and acquire no binding.

This validates a declaration, not the truth of its complete coverage. The actual
selector must independently establish the full aggregate denominator, and the
native owner must authenticate the observed check/App or context/creator and
bind it to the subject, policy and selection. A successful static contract is
neither that join nor qualification eligibility. Missing bindings and incomplete
join evidence remain `NOT_PROVEN`; an observed adverse result stays visible.
No native applicability consumer, reporter authentication, producer activation,
positive exclusion or selection-to-reporter join is installed here.
