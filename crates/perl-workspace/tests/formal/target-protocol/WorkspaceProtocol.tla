----------------------- MODULE WorkspaceProtocol -----------------------
EXTENDS Naturals, FiniteSets

CONSTANTS Tickets, Capacity, MaxVersion, MaxIncarnation, Fault

VARIABLES present, incarnation, root, document, config,
          phase, subject, outcome, settlements, revoked, active,
          snapshot, waiter, waitSubject, notified, event

versions == 0..MaxVersion
subjects == [root : 1..MaxIncarnation, document : versions, config : versions]
emptySubject == [root |-> 1, document |-> 0, config |-> 0]
Active(t) == phase[t] \in {"registered", "running"}
CurrentSubject == [root |-> root, document |-> document, config |-> config]
Current(t) == present /\ subject[t] = CurrentSubject
SnapshotCurrent == snapshot # 0 /\ Current(snapshot)
Ready == IF Fault = "readyWithoutSnapshot" THEN TRUE
         ELSE IF Fault = "failureRestoresCurrent" /\ event = "failed" THEN snapshot # 0
         ELSE IF Fault = "ignoreDocument" THEN
             snapshot # 0 /\ present /\ subject[snapshot].root = root
             /\ subject[snapshot].config = config
         ELSE SnapshotCurrent
Building == IF Fault = "phantomBuilding" THEN TRUE
            ELSE \E t \in Tickets : Active(t) /\ Current(t)
Availability == IF Fault = "alwaysUnavailable" THEN "unavailable"
                ELSE IF Fault = "rootAvailabilityLeak" /\ snapshot # 0
                     THEN "stale_available"
                ELSE IF Fault = "failureDropsAvailability" /\ event = "failed"
                        /\ snapshot # 0 /\ ~SnapshotCurrent THEN "unavailable"
                ELSE IF ~present THEN "detached"
                ELSE IF snapshot = 0 THEN "unavailable"
                ELSE IF subject[snapshot].root # root THEN "unavailable"
                ELSE IF SnapshotCurrent THEN "current" ELSE "stale_available"

protocol == <<present, incarnation, root, document, config, phase, subject,
              outcome, settlements, revoked, active, snapshot>>
vars == <<protocol, waiter, waitSubject, notified, event>>

Init == /\ present = TRUE /\ incarnation = 1 /\ root = 1
        /\ document = 0 /\ config = 0
        /\ phase = [t \in Tickets |-> "unused"]
        /\ subject = [t \in Tickets |-> emptySubject]
        /\ outcome = [t \in Tickets |-> "none"]
        /\ settlements = [t \in Tickets |-> 0]
        /\ revoked = {} /\ active = 0 /\ snapshot = 0
        /\ waiter = "idle" /\ waitSubject = emptySubject
        /\ notified = FALSE /\ event = "init"

Register(t) ==
    /\ present /\ phase[t] = "unused"
    /\ subject' = [subject EXCEPT ![t] = CurrentSubject]
    /\ IF active < Capacity THEN
           /\ phase' = [phase EXCEPT ![t] = "registered"]
           /\ active' = active + 1
           /\ UNCHANGED <<outcome, settlements>>
           /\ event' = "register"
       ELSE
           /\ phase' = [phase EXCEPT ![t] = "terminal"]
           /\ outcome' = [outcome EXCEPT ![t] = "rejected"]
           /\ settlements' = [settlements EXCEPT ![t] = 1]
           /\ active' = IF Fault = "rejectionAccounting" THEN active + 1 ELSE active
           /\ event' = "rejected"
    /\ snapshot' = IF active >= Capacity /\ Fault = "rejectionDropsPrior" THEN 0
                    ELSE IF active >= Capacity /\ Fault = "rejectedPublication" THEN t
                    ELSE snapshot
    /\ UNCHANGED <<present, incarnation, root, document, config, revoked,
                   waiter, waitSubject, notified>>

Start(t) ==
    /\ phase[t] = "registered"
    /\ phase' = [phase EXCEPT ![t] = "running"]
    /\ event' = "start"
    /\ UNCHANGED <<present, incarnation, root, document, config, subject,
                   outcome, settlements, revoked, active, snapshot,
                   waiter, waitSubject, notified>>

