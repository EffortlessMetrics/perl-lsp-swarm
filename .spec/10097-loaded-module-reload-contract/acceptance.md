# Acceptance Criteria: #10097 — loaded-module reload contract (R01)

This is a checked, declarative contract plus deterministic model/fake-runtime
validation. It implements no runtime reload, no debugger command, no wire
registration, no capability advertisement, and no editor surface. The
executable authority is `crates/perl-dap/src/reload/`; this bundle is the
machine-checkable corpus bound to it by `cargo test -p perl-dap`.

## §Behavior

| Input / condition | Required result | Evidence boundary |
|---|---|---|
| A proposed reload subject is classified | Exactly one of the thirteen closed dispositions applies, deterministically under the frozen precedence (readiness → runtime → presence → authority → mapping → classification → dirty → stale → active frame → eligible); only `eligible_source_backed_perl_module` admits | Classification corpus (15 documents, all 13 dispositions reached); unit precedence tests |
| Identity is presented as a basename, package name, or path spelling alone | Binding fails with `insufficient_subject_identity`; classification yields `source_not_exact_or_stale`; no plan is produced | Fixtures 15 + negative control 1 |
| The subject is bound, then the session/process is replaced, the observation goes stale, or the saved digest moves | The binding is no longer current; classification yields `source_not_exact_or_stale`; the subject must re-bind | Fixtures 9, 10; subject currentness tests |
| Dirty client source is presented as the saved runtime subject | Refusal `dirty_or_unsaved_source`; the adapter subject is saved disk source only | Fixture 4; negative control 2 |
| An active frame executes in the target module | Refusal `active_frame_in_target` — no earned rule admits it in the initial cohort | Fixture 2; negative control 4 |
| XS, source-filter/compile-hook, or generated/eval subjects are proposed | Refusal by class before any debugger mutation | Fixtures 6–8; negative controls 5a–5c |
| The same module/package name resolves under two include roots | Refusal `ambiguous_runtime_mapping`; no exact runtime subject binds | Fixture 3 |
| The subject path lies outside the validated launch root | Refusal `outside_launch_authority` | Fixture 5 |
| A transaction fails before `runtime_mutation_begins` | Terminal `failed_before_mutation`; no generation advance; empty invalidation plan | Transaction fixtures 3–4; generation tests |
| A timeout, transport loss, or ambiguous response occurs at or after `runtime_mutation_begins` | Terminal `indeterminate_possibly_applied`; the runtime-module generation advances; the full invalidation table applies; the outcome never projects as clean or empty | Transaction fixtures 2, 5; negative controls 6–7 |
| The runtime accepts and reads back the replacement | Terminal `reloaded`; the generation advances; the full invalidation table applies | Transaction fixtures 1, 6 |
| Generation monotonicity is queried | `RuntimeModuleGeneration` is per-process monotonic, advances on both `reloaded` and `indeterminate_possibly_applied`, never on refusals or pre-mutation failures, never rolls over at exhaustion, and resets only on process/session replacement; retained observations are bounded at 128 and fail closed | Generation unit tests |
| Old DAP objects are queried after a terminal mutation outcome | Every enumerated object kind has a disposition: inspection references and exception/stop facts stale by generation composition (runtime-module OR suspension), positional module ids/observations/source reads/applied installations/retained query results always stale, thread references re-projected as adapter projections, durable client breakpoint configuration preserved for #10102 | Invalidation unit tests; negative control 7 |
| The request surface is reviewed | A namespaced/versioned custom family with correlation identity; raw paths, debugger commands, and Perl expressions are refused; a bare standard DAP command name can never be the family; a standard capability spelling is never invented; advertisement before R04 proof is refused | Surface unit tests; negative controls 8a–8c, 9 |
| A mechanism claim is reviewed | Compile success, prompt observation, and package replacement are never claimed as proof; availability never grants product authority (Class::Refresh is a measured compatibility subject only); every mechanism record carries limitation statements including the shared Perl runtime truths | Mechanism unit tests; negative controls 3, 10 |
| Any vocabulary is widened | Unknown spellings fail closed at every parse site; the schema and the Rust enums are kept in exact sync by a fixture-driven test | Schema-sync test |

## §Required fixtures

1. Ordinary source-backed `.pm` module, no active frame → admitted.
2. Active frame in that module → refused (`active_frame_in_target`).
3. Same module/package names under two include roots → refused
   (`ambiguous_runtime_mapping`).
4. Dirty editor source versus saved disk source → refused
   (`dirty_or_unsaved_source`).
5. Module outside launch authority → refused (`outside_launch_authority`).
6. XS module → refused by class.
7. Source-filter/compile-hook module → refused by class.
8. Generated/eval source → refused by class.
9. Module removed or renamed after loading → identity not current →
   refused (`source_not_exact_or_stale`).
10. Process/session replacement → stale binding → refused
    (`source_not_exact_or_stale`).
11. Ambiguous timeout after the mutation boundary →
    `indeterminate_possibly_applied`, generation advances, never clean.

## §Negative controls

Fails when: basename/package alone authorizes (1); dirty source is
represented as the saved subject (2); compile success is represented as
reload success (3); an active target frame is accepted without an earned
rule (4); XS/source-filter/generated modules enter the initial cohort
(5a–5c); a post-mutation timeout leaves old generations current (6); old
frame/value identities survive a possibly-applied outcome (7); a client
can submit raw paths, debugger commands, or Perl expressions (8a–8c); a
standard DAP capability is invented (9); Class::Refresh or another
dependency becomes product authority merely because it is available (10).

## §Boundaries

- No live debuggee mutation, no debugger command, no injected runtime
  helper code (that is #10098).
- No wire format, family registration, or generated contracts (that is
  #10138).
- No session wiring, invalidation execution, or breakpoint reconciliation
  (that is #10102).
- No editor command, save watcher, or save policy (R05A/R05B).
- No capability advertisement; the family stays unadvertised until R04.
- The model/fake-runtime fixtures validate the contract only; they do not
  prove live reload.
