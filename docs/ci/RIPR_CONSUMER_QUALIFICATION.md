# Exact RIPR consumer qualification inputs

`scripts/ci/prepare_ripr_consumer_packet.py` prepares offline workload inputs for
the source profile diagnosed in [#16126](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/16126)
and [#15498](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/15498#issuecomment-5995297478).
It reads existing Git objects and two bounded native recursive Git-tree snapshots.
It does not fetch, materialize a corpus, invoke Cargo/RIPR, select a runner, change a
cache, or publish a required result. Exit zero means collection succeeded;
`admission_effect` stays `none` and `qualification` stays `NOT_PROVEN`.

The accepted source profile is the complete evaluated tree
`beea1d3a1b7b3c42ef510cd66f1a4337ce76f894`, including the workflow blob
`f39dbbda84d264af20a5025c9083d520fa2090de`, producer blob
`daaaefa06dc13e5340ce6a5ba31027fa5239caf3`, and gate blob
`4319363165a78b80bb826299246c6c56da271b5d`. The independently pinned full tree
binds the static phase recipe, including dispatcher, observer, dependencies and
configuration identities. The three named blobs alone would allow an unreviewed
dispatcher or observer to change the recipe. Any different evaluated tree requires
reviewed recipe qualification;
the collector refuses it instead of guessing compatibility. The live consumer
recipe describes the primary hosted `ripr-github` lane; selfhosted and disk-full
fallback lanes need separate qualification. It pins **0.10.0**. The separately identified inactive `.ci/ripr-proof.sh` profile
requires **0.10.1**; neither that script nor an upstream source version qualifies
a replacement binary. Executed binary hash, features and toolchain remain runtime
evidence requirements.

## Reproduce the diagnostic input packet

The named E/B/H commits and workflow blob must already exist in the selected local
repository. A metadata-only fetch can supply commits separately; the collector
sets `GIT_NO_LAZY_FETCH=1` and refuses missing objects. Capture complete native
tree responses with these endpoints, preserving UTF-8:

```text
repos/EffortlessMetrics/perl-lsp-swarm/git/trees/beea1d3a1b7b3c42ef510cd66f1a4337ce76f894?recursive=1
repos/EffortlessMetrics/perl-lsp-swarm/git/trees/d7baa1c8349fe90a12335ae4f781fabe0b7da266?recursive=1
```

When a shell transcodes native JSON, export `gh api <endpoint> --jq '@base64'`
and decode its ASCII output into UTF-8 bytes. This reserializes JSON; retain the
resulting file digest rather than claiming the original HTTP-response digest.
Each snapshot is capped at 8 MiB; each read Git object is capped at 1 MiB.
The expected subject arguments come from the controlling evidence, independently
of the snapshots:

```bash
python3 -B scripts/ci/prepare_ripr_consumer_packet.py \
  --repo /path/to/selected/repository \
  --evaluated-head 2d896c145cebf0b767ecade32dd139e979279a72 \
  --base 5ac2ef15dccd00884b751f078168cde4122467a8 \
  --pr-head 303855e2e7157bfd994b630bc56d526198851d5f \
  --expected-tree beea1d3a1b7b3c42ef510cd66f1a4337ce76f894 \
  --evaluated-tree-json evaluated-tree.json \
  --base-tree-json base-tree.json > consumer-packet.json
```

Git verifies the commit object bytes and ordered B/H parents. The collector
reconstructs every supplied subtree and the root from paths, modes and object IDs.
An omitted entry is rejected even with `truncated=false`. Duplicate paths,
missing parents, altered modes, foreign subjects and unsupported source trees
also fail before a packet is printed. Complete leaf comparison disables rename
detection; it is independent of the compare API's file-list limits.

The packet includes the tracked regular Rust candidate manifest and a named
configuration group: every `Cargo.toml`/`Cargo.lock` filename, root `.cargo/**`,
`.config/**`, toolchain, `ripr.toml`, and RIPR suppressions. This includes fixtures
and archives and is not an effective-workspace configuration or selected-analysis
claim. Git tree hashes bind identity, not the API's size field: byte counts are
explicitly **API-reported**, not verified by reading all blob contents.

## Full-sequence cost boundary

For failed #17293 attempt 2, supported worker metadata records these wall intervals;
they include wrappers and are not process CPU measurements:

| Stage | Observed wall | Evidence |
| --- | ---: | --- |
| Initial xtask build | 6m27s | completed |
| PR evidence | 12m51s | completed |
| Repository-wide baseline | 43m22s | completed |
| Guidance | unconcluded | job became terminal 70m13s after step start |
| Validation, gate, exports | unknown | never started |

See the [exact run/attempt](https://github.com/EffortlessMetrics/perl-lsp-swarm/actions/runs/37279853114/attempts/2).
The retained worker log ends during baseline. It does not show guidance-child entry,
and establishes no timeout, OOM or initiating lost-contact cause.

Host preparation includes full-history checkout, the freshness handoff, restore-only
Cargo/fact caches, locked RIPR installation, observer fixture controls, host xtask
build and doctor. Cold/warm accounting includes these costs. The PR producer then
pulls and identifies its image, installs system dependencies
and locked RIPR 0.10.0, records binary/toolchain identity, builds xtask again inside
the container, and runs `ripr doctor`. Its timed analysis is followed immediately
by a nonempty exposure check, `ripr-pr --check` and the invocation freshness-token
check. Analysis has TERM at 25 minutes plus a 30-second kill delay; the enclosing
container has TERM at 40 minutes plus a 30-second kill delay, covering setup,
build, analysis and that check. Image pull and inspection have separate bounds.
The EXIT trap bounds container inspection/removal separately; ownership restoration
has no separate timeout. Configured bounds do not establish process-tree settlement.
The host fact-cache path is not passed or mounted into this container; its restoration
does not establish warm container PR analysis.

The subsequent recipe includes fresh baseline badge/seam analysis, required guidance,
derived outputs, six contract checks, both genuine-gap gate passes, freshness-bound
proof export, separate diagnostic export and the unchanged successful fresh-producer
memo-save. **Mandatory `ripr-plus --check` runs two more repository-wide analyses**
before comparing receipt bytes. A cold generation therefore has four baseline
analysis invocations; an exact memo hit avoids generation only. `ripr-pr --check`
does not rerun RIPR analysis. The 3600-second bound is for the guidance child,
not its entire Cargo/fallback/settlement phase or the 135-minute hosted job.
No full-sequence cost can be derived by adding configured limits or extrapolating
unreached checks.

Downstream producers, validation, gate and summary attempt `always()` continuation.
The export's `ready` guard proves invocation invalidation, not successful producer
completion; a partial archive does not establish validated proof. The existing gate
interprets guidance status: missing/invalid/stale receipts block; incomplete guidance
can name actionable failure evidence, while static-limitation credit requires completed
head-bound guidance. Collection preserves that contract and supplies no gate verdict.

Qualification must retain actual selected/parsed/indexed/closure populations,
effective exclusions, cold/warm reuse, per-phase user/system CPU and wall time,
direct-child and simultaneous aggregate peak RSS, allocations and retained memory,
cancellation/process-tree settlement, repeated workspace lifecycle, validated
receipt/export digests, full semantic parity and the genuine-gap verdict. Missing
cells remain `NOT_PROVEN`; tracked Rust counts cannot fill runtime analysis counts.

Corpus governance remains [#8209](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/8209)/
[#8789](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/8789), work/resource
receipts [#7251](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7251), and
upstream Rust guidance replay [#4693](https://github.com/EffortlessMetrics/ripr-swarm/issues/4693).
These Rust consumer inputs do not replace ordinary/large Perl product corpora.
[#10064](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10064)/
[#10066](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10066) own executor
qualification, #17122 owns canonical orchestration, #13718 owns admission inputs, and
#17224 owns authorized protected continuation. This collector grants none of those
authorities and cannot recover the consumed attempt.

The lightweight controls use real Git fixture commits/trees, including Unicode
and directory/file ordering. They also reject a valid merge with a changed dispatcher
or observer even when its three named blobs and caller-supplied tree match. They run
in the existing CI Gate Self-Tests job:

```bash
python3 -B -m unittest scripts.tests.test_ripr_consumer_packet
```