Finish(t, result) ==
    /\ Active(t)
    /\ result \in {"accepted", "failed", "cancelled", "superseded"}
    /\ result = "accepted" => (Current(t) \/ Fault = "stalePublication")
    /\ phase' = [phase EXCEPT ![t] = "terminal"]
    /\ outcome' = [outcome EXCEPT ![t] = result]
    /\ settlements' = [settlements EXCEPT ![t] = @ + 1]
    /\ active' = active - 1
    /\ revoked' = IF result \in {"cancelled", "superseded"}
                   THEN revoked \cup {t} ELSE revoked
    /\ snapshot' = IF result = "accepted" THEN t
                    ELSE IF Fault = "dropRetained" THEN 0 ELSE snapshot
    /\ notified' = (notified \/ (t = 1 /\ waiter \in {"registered", "sleep"}))
    /\ event' = IF result = "accepted" THEN "publish" ELSE "failed"
    /\ UNCHANGED <<present, incarnation, root, document, config, subject,
                   waiter, waitSubject>>

Advance(t) == Fault # "stuckOperation" /\
    (Start(t) \/ \E r \in {"accepted", "failed", "cancelled", "superseded"} : Finish(t, r))

ChangeDocument ==
    /\ present /\ document < MaxVersion
    /\ document' = document + 1 /\ event' = "document"
    /\ UNCHANGED <<present, incarnation, root, config, phase, subject, outcome,
                   settlements, revoked, active, snapshot, waiter, waitSubject, notified>>
ChangeConfig ==
    /\ present /\ config < MaxVersion
    /\ config' = config + 1 /\ event' = "config"
    /\ UNCHANGED <<present, incarnation, root, document, phase, subject, outcome,
                   settlements, revoked, active, snapshot, waiter, waitSubject, notified>>
RemoveRoot ==
    /\ present /\ present' = FALSE /\ event' = "remove"
    /\ UNCHANGED <<incarnation, root, document, config, phase, subject, outcome,
                   settlements, revoked, active, snapshot, waiter, waitSubject, notified>>
ReaddRoot ==
    /\ ~present /\ incarnation < MaxIncarnation
    /\ present' = TRUE /\ incarnation' = incarnation + 1
    /\ root' = IF Fault = "reuseRoot" THEN root ELSE root + 1
    /\ event' = "readd"
    /\ UNCHANGED <<document, config, phase, subject, outcome, settlements, revoked,
                   active, snapshot, waiter, waitSubject, notified>>

Observe ==
    /\ event' = "observe"
    /\ outcome' = IF Fault = "consumingObservation"
                   THEN [t \in Tickets |-> "none"] ELSE outcome
    /\ UNCHANGED <<present, incarnation, root, document, config, phase, subject,
                   settlements, revoked, active, snapshot, waiter, waitSubject, notified>>

WaitCheck ==
    /\ waiter = "idle" /\ phase[1] # "unused"
    /\ waitSubject' = IF Fault = "wrongWaitSubject" THEN emptySubject ELSE subject[1]
    /\ waiter' = IF phase[1] = "terminal" THEN "done" ELSE "checked"
    /\ event' = "wait-check"
    /\ UNCHANGED <<protocol, notified>>
WaitRegister ==
    /\ waiter = "checked" /\ waiter' = "registered"
    /\ event' = "wait-register"
    /\ UNCHANGED <<protocol, waitSubject, notified>>
WaitRecheck ==
    /\ waiter = "registered"
    /\ waiter' = IF phase[1] = "terminal" /\ Fault # "lostWakeup"
                  THEN "done" ELSE "sleep"
    /\ event' = "wait-recheck"
    /\ UNCHANGED <<protocol, waitSubject, notified>>
WaitWake ==
    /\ waiter = "sleep" /\ notified /\ waiter' = "done"
    /\ event' = "wait-wake"
    /\ UNCHANGED <<protocol, waitSubject, notified>>
WaitStep == WaitCheck \/ WaitRegister \/ WaitRecheck \/ WaitWake

