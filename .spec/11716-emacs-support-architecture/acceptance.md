# Acceptance Criteria: #11716 — durable Emacs support architecture and evidence boundaries

This is a checked, declarative architecture contract. It implements no Emacs
behavior, host runner, adapter, receipt, train manifest, packet, CI route, or
support claim.

## §Behavior

| Input / condition | Required result | Evidence boundary |
|---|---|---|
| An exact Emacs/client subject is selected | Identity binds product `perl-lsp`, executable `perllsp`, launch `perllsp --stdio`, integration `generic_lsp`, and one of five client families at exact digests | Version strings or another server never satisfy identity |
| A plane truth is consumed | Stable architecture, exact current-tree, live candidate, and behavior/public/support evidence stay non-substitutable per the ten-law table | issue/PR closed is not tree state; a receipt is not host observation |
| Host reliability is claimed | Generic mechanics come from #10894; #8734 proves Emacs adoption/conformance only | #8734 is never cited as generic process authority |
| A receipt is produced or consumed | #7777/#10527 semantics and #11360 observation → #11361 producer are consumed | No Emacs-local receipt ontology duplicates them |
| Diagnostics are evidenced | Bundled push, standalone pull with result-id lifecycle, and lsp-mode paths are observed per exact subject | Synthetic fixtures prove negotiation shapes only |
| Root evidence is claimed | `stock_project_discovery`, `standard_user_project_override`, and future `custom_repository_helper` stay distinct | Manual binding is never stock; rootUri is never semantics |
| A public/support stage is claimed | `exact_source_local`, `release_candidate`, `public_artifact` and the four registration/upstream stages each require exact direct evidence | No relabeling, inheritance, or upstream-accepted→built-in inference |
| A platform claim is made | Linux evidence never implies macOS/Windows/TRAMP; optional breadth stays optional | Unsupported capability is `not_proven`, never inferred |
| A leaf is packetized or scheduled | One proposition, dependency classes, writer slot + conflict key, claim ceiling, falsifier, controls, rollback/stop exist before work | Controllers, fan-ins, and external gates never become builder leaves |
| An actual-host journey is planned | Checked #11768 journey/cell identities are selected | No free-form receipt truth; no host driver created by #11768 |
| The train graph changes after publication | #11770 revision/impact operations classify and invalidate affected work | No silent graph-byte replacement of decisions |

## §Hazards

| Class | Invariant | Surface | Required adversarial check |
|---|---|---|---|
| Subject identity | Exact subject identity is non-substitutable | `context.md` identity decisions | version-string or wrong-server substitution is rejected |
| Plane substitution | The four truth planes never substitute | `context.md` four truth planes | each of the ten non-substitution laws is individually rejectable |
| Authority drift | #10894 generic vs #8734 Emacs split holds | `context.md` authority and ownership | #8734-as-generic-authority design is rejected |
| Receipt duplication | #7777/#10527/#3983/#10872/#10881 are consumed only | authority + non-goals | a cloned Emacs-local receipt, spec, or packet ontology is rejected |
| Cohort flattening | Push/pull/client/source generations stay independent | `context.md` diagnostic cohorts | one generation filling another is rejected |
| Root conflation | Discovery, override, and semantics stay distinct | `context.md` project/root contract | manual-root-as-stock and rootUri-as-semantics are rejected |
| Stage inflation | Local, release-candidate, and public stages stay distinct | `context.md` public stages | local→public and source→released relabeling are rejected |
| Platform overreach | Linux first-mile never implies other platforms | `context.md` platform section | Linux-pass-as-macOS/TRAMP claim is rejected |
| Role leakage | Controller/fan-in/external gates keep non-builder dispositions | `context.md` agentic execution law | reclassification into a builder leaf is rejected |
| Mutable leakage | Durable bytes carry stable identities only | `context.md` stable vs mutable | current SHA/PR/check/writer state in spec bytes is rejected |
| Journey ambiguity | Actual-host leaves select checked #11768 cells | authority + per-leaf contracts | free-form journey/receipt truth is rejected |
| Revision silence | Material graph change routes through #11770 | `context.md` topology | silent manifest replacement is rejected |
| Determinism | Same tree produces same ordered check output twice | `checklist.md` proof | second run is byte-clean |

