# Reusable LSP runtime identity and state vocabulary

Status: normative  
Authority: #11045 under architecture #7384 and checked train #10360  
Machine source: `.spec/11045-lsp-runtime-vocabulary/contract.v1.json` plus its four versioned fragments

## Claim boundary

Normative identities, state propositions, legal joins, and checked journeys only; no runtime implementation or migration.

This contract freezes propositions and legal joins. It does not freeze production types, require one implementation object, or authorize module movement, package publication, or runtime migration.

## Governing laws

**Request state.** One canonical request entry advances on orthogonal axes; no total phase value stands in for the full state.

**One authority.** One canonical semantic owner does not require one object, actor, global lock, or mutable store.

**Currentness.** Currentness is opaque evidence minted and validated only by its owning authority.

Client consumption is outside reusable-runtime claim authority. The runtime may prove output admission, write, flush, failure, and a bounded delivery fate. It may not turn any of those into a claim that a client consumed the message.

## Identity domains

`ConnectionId` identifies one accepted physical or logical connection occurrence. `SessionId` identifies one lifecycle/runtime incarnation on that connection. They remain distinct even when the selected architecture makes them one-to-one.

`MessageId` identifies one observed message occurrence. `IngressSequence` orders those occurrences inside one connection. Equal bytes observed twice remain two messages.

`RequestKey` is the exact numeric-or-string JSON-RPC id plus connection and session scope. Numeric `1` and string `"1"` are different. The same raw variant on another connection or session is another request. Reuse inside one scope becomes legal only after exact owner cleanup.

`RouteId`, `OperationId`, and `WorkId` are different propositions: selected route, semantic application operation, and concrete queued/running work occurrence. A request may create more than one work occurrence without changing its request identity.

`ReservationId` identifies one capacity/accounting obligation. Queue membership and work count do not establish a reservation.

`MutationBarrierId` orders state-changing work. `DeadlineId`, `TaskId`, and `FailureId` identify their own bounded obligations or occurrences; none is a request identity.

`PublicationId` identifies one committed causal publication. `OutputId` identifies one writer obligation projected from that publication. `OutputSequence` orders obligations at one writer authority.

`ProgressKey` and `ReverseRequestKey` remain separate from `RequestKey` and from each other, even where their raw wire values have the same representation.

`CurrentnessToken` is opaque owner-minted evidence. Consumers ask the owning authority to validate it. They do not parse it as a timestamp, URI, hash, generation number, or boolean.

## Orthogonal state axes

A canonical request entry carries independently established propositions across:

1. protocol/message admission;
2. tracking and registration;
3. execution;
4. control;
5. terminal decision;
6. causal publication;
7. output and delivery;
8. cleanup.

Protocol lifecycle, connection state, application readiness, and runtime task settlement are separate axes around that request model.

The following statements are therefore invalid:

- protocol-valid means reserved, registered, or execution-admitted;
- application-completed means terminal-selected or publication-committed;
- terminal-selected means output-admitted, written, flushed, or delivered;
- publication-committed means output-admitted, written, flushed, or client-consumed;
- writer failure changes the already selected terminal cause;
- connection-closed means protocol-exited, task-settled, delivery-settled, or owner-cleaned;
- protocol-exited means connection-closed or owner-cleaned;
- map absence or a zero counter proves cleanup.

## Boundary terms

**ApplicationCompletion** records that application work produced a result or failure. It is evidence presented to terminal authority; it is not a terminal decision.

**TerminalSignal** is one candidate cause such as success, cancellation, timeout, stale result, or failure.

**TerminalDecision** is the single selected request outcome. Later application completion and writer failure cannot rewrite it.

**DeliveryFate** records the writer/transport result for an output obligation. It is independent from terminal cause and never means client consumption.

## Exact stage language

Use the exact proposition rather than overloaded shorthand:

```text
message observed
protocol valid
request tracking reserved
request registered
execution admitted | execution rejected
enqueued
running
application completed
terminal selected
causal publication committed | stale publication rejected
output prepared
output admitted
output write committed
output flush committed | output failed
delivery fate known
task group settled
owner cleaned
connection closing | connection closed
```

`accepted`, `admitted`, `active`, `pending`, `completed`, `terminal`, `published`, `delivered`, and `closed` are forbidden without a qualified proposition.

The current `pending_request_ids` set is a scheduler-protection projection. It is not the canonical request entry, terminal obligation, publication state, delivery state, or cleanup proof.

## Checked journeys

### Ordinary success

Each stage is established by its own authority: observation, protocol validation, reservation, registration, admission, queueing, running, application completion, terminal selection, publication, output preparation/admission, write/flush, delivery-fate settlement, task settlement, and cleanup.

### Execution rejection after registration

Registration creates terminal and cleanup obligations even when execution is rejected. Reservation and request ownership remain until the canonical terminal, publication, delivery, task, and cleanup paths settle.

### Cancellation during blocking work

Cancellation may select the terminal outcome while physical work continues. Late application completion cannot select another terminal outcome or commit another publication.

### Stale publication

