# Returned closure linkage: bounded owner handoff (#18352)

The executable falsifier is ready: all 10 Perl assertions pass; changing either
captured initializer from 5 to 6 kills all three invocation forms; the **same
direct-factory mutation survives the assertion on the other factory instance**.
Thus a factory-owner marker plus a strong oracle in the same test cannot prove
changed-capture relevance. No producer, wire schema, or consumer was changed.

Fresh producer execution is **NOT_PROVEN**: the supported no-build preflight
refused the exact focused build before acquiring a lease. This handoff separates
executed Perl behavior, current-source predictions, and historical packet evidence.

## Subject and custody

- Source/base: `EffortlessMetrics/perl-lsp-swarm` main
  `7e57f3ab9d214fc87d6630decc398b00f99521e0` (remote read-back matched the checkout).
- Proof branch: `probe/18352-closure-return-handoff`.
- Existing authorities, inspected before mutation:
  [#18352](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/18352),
  [its contract triage](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/18352#issuecomment-6392562016),
  [#8134](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/8134),
  [#4747](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4747),
  [#9274](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/9274), and
  [#9276](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/9276).
- The wire/type/validator landing remains #9274/#9276; canonical relations #9293;
  capture deltas #9285; oracle relevance #9296; downstream consumption
  [ripr-swarm#3223](https://github.com/EffortlessMetrics/ripr-swarm/issues/3223).
  Public issue/PR searches found no equivalent #18352 probe candidate. This is
  bounded proof preparation, not a claim that unpublished work is idle or unowned.
- No changes to #18343, #18333, or #18114; no external-agent contact, RIPR writes,
  installation, policy change, workflow dispatch, readiness change, or merge.

## Executed discriminating proof

Run from the proof checkout:

```sh
python3 testdata/ripr/closure-return/probe.py --runtime
```

The driver uses installed Perl `v5.40.1`, Test::More, TAP::Harness `v3.48`, and
Python `3.12.14`. It runs the following direct commands in the fixture directory
or an owned temporary copy. It checks assertion counts and failure signatures;
an interpreter/setup failure cannot count as a successful mutation kill.

| Subject | Command | Observed result |
| --- | --- | --- |
| Original fixture | `prove -Ilib t/direct.t t/bound.t t/conditional.t t/evaluated.t t/instances.t` | exit 0, 5 files / 10 assertions, PASS |
| Direct initializer `5 -> 6` | `prove -Ilib t/direct.t` | exit 1, all 3 assertions FAIL; got 6/7/8, expected 5/6/7 |
| Same direct mutation, other instance | `prove -Ilib t/instances.t` | exit 0, 1 assertion, PASS; still 40 |
| Bound initializer `5 -> 6` | `prove -Ilib t/bound.t` | exit 1, all 3 assertions FAIL; got 6/7/8, expected 5/6/7 |

The direct and variable-bound returns each exercise `$c->()`, `&$c()`, and
`&{$c}()`. The conditional and eval controls run successfully in Perl, while
remaining unsupported for exact static linkage in the requested first slice.
Runtime success is not evidence of static resolution. `instances.t` calls the
changed instance, then asserts only the unchanged instance from the same factory.
The first runtime mutation changes only the ternary's selected value, leaving
the other instance's initializer at 40. This tests relevance as well as identity.

## Producer output: observed history versus current-source expectation

The unchanged [historical packet](https://github.com/EffortlessMetrics/ripr-swarm/blob/e1a2efdde84e25b51e6553d01a6243e6a400c483/fixtures/perl-verdict-corpus/packets/perl-idiom-closure-counter.json)
at ripr-swarm#9485 head `e1a2efdde84e25b51e6553d01a6243e6a400c483` was inspected
directly. It has the factory owner
`owner:lib/Toolkit.pm:sub:Toolkit::make_counter:45-141`, change
`change:file:lib/Toolkit.pm:7:7`, and a `direct_owner_call` relation to
`test:t/toolkit.t`. That relation's `oracle_id` is null. Both counter assertions
have null `target_owner_id` and `observed_sink`; the change has
`behavior_hint: unknown` and null `changed_observable`. These are historical raw
producer bytes, **not** output from this new fixture or a current consumer run.

At the pinned producer base:

| Source | Exact blob | Load-bearing behavior |
| --- | --- | --- |
| [symbol declarations](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/7e57f3ab9d214fc87d6630decc398b00f99521e0/crates/perl-symbol/src/surface/decl.rs#L188) | `48488224285072e743cefd5e6fa63534d8c142a4` | Anonymous subs are walked but receive no declaration identity. |
| [symbol references](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/7e57f3ab9d214fc87d6630decc398b00f99521e0/crates/perl-symbol/src/surface/ref.rs#L137) | `fee85c62963d401589d297da9cde985baf8b74db` | `->()` / `&{}` function-call sentinels are excluded from named-call refs. |
| [relation emission](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/7e57f3ab9d214fc87d6630decc398b00f99521e0/crates/perl-ripr-facts/src/emitter/relations.rs#L165) | `fc494da240d05237f4ac7587ac51b0af6e5b4bf0` | Qualified factory calls produce owner/test relations with null oracle IDs; no returned-value edges. |
| [packet assembly](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/7e57f3ab9d214fc87d6630decc398b00f99521e0/crates/perl-ripr-facts/src/packet.rs#L647) | `9c1cac949819bfd047b478f644b759d823ef98dc` | Oracle annotation requires factory callable text in the assertion; `$c` assertions here do not contain it. |
| [boundary extraction](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/7e57f3ab9d214fc87d6630decc398b00f99521e0/crates/perl-ripr-facts/src/emitter/boundaries.rs#L20) | `7f48258bc02311cdfcb90d60c8d37164c75dd62b` | Eval is a boundary; no closure-return-specific conditional refusal fact exists. |
| [v1 schema](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/7e57f3ab9d214fc87d6630decc398b00f99521e0/schemas/ripr-perl-facts-v1.schema.json) | `afe904d219273ca9afb8332069b8ffad42f25575` | Closed object properties; no `returns_code_ref` field or closure relation kind. |

Source predicts null oracle links for all five cases. This prediction is not
promoted to fresh execution evidence. The driver can obtain that missing evidence
from an existing immutable executable, without implementing a producer:

```sh
python3 testdata/ripr/closure-return/probe.py \
  --producer /absolute/path/to/immutable/perl-ripr-facts \
  --out /absolute/path/to/new-output-directory
```

For each case it copies the actual fixture, creates a one-line `4 -> 5` unified
diff against the selected initializer/return, and invokes the canonical CLI:

```sh
/absolute/path/to/immutable/perl-ripr-facts ripr-facts \
  --schema ripr-perl-facts-v1 --root fixture \
  --base fixture-before --head fixture-after \
  --fact-classes files,owners,changes,tests,oracles,relations,dynamic_boundaries,limitations,provenance \
  --diff change.patch --out packet.json
```

It preserves raw packets, exact argv/exit, binary SHA-256 and packet SHA-256,
and writes a separately labeled inspection projection. It requires nonempty
change/oracle/relation paths and verifies the source-predicted missing linkage.
The diff names are caller assertions, not verified Git revision identities;
these packets are not ready for production ingestion or compatibility acceptance.
The replay path itself was not executed here because no admitted producer exists.

Fixture review found and removed factory-name words from assertion labels before
publication: the current annotation scans the entire assertion expression,
including labels. A label mentioning `bound` or `conditional` would contaminate
this missing-link characterization by accidentally matching the factory name.
The runtime controls were re-executed after that correction. Label/name spoofing
remains an additional source-backed weakness, not an executed producer result.

## Actual consumer allowlist and compatibility reconciliation

Consumer source was pinned from public main to
`1818f481c279b84634c32ee19e9c23d199459967`.
[Perl adapter](https://github.com/EffortlessMetrics/ripr-swarm/blob/1818f481c279b84634c32ee19e9c23d199459967/crates/ripr/src/analysis/language/perl/mod.rs)
blob: `ba24d7b9e8ca0d131278a4d92b9cdfa341d2358b`.

- [Producer names](https://github.com/EffortlessMetrics/ripr-swarm/blob/1818f481c279b84634c32ee19e9c23d199459967/crates/ripr/src/analysis/language/perl/mod.rs#L46):
  only `perl-ripr-facts`, `perllsp`, `perl-lsp`.
- [Fact classes](https://github.com/EffortlessMetrics/ripr-swarm/blob/1818f481c279b84634c32ee19e9c23d199459967/crates/ripr/src/analysis/language/perl/mod.rs#L1247):
  files, owners, changes, tests, oracles, relations, dynamic_boundaries,
  verify_commands, limitations, provenance. There is no closure-return class.
- [Decode and schema check](https://github.com/EffortlessMetrics/ripr-swarm/blob/1818f481c279b84634c32ee19e9c23d199459967/crates/ripr/src/analysis/language/perl/mod.rs#L178)
  deserializes the v1 struct, then validates ingestion. Neither the packet nor
  owner serde type opts into `deny_unknown_fields`; unknown fields therefore
  confer no semantic meaning. [SPEC-0064 lines 222-225](https://github.com/EffortlessMetrics/ripr-swarm/blob/1818f481c279b84634c32ee19e9c23d199459967/docs/specs/RIPR-SPEC-0064-perl-fact-packet-contract.md#L222)
  explicitly ignores future top-level fields but rejects unknown closed enums.
  In contrast, producer v1 JSON Schema forbids extra properties. Adding an
  unconsumed marker is neither schema-valid nor a consumer implementation.
- [Closed relation enum](https://github.com/EffortlessMetrics/ripr-swarm/blob/1818f481c279b84634c32ee19e9c23d199459967/crates/ripr/src/analysis/language/perl/mod.rs#L3290):
  direct_owner_call, package_reference, method_receiver, test_name_match,
  file_proximity, helper_call, fixture_setup, unknown. A new closure kind is a
  coordinated version/type change. Only direct_owner_call currently supports
  positive reachability; relabeling closure evidence as direct would bypass that
  policy rather than implement the missing edges.
- [Rule 20](https://github.com/EffortlessMetrics/ripr-swarm/blob/1818f481c279b84634c32ee19e9c23d199459967/crates/ripr/src/analysis/language/perl/mod.rs#L805)
  downgrades reachable-unrevealed evidence with a null oracle link and a
  same-test code-reference assertion. [Detection](https://github.com/EffortlessMetrics/ripr-swarm/blob/1818f481c279b84634c32ee19e9c23d199459967/crates/ripr/src/analysis/language/perl/mod.rs#L1813)
  recognizes arrow and both ampersand forms in packet expressions. This is a
  conservative cap, not resolution of the closure or its captures.
- [Ingestion](https://github.com/EffortlessMetrics/ripr-swarm/blob/1818f481c279b84634c32ee19e9c23d199459967/crates/ripr/src/analysis/language/perl/mod.rs#L1506)
  additionally requires `test_facts` for tests/oracles and verifies the declared
  fingerprint and request identity. The private model and selective v1
  fingerprint cannot silently become the shared v2 authority.

No RIPR process ran and no consumer verdict is claimed for these new fixtures.
The merged #10112 identity remains `50042a28a5362df26c865fcba2ec08a96a501479`;
the historical #9485 expected row is not current consumer acceptance.

## Concrete typed-contract delta for the existing owners

These are required typed concepts/test obligations for #9274/#9276 and the
existing relation/relevance leaves, **not implemented schema or new extraction
authority**. Use equivalent existing canonical types where available.

| Typed concept | Required references / discrimination | Owner |
| --- | --- | --- |
| Closure template | Distinct `ClosureTemplateId`, enclosing logical `OwnerId`, body/return anchor, source/content/generation; logical identity separate from byte range. | #9274 + canonical substrate/#9293 |
| Returned value origin | `ReturnSiteId`, template, optional producer-local lexical `BindingDefinitionId`; distinguish direct return from `sub -> $next -> return $next`. | #9293 |
| Factory result binding | `FactoryCallSiteId`, returned origin, destination lexical definition **and version**, bounded test/subtest scope; two calls in `instances.t` have different value origins even with one template. | #9293/#9288 |
| Code-reference invocation | Typed arrow/ampersand/braced-ampersand form, operand/binding version, factory origin and template, test/subtest, result value and actual oracle operand reference. | #9293/#9290/#9296 |
| Capture relevance | Canonical captured place, initialization/definition version, capture read/write, closure result flow, changed delta and exact/candidate/unlinked relevance. Owner reachability is separate. | #9285/#9296 |
| Refusal/currentness | Typed dynamic/conditional/eval or unsupported-substrate reason with scoped references; all decision edges resolve within the same source/content/generation. | #9274/#9276 + semantic owners |

Acceptance expectations:

1. `direct.t`: explicit owner -> returned template -> callsite result binding ->
   each of the three invocations -> assertion actual operand. A marker alone
   fails. Exact changed-capture relevance requires the canonical value/control
   proof that the `direct(1)` initializer selects the changed value.
2. `bound.t`: additionally require `$next`'s reaching definition and return edge;
   do not recover it by matching variable names. Its three assertions observe
   the same returned binding and advancing captured place.
3. `instances.t`: preserve both factory callsites/result bindings. Never attach
   the oracle on `$other` to `$changed`'s value origin or claim relevance to the
   changed selected initializer. If canonical branch/value facts are missing,
   return candidate/unlinked with a reason, not exact or a repair-gap claim.
4. `conditional.t` / `evaluated.t`: no exact returned-target link in this first
   slice. Emit a scoped typed conditional ambiguity / eval boundary or missing
   canonical-substrate limitation. Merely omitting a fact is insufficient. Do
   not invent a closed v1 enum spelling. More precise future conditional facts
   belong to the existing canonical owner and a separately reviewed profile.
5. Validator negatives: substitute another callsite/binding version or template;
   dangle a return/capture/oracle-operand reference; mix generations; overwrite
   a binding; cross a sibling subtest. All must reject exact linkage or preserve
   an explicit limitation. Repeated execution at one static callsite (loops,
   recursion), alias/escape and runtime instance identity remain unsupported
   unless modeled; a static callsite ID is not a runtime instance ID.
6. Once v2 exists, run the real producer -> shared validator -> pinned consumer
   with no hand normalization. A known wrong instance merge must fail the
   acceptance expectation while the direct/bound positive controls can pass.
   The Perl mutant behavior above is an independent oracle, not an implementation
   of the RIPR verdict or proof of the static flow facts.

## Exact execution limit and next action

Executed in the isolated proof checkout, with only the saved installed toolchain
environment selected (no setup/install script):

```sh
export RUSTUP_HOME=/workspace/tooling/rustup
export CARGO_HOME=/workspace/tooling/cargo
export DEVPLANE=/workspace/tooling/devplane
export PATH="/workspace/tooling/cargo/bin:$PATH"
scripts/cargo-admitted --preflight build -p perl-ripr-facts \
  --bin perl-ripr-facts --locked --offline
```

Observed exit **75 / DENY**, `lease_acquired: false`,
`free_bytes: 31792128000`, `required_free_bytes: 42949672960`,
`shortfall_bytes: 11157544960`. This was repository capacity admission, not an
automatic approval rejection. No build started; no budget/reserve/runner
workaround or installation was attempted. No existing producer binary was found
in this saved environment. Raw GitHub CLI GET was also Forbidden; connected
GitHub read tools supplied the linked public source instead.

Next owner: #9274/#9276 take the typed concepts into their existing contract,
with #9293/#9285/#9296 supplying canonical edges. On already-admitted capacity,
run the supplied replay driver against an immutable producer from the pinned
source, publish its untouched output, and bind the eventual v2 result to the
shared consumer/validator handshake. Fresh producer output, v2 validation,
consumer execution, current-source qualification and independent candidate
acceptance remain open. This proof branch is not merge-ready implementation.
