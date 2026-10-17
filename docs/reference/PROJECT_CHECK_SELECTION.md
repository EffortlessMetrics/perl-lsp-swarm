# Project-check selection decision v1

Status: reviewed research candidate for [#11266](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11266).
Implementation consumer: [#10756](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10756).
This policy is a decision artifact; canonical selection is **not implemented**.
Current `--check-project` remains the 80 percent parsability report.

The [policy](../../policy/project-check-selection.v1.json) has stable row IDs,
retain/correct/refuse/defer dispositions, rationales, owners, architecture comparison
and reversal criteria. Its identity hashes rows, architecture, typed selection
parameters and discovery budgets. The
[strict schema](../../testdata/project-check-selection/schema.v1.json),
[hand-authored fixtures](../../testdata/project-check-selection/fixtures.v1.json)
and [Linux decision receipt](../../testdata/project-check-selection/linux-decision.v1.json)
are reusable inputs to #10756, not another product JSON envelope.

## Measured current behavior

Source: `main@9e83b961ee60b16f299a34b08e2ae07d5c8a4527`,
`crates/perl-lsp-rs/src/cli/check_project.rs`. The receipt binds source, fixture
and immutable binary bytes. The actual public production binary exercised 35
non-root Linux temporary-tree cases. `my $selection_probe = ;` independently
failed strict `--check`, making selected file paths visible in the report's
blocking findings. No replacement walker was implemented.

| Subject | Observation | Policy row / disposition |
| --- | --- | --- |
| Families/case | Only lowercase `.pm`, `.pl`, `.t`; no shebang, CGI, PSGI, XS or template admission | `extensions` / retain narrow initial families |
| Nested exclusions | Exact case-sensitive descendant basenames `local`, `blib`, `vendor`, `node_modules`, `.git`, `target`, `.build`, `auto`; near matches and uppercase variants admitted | Eight `exclude-*` rows; retain seven, correct `exclude-auto` |
| Explicit excluded-name root | All eight requested roots traversed; their nested excluded directories still skipped | `root-excluded-name` / retain |
| Hidden/ignore | Hidden paths and `.gitignore`/`.ignore`-listed source admitted; `.hg`, `.svn`, `.cache`, `.perl-lsp` also admitted | `hidden-ignore` / retain disk-only policy |
| Symlinks | File/directory aliases and outside-root targets admitted; an alias to `vendor` bypasses exclusion by target name | `symlinks`, `outside-loop` / refuse descendant links in canonical policy |
| Loops/broken links | Retained as unscanned rows; a loop-only root still exits 0 | `outside-loop`, `no-input` / refuse links and correct completeness |
| Hardlinks | Two logical paths to one inode counted twice | `hardlinks` / retain logical subjects |
| Permission failures | Selected read failure counted as blocking; unreadable subtree outside denominator; one clean file plus unreadable subtree passes 100% | `unreadable` / correct selected/evaluated/unknown distinction |
| Empty/unreadable root | Zero selected files returns 0, including an unreadable directory with unscanned evidence | `no-input` / correct canonical disposition |
| Root argument | Missing root and explicit file root exit 1; explicit directory link followed | `root-directory` / retain directory subject; canonicalize explicit root once |
| Native path identity | Unicode variants remain distinct; two invalid-byte names display identically as `bad-�.pm` despite six scanned logical paths and five distinct displayed paths | `path-identity` / correct reversible byte identity |
| Colon-space names | `a: b.pm`, `a.pm: b.pm` and an unreadable `locked: subtree` retain their full displayed identities in controlled report evidence | `path-identity`, `unreadable` / instrument join, unchanged production policy |
| Ordering | Same selected set in forward/reverse trees; rendered order varies and is not lexical | `ordering` / correct ordering of bounded static plan |
| Threshold | Four clean files compensate for one blocking file: legacy report passes at 80% | Existing #10742/#10766 report-versus-strict ruling, not a selection-policy change |

One observed forward order was `middle.t, a.pm, z.pl`; reverse was
`z.pl, a.pm, middle.t`. Another run reported the same `a.pm, z.pl, middle.t`
order for both trees. Creation sequence and directory identity are not isolated
as independent causal variables by this experiment. This proves no stable order
claim, not a particular filesystem's enumeration algorithm. The deterministic
receipt canonicalizes observed sets; raw order remains research evidence.

## Architecture and migration

Retain CLI/result ownership in #10756, extracting the smallest static
selection-plan/result boundary. Do not introduce a general scanner. The
workspace discovery API (`perl_workspace::discovery`, re-exported by
`runtime/file_discovery.rs`) is separable from indexing, but wholesale reuse
would import Git processes, tracked/ignored state, broader case-insensitive
families, content probes and include-root semantics. Its walk/linked modes drop
some traversal errors; `DiscoveryResult` lacks per-path operational evidence and
a finite entry budget. Sorting and cooperative cancellation are useful precedents,
not grounds to adopt its semantic defaults.

The pure source-extension and workspace-ignore predicates also differ from CLI
v1. Reuse requires exact policy compatibility, complete error evidence and a
bounded static contract, not only fewer lines. The policy artifact records the
four compared alternatives and conditions that would reverse this decision.

`auto` is too broad for canonical own-source selection: it can hold user `.pm`
source while XS artifacts are already outside admitted families. The fixtures
explicitly characterize its current exclusion; they do not endorse it.

#10756 must preserve the legacy report under a named compatibility identity and
carry operational evidence. Correct/refuse rows are intentional future canonical
migration obligations; #10766 owns strict/report exits and alias exceptions.
#10897 owns canonical parse evaluation and #10809 typed categories. Later
#11269/#11270 consume the same result without another walk or parse loop.
The amended #10742 graph governs these dependencies. Older #7233's suggested
`--check-project --json` conflicts with later #11270: the legacy alias remains
text-only; canonical JSON uses the future strict/report `--output-format json`
interface. No such flags are added or claimed to work here.

## Budgets and limitations

The initial canonical discovery decision requires finite internal ceilings:
10,000 selected paths, 100,000 visited entries, depth 128 and 60 seconds elapsed.
Cancellation is checked between entries. Exhaustion retains partial evidence and
marks incomplete. These are conservative policy choices, not measured capacity
guarantees or proposed CLI options. Source-byte/parser interruption limits remain
a required #10756/#10897 decision before claiming complete canonical evaluation.

The current walker has no discovery cap/cancellation callback. The harness limits
each CLI invocation to 10 seconds and rejects fixtures above 256 literal entries
or expected scanned logical files before execution. It runs processes
sequentially. This bounded evidence does not prove a large-tree resource envelope
or production cancellation. Every process is the synchronous CLI source route;
the timeout is instrument protection, not a general descendant-settlement proof.

Report observations join reported lines to a separate literal report-subject
inventory, including unselected, excluded, directory and explicit alias spellings.
Validation requires coverage of created paths and their ancestor spellings without
walking or classifying; aliases are literal inventory entries. The expected
selected set does not define this inventory. Colon-space delimiters in error text
are not path boundaries. A subject is
recorded only when the report contains it. Unknown or multiply matching path lines
fail the instrument rather than inventing identity. This remains controlled human
report characterization, not a general text or machine-consumer interface.

Concurrent disappearance/replacement during traversal/read, atomic snapshots,
non-UTF-8 root arguments, native Windows/macOS, drive/UNC/case normalization,
multi-root and parser interruption remain explicitly unqualified. No race hook,
heavy Windows allocation or platform inference is introduced. PATH interpreter
traps (with a positive trap control), a parseable `BEGIN` marker and the native
source trace support no project execution for these inputs; no exhaustive syscall
or absolute-interpreter audit is claimed. Installed/RC/editor and canonical
executor transport proof remain separate.

## Repeatable proof

Use Python 3.10 or newer with the repository's existing `jsonschema` development
dependency.
Build only `perllsp --bin perllsp --profile agent --locked` through the pinned
1.95.0 admitted route after exact-request host admission. Hold the root build
lane through capturing an immutable executable; never consume sibling output.

```bash
python3 -m unittest discover -s scripts/tests -p 'test_project_selection_characterization.py' -v
python3 scripts/characterize_project_selection.py
python3 scripts/characterize_project_selection.py --binary /absolute/immutable/perllsp --write-receipt testdata/project-check-selection/linux-decision.v1.json --log-dir /absolute/proof/observations
python3 scripts/characterize_project_selection.py --binary /absolute/immutable/perllsp --check-receipt testdata/project-check-selection/linux-decision.v1.json
git diff --check
```

Changing the binary changes receipt identity even if behavior remains the same;
regenerate and review the exact observed subject deliberately. The set assertions
are **current-behavior characterization** and may need reviewed migration when
the owning implementation changes. No CI gate is added, required or relaxed.

Receipt writing and checking are mutually exclusive, including aliased paths.
Checking compares exact bytes and does not write evidence. Root or non-Linux
execution refuses before invoking the instrument. The empty-success behavioral
falsifier requires actual non-root Linux execution; its explicit prerequisite
refusal has a separate test, and a skipped behavioral control is not a pass.

Validation requires the declared Git commit to exist locally and its exact walker
blob to match the observed source. Binary build provenance remains an operator
obligation established by the admitted build and immutable executable capture;
a matching binary digest alone does not attest its source or installed behavior.

Local admission observed 30 GiB free, 16 GiB cgroup memory capacity, a prior
retained agent footprint of 1.8 GiB and no active Cargo lane. The operator's
6 GiB reserve plus 16 GiB additional-growth forecast was unverified and scoped
to one two-job offline agent-profile binary build. Actual build: 237.2 seconds,
2.58 GiB peak cgroup memory, minimum 29.04 GiB free, new private output footprint
941 MiB. A 12 GiB memory stop and 900-second deadline were not reached. The
admitted owner proved kernel child settlement and matching lease release.

Negative controls reject empty-success instruments, unknown/missing/wrong-version
fields, missing unreadable/alias/outside-root/no-input observations, absent
dispositions, semantic changes without policy identity movement, duplicate
subjects/keys, nonexistent or mismatched source subjects, fixture ceiling
violations, alternate discovery authority and platform overclaims. Second
generation is byte-identical for the same executable/source/fixture subject.
