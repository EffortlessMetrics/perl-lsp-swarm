# Owned fat-arrow list resource observations, 2026-10-13

Issue [#17734](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/17734),
candidate [#17739](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17739).
The existing parser consumed a `Vec<Node>` but cloned both members of every
adjacent pair before dropping the originals. Nested parenthesized fat-arrow
lists therefore repeatedly copied and destroyed their inner ASTs. The repair
moves those owned nodes. Selection, pair order, node fields/spans, charging,
diagnostics, recovery and limits remain unchanged.

The production depth limit is still 128. Depth 32 in the measured family reaches
104 charged recursion levels. These observations concern avoidable copying
inside an admitted bounded family; they do not establish general parser
complexity or a universal speed improvement.

## Method and identities

Source base: `9200621a383ba3fd96c25e3a9be7ce8327f5851d`. Original construction
commits `aad8e491035b3a1495221cefd008c069a20784ac` (RED proof) and
`070049e05c5972f8d67ea791ddc33f61e1190c6d` (repair/initial runner) are retained.
Initial published commit `2bf76d034a6ce1788458bcfbd5c44887a1cd11c6` has the same
tree `3f26cc03ec3e4c654df7c0e963feb8f14f023f62` as the construction candidate;
the publication route changes commit metadata, not source.

Both executables use Rust/Cargo 1.95.0, the repository's agent profile
(`opt-level=1`, line-table debug, no incremental compilation), the same allocator
instrument and default parser configuration. Linux 6.18.44 / glibc 2.41,
AMD EPYC 9V74, five allowed CPUs, Python 3.12.14, Perl 5.40.1. No CPU affinity
pinning or frequency control was applied. Builds used the admitted private
target/build paths, two jobs and one serial build lane. The scoped 16 GiB growth
estimate and 6 GiB reserve are operator declarations, not measured bounds.
Each build owner reported kernel ECHILD and matching lease release.

Executables were copied to immutable, hash-verified paths while their original
build leases were held:

- Baseline SHA256: `247921a881475196f99b8808fac7f8c14efe430381ef4a12e50350a81ac94e0f`.
- Repair SHA256: `6fcad750be58709248c405c9cb73e1e964d5c1f31308b320bef69b0ed14f7f84`.

`parser_resources FILE ITERATIONS [--oracle]` reads UTF-8 input before timing.
Each iteration times `Parser::new` plus `parse_with_recovery`; separate timing
covers destruction of the returned output. A constant-initialized thread-local
allocator counter brackets construction/parsing only. It counts allocation
requests and cumulative requested sizes, including the entire new size of
`realloc`. These bytes are neither retained heap nor allocator overhead nor RSS.
Projection, source generation, file loading and output destruction are outside
that counter and the parser timer.

The runner uses five paired rounds of 100 parses per executable/case and
alternates which executable runs first. No build or other resource campaign ran
concurrently. `os.wait4` supplies each child's user/system CPU for its whole
lifetime (startup, file loading, parsing, dropping and reporting). CPU below is
median user-plus-system seconds divided by 100. Parser wall time is the median
reported parse interval divided by 100. The table uses the memory-qualified
campaign, whose approximately 1 ms sampler adds host work outside the measured
child. No causal claim is made from small ordinary-code timing differences.

An initial campaign's `wait4.ru_maxrss` was about 21 MiB even for tiny cases and
included the Python parent's inherited pre-exec footprint. It was retained and
excluded from native memory claims. The corrected runner checks the exact
immutable `/proc/PID/exe`, then samples that child's current `VmRSS` and `VmHWM`
at approximately 1 ms intervals. Reported memory is the median **observed native
VmHWM**, not a guaranteed peak or bound. All 330 corrected child observations
had at least one native sample. A separate small/40 MiB allocation control
produced native observations 6,428 / 47,392 KiB, while the small child's
`wait4` maximum remained 13,056 KiB, discriminating the inherited-footprint
problem. Sampling, page residency and allocator reuse limit interpretation.

## Corpus and correctness controls

The runner generates depths 0/4/8/16/24/32 with leaf widths 1/256/2048, plus
multi-pair, odd fat-arrow, even comma-only, chained-arrow and malformed-closer
controls. Ten ordinary inputs cover the three-file `demo_workspace`, three-file
conventional modules fixture, and installed Perl `File::Spec`, `File::Spec::Unix`,
`File::Basename` and `Scalar::Util` modules. Input bytes and SHA256 identities are
recorded in the manifest; installed module observations are bound to those
contents, not any other machine's installation.

All 33 cases agree before/after on a complete native debug S-expression
projection, every preorder node kind/span/child cardinality, diagnostics,
terminal stop cause and all budget counters. The native debug projection is
non-normative and is **not AST equality or source fidelity**. Independent focused
tests additionally assert every nested key/value, number order and byte span,
three ordered adjacent pairs, odd/even selection, next-declaration retention,
exact charged work and a tight node-budget refusal. Those tests protect the
fields affected by moving the owned nodes. Malformed input retains its existing
inserted-closer diagnostic and recovered span, including the existing span
ending before the final value; this optimization does not silently repair that
separate geometry behavior.

## Observed bounded-family results

Values are per parse except observed VmHWM. Source construction and input
loading are excluded from parser wall time/allocation requests.

| Depth / leaf width | Input bytes | Parser wall µs before / after | Process CPU ms before / after | Requested bytes before / after | Native observed VmHWM KiB before / after |
| --- | ---: | ---: | ---: | ---: | ---: |
| 4 / 256 | 980 | 568.5 / 319.5 | 0.607 / 0.361 | 1,257,604 / 143,956 | 3,248 / 3,012 |
| 8 / 256 | 1,020 | 758.4 / 345.8 | 0.797 / 0.385 | 2,455,516 / 149,820 | 3,264 / 3,060 |
| 16 / 256 | 1,106 | 1,314.4 / 384.1 | 1.356 / 0.421 | 4,877,737 / 161,560 | 3,384 / 3,208 |
| 32 / 1 | 370 | 364.0 / 115.7 | 0.382 / 0.133 | 579,359 / 50,918 | 3,300 / 3,292 |
| 32 / 2,048 | 9,498 | 18,815.2 / 2,508.5 | 19.068 / 2.719 | 76,556,873 / 1,135,856 | 5,100 / 3,868 |

At a fixed wide leaf, the retained tree grows little with nesting while the old
implementation recopies the leaf at every ancestor. A single-number leaf also
exposes the smaller quadratic ancestor-copy term. No unlimited-depth
asymptotic claim follows from these capped measurements.

Ordinary controls show mixed timing changes and small native memory differences.
Seven of ten have identical allocation totals. `Utils.pm` requests
27,534 → 20,676 bytes, `Registry.pm` 27,838 → 26,711 and installed `File::Spec`
34,124 → 25,068; all other ordinary allocation totals are unchanged. This
supports a precise allocation improvement at exercised fat-arrow conversions,
not a blanket ordinary-source CPU/memory claim.

## Reproduction and retained evidence

Build the example at the baseline source with the same example/counter files,
then at the repair, through the host-admitted Cargo route. Preserve each actual
executable under its build ownership before any independent consumption:

```text
scripts/cargo-admitted --budget-file HOST_POLICY run -p perl-parser-core \
  --example parser_resources --profile agent --locked -- INPUT 1 --oracle
```

Run the paired corpus command with the immutable paths and a new output folder:

```bash
python3 scripts/bench_hash_list_resources.py \
  --baseline BASELINE_EXE --candidate CANDIDATE_EXE --output NEW_DIRECTORY \
  --iterations 100 --rounds 5 \
  --ordinary demo_workspace/main.pl \
  --ordinary demo_workspace/lib/Utils.pm \
  --ordinary demo_workspace/lib/Database.pm \
  --ordinary testdata/ux/first_ten_minutes/conventional-modules-v1/bin/app.pl \
  --ordinary testdata/ux/first_ten_minutes/conventional-modules-v1/lib/App/Registry.pm \
  --ordinary testdata/ux/first_ten_minutes/conventional-modules-v1/lib/App/Format.pm \
  --ordinary /usr/lib/x86_64-linux-gnu/perl-base/File/Spec.pm \
  --ordinary /usr/lib/x86_64-linux-gnu/perl-base/File/Spec/Unix.pm \
  --ordinary /usr/lib/x86_64-linux-gnu/perl-base/File/Basename.pm \
  --ordinary /usr/lib/x86_64-linux-gnu/perl-base/Scalar/Util.pm
```

The saved cloud packet `/workspace/parser-resource-evidence` retains original
logs, immutable executables/captures, source snapshots, budgets, both measurement
campaigns, oracle projections, sampler controls and mutant results. The corrected
campaign `paired-mm` identities are:

- Manifest SHA256 `e42ef0b2b4a50fe02fa9dcd44603de8818e53cd6aca0136e9aa341f2625b0182`.
- Raw measurements SHA256 `07d977b50052b9b843ac9a2c689a47f6e8b708e0e9cef4d906669b9cd417a236`.
- Summary SHA256 `de4f0dde2cdc99716cd426ccf5e90c873f92267c6b00934e2e7f7d049d4c1385`.

The checked-in JSON derives its CPU median from the sum in each identified raw
round. The original summary's separate user/system medians are retained under
their original identity. No timing was repeated for that arithmetic correction.
The reusable sampler now refuses a successful child with zero native samples;
zero bytes cannot stand in for unavailable memory evidence.

The packet is saved environment evidence, not a remotely uploaded artifact.
Hosted qualification and protected integration are separate from these local
measurements; their current status belongs on the PR.

After the recorded campaign, review repaired the example's optional-count
argument handling (`FILE --oracle`) and the sampler's executable comparison
under filesystem aliases (`os.path.samefile`). A separately captured executable,
SHA256 `51962b451acb14a2c19fc2b08c8edd537f053e22cff004eb5b6efbfa5ad264af`,
passes ten real CLI controls and agrees with the earlier repair executable on
all 33 projections and allocation counters. Recorded CPU/RSS values remain
bound to the two original measured executables; no new performance campaign
or speed claim follows from this tool-only repair.

A real child executing one hardlink while the control names another link to
the same inode rejects the old path comparison and yields native samples with
the repaired comparison. The test injects only that launch alias; an actual
bind mount was not created or qualified. Original executable inodes remain
unchanged; the control uses a separate image copy. No mount, privilege or access
configuration changed.