Application completion remains evidence. Currentness authority rejects publication, so no publication or output identity is committed for the stale result.

### Writer failure

Terminal and publication remain unchanged. Output fate becomes failed. Cleanup remains incomplete until output, task, and request resources settle.

### Graceful shutdown

Protocol transition, application readiness, connection closeout, task settlement, output settlement, and request cleanup are each explicit.

### Transport loss

Connection close does not invent protocol exit, terminal completion, output settlement, task settlement, or cleanup.

### Request-id reuse

A raw numeric or string id cannot be reused while prior ownership remains. Reuse after exact cleanup is legal only inside the same connection/session domain; cross-session equality is never inferred.

## Downstream use

Later request, reservation, event, scheduler, terminal, publication, writer, lifecycle, and checker contracts reference these stable ids. They may add domain-specific types and implementations, but they may not privately redefine these propositions or legal joins.

<!-- BEGIN CHECKED LSP RUNTIME VOCABULARY INDEX -->
## Checked machine index

- **Schema:** `lsp_runtime_identity_state.v1` v1
- **Authority:** #11045 under #7384; checked train #10360
- **Axes:** `application_readiness`, `cleanup`, `connection`, `control`, `execution`, `output_delivery`, `protocol_admission`, `protocol_lifecycle`, `publication`, `task_settlement`, `terminal`, `tracking_registration`
- **Identities:** `connection_id`, `currentness_token`, `deadline_id`, `failure_id`, `ingress_sequence`, `lifecycle_transition_id`, `message_id`, `mutation_barrier_id`, `operation_id`, `output_id`, `output_sequence`, `progress_key`, `publication_id`, `request_key`, `reservation_id`, `reverse_request_key`, `route_id`, `session_id`, `task_id`, `work_id`
- **Boundary terms:** `application_completion`, `delivery_fate`, `terminal_decision`, `terminal_signal`
- **States:** `application_completed`, `application_ready`, `application_unavailable`, `cancellation_requested`, `client_consumed`, `connection_closed`, `connection_closing`, `connection_open`, `deadline_expired`, `delivery_fate_known`, `enqueued`, `execution_admitted`, `execution_rejected`, `message_observed`, `output_admitted`, `output_failed`, `output_flush_committed`, `output_prepared`, `output_write_committed`, `owner_cleaned`, `protocol_exited`, `protocol_initialized`, `protocol_shutdown_requested`, `protocol_uninitialized`, `protocol_valid`, `publication_committed`, `request_registered`, `request_tracking_reserved`, `running`, `stale_publication_rejected`, `task_group_settled`, `terminal_selected`
- **Relationships:** `admitted_requires_prepared`, `application_completed_forbids_publication`, `application_completed_forbids_terminal`, `application_completion_forbids_publication`, `application_completion_forbids_terminal`, `cancel_permits_terminal`, `cleanup_permits_request_reuse`, `cleanup_requires_delivery_fate`, `cleanup_requires_task_settlement`, `cleanup_requires_terminal`, `connection_closed_forbids_cleanup`, `connection_closed_forbids_delivery_fate`, `connection_closed_forbids_protocol_exit`, `connection_closed_forbids_task_settlement`, `delivery_forbids_client_consumption`, `delivery_forbids_terminal`, `enqueue_requires_admission`, `execution_requires_registered`, `failure_permits_fate`, `failure_requires_admitted`, `flush_forbids_client_consumption`, `flush_permits_fate`, `flush_requires_write`, `observed_precedes_valid`, `output_admission_forbids_client_consumption`, `output_failure_independent_terminal`, `prepared_requires_publication`, `protocol_exit_forbids_cleanup`, `protocol_exit_forbids_connection_closed`, `protocol_initialized_independent_readiness`, `publication_forbids_client_consumption`, `publication_forbids_flush`, `publication_forbids_output_admission`, `publication_forbids_write`, `publication_requires_terminal`, `registered_requires_reserved`, `rejection_requires_registered`, `request_independent_progress`, `request_independent_reverse`, `request_requires_connection`, `request_requires_session`, `reverse_independent_progress`, `running_requires_enqueue`, `session_requires_connection`, `signal_forbids_decision`, `stale_rejection_forbids_publication`, `stale_rejection_requires_completion`, `terminal_forbids_delivery_fate`, `terminal_forbids_flush`, `terminal_forbids_output_admission`, `terminal_forbids_write`, `terminal_independent_delivery`, `terminal_requires_registered`, `valid_forbids_execution`, `valid_forbids_registered`, `valid_forbids_reserved`, `write_forbids_client_consumption`, `write_requires_admitted`
- **Ambiguous terms:** `accepted`, `active`, `admitted`, `closed`, `completed`, `delivered`, `pending`, `published`, `terminal`
- **Journeys:** `cancellation_during_work`, `execution_rejection`, `graceful_shutdown`, `ordinary_success`, `request_id_reuse`, `stale_publication`, `transport_loss`, `writer_failure`
<!-- END CHECKED LSP RUNTIME VOCABULARY INDEX -->