DoubleSettle ==
    /\ Fault = "doubleSettlement"
    /\ \E t \in Tickets :
        /\ phase[t] = "terminal" /\ settlements[t] = 1
        /\ settlements' = [settlements EXCEPT ![t] = 2]
    /\ event' = "double-settle"
    /\ UNCHANGED <<present, incarnation, root, document, config, phase, subject,
                   outcome, revoked, active, snapshot, waiter, waitSubject, notified>>
RevokedPublish ==
    /\ Fault = "revokedPublication"
    /\ \E t \in revoked : snapshot' = t
    /\ event' = "revoked-publish"
    /\ UNCHANGED <<present, incarnation, root, document, config, phase, subject,
                   outcome, settlements, revoked, active, waiter, waitSubject, notified>>

Next == (\E t \in Tickets : Register(t) \/ Advance(t))
        \/ ChangeDocument \/ ChangeConfig \/ RemoveRoot \/ ReaddRoot
        \/ Observe \/ WaitStep \/ DoubleSettle \/ RevokedPublish
Spec == Init /\ [][Next]_vars
        /\ (\A t \in Tickets : WF_vars(Advance(t))) /\ WF_vars(WaitStep)

TypeOK ==
    /\ present \in BOOLEAN /\ incarnation \in 1..MaxIncarnation
    /\ root \in 1..MaxIncarnation /\ document \in versions /\ config \in versions
    /\ phase \in [Tickets -> {"unused", "registered", "running", "terminal"}]
    /\ subject \in [Tickets -> subjects]
    /\ outcome \in [Tickets -> {"none", "accepted", "rejected", "failed", "cancelled", "superseded"}]
    /\ settlements \in [Tickets -> 0..2] /\ revoked \subseteq Tickets
    /\ active \in Nat /\ snapshot \in Tickets \cup {0}
    /\ waiter \in {"idle", "checked", "registered", "sleep", "done"}
    /\ waitSubject \in subjects /\ notified \in BOOLEAN
ExactlyOnce == \A t \in Tickets : settlements[t] <= 1
TerminalConsistency == \A t \in Tickets :
    (phase[t] = "terminal") <=> (settlements[t] = 1 /\ outcome[t] # "none")
CapacityAccounting == active = Cardinality({t \in Tickets : Active(t)}) /\ active <= Capacity
ReadinessSoundness == Ready => SnapshotCurrent
BuildingSoundness == Building => \E t \in Tickets : Active(t) /\ Current(t)
ReincarnationIsolation == root = incarnation
NoRevokedPublication == snapshot # 0 => snapshot \notin revoked
AcceptedSnapshot == snapshot # 0 =>
    phase[snapshot] = "terminal" /\ outcome[snapshot] = "accepted"
RetainedAvailability == present /\ snapshot # 0 /\ subject[snapshot].root = root
    => Availability \in {"current", "stale_available"}
AvailabilityCurrentness == (Availability = "current") <=> SnapshotCurrent
DetachedUnavailable == ~present => Availability = "detached"
ReincarnatedUnavailable == present /\ snapshot # 0 /\ subject[snapshot].root # root
    => Availability = "unavailable"
WaiterSafety == waiter = "done" => phase[1] = "terminal" /\ waitSubject = subject[1]

NoStalePublication == [][snapshot' # snapshot /\ snapshot' # 0 =>
    present' /\ subject'[snapshot'] = [root |-> root', document |-> document', config |-> config']]_vars
PreserveAccepted == [][(\E t \in Tickets : phase[t] # "terminal"
    /\ phase'[t] = "terminal" /\ outcome'[t] # "accepted") => snapshot' = snapshot]_vars
PreserveAvailability == [][(\E t \in Tickets : phase[t] # "terminal"
    /\ phase'[t] = "terminal" /\ outcome'[t] # "accepted") => Availability' = Availability]_vars
ObservationPurity == [][event' = "observe" => UNCHANGED protocol]_vars
EventuallySettled == \A t \in Tickets : Active(t) ~> (phase[t] = "terminal")
EventuallyObserved == (waiter \in {"checked", "registered", "sleep"}
                       /\ phase[1] = "terminal") ~> (waiter = "done")
=============================================================================
