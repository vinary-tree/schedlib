---- MODULE DurableResume ----
EXTENDS FiniteSets, Integers, Naturals, Sequences, TLC

(***************************************************************************)
(* Durable identity, journal-first publication, crash, and resume contract. *)
(*                                                                         *)
(* The journal is authoritative. Appending a canonical event precedes its  *)
(* logical publication. Publication is idempotent by (plan, ordinal), so a *)
(* crash after the append can replay publication without rerunning effects. *)
(* A crash before the append may rerun a task only when its immutable plan  *)
(* metadata carries an explicit replay-safety witness.                      *)
(*                                                                         *)
(* This finite model represents the protocol, not a storage implementation. *)
(* schedlib-interop owns bounded canonical bytes and vinary-runtime owns     *)
(* durable artifacts. Production refinement must use iterative heap state.  *)
(***************************************************************************)

CONSTANTS TaskCount, MaxCrashes, Scenario

ASSUME /\ TaskCount \in Nat \ {0}
       /\ MaxCrashes \in Nat
       /\ Scenario \in
            {"Crash",
             "Resume",
             "Outcomes",
             "Resources",
             "Stale",
             "KeyCollision",
             "Malformed",
             "UnsafeReplay"}

Tasks == 1..TaskCount
PlanIdentities == {"plan-a", "plan-b"}
OutcomeKinds == {"Success", "Failure", "Incomplete"}
EventKinds ==
  {"Success", "Failure", "Incomplete", "Cancelled",
   "ResourceLimited", "Completed"}
Phases ==
  {"Validate", "Ready", "Computed", "Journaled", "Publishing", "Crashed",
   "Completed", "Failed", "Incomplete", "Cancelled", "ResourceLimited",
   "Rejected"}
TerminalPhases ==
  {"Completed", "Failed", "Incomplete", "Cancelled", "ResourceLimited",
   "Rejected"}

NoEvent == [plan |-> "plan-a", ordinal |-> 0, task |-> 0, kind |-> "Completed"]

Event(planId, ordinal, task, kind) ==
  [plan |-> planId, ordinal |-> ordinal, task |-> task, kind |-> kind]

SuccessEvent(planId, task) == Event(planId, task, task, "Success")

RECURSIVE SuccessPrefix(_, _)
SuccessPrefix(planId, count) ==
  IF count = 0
  THEN <<>>
  ELSE SuccessPrefix(planId, count - 1) \o <<SuccessEvent(planId, count)>>

MalformedPrefix(planId) ==
  <<Event(planId, 2, 1, "Success")>>

EventId(event) == [plan |-> event.plan, ordinal |-> event.ordinal]

SeqSet(sequence) ==
  {sequence[index] : index \in 1..Len(sequence)}

IsPrefix(prefix, whole) ==
  /\ Len(prefix) <= Len(whole)
  /\ \A index \in 1..Len(prefix) : prefix[index] = whole[index]

MinNat(values) ==
  CHOOSE candidate \in values :
    \A other \in values : candidate <= other

IsTaskKind(kind) == kind \in OutcomeKinds

TerminalPhaseFor(kind) ==
  CASE kind = "Failure" -> "Failed"
    [] kind = "Incomplete" -> "Incomplete"
    [] kind = "Cancelled" -> "Cancelled"
    [] kind = "ResourceLimited" -> "ResourceLimited"
    [] kind = "Completed" -> "Completed"
    [] OTHER -> "Ready"

VARIABLES planIdentity,
          checkpointPlanIdentity,
          keyMapValid,
          manifestValid,
          checkpointShapeValid,
          replaySafeTasks,
          outcomes,
          cancelAfter,
          resourceAfter,
          initialCount,
          initialPublished,
          phase,
          nextTask,
          journal,
          receipts,
          staged,
          crashes,
          effectsStarted,
          replayBlocked

inputVars ==
  <<planIdentity,
    checkpointPlanIdentity,
    keyMapValid,
    manifestValid,
    checkpointShapeValid,
    replaySafeTasks,
    outcomes,
    cancelAfter,
    resourceAfter,
    initialCount,
    initialPublished>>

protocolVars ==
  <<phase,
    nextTask,
    journal,
    receipts,
    staged,
    crashes,
    effectsStarted,
    replayBlocked>>

vars == <<inputVars, protocolVars>>

InitialCountDomain ==
  IF Scenario = "Resume" THEN 0..TaskCount ELSE {0}

InitialPublishedDomain(count) ==
  IF Scenario = "Resume" THEN 0..count ELSE {0}

