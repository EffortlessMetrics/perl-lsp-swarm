------------------- MODULE WorkspaceIndexResourceRejection -------------------
EXTENDS Naturals, FiniteSets

\* As-is, serial abstraction of the production IndexCoordinator, not the
\* separate state_machine module. WSI-COORD-002 explicitly records that state()
\* drains a resource rejection; this model does NOT establish observation purity.
CONSTANTS Kept, Rejected, ConsumeRejection, ClearRejection, PublishRejected, DropAccepted

ASSUME Kept # Rejected

VARIABLES admitted, rejected, pending, state, transitions, contentVersion,
          unsafeFullQuery

vars == <<admitted, rejected, pending, state, transitions, contentVersion,
          unsafeFullQuery>>

\* The fixture has already admitted one file and explicitly transitioned Ready.
Init ==
    /\ admitted = {Kept}
    /\ rejected = FALSE
    /\ pending = FALSE
    /\ state = "Ready"
    /\ transitions = 0
    /\ contentVersion = 0
    /\ unsafeFullQuery = FALSE

\* One new identity at max_files = 1. No eviction or replacement is permitted.
RejectNewFile ==
    /\ ~rejected
    /\ admitted' = (IF DropAccepted THEN {} ELSE admitted)
                    \cup (IF PublishRejected THEN {Rejected} ELSE {})
    /\ rejected' = TRUE
    /\ pending' = TRUE
    /\ UNCHANGED <<state, transitions, contentVersion, unsafeFullQuery>>

\* Updating the already-admitted file at capacity is still allowed.
UpdateAcceptedFile ==
    /\ contentVersion = 0
    /\ contentVersion' = 1
    /\ UNCHANGED <<admitted, rejected, pending, state, transitions, unsafeFullQuery>>

\* Production state() takes the pending rejection before returning a snapshot.
\* ClearRejection is deliberately a fault switch, not a proposed product option.
DrainObservation ==
    LET consume == pending /\ ConsumeRejection
    IN  /\ pending' = IF consume /\ ClearRejection THEN FALSE ELSE pending
        /\ state' = IF consume THEN "Degraded" ELSE state
        /\ transitions' = transitions + (IF consume THEN 1 ELSE 0)
        /\ UNCHANGED <<admitted, rejected, contentVersion>>

Observe ==
    /\ DrainObservation
    /\ UNCHANGED unsafeFullQuery

\* Production query() obtains state() before choosing its full/partial closure.
Query ==
    /\ DrainObservation
    /\ unsafeFullQuery' = unsafeFullQuery \/ (rejected /\ state' = "Ready")

Next == RejectNewFile \/ UpdateAcceptedFile \/ Observe \/ Query
Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ admitted \subseteq {Kept, Rejected}
    /\ rejected \in BOOLEAN
    /\ pending \in BOOLEAN
    /\ state \in {"Ready", "Degraded"}
    /\ transitions \in Nat
    /\ contentVersion \in {0, 1}
    /\ unsafeFullQuery \in BOOLEAN

AcceptedPreserved == Kept \in admitted
NoRejectedPublication == Rejected \notin admitted
NoFullQueryAfterRejection == ~unsafeFullQuery
SingleDrain == transitions <= (IF rejected THEN 1 ELSE 0)

=============================================================================
