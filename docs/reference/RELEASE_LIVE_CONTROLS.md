# Release live controls observer

`cargo xtask release-live-controls --json --out <receipt.json>` observes GitHub
controls through read-only REST requests. The default repository subjects come
from product identity; their branches come from contributor topology. Explicit
`--repository owner/name` subjects default to `main`; `--branch` overrides the
branch for all subjects.

The contract is `schemas/release_live_controls.v1.schema.json`. Earlier development
snapshots missing identity, reviewer, deployment-policy, binding, or
owner-enforcement fields must be regenerated; they are not a supported legacy
contract. No missing field is backfilled with a conclusive value. An API-null
deployment branch policy is recorded as `ABSENT` (explicitly unrestricted),
which survives JSON serialization and snapshot replay.

The observer records repository and branch identity, classic protection, branch
and tag rulesets, environment restrictions, reviewer identities, named deployment
branch/tag policies, installed custom deployment-protection Apps, secret counts,
and immutable-release settings. Immutability and owner enforcement come from
`GET /repos/{owner}/{repo}/immutable-releases`, not the repository identity body.
Named environment policies are read only when custom policies are enabled.
Installed custom rules use the separate `deployment_protection_rules` endpoint.
An unreadable endpoint remains `NOT_PROVEN`; it is never an empty roster.

Required checks retain their App bindings in each source plane. The derived
required-context union contains names and source labels; it does not reconcile
App bindings or evaluate whether an individual check run satisfies enforcement.
Ruleset checks with omitted or null `integration_id` remain `NOT_PROVEN`: the
pinned GET contract does not establish the check origin from an omission.
Classic checks separately support the API's explicit `app_id: null` form.
Known inactive rulesets do not contribute checks. Active rulesets with unknown
applicability or unknown rules keep the union inconclusive when the unknown
could change its result.

The ref matcher implements the documented Ruby `File.fnmatch` pathname subset:
segment `*`, character `?`, bracket classes/ranges, and recursive whole `**/`
components. Unsupported syntax and inputs exceeding 1,024 characters remain
unknown. A decisive exclusion can still establish non-applicability. The
reference oracle is Ruby 3.2.3 with `File::FNM_PATHNAME`; terminal `**` does not
cross path separators.

The modeled ruleset families are required status checks, pull requests, and
parameterless creation, deletion, update, linear-history, signing, and
non-fast-forward rules. Unsupported rule families or optional parameters are
reported as `NOT_PROVEN`. This is an observation of modeled controls, not a
complete policy interpreter or a release-readiness verdict. API shapes are
checked against the [pinned GitHub REST OpenAPI description](https://github.com/github/rest-api-description/blob/3cef12e8a02d612ad032473d4fb87266f2befeae/descriptions/api.github.com/api.github.com.json).

Receipts omit secret names/values, App URLs, and raw API failure bodies. Human
output includes limitations as well as the overall verdict. A replay is always
marked `SNAPSHOT`; it does not establish current GitHub state.

Focused proof: `cargo test -p xtask --test release_live_controls --locked -j1`.
The mocked read surface rejects unstubbed routes and tests conclusive controls,
malformed or missing fields, identity contradictions, pagination, bindings,
pattern matching, redaction, and schema/serde replay.