OutcomeDomain ==
  IF Scenario = "Outcomes"
  THEN [Tasks -> OutcomeKinds]
  ELSE {[task \in Tasks |-> "Success"]}

CancelDomain ==
  IF Scenario = "Resources" THEN 0..(TaskCount + 1) ELSE {TaskCount + 1}

ResourceDomain ==
  IF Scenario = "Resources" THEN 0..(TaskCount + 1) ELSE {TaskCount + 1}

ReplaySafeDomain ==
  IF Scenario = "UnsafeReplay" THEN {{}} ELSE {Tasks}

InitialJournal(planId, count) ==
  IF Scenario = "Malformed"
  THEN MalformedPrefix(planId)
  ELSE SuccessPrefix(planId, count)

InitialReceipts(planId, count) ==
  {EventId(SuccessEvent(planId, ordinal)) : ordinal \in 1..count}

CanonicalSuccessAt(sequence, index) ==
  sequence[index] = SuccessEvent(planIdentity, index)

CanonicalTerminalAt(sequence, index) ==
  LET event == sequence[index]
  IN /\ index = Len(sequence)
     /\ event.plan = planIdentity
     /\ event.ordinal = index
     /\ event.kind \in EventKinds \ {"Success"}
     /\ IF event.kind \in {"Failure", "Incomplete"}
           THEN event.task = index /\ index \in Tasks
           ELSE event.task = 0

JournalWellFormed(sequence) ==
  /\ Len(sequence) <= TaskCount + 1
  /\ \A index \in 1..Len(sequence) :
       IF sequence[index].kind = "Success"
       THEN CanonicalSuccessAt(sequence, index)
       ELSE CanonicalTerminalAt(sequence, index)
  /\ \A index \in 1..(Len(sequence) - 1) :
       sequence[index].kind = "Success"

ReceiptsReferenceJournal(sequence, published) ==
  published \subseteq {EventId(sequence[index]) : index \in 1..Len(sequence)}

UnpublishedOrdinals ==
  {index \in 1..Len(journal) : EventId(journal[index]) \notin receipts}

AllJournalPublished == UnpublishedOrdinals = {}

FirstUnpublishedOrdinal == MinNat(UnpublishedOrdinals)

ValidInputs ==
  /\ keyMapValid
  /\ manifestValid
  /\ checkpointShapeValid
  /\ checkpointPlanIdentity = planIdentity
  /\ initialPublished <= initialCount
  /\ JournalWellFormed(InitialJournal(planIdentity, initialCount))
  /\ ReceiptsReferenceJournal(
       InitialJournal(planIdentity, initialCount),
       InitialReceipts(planIdentity, initialPublished))

