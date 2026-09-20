# #14406 publication-admission law — retained-carrier preparation

Status: first pure leaf authorized by root after review of spec bcfcc57c; implementation remains nonqualifying. This is
preparation in the retained draft #14479 carrier, not a standalone mergeable
partial graph. It advances #14406 and closes no release prerequisite.

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
