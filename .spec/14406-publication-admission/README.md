# #14406 publication-admission law — retained-carrier preparation

Status: first pure leaf authorized by root after review of spec bcfcc57c; implementation remains nonqualifying. This is
preparation in the retained draft #14479 carrier, not a standalone mergeable
partial graph. It advances #14406 and closes no release prerequisite.

Current #16762 recovery supersedes the historical sequencing prohibition below:
normal c5c6c72 main integration is admitted and in progress. The current slice is
private acquisition/execution-boundary repair, not completed canonical producer
integration or release qualification. Historical leaf decisions remain preserved.

## Authorities and reconstruction boundary

- [Accepted admission decision](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/14406#issuecomment-5599441910).
- [Complete reconstruction ruling](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14479#issuecomment-5706715736).
- [RC ceiling and release gates](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/13768).
- [Owned recovery](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/14406#issuecomment-5751171483).
- #14923 owns complete required-VSIX composition; #9422/#8576 own executed harmless
  rejection and public-mutation observations; #6055 consumes concrete integrity leaves.

Retain ancestry at 2f7faa71c4e1ad31aa8f4ba752e50638635466b2. Before execution remove
`.github/workflows/finalize-release-prerequisite-14406.yml` and
`.ci/finalize-release-prerequisite-14406.trigger`. Do not run the old graph or
normally merge main yet. The subsequent coherent rewrite must build and validate
all selected private candidate bytes, emit one terminal immutable manifest, obtain
one common eligibility decision, and only then reach any public mutation. Existing
tag-before-build, public-download reconstruction and lexical graph checks are not
acceptable authority. Main integration follows that coherent rewrite.

Source interfaces below were inspected at live main
`46588022115d4bec71c7db544323528852b5ffb9`; they are not assumed present in this old
carrier. Import/integration of their code is a later explicitly reviewed step.

## First concrete leaf and proposed interfaces

Implement pure permission derivation and exact-subject joins, with no subprocess,
network, filesystem mutation, dispatch or publisher entry point. Proposed module:
`scripts/ci/release_publication_admission.py`; focused tests beside it. It returns a
validated derived selection or a structured refusal, never release-ready or eligible.

Separate candidate material from independently supplied expected context:

1. `derive_selection(policy, profile_id, prerelease, open_vsx_evidence)` derives
   the permission ceiling. Unknown profile refuses. Missing Open VSX adapter proof
   cannot activate that channel. No caller channel list or skip boolean grants
   permission. Until its concrete accepted evidence adapter exists, requesting
   Open VSX refuses; omission retains explicit deferral, not proof of satisfaction.
2. `validate_subject(candidate, expected, selection)` compares each load-bearing
   exact field and returns the bound selection. Both are closed typed data, not
   dictionaries carrying arbitrary status assertions. This pure function proves
   equality/permission laws only; it cannot authenticate its expected argument.
3. A future trusted adapter obtains policy bytes from the independently pinned
   workflow revision and binds their SHA-256 before parsing. Candidate-selected
   checkout/path/revision is never a trusted-policy source. Workflow integration
   must supply expected producer/artifact identities from actual platform context.
   No public `native=true` switch, CLI mode or serialized provenance claim confers
   trust. This leaf introduces no executable adapter pretending that it does.

Proposed closed schema ID: `perl_lsp.release_publication_admission.v1`. Exact object
keys for a derived packet are `schema_version`, `subject`, `policy`, `producer`,
`artifact`, `input_packet_sha256`, `terminal_manifest_sha256`, `channels`.

- `subject`: `source_sha`, `prepared_sha`, `tag`, `version`, `prerelease`,
  `topology_sha256`. Source and prepared commits remain distinct; no implicit
  fallback joins them. Their actual topology correspondence is validator-owned.
- `policy`: `repository`, `workflow_revision`, `sha256`, `profile_id`.
- `producer`: `repository`, `workflow_id`, `ref`, `head_sha`, `run_id`, `run_attempt`.
- `artifact`: `id`, `sha256`. Name alone cannot select an artifact; platform artifact
  digest is not silently treated as the digest of an unpacked manifest.
- `channels`: deterministically ordered derived channel identifiers, unique; no
  absent/default/unknown channel admits permission. Use canonical topology channel
  spellings, not aliases or a replacement inventory.

Commit IDs are lowercase 40-hex, digests lowercase 64-hex, numeric identities are
positive integers excluding booleans. Reject duplicate/unknown keys, malformed
UTF-8, nonfinite numbers and unknown schema/profile. Version/tag correspondence
uses the actual admitted topology and immutable-tag authority; do not invent a
second semver/RC mapping implementation. Hash exact captured bytes and compare
those same bytes throughout the adapter; equal hashes do not establish provenance.

Proposed checked-in policy path: `policy/release-publication-admission.json`, closed
keys `schema_version`, `profiles`. The initial named profile `public-beta-rc` has
closed keys `required`, `conditional`, `forbidden`, `requires_prerelease`.
`github_release` is required; `open_vsx` conditional; `crates_io`,
`vscode_marketplace`, `docker`, `homebrew` forbidden in this initial selection.
`requires_prerelease` is true. Stable/default publication refuses independently of
legacy skip flags. Root must confirm this explicit encoding against #13768 before
population; it is permission policy, never a topology inventory rewrite.

## Existing concrete adapters and gaps

- `scripts/generate_release_topology.py::validate_manifest(manifest, root,
  expected_sha=None, expected_prepared_sha=None, frozen_topology=None,
  frozen_topology_digest=None, frozen_topology_path=None, frozen_root=None)` owns
  canonical topology admission. Consume its supported v1/v2/v3 schemas in
  `schemas/release_topology.v{1,2,3}.schema.json`; do not reduce admission to digest
  equality or reinterpret `primary_channels` as permission. #14923's future
  mapped topology remains a dependency, not copied branch-private authority.
- `scripts/release_terminal_manifest.py::build_manifest(candidate, source_sha, tag)`
  validates actual private archive/checksum/SBOM/build-evidence material.
  `check_outputs(candidate, source_sha, tag)` compares exact canonical manifest and
  attestation inventory bytes, returning their paths. Its schema is
  `perl_lsp.release_terminal_manifest.v1`, with source_sha/tag/version and
  release_topology_sha256. Its existing `status: eligible` is candidate structural
  completeness, not permission, authenticated producer provenance or full #14923
  VSIX acceptance. This leaf never promotes that status.
- `scripts/release_tag_authority.py` owns immutable tag authority. Policy validation
  does not create or resolve a mutable tag as a substitute for the expected SHA.
- Retained `scripts/ci/release_workflow_gate.py::RunIdentity`,
  `select_new_exact_run(... prior_ids, expected_sha, dispatch_started)` and
  `validate_terminal_run(... expected_sha, expected_workflow_id)` are useful
  external checks, but SHA/time correlation is insufficient. Exact input packet,
  topology, channel selection, manifest, run/attempt and artifact joins remain
  mandatory. Preserve rejection of nonterminal/non-success external producers.
- Native integration requires actual mandatory predecessor jobs and same-run/attempt
  immutable artifacts; it must not wait for its enclosing run to finish. This
  trusted context is not currently provided by a concrete adapter. No pure fixture
  grants native entry or establishes platform authentication.
- Open VSX evidence and complete #14923 VSIX proof adapters remain missing here.
  Required VSIX is an artifact role even if all editor registries are deferred.
  Refuse a complete admission claim until the accepted adapters are integrated.

## Proof before population

Use fallible paired tests through the production leaf: exact accepted RC context;
wrong source/prepared/tag/version/topology/policy; unknown profile; duplicate and
unknown fields; candidate permission enlargement; stable/prerelease-false with all
skip flags; missing required VSIX; conditional registry without accepted evidence;
wrong repo/workflow/ref/SHA/run/attempt/artifact/input/manifest; equal SHA but distinct
transactions; every terminal non-success and nonterminal external result; deterministic
selection independent of map order. Negative controls change one load-bearing field
while keeping unrelated values equal. No test may assert authentication from a
caller-supplied boolean or fabricated evidence status.

Workflow-dependent controls (native enclosing run still active, forged native entry,
credential availability, always/skip/cancel/retry bypass, publisher-only rerun,
zero public mutation) remain explicit unexecuted integration obligations, not green
unit tests or closure evidence for this leaf.

Planned commands after implementation authorization:

```text
python -B -m unittest discover -s scripts/ci -p test_release_publication_admission.py
python -B -m unittest discover -s scripts/ci -p test_release_workflow_gate.py
python -B -m unittest discover -s scripts -p test_release_terminal_manifest.py
python -B -m unittest discover -s scripts -p test_release_tag_authority.py
git diff --check
```

The last two suites require the accepted current-main dependency to be integrated;
absence in the old carrier is NOT_PROVEN, not a reason to replace their validators.
No Cargo, workflow dispatch or installed/release execution belongs to this packet.

## Cage, return conditions and completion

Current preparation cage: this spec plus deletion of the exact two obsolete files.
Proposed later implementation cage: new admission Python module/test, exact policy
and schema/fixtures, necessary narrow policy/non-Rust ownership entries; retained
workflow gate changes only where an independently reviewed adapter plan requires.
No workflow activation in the first leaf. No new dependency or blanket exception.

Return before code for root confirmation of the proposed closed fields/profile,
source-versus-prepared correspondence, and the concrete trusted-context adapter
boundary. Missing Open VSX/VSIX/native adapter contracts stay unresolved, not generic
status wrappers. Do not expand to settings, credentials, version choice, topology
channel changes, #7831 public-state replacement or aggregate release integrity.

This spec and pure leaf are preparation only. The carrier cannot merge until the
complete ten-item reconstruction ruling, current-main integration, graph/policy
proof, exact-head review and protected checks are satisfied. Preserve retained bytes
across retries. Root owns publication, branch state and any later claim changes.
Rollback of a future leaf removes its policy/tests together; never restore or run
the deleted obsolete finalizer merely to obtain an executable route.

## Implemented first-leaf boundary and local proof

`derive_selection(policy, profile_id, prerelease, requested_channels)` accepts only
an explicit requested subset of the independently governed ceiling; requesting
Open VSX always refuses because its concrete evidence adapter is absent. There is
no evidence boolean. `validate_subject(candidate, expected, selection)` validates
closed shapes, all exact identity fields, and equality to independently expected
inputs, returning immutable `SubjectComparison` with permanent
`qualification="not_proven"` and explicit missing-adapter obligations. The checked
policy is populated only with the accepted RC profile. Closed schemas are enforced
in the pure Python functions; no extra JSON Schema runtime dependency is added.

The code does not authenticate the expected packet or bind checked-in policy bytes
to its supplied policy digest: those are explicit trusted-adapter obligations.
Likewise it does not parse versions, assert source/prepared correspondence, inspect
archives or mark VSIX complete. Such absent adapters cannot become optional green
booleans. External producer terminal-result tests remain in the retained gate; no
native route or workflow wiring is introduced.

Initial test-first run: ten tests, three positive-control failures against explicit
not-implemented stubs (not behavioral regression evidence against the old graph).
After implementation and independent review: eleven leaf tests and ten retained external gate tests pass on
Python 3.14.6. No Cargo or workflow execution. No current-main validators were
copied or executed from the stale carrier. The next dependency is the concrete
trusted private-candidate adapter, consuming canonical topology and terminal-manifest
validators plus landed #14923 composition, then actual mandatory predecessor wiring.
The complete reconstruction ruling remains the merge boundary.
Independent review separated prerelease-false from unknown skip-field rejection. Removing only the packet prerelease guard causes the focused regression to fail; restoring production returns all eleven controls to passing. This mutation establishes that specific discriminator, not graph or publication qualification.

## Authorized private-byte adapter, second preparation step

Root decision [5751253711](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/14406#issuecomment-5751253711)
authorizes exact-file canonical integration from main `46588022115d4bec71c7db544323528852b5ffb9`, not full-main merge
or graph activation. `canonical-ports.json` records all16 exact source blobs;
`bash_binary.py` is the retained topology-test import dependency. Existing canonical
tests remain byte-for-byte with their inherited assertion debt; these ports and
file-ownership receipts are not approval of those exceptions or a cleanup claim.

`scripts/ci/release_private_candidate.py::validate_private_candidate` receives a
candidate directory, the candidate and independently expected closed subject packet,
exact policy bytes, and `TopologyContext` (independently pinned frozen/prepared roots,
captured frozen topology bytes and independently expected digest). It captures only
regular candidate files into a unique temporary copy, then gives that same copy to
canonical `check_outputs`. It binds actual terminal/inventory bytes, uses prepared
SHA for terminal build/tag identity, requires producer head to match that SHA, calls
actual topology admission with the separate frozen SHA and both source roots, and
joins canonical selected targets to actual archive and evidence target sets.

The adapter returns concrete byte digests and an immutable local comparison. It
removes only the canonical-topology/terminal-byte missing obligations. Qualification
remains permanently `not_proven`: complete VSIX, policy source authentication,
producer/artifact authentication, predecessor graph, externally isolated worker
provenance and immutable-tag authority remain absent. The tag validator is ported
and its controls retained but no live ruleset/ref collector is invented. Open VSX
still refuses; unlanded #16207/#16230 code is not imported.

Execution precondition is external: only independently reviewed/pinned source roots
in a credential-free isolated worker. Canonical topology admission executes the
source publication helper and acquires Git/Cargo metadata. Clearing environment
variables alone is not filesystem credential isolation. No boolean or fabricated
status establishes this precondition. Local tests execute only the reviewed canonical
helper in owned fixture roots; Git/Cargo metadata acquisition is mocked, while
schema, source-transition, topology, archive/member, checksum, SBOM and terminal
validation run as implemented. No installed executable or publisher runs.

Eight adapter tests cover a valid constructed private candidate, seven actual
same-length byte-corruption cases (archive/checksum/SBOM/receipt/manifest/inventory/
topology), prepared source drift, frozen/policy byte identity, prepared-versus-frozen
build identity, single captured candidate usage, and omission of a genuine canonical
topology-selected target. The last control requires its specific denominator refusal,
so malformed topology cannot falsely satisfy it. Fixture-only metadata acquisition
and v1 topology coverage do not establish real Cargo output, all v2/v3 paths, provider
provenance or hosted isolation. Retained canonical tests are separate regression proof.

Corrections during local proof: stripped process environment initially hid the
installed JSON Schema package; the launcher explicitly supplies its already-installed
site directory without forwarding credential environment variables. The first Git
fixture returned prepared SHA for both roots and was corrected to distinguish roots.
The added two-target fixture initially omitted its matching downstream target row;
correcting that canonical fixture reached the intended archive-denominator rejection.
These were fixture/instrument repairs, not weakened production validation.

Next graph dependency remains actual isolated pre-eligibility producer wiring plus
accepted #14923 composition and authenticated artifact/policy context. This adapter
is not permission to merge the partial carrier or run its obsolete retained graph.

Independent adapter review corrected a masked negative: prepared-versus-frozen now changes both expected prepared SHA and producer head, and requires the canonical terminal validator's source-identity rejection. Root and child symbolic-link/junction refusal branches have deterministic controls; removing only root-junction rejection fails the control, and exact restoration passes. These mocked path-predicate controls prove refusal logic, not native detection or filesystem isolation. The updated eight adapter tests pass; the earlier 49 other focused canonical/leaf/gate tests remain unchanged proof, for 57 distinct tests across the two runs. Broad corruption controls establish rejection without attributing each rejection to a unique internal validator.
# #16762 current-main private producer recovery

Accepted packet: issue comment 5888739071. Normal integration consumes main
`c5c6c72cb0a20ace218c5f449dc8e5077e67dd72` without changing the separate conflicted
#6231 checkout. Main supersedes the old tag-first workflow, topology v3-only
implementation, helper and dependency snapshots; private permission/byte adapters,
their tests and self-mutating finalizer removal survive. Policy retains main's
evolved shared rows and adds only private rows absent from main.

Implementation cage: existing release.yml/orchestration, canonical topology
producer grammar and paired tests, private producer observation adapter/tests,
existing prerequisite-graph checker/tests and this plan. Private mode is the safe
default. Its planned tag is a label, not an assertion of existing immutable tag
authority. Candidate building has read-only repository permissions and no OIDC,
attestation, publisher environment or downstream publication. Exact source,
transaction, run/attempt, policy bytes and canonical terminal membership accompany
the private result. No same-SHA dispatch selection grants authority.

The common fan-in must remain terminal on failed/missing predecessors. It derives
NOT_PROVEN while authentication, isolation, accepted policy/currentness or complete
VSIX adapters are absent; no unsigned digest becomes publication eligibility.
Public jobs require affirmative qualification plus separate explicit publication
authorization. No such authorization is exercised by this packet. #16207 is landed;
#16230 is not, so complete mapped VSIX remains NOT_PROVEN.

Proof commands: `python -m unittest discover -s scripts/ci -p test_release_publication_admission.py`, the corresponding
`test_release_private_candidate.py` and `test_release_private_producer.py`
modules, plus canonical `test_generate_release_topology.py` and
`test_release_terminal_manifest.py`. Actual workflow controls live in
`test_release_prerequisite_graph.py`; actionlint checks both selected workflows.
Per-module unittest counts count test methods, not individual mutations; current
combined admission/candidate/producer execution is 21 methods (11/8/2). #9422/#8576 receive
exact job/output/environment/dispatch observation identifiers, never fabricated
hosted counts. Rollback reverts the coherent source changes while retaining the
canonical main graph and private-adapter history. No public operation is run.

## Executable observation handoff (#9422 / #8576)

`release.yml` is the single selected child workflow. Inputs include exact
`expected_sha`, `transaction_id`, expected policy/topology digests and expected
run attempt; `no_publish` defaults true. Orchestration forwards those inputs and
always requests private mode. Planned `tag` labels bytes only; it does not claim
a created or authenticated immutable tag. The selected caller still owes trusted
input acquisition and producer/artifact authentication.

Failure input: `fail_before_publish=true`. Required refused predecessor:
`publisher-eligibility` / `Bind and seal terminal candidate authority`, after
canonical private-byte observation and before any affirmative qualification.
Failed/skipped candidate or metadata results fail that terminal fan-in; missing
bytes also refuse. Its only implemented qualification output is `not_proven`.
The emitted `private_producer_observation.v1` is a local observation, not the
named release-integrity aggregate or a new admitted qualification schema.

Observation artifact: `private-producer-<transaction_id>-<run_attempt>`; private
candidate artifact: `release-terminal-candidate-<transaction_id>-<run_attempt>`.
Record exact repository, workflow ref, SHA, run ID/attempt, input identity, policy
and topology digests. Artifact names route bytes, not authenticated authority.

All public jobs are `publish-release` and `dispatch-publishers`. The former holds
deferred attestation/tag/GitHub Release operations; the latter owns crates/editor/
container and package-manager fan-out. Both require explicit non-private mode AND
`publisher-eligibility.qualification == satisfied`; no current adapter can emit
that value. No publisher environment is selected by the private path.

Hosted probes must observe those jobs, effective environment/credential admission,
attestation/tag/release APIs and every downstream dispatch, plus topology-selected
public-state surfaces before/after. Expected counts are zero; actual counts and
public-state equality are NOT_PROVEN here. Local source/fixture passes do not fill
them. Complete VSIX, isolated-worker provenance, trusted policy, artifact/producer
authentication and relied-upon live controls remain mandatory unresolved adapters.


## #16762 explicit source acquisition and #8576 executor prerequisite

The optional release inputs `source_context_run_id`, `source_context_artifact_name`
and `source_context_sha256` select one same-repository, exact-run named artifact
using actions:read. Missing/unavailable input is NOT_PROVEN, not compatibility green.
The unpacked artifact must contain exactly regular `context.json` and
`frozen-topology.json` files. Context JSON must be canonical, duplicate-free and
closed-schema `private_source_context.v1`: repository, positive artifact_run_id,
artifact_name, distinct frozen_sha/prepared_sha, frozen_topology_sha256,
independent vsix_mapping and sorted unique targets. The expected context digest
binds exact JSON bytes; its frozen digest binds exact frozen topology bytes.
Repository/run/name equality is an observed acquisition join, not authenticated
release authority. Separate checkouts use exact declared commits, no persisted
credentials, and complete Git history for the frozen-ancestor-of-prepared check.
Producer SHA must equal the declared prepared role; tag is only a planned label.

No artifact field, CLI option or workflow boolean admits source execution. The
current CLI does not execute declared-root topology helpers or Cargo metadata.
`admitted_source_executor` is an internal callable seam, unavailable to CLI;
`validate_v4_sources` preserves the canonical v4 callable for a future admitted
adapter and is tested on owned fixture roots. It validates the exact independent
mapping, frozen/prepared transition and topology/archive/evidence target equality.
The ordinary acquisition path reports source_validation NOT_PROVEN and leaves
reviewed_source_execution and credential_free_worker as missing adapters. Even a
locally admitted callable never qualifies publication by itself. The observation
always reports source_validation NOT_PROVEN until a concrete admitted result
interface exists; a no-op callable cannot upgrade this receipt. Direct owned-fixture
canonical execution remains a separate test result.

[Existing #8576](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/8576)
owns the executable prerequisite; do not create a second authority model. Its
next adapter must independently admit the reviewed validator closure and source
roots, establish credential-free isolation before helper/metadata execution, bind
read-only roots and exact source roles/ancestry, and prove denied credentials,
network and process capabilities where the accepted execution contract requires
it. Canonical `publish_dependency_graph` executes root/scripts/publish-topo.py;
`cargo_metadata` invokes Cargo in the root. Digest pinning and
persist-credentials:false alone cannot authorize these operations.

Discriminators: default missing executor must never reach a throwing helper;
missing/malformed context, wrong digest/repository/run/source/targets refuse;
owned v4 canonical positive plus wrong source and omitted/wrong targets refuse;
no_publish=false with NOT_PROVEN must leave both public jobs unreachable. #8576
must supply real before/after private execution and #9422 mutation observations
before any hosted completion claim. Current tests are local source/fixture proof;
actual hosted acquisition, isolated executor, full VSIX, authenticated qualification
and observed zero public mutations remain NOT_PROVEN. No public operation is
permitted in this goal. Root owns issue publication and final disposition.


## Retained child-run mechanical leaf disposition

`release_workflow_gate.py` is preserved donor value, not used by current
orchestration. Selection now binds repository, requested branch/tag ref component,
workflow ID, attempt and optional explicit run ID. Terminal validation rechecks
selected run ID/attempt and full identity, then joins the actual named
`private-producer-<transaction>-<attempt>` observation bytes to the independently
expected transaction/source/repository/full workflow ref/run/attempt. Run metadata
cannot supply workflow input or transaction identity. A future admitted artifact
loader must obtain the exact same-repository selected-run named artifact and
return verified bytes plus observed identity. The current CLI has no such adapter
and returns NOT_PROVEN before dispatch. Mechanical terminal success never means
release qualification; the observation must remain explicitly nonqualifying.
Paired tests mock all API/artifact operations: wrong ref/workflow, substituted
selected ID/attempt, same-SHA distinct transaction, absent identity adapter and
wrong producer identity refuse. No network or dispatch execution occurs in proof.


Mechanical selection retains a dispatch_started minus 60-second tolerance. An
unseen just-pre-dispatch run with the same ref/SHA/attempt and reused transaction
ID is not causally distinguished by these joins. The disconnected helper and
missing-loader CLI remain nonqualifying. #8576 must independently admit one
execution/transaction identity and authenticated acquisition before any causal
claim; do not invent a parallel registry or infer causality from timestamps.


The #16762 source checker admits only the current restricted workflow grammar:
plain mapping keys, inline exact dependency sets, exact qualification expressions,
named steps and literal run blocks. It reads real authority fields and executable
standalone predecessor/producer commands and dispatch continuation arguments.
Comments, quoted marker strings and echo output cannot replace them. Duplicate
fields, aliases, merges, ambiguous continuations and unsupported command wrappers
refuse rather than falling back to token-presence proof. This restricted reader is
not a general YAML/shell interpreter or a hosted execution/authentication result.


### Restricted checker execution grammar

The graph ratchet admits the reviewed `fade881` eligibility job and child
transaction dispatch job as closed lexical productions, including every named
prior step, action, ID, environment binding, and every command through the final
output write. Comments and blank lines are ignored; executable or metadata edits
require a deliberate reviewed production update, including otherwise legitimate
workflow metadata or command changes. This maintenance tradeoff intentionally
rejects unreviewed equivalent syntax rather than claiming generalized equivalence.
Productions are fixed reviewed constants, never generated from validator input.
Custom workflow defaults,
changed global environments, shell overrides, duplicate or quoted authority keys,
extra steps, split-format output writes and shell-substitution decoys reject.
This intentionally restricted source style is not a general YAML/shell parser,
proof of arbitrary equivalent syntax, hosted execution, or authenticated authority.
The live producer qualification remains `NOT_PROVEN`.


Job-level `defaults` are unsupported throughout the selected release workflow,
including all private predecessors and public jobs. Actual step mappings admit
only the existing explicit `shell: bash` spelling; custom executable templates
and duplicate/quoted shell keys reject. This shell boundary constrains command
execution style; it does not imply that shells control GitHub job conditions or
that all upstream shell programs have been semantically verified.


Root and explicit private predecessor permission maps admit only plain unique
`actions`/`contents` keys with unquoted `read` values. Missing job permissions
inherit the checked read-only root map. Quoted keys/values, unknown permissions,
nested permission syntax and private job environments reject. Remaining mutation
marker scans are conservative source ratchets, not complete shell/action semantic
proof or verification of every possible public-authority route.


The release workflow admits exactly its six reviewed jobs. The upstream
`release-metadata`, `build`, and `candidate` jobs are pinned by fixed SHA256
productions from reviewed `b5c6c4116d657211ad86905bfbd9ea1afd39fb8a`.
Only YAML comments/blank lines outside literal or folded scalar bodies are
ignored there; every scalar-body line, including comments and blanks, is retained
with indentation. Upstream action, secret, environment, metadata and command
changes require a deliberate reviewed digest update. These constants are not
computed from incoming workflow content. This closes the selected source style,
not arbitrary YAML/shell equivalence or hosted public-authority execution.

All selected release-job steps require the supported plain `- name:` list
header. A shell or other authority key in the first list position rejects;
subsequent step fields retain the explicit shell boundary described above.

The orchestration workflow admits exactly `validate` and `trigger-release`.
Its existing validation job is pinned to a fixed reviewed production, including
read-only permissions and its commands; extra jobs or validation-job mutations
reject. The legacy combined-status polling in that production is not proof of
all current required branch checks or authenticated publisher qualification.
Publisher eligibility provisions the pinned Python/schema dependencies before
inspecting source context. Removing that setup, or replacing installation with
an echo, rejects. These are source-routing prerequisites; clean hosted execution
and release qualification remain `NOT_PROVEN`.