Init ==
  /\ planIdentity = "plan-a"
  /\ checkpointPlanIdentity =
       IF Scenario = "Stale" THEN "plan-b" ELSE "plan-a"
  /\ keyMapValid = (Scenario # "KeyCollision")
  /\ manifestValid = TRUE
  /\ checkpointShapeValid = (Scenario # "Malformed")
  /\ replaySafeTasks \in ReplaySafeDomain
  /\ outcomes \in OutcomeDomain
  /\ cancelAfter \in CancelDomain
  /\ resourceAfter \in ResourceDomain
  /\ initialCount \in InitialCountDomain
  /\ initialPublished \in InitialPublishedDomain(initialCount)
  /\ phase = "Validate"
  /\ nextTask = initialCount + 1
  /\ journal = InitialJournal(planIdentity, initialCount)
  /\ receipts = InitialReceipts(planIdentity, initialPublished)
  /\ staged = NoEvent
  /\ crashes = 0
  /\ effectsStarted = FALSE
  /\ replayBlocked = FALSE

RejectInvalid ==
  /\ phase = "Validate"
  /\ ~ValidInputs
  /\ phase' = "Rejected"
  /\ UNCHANGED <<inputVars,
                  nextTask, journal, receipts, staged, crashes,
                  effectsStarted, replayBlocked>>

AcceptValid ==
  /\ phase = "Validate"
  /\ ValidInputs
  /\ phase' = "Ready"
  /\ UNCHANGED <<inputVars,
                  nextTask, journal, receipts, staged, crashes,
                  effectsStarted, replayBlocked>>

StageBoundary(kind) ==
  /\ phase = "Ready"
  /\ AllJournalPublished
  /\ staged' = Event(planIdentity, Len(journal) + 1, 0, kind)
  /\ phase' = "Computed"
  /\ UNCHANGED <<inputVars,
                  nextTask, journal, receipts, crashes,
                  effectsStarted, replayBlocked>>

StageCompleted ==
  /\ nextTask = TaskCount + 1
  /\ StageBoundary("Completed")

StageCancelled ==
  /\ nextTask <= TaskCount
  /\ cancelAfter = nextTask - 1
  /\ StageBoundary("Cancelled")

StageResourceLimited ==
  /\ nextTask <= TaskCount
  /\ resourceAfter = nextTask - 1
  /\ cancelAfter # nextTask - 1
  /\ StageBoundary("ResourceLimited")

StageTask ==
  /\ phase = "Ready"
  /\ AllJournalPublished
  /\ nextTask \in Tasks
  /\ cancelAfter # nextTask - 1
  /\ resourceAfter # nextTask - 1
  /\ staged' =
       Event(planIdentity, Len(journal) + 1, nextTask, outcomes[nextTask])
  /\ effectsStarted' = TRUE
  /\ phase' = "Computed"
  /\ UNCHANGED <<inputVars,
                  nextTask, journal, receipts, crashes, replayBlocked>>

StageUnpublished ==
  /\ phase = "Ready"
  /\ ~AllJournalPublished
  /\ staged' = journal[FirstUnpublishedOrdinal]
  /\ phase' = "Journaled"
  /\ UNCHANGED <<inputVars,
                  nextTask, journal, receipts, crashes,
                  effectsStarted, replayBlocked>>

AppendJournal ==
  /\ phase = "Computed"
  /\ ~replayBlocked
  /\ journal' = Append(journal, staged)
  /\ phase' = "Journaled"
  /\ UNCHANGED <<inputVars,
                  nextTask, receipts, staged, crashes,
                  effectsStarted, replayBlocked>>

PublishOnce ==
  /\ phase = "Journaled"
  /\ receipts' = receipts \cup {EventId(staged)}
  /\ phase' = "Publishing"
  /\ UNCHANGED <<inputVars,
                  nextTask, journal, staged, crashes,
                  effectsStarted, replayBlocked>>

AdvanceCheckpoint ==
  /\ phase = "Publishing"
  /\ EventId(staged) \in receipts
  /\ IF staged.kind = "Success" /\ staged.ordinal = nextTask
        THEN /\ nextTask' = nextTask + 1
             /\ phase' = "Ready"
        ELSE IF staged.kind = "Success"
             THEN /\ nextTask' = nextTask
                  /\ phase' = "Ready"
             ELSE /\ nextTask' = nextTask
                  /\ phase' = TerminalPhaseFor(staged.kind)
  /\ staged' = NoEvent
  /\ UNCHANGED <<inputVars,
                  journal, receipts, crashes,
                  effectsStarted, replayBlocked>>

Crash ==
  /\ phase \in {"Computed", "Journaled", "Publishing"}
  /\ crashes < MaxCrashes
  /\ crashes' = crashes + 1
  /\ replayBlocked' =
       (replayBlocked \/
        (phase = "Computed" /\ IsTaskKind(staged.kind) /\
           staged.task \notin replaySafeTasks))
  /\ staged' = NoEvent
  /\ phase' = "Crashed"
  /\ UNCHANGED <<inputVars,
                  nextTask, journal, receipts, effectsStarted>>

RecoverBlocked ==
  /\ phase = "Crashed"
  /\ replayBlocked
  /\ phase' = "Rejected"
  /\ UNCHANGED <<inputVars,
                  nextTask, journal, receipts, staged, crashes,
                  effectsStarted, replayBlocked>>

RecoverJournaled ==
  /\ phase = "Crashed"
  /\ ~replayBlocked
  /\ ~AllJournalPublished
  /\ staged' = journal[FirstUnpublishedOrdinal]
  /\ phase' = "Journaled"
  /\ UNCHANGED <<inputVars,
                  nextTask, journal, receipts, crashes,
                  effectsStarted, replayBlocked>>

RecoverPublished ==
  /\ phase = "Crashed"
  /\ ~replayBlocked
  /\ AllJournalPublished
  /\ Len(journal) >= nextTask
  /\ EventId(journal[nextTask]) \in receipts
  /\ staged' = journal[nextTask]
  /\ phase' = "Publishing"
  /\ UNCHANGED <<inputVars,
                  nextTask, journal, receipts, crashes,
                  effectsStarted, replayBlocked>>

RecoverBeforeJournal ==
  /\ phase = "Crashed"
  /\ ~replayBlocked
  /\ AllJournalPublished
  /\ Len(journal) < nextTask
  /\ phase' = "Ready"
  /\ UNCHANGED <<inputVars,
                  nextTask, journal, receipts, staged, crashes,
                  effectsStarted, replayBlocked>>

Next ==
  RejectInvalid
  \/ AcceptValid
  \/ StageCompleted
  \/ StageCancelled
  \/ StageResourceLimited
  \/ StageTask
  \/ StageUnpublished
  \/ AppendJournal
  \/ PublishOnce
  \/ AdvanceCheckpoint
  \/ Crash
  \/ RecoverBlocked
  \/ RecoverJournaled
  \/ RecoverPublished
  \/ RecoverBeforeJournal

Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
  /\ planIdentity \in PlanIdentities
  /\ checkpointPlanIdentity \in PlanIdentities
  /\ keyMapValid \in BOOLEAN
  /\ manifestValid \in BOOLEAN
  /\ checkpointShapeValid \in BOOLEAN
  /\ replaySafeTasks \subseteq Tasks
  /\ outcomes \in [Tasks -> OutcomeKinds]
  /\ cancelAfter \in 0..(TaskCount + 1)
  /\ resourceAfter \in 0..(TaskCount + 1)
  /\ initialCount \in 0..TaskCount
  /\ initialPublished \in 0..initialCount
  /\ phase \in Phases
  /\ nextTask \in 1..(TaskCount + 1)
  /\ journal \in Seq(
       [plan : PlanIdentities,
        ordinal : 0..(TaskCount + 1),
        task : 0..TaskCount,
        kind : EventKinds])
  /\ receipts \subseteq
       [plan : PlanIdentities, ordinal : 1..(TaskCount + 1)]
  /\ staged \in
       [plan : PlanIdentities,
        ordinal : 0..(TaskCount + 1),
        task : 0..TaskCount,
        kind : EventKinds]
  /\ crashes \in 0..MaxCrashes
  /\ effectsStarted \in BOOLEAN
  /\ replayBlocked \in BOOLEAN

InvalidInputHasNoEffects ==
  ~ValidInputs =>
    /\ journal = InitialJournal(planIdentity, initialCount)
    /\ ~effectsStarted
    /\ phase \in {"Validate", "Rejected"}

CommittedJournalIsCanonical ==
  phase # "Validate" /\ ValidInputs => JournalWellFormed(journal)

PublishedEventsAreDurable ==
  ReceiptsReferenceJournal(journal, receipts)

ReceiptsAreCanonicalPrefix ==
  /\ Cardinality(receipts) <= Len(journal)
  /\ receipts =
       {EventId(journal[index]) : index \in 1..Cardinality(receipts)}

EventIdsAreExactlyOnce ==
  Len(journal) =
    Cardinality({EventId(event) : event \in SeqSet(journal)})

JournalPrecedesPublication ==
  phase = "Publishing" =>
    /\ journal # <<>>
    /\ staged \in SeqSet(journal)
    /\ EventId(staged) \in receipts

CheckpointCursorMatchesPrefix ==
  phase \in {"Ready", "Computed"} => Len(journal) = nextTask - 1

RecoveryNeverInventsAnEvent ==
  phase \in {"Journaled", "Publishing"} =>
    /\ journal # <<>>
    /\ staged \in SeqSet(journal)

UnsafeReplayFailsClosed ==
  replayBlocked => phase \in {"Crashed", "Rejected"}

TerminalOutcomeIsExact ==
  phase \in TerminalPhases \ {"Rejected"} =>
    /\ journal # <<>>
    /\ phase = TerminalPhaseFor(journal[Len(journal)].kind)
    /\ EventId(journal[Len(journal)]) \in receipts

BoundaryPriorityIsDeterministic ==
  phase = "ResourceLimited" => cancelAfter # nextTask - 1

CompletionIsTotal ==
  phase = "Completed" =>
    /\ Len(journal) = TaskCount + 1
    /\ \A index \in Tasks : journal[index] = SuccessEvent(planIdentity, index)
    /\ journal[TaskCount + 1].kind = "Completed"

InputsAreImmutable == [][UNCHANGED inputVars]_vars

JournalOnlyGrows == [][IsPrefix(journal, journal')]_vars

ReceiptsOnlyGrow == [][receipts \subseteq receipts']_vars

DurableAt(index) == ValidInputs /\ Len(journal) >= index

PublishedAt(index) ==
  IF DurableAt(index)
  THEN EventId(journal[index]) \in receipts
  ELSE FALSE

EveryDurableEventEventuallyPublished ==
  \A index \in 1..(TaskCount + 1) :
    DurableAt(index) ~> PublishedAt(index)

EventuallyTerminal == <> (phase \in TerminalPhases)

====
