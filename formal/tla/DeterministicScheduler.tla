---- MODULE DeterministicScheduler ----
EXTENDS FiniteSets, Naturals, SchedulerKernels, Sequences, TLC

(***************************************************************************)
(* Formal contract for schedlib's deterministic, sync-first scheduler.     *)
(*                                                                         *)
(* The model deliberately separates an immutable plan from execution. A    *)
(* canonical stable-task-id topological order is folded into deterministic  *)
(* first-fit batches. Workers may finish in any order, but results become    *)
(* observable only through stable ordered commit. Failure is fail-fast,     *)
(* cancellation occurs at a canonical commit boundary, and resource or     *)
(* cyclic-graph rejection happens before any worker result is published.    *)
(*                                                                         *)
(* Recursive operators below are finite denotational definitions used only  *)
(* by the specification. The refinement contract requires production Rust  *)
(* to realize them with heap-resident queues, arenas, and iterative loops;   *)
(* it may not map input depth to native call-stack depth.                    *)
(***************************************************************************)

CONSTANTS TaskCount, ResourceCount, Budget, Scenario

ASSUME /\ TaskCount \in Nat \ {0}
       /\ ResourceCount \in Nat \ {0}
       /\ Budget \in Nat \ {0}
       /\ Scenario \in {"Dependencies", "Effects", "Resources", "Outcomes"}

Tasks == 1..TaskCount
Resources == 1..ResourceCount
Outcomes == {"Success", "Failure"}

TaskPairs == Tasks \X Tasks
NonReflexiveTaskPairs ==
  {edge \in TaskPairs : edge[1] # edge[2]}

SeqSet(sequence) ==
  {sequence[index] : index \in 1..Len(sequence)}

IsPrefix(prefix, whole) ==
  /\ Len(prefix) <= Len(whole)
  /\ \A index \in 1..Len(prefix) : prefix[index] = whole[index]

StrictlyIncreasing(sequence) ==
  \A leftIndex, rightIndex \in 1..Len(sequence) :
    leftIndex < rightIndex => sequence[leftIndex] < sequence[rightIndex]

MinNat(values) ==
  CHOOSE candidate \in values :
    \A other \in values : candidate <= other

MaxNat(values) ==
  CHOOSE candidate \in values :
    \A other \in values : other <= candidate

Predecessors(relation, task) ==
  {predecessor \in Tasks : <<predecessor, task>> \in relation}

IsTopologicalRank(relation, rank) ==
  /\ rank \in [Tasks -> 1..TaskCount]
  /\ Cardinality({rank[task] : task \in Tasks}) = TaskCount
  /\ \A edge \in relation : rank[edge[1]] < rank[edge[2]]

Acyclic(relation) ==
  \E rank \in [Tasks -> 1..TaskCount] :
    IsTopologicalRank(relation, rank)

ReadyAfter(relation, completed, task) ==
  /\ task \in Tasks \ completed
  /\ Predecessors(relation, task) \subseteq completed

RECURSIVE CanonicalTopoStep(_, _, _)
CanonicalTopoStep(relation, completed, order) ==
  IF completed = Tasks
  THEN order
  ELSE
    LET ready ==
          {task \in Tasks :
             ReadyAfter(relation, completed, task)}
    IN IF ready = {}
       THEN order
       ELSE
         LET nextTask == MinNat(ready)
         IN CanonicalTopoStep(
              relation,
              completed \cup {nextTask},
              Append(order, nextTask))

CanonicalTopo(relation) ==
  CanonicalTopoStep(relation, {}, <<>>)

RECURSIVE SequenceCost(_, _)
SequenceCost(sequence, taskCosts) ==
  IF Len(sequence) = 0
  THEN 0
  ELSE taskCosts[Head(sequence)] +
       SequenceCost(Tail(sequence), taskCosts)

PairwiseIndependent(batch, readSets, writeSets) ==
  \A leftIndex, rightIndex \in 1..Len(batch) :
    leftIndex # rightIndex =>
      Independent(
        readSets,
        writeSets,
        batch[leftIndex],
        batch[rightIndex])

BatchAccepts(batch, task, readSets, writeSets, taskCosts) ==
  /\ \A member \in SeqSet(batch) :
       Independent(readSets, writeSets, member, task)
  /\ SequenceCost(batch, taskCosts) + taskCosts[task] <= Budget

RECURSIVE FlattenBatches(_)
FlattenBatches(batches) ==
  IF Len(batches) = 0
  THEN <<>>
  ELSE Head(batches) \o FlattenBatches(Tail(batches))

TaskBatchIndex(batches, task) ==
  CHOOSE index \in 1..Len(batches) :
    task \in SeqSet(batches[index])

DependencyFloor(batches, relation, task) ==
  LET predecessors == Predecessors(relation, task)
  IN IF predecessors = {}
     THEN 0
     ELSE MaxNat(
            {TaskBatchIndex(batches, predecessor) :
               predecessor \in predecessors})

PlaceTask(
    batches,
    relation,
    readSets,
    writeSets,
    taskCosts,
    task) ==
  LET floor == DependencyFloor(batches, relation, task)
      candidates ==
        {index \in (floor + 1)..Len(batches) :
           BatchAccepts(
             batches[index],
             task,
             readSets,
             writeSets,
             taskCosts)}
  IN IF candidates = {}
     THEN Append(batches, <<task>>)
     ELSE
       LET target == MinNat(candidates)
       IN [batches EXCEPT
             ![target] = Append(@, task)]

RECURSIVE BuildPlan(_, _, _, _, _, _, _)
BuildPlan(
    order,
    orderIndex,
    batches,
    relation,
    readSets,
    writeSets,
    taskCosts) ==
  IF orderIndex > Len(order)
  THEN batches
  ELSE
    LET task == order[orderIndex]
        nextBatches ==
          PlaceTask(
            batches,
            relation,
            readSets,
            writeSets,
            taskCosts,
            task)
    IN BuildPlan(
         order,
         orderIndex + 1,
         nextBatches,
         relation,
         readSets,
         writeSets,
         taskCosts)

ResourcesAdmissible(taskCosts) ==
  \A task \in Tasks : taskCosts[task] <= Budget

CanonicalPlan(relation, readSets, writeSets, taskCosts) ==
  IF Acyclic(relation) /\ ResourcesAdmissible(taskCosts)
  THEN BuildPlan(
         CanonicalTopo(relation),
         1,
         <<>>,
         relation,
         readSets,
         writeSets,
         taskCosts)
  ELSE <<>>

PlanWellFormed(
    batches,
    relation,
    readSets,
    writeSets,
    taskCosts) ==
  LET flattened == FlattenBatches(batches)
  IN /\ Len(flattened) = TaskCount
     /\ SeqSet(flattened) = Tasks
     /\ \A batchIndex \in 1..Len(batches) :
          /\ StrictlyIncreasing(batches[batchIndex])
          /\ PairwiseIndependent(
               batches[batchIndex],
               readSets,
               writeSets)
          /\ SequenceCost(
               batches[batchIndex],
               taskCosts) <= Budget
     /\ \A edge \in relation :
          TaskBatchIndex(batches, edge[1]) <
            TaskBatchIndex(batches, edge[2])

NoDependencies == {}
ForkJoinDependencies ==
  IF TaskCount = 3
  THEN {<<1, 3>>, <<2, 3>>}
  ELSE {}

EmptyReads == [task \in Tasks |-> {}]
EmptyWrites == [task \in Tasks |-> {}]
UnitCosts == [task \in Tasks |-> 1]
AllSuccess == [task \in Tasks |-> "Success"]

DependencyInputs ==
  IF Scenario = "Dependencies"
  THEN SUBSET NonReflexiveTaskPairs
  ELSE
    IF Scenario = "Outcomes"
    THEN {ForkJoinDependencies}
    ELSE {NoDependencies}

ReadInputs ==
  IF Scenario = "Effects"
  THEN [Tasks -> SUBSET Resources]
  ELSE {EmptyReads}

WriteInputs ==
  IF Scenario = "Effects"
  THEN [Tasks -> SUBSET Resources]
  ELSE {EmptyWrites}

CostInputs ==
  IF Scenario = "Resources"
  THEN [Tasks -> 1..(Budget + 1)]
  ELSE {UnitCosts}

OutcomeInputs ==
  IF Scenario = "Outcomes"
  THEN [Tasks -> Outcomes]
  ELSE {AllSuccess}

CancelInputs ==
  IF Scenario = "Outcomes"
  THEN 0..TaskCount
  ELSE {TaskCount}

VARIABLES deps,
          reads,
          writes,
          costs,
          outcomes,
          cancelAfter,
          phase,
          plan,
          batchIndex,
          completedResults,
          commitIndex,
          committed,
          cancelRequested

inputVars == <<deps, reads, writes, costs, outcomes, cancelAfter>>
executionVars ==
  <<phase,
    plan,
    batchIndex,
    completedResults,
    commitIndex,
    committed,
    cancelRequested>>
vars == <<inputVars, executionVars>>

Phases ==
  {"Unvalidated",
   "Executing",
   "Committing",
   "Completed",
   "Failed",
   "Cancelled",
   "RejectedCycle",
   "Exhausted"}

TerminalPhases ==
  {"Completed", "Failed", "Cancelled", "RejectedCycle", "Exhausted"}

ExecutionTerminalPhases ==
  {"Completed", "Failed", "Cancelled"}

CurrentBatch ==
  IF batchIndex \in 1..Len(plan)
  THEN plan[batchIndex]
  ELSE <<>>

CancelIsDue ==
  /\ cancelAfter < TaskCount
  /\ Len(committed) = cancelAfter

Init ==
  /\ deps \in DependencyInputs
  /\ reads \in ReadInputs
  /\ writes \in WriteInputs
  /\ costs \in CostInputs
  /\ outcomes \in OutcomeInputs
  /\ cancelAfter \in CancelInputs
  /\ phase = "Unvalidated"
  /\ plan = <<>>
  /\ batchIndex = 0
  /\ completedResults = {}
  /\ commitIndex = 0
  /\ committed = <<>>
  /\ cancelRequested = FALSE

RejectCycle ==
  /\ phase = "Unvalidated"
  /\ ~Acyclic(deps)
  /\ phase' = "RejectedCycle"
  /\ UNCHANGED <<inputVars,
                  plan,
                  batchIndex,
                  completedResults,
                  commitIndex,
                  committed,
                  cancelRequested>>

RejectResources ==
  /\ phase = "Unvalidated"
  /\ Acyclic(deps)
  /\ ~ResourcesAdmissible(costs)
  /\ phase' = "Exhausted"
  /\ UNCHANGED <<inputVars,
                  plan,
                  batchIndex,
                  completedResults,
                  commitIndex,
                  committed,
                  cancelRequested>>

AcceptPlan ==
  /\ phase = "Unvalidated"
  /\ Acyclic(deps)
  /\ ResourcesAdmissible(costs)
  /\ plan' = CanonicalPlan(deps, reads, writes, costs)
  /\ Len(plan') > 0
  /\ phase' = "Executing"
  /\ batchIndex' = 1
  /\ commitIndex' = 0
  /\ UNCHANGED <<inputVars,
                  completedResults,
                  committed,
                  cancelRequested>>

WorkerComplete(task) ==
  /\ phase = "Executing"
  /\ ~cancelRequested
  /\ task \in SeqSet(CurrentBatch) \ completedResults
  /\ completedResults' = completedResults \cup {task}
  /\ UNCHANGED <<inputVars,
                  phase,
                  plan,
                  batchIndex,
                  commitIndex,
                  committed,
                  cancelRequested>>

BeginCommit ==
  /\ phase = "Executing"
  /\ ~cancelRequested
  /\ ~CancelIsDue
  /\ SeqSet(CurrentBatch) \subseteq completedResults
  /\ phase' = "Committing"
  /\ commitIndex' = 1
  /\ UNCHANGED <<inputVars,
                  plan,
                  batchIndex,
                  completedResults,
                  committed,
                  cancelRequested>>

RequestCancellation ==
  /\ phase \in {"Executing", "Committing"}
  /\ CancelIsDue
  /\ ~cancelRequested
  /\ cancelRequested' = TRUE
  /\ UNCHANGED <<inputVars,
                  phase,
                  plan,
                  batchIndex,
                  completedResults,
                  commitIndex,
                  committed>>

AcknowledgeCancellation ==
  /\ phase \in {"Executing", "Committing"}
  /\ cancelRequested
  /\ phase' = "Cancelled"
  /\ UNCHANGED <<inputVars,
                  plan,
                  batchIndex,
                  completedResults,
                  commitIndex,
                  committed,
                  cancelRequested>>

CommitNext ==
  /\ phase = "Committing"
  /\ ~cancelRequested
  /\ ~CancelIsDue
  /\ commitIndex \in 1..Len(CurrentBatch)
  /\ LET task == CurrentBatch[commitIndex]
         nextCommitted == Append(committed, task)
     IN /\ committed' = nextCommitted
        /\ IF outcomes[task] = "Failure"
              THEN
                /\ phase' = "Failed"
                /\ batchIndex' = batchIndex
                /\ commitIndex' = commitIndex + 1
              ELSE
                IF commitIndex < Len(CurrentBatch)
                THEN
                  /\ phase' = "Committing"
                  /\ batchIndex' = batchIndex
                  /\ commitIndex' = commitIndex + 1
                ELSE
                  IF batchIndex < Len(plan)
                  THEN
                    /\ phase' = "Executing"
                    /\ batchIndex' = batchIndex + 1
                    /\ commitIndex' = 0
                  ELSE
                    /\ phase' = "Completed"
                    /\ batchIndex' = batchIndex
                    /\ commitIndex' = commitIndex + 1
  /\ UNCHANGED <<inputVars,
                  plan,
                  completedResults,
                  cancelRequested>>

Next ==
  RejectCycle
  \/ RejectResources
  \/ AcceptPlan
  \/ (\E task \in Tasks : WorkerComplete(task))
  \/ BeginCommit
  \/ RequestCancellation
  \/ AcknowledgeCancellation
  \/ CommitNext

Spec ==
  Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
  /\ deps \in SUBSET TaskPairs
  /\ reads \in [Tasks -> SUBSET Resources]
  /\ writes \in [Tasks -> SUBSET Resources]
  /\ costs \in [Tasks -> Nat \ {0}]
  /\ outcomes \in [Tasks -> Outcomes]
  /\ cancelAfter \in 0..TaskCount
  /\ phase \in Phases
  /\ plan \in Seq(Seq(Tasks))
  /\ batchIndex \in 0..TaskCount
  /\ completedResults \in SUBSET Tasks
  /\ commitIndex \in 0..(TaskCount + 1)
  /\ committed \in Seq(Tasks)
  /\ cancelRequested \in BOOLEAN

ValidationIsExact ==
  /\ (phase = "RejectedCycle" => ~Acyclic(deps))
  /\ (phase = "Exhausted" =>
        Acyclic(deps) /\ ~ResourcesAdmissible(costs))
  /\ (phase \notin {"Unvalidated", "RejectedCycle", "Exhausted"} =>
        Acyclic(deps) /\ ResourcesAdmissible(costs))

PlanIsCanonicalAndLawful ==
  phase \notin {"Unvalidated", "RejectedCycle", "Exhausted"} =>
    /\ plan = CanonicalPlan(deps, reads, writes, costs)
    /\ PlanWellFormed(plan, deps, reads, writes, costs)

OrderedCommit ==
  IsPrefix(committed, FlattenBatches(plan))

ResultsAreScoped ==
  /\ completedResults \subseteq SeqSet(FlattenBatches(plan))
  /\ (plan # <<>> =>
        \A task \in completedResults :
          TaskBatchIndex(plan, task) <= batchIndex)

NoDependencyViolation ==
  \A position \in 1..Len(committed) :
    Predecessors(deps, committed[position]) \subseteq
      SeqSet(SubSeq(committed, 1, position - 1))

FailurePositions ==
  {position \in 1..Len(FlattenBatches(plan)) :
     outcomes[FlattenBatches(plan)[position]] = "Failure"}

FirstFailurePosition ==
  IF FailurePositions = {}
  THEN TaskCount + 1
  ELSE MinNat(FailurePositions)

SerialTerminalPhase ==
  IF cancelAfter < FirstFailurePosition /\ cancelAfter < TaskCount
  THEN "Cancelled"
  ELSE
    IF FirstFailurePosition <= TaskCount
    THEN "Failed"
    ELSE "Completed"

SerialCommitCount ==
  IF SerialTerminalPhase = "Cancelled"
  THEN cancelAfter
  ELSE
    IF SerialTerminalPhase = "Failed"
    THEN FirstFailurePosition
    ELSE TaskCount

SerialParallelObservationalEquivalence ==
  phase \in ExecutionTerminalPhases =>
    /\ phase = SerialTerminalPhase
    /\ committed =
         SubSeq(FlattenBatches(plan), 1, SerialCommitCount)

FailurePropagationIsFailFast ==
  phase = "Failed" =>
    /\ Len(committed) > 0
    /\ outcomes[committed[Len(committed)]] = "Failure"

CancellationIsBoundaryExact ==
  phase = "Cancelled" =>
    /\ cancelRequested
    /\ cancelAfter < TaskCount
    /\ Len(committed) = cancelAfter

CompletionIsTotal ==
  phase = "Completed" =>
    /\ SeqSet(committed) = Tasks
    /\ Len(committed) = TaskCount
    /\ \A task \in Tasks : outcomes[task] = "Success"

ResultsOnlyGrow ==
  [][completedResults \subseteq completedResults']_vars

PlanIsImmutableAfterValidation ==
  [][phase # "Unvalidated" => plan' = plan]_vars

EventuallyTerminal ==
  <> (phase \in TerminalPhases)

====