## §Contracts

Per-leaf contracts with explicit evidence boundaries. Each row is one
proposition with its claim/evidence ceiling; the durable ordering is in
`context.md` §Stable implementation topology and dependency ordering.

| Leaf | One proposition | Evidence boundary (ceiling) |
|---|---|---|
| #11717 checked leaf-spec dispositions fan-in (E02) | every stable Emacs node gets exactly one checked spec disposition or explicit reviewed non-builder disposition | changes no behavior, proof result, or candidate state; engine mechanics belong to #11751 |
| #11718 exact-tree context fan-in (E04) | each authoritative node maps onto one exact current tree for fresh-agent navigation | paths/symbols are navigation evidence, never durable scope authority; live state stays #10930 |
| #11719 packet adapter (E06) | joins architecture, train, specs, tree context, and live observation into #10872/#10881 packets | no Emacs packet schema, no model invocation, no GitHub mutation, no scheduling |
| #11744 subject manifest/resolver core + bundled Eglot | one immutable checked manifest/resolver/cache authority proven by bundled Eglot subjects | manifest identity is intended input, never runtime proof; no journey/profile/root/support claim |
| #11745 external Eglot subjects | one released and one pinned-source Eglot row through #11744 semantics | released/source non-interchangeable; no semantic support, upstream acceptance, or public claim |
| #11746 external lsp-mode subjects | one released and one pinned-source lsp-mode row through #11744 semantics | no client-selection, journey, or support claim earned |
| #11747 Eglot stock-root matrix | the canonical twelve-case matrix through actual stock Eglot/project.el, unprebound | observation only: no semantics, no lsp-mode inheritance, no custom backend, no expected-root injection |
| #11748 lsp-mode stock-root matrix | the canonical twelve-case matrix through actual stock lsp-mode, unprebound | observation only: no Eglot/project.el inheritance, no custom backend |
| #11749 Eglot root semantics/overrides | behavior-bearing root/isolation/configuration verdicts from #11747 via sentinels and root-sensitive queries | correct rootUri is not a pass; override stays distinct from stock; product defects transfer to owners |
| #11750 lsp-mode root semantics/overrides | behavior-bearing lsp-mode root/isolation/configuration verdicts from #11748 | same boundary; no Eglot inheritance; defects transfer |
| #11751 spec disposition engine | one deterministic mechanical adapter over shared #3983 machinery for plan/compile/check/explain | no second repository-wide spec engine; full population stays in #11752-#11755 |
| #11752 substrate/adapter spec population | checked dispositions for #10894/#8734/#8755/#11744-#11746/#11360/#11361/#8776/#8795/#10527-consumption | compiler mechanics unchanged; generic-vs-Emacs split explicit |
| #11753 profile/actual-host spec population | checked dispositions for #8819/#8821 profiles and #8822-#8830 actual-host verdicts | ceilings prevent profile→host and source→release widening; cohorts independent |
| #11754 root/public/projection spec population | checked dispositions for #11366/#8834/#11747/#11748/#8838/#11749/#11750/#8842/#8846/#8849/#8853/#8858/#8862/#8865 | fixture→observation→semantics and local→public separations preserved; certification repairs nothing |
| #11755 controller/external dispositions | non-builder dispositions for #7979/#8706/#9310/#9413/#9374-line/#7692/#7707/#7774-#7776/#7989/#7995 | external/upstream gates never become ordinary coding leaves |
| #11756 context engine | deterministic `emacs_node_context.v1` resolver/renderer bound to exact tree digests | fails closed on ambiguity; no live GitHub/network; no broad write permission |
| #11757 substrate/adapter context population | exact-tree mappings for substrate, subject, adapter, profile, and host leaves | navigation only; engine defects return to #11756 |
| #11758 root/public context population | exact-tree mappings for root, public replay, registry, docs, certification | generated docs never registry authority; binaries never public-artifact authority |
| #11759 deterministic routing scenarios | packet→typed-action/non-pass suite for scenario classes A-L without real models | no Emacs-local dogfood schema; #11114 vocabulary consumed |
| #11760 real agent/reviewer cohorts | bounded fresh-agent, lower-cost, reviewer, resume, and refusal cases against current packets | evaluation observation only; no ranking/scheduling policy; mechanics defects follow up separately |
| #11768 governed host journeys | versioned checked `emacs_host_journeys.v1` journey/cell manifest with offline check/explain | creates no host driver, adapter, observation, receipt result, or support claim; missing canonical truth blocks a cell |
| #11770 semantic revision governance | versioned change model plus diff/check/impact operations for material graph movement | placed after #10918; edits no issue bodies; no live GitHub mutation; #10554 primitives reused where concrete |

## §API-Shape

No Rust or public API is introduced. The names below are declarative semantic
contract terms owned by their respective issues; none is implemented here.

| Item | Kind | Contract shape | Dup-risk / owner |
|---|---|---|---|
| `emacs_train.v1` | manifest identity | stable checked implementation topology preserving this bundle | #10918; never a second architecture authority |
| `emacs_host_journeys.v1` | manifest identity | governed journey/fixture-expectation/cell catalog | #11768; no host driver or receipt result |
| `emacs_node_context.v1` | projection identity | exact-tree navigation context per node | #11718/#11756; navigation evidence only |
| `editor_client_compat.v1` | receipt schema (consumed) | observation→receipt producer output | #11361/#10527; never cloned Emacs-locally |
| `agent_implementation_packet.v1` | packet schema (consumed) | bounded builder/reviewer payloads | #10872/#10881; Emacs supplies fields only |

N/A — no public function, type, protocol field, crate, dependency, workflow,
or support surface changes in this spec-only PR.

## §Test-Grid

The rows are negative controls for candidate designs and specifications; they
are intentionally discriminating rather than implementation tests. They are
the fifteen required falsifiers of #11716 in fixed order.

| # | Scenario | Kind | Required verdict |
|---:|---|---|---|
| 1 | Runner, profile, or schema presence is represented as actual Emacs host support | negative | reject; only typed host observation proves host support |
| 2 | A client `shutdown_completed` event or exit status 0 alone proves descendant cleanup | negative | reject; cleanup requires independent observation via #10894/#8734 |
| 3 | A synthetic capability profile becomes actual-client evidence | negative | reject; profiles prove negotiation/result shapes only |
| 4 | A manually bound fixture root becomes stock project discovery | negative | reject; stock discovery is observed unprebound (#11747/#11748) |
| 5 | A correct rootUri becomes root-sensitive semantics | negative | reject; semantics require behavior-bearing proof (#11749/#11750) |
| 6 | Local exact-source evidence becomes a public artifact claim | negative | reject; public stages require exact direct evidence |
| 7 | An upstream source head becomes a released client subject | negative | reject; released identity requires package/archive identity |
| 8 | Accepted-unreleased upstream integration becomes shipped built-in discovery | negative | reject; released built-in state requires its own evidence |
| 9 | Linux evidence becomes a macOS, Windows, or TRAMP support claim | negative | reject; platforms and TRAMP require their own proof |
| 10 | Eglot evidence becomes lsp-mode, or one client generation fills another | negative | reject; cohorts stay client- and generation-exact |
| 11 | Protocol traffic becomes a host-visible semantic pass without #11360/#11361 | negative | reject; host visibility flows only through observation and producer |
| 12 | A controller, fan-in, or external gate becomes an ordinary builder leaf | negative | reject; roles keep their non-builder dispositions |
| 13 | Current SHA/PR/check/model/writer state enters durable spec bytes | negative | reject; durable specs carry stable identities only |
| 14 | An Emacs-local receipt/spec/packet ontology duplicates #7777/#10527/#10872/#10881 | negative | reject; shared authorities are consumed, not cloned |
| 15 | Optional breadth becomes an initial-Linux hard prerequisite without a selected claim requiring it | negative | reject; optional breadth stays optional (#9310) |

## §Blast-Radius

| Consumer / surface | Impact | Required update |
|---|---|---|
| #10918 `emacs_train.v1` | Consumes this bundle as durable architecture; must preserve its boundaries | Separate E01 PR; no graph byte amends a decision here |
| #11717/#11718/#11719 control planes and #11751-#11760 engines/populations | Consume identity, authority, ceiling, and ordering decisions | Separate spec/context/packet PRs |
| #11744-#11750 subject/root lanes, #8755/#8834/#8838 fan-ins | Consume subject, cohort, and root evidence boundaries | Separate implementation PRs |
| #11768/#11770 | Consume journey governance and revision placement | Separate governance PRs |
| #9310/#9413 and optional-breadth controllers | Referenced as separate trains; not modified | None |
| `.spec/10894-editor-host-reliability/` | Referenced sibling projection; unchanged | None |
| Docs/policy/test-scaffolding surfaces | Described qualitatively as current-tree basis; not modified | None |
| Host/editor/CI/support surfaces | No impact in this PR | Must-not-touch boundary |

Must-not-touch: `crates/`, `xtask/`, editor/client adapters, host harnesses,
`.github/workflows/`, CI routes, registry/docs mutations, support/public
claims, generated status, and external processes.

## §Coverage-Map

| #11716 acceptance item | Covered by |
|---|---|
| Every stable identity/evidence/root/public/upstream/platform/optional boundary has one durable decision or exact reference | `context.md` identity, authority, cohorts, root, public stages, platform sections |
| #7777/#10527/#11360/#11361 ownership explicit and non-duplicated | `context.md` authority; falsifiers 11, 14 |
| Push/pull/client/source/released/manual/public evidence dimensions independent | `context.md` diagnostic cohorts and public stages; falsifiers 1, 6, 7, 10 |
| Project discovery and behavior-bearing root semantics independent | `context.md` project/root contract; falsifiers 4, 5 |
| Initial Linux cut and later platform/optional/upstream programmes separate | `context.md` platform section; falsifiers 8, 9, 15 |
| Stable/current-tree/live/support truth planes explicit and non-substitutable | `context.md` four truth planes; falsifier 13 |
| Every concrete leaf compilable later without controller archaeology | `context.md` topology + `§Contracts` per-leaf table |
| Shared spec/builder/reviewer/close authorities consumed, not cloned | `context.md` authorities and alternatives; falsifier 14 |
| Falsifier mutations fail deterministically | `§Test-Grid` rows 1-15 + `checklist.md` structural checker |
| Generated projection deterministic and second-run clean | `checklist.md` two-run SHA-256 proof |
| No product behavior, host execution, readiness, candidate, registry/support, release, or external changes | Scope boundary, blast radius, and claim boundary below |

## Scope, rollback, and proof claims

- **In scope:** only the three files in `.spec/11716-emacs-support-architecture/`.
- **Rollback:** revert this bundle's commit; the issues retain full authority.
  Any projection already derived from this bundle is invalidated or reverted
  through its owning issue (material graph movement routes through #11770
  revision governance), never by silently keeping or editing this bundle's
  bytes.
- **Transfer:** transfer only with an exact current subject, evidence
  inventory, and named receiving owner; otherwise remain `not_proven`.
- **Stop:** stop and return to #11716 when a boundary above would have to be
  weakened to make a downstream check green, or when a material authority
  decision is contradicted by current main.
- **Claim boundary:** this PR proves a durable checked architecture contract
  and deterministic structural inspection only. It does not prove Emacs
  behavior, host execution, subject materialization, root semantics, public
  artifacts, registry/docs state, support, or release readiness.
