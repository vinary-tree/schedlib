---- MODULE RayonAdapter ----
EXTENDS FiniteSets, Naturals, Sequences, TLC

(***************************************************************************)
(* Formal contract for schedlib's optional Rayon batch adapter.            *)
(*                                                                         *)
(* The serial scheduler has already proved that every dispatched batch is  *)
(* immutable, budget-admissible, and pairwise independent. This model      *)
(* therefore starts at one nonempty accepted batch and specifies only the  *)
(* adapter boundary: bounded dispatch, exact-once completion, full join,    *)
(* and canonical return. Completion order is deliberately nondeterministic. *)
(* The returned sequence is not observable until every worker has joined.  *)
(*                                                                         *)
(* All model transitions are flat set/sequence operations. The production  *)
(* refinement may use Rayon internally, but schedlib itself must introduce *)
(* no input-dependent native recursion or shared mutable task state.        *)
(***************************************************************************)

CONSTANTS TaskCount, WorkerCounts

ASSUME /\ TaskCount \in Nat \ {0}
       /\ WorkerCounts \subseteq 1..TaskCount
       /\ WorkerCounts # {}

Tasks == 1..TaskCount
Phases == {"Running", "Returned"}

CanonicalOrder ==
  [index \in 1..TaskCount |-> index]

SeqSet(sequence) ==
  {sequence[index] : index \in 1..Len(sequence)}

VARIABLES workerLimit,
          pending,
          active,
          completed,
          completionTrace,
          returned,
          phase

vars ==
  <<workerLimit,
    pending,
    active,
    completed,
    completionTrace,
    returned,
    phase>>

Init ==
  /\ workerLimit \in WorkerCounts
  /\ pending = Tasks
  /\ active = {}
  /\ completed = {}
  /\ completionTrace = <<>>
  /\ returned = <<>>
  /\ phase = "Running"

Dispatch(task) ==
  /\ phase = "Running"
  /\ task \in pending
  /\ Cardinality(active) < workerLimit
  /\ pending' = pending \ {task}
  /\ active' = active \cup {task}
  /\ UNCHANGED
       <<workerLimit, completed, completionTrace, returned, phase>>

Complete(task) ==
  /\ phase = "Running"
  /\ task \in active
  /\ active' = active \ {task}
  /\ completed' = completed \cup {task}
  /\ completionTrace' = Append(completionTrace, task)
  /\ UNCHANGED <<workerLimit, pending, returned, phase>>

Join ==
  /\ phase = "Running"
  /\ pending = {}
  /\ active = {}
  /\ completed = Tasks
  /\ returned' = CanonicalOrder
  /\ phase' = "Returned"
  /\ UNCHANGED
       <<workerLimit, pending, active, completed, completionTrace>>

Next ==
  \/ \E task \in Tasks : Dispatch(task)
  \/ \E task \in Tasks : Complete(task)
  \/ Join

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ WF_vars(Next)

TypeOK ==
  /\ workerLimit \in WorkerCounts
  /\ pending \subseteq Tasks
  /\ active \subseteq Tasks
  /\ completed \subseteq Tasks
  /\ completionTrace \in Seq(Tasks)
  /\ returned \in {<<>>, CanonicalOrder}
  /\ phase \in Phases

TaskPartitionIsExact ==
  /\ pending \cup active \cup completed = Tasks
  /\ pending \cap active = {}
  /\ pending \cap completed = {}
  /\ active \cap completed = {}

CompletionIsExactOnce ==
  /\ SeqSet(completionTrace) = completed
  /\ Len(completionTrace) = Cardinality(completed)

ActiveWorkersRespectLimit ==
  Cardinality(active) <= workerLimit

NoReturnBeforeJoin ==
  phase = "Running" => returned = <<>>

ReturnRequiresJoin ==
  phase = "Returned" =>
    /\ pending = {}
    /\ active = {}
    /\ completed = Tasks

WorkerCountIndependentReturn ==
  phase = "Returned" => returned = CanonicalOrder

CompletionPermutationIsUnobservable ==
  phase = "Returned" =>
    /\ SeqSet(completionTrace) = Tasks
    /\ Len(completionTrace) = TaskCount
    /\ returned = CanonicalOrder

EventuallyReturned ==
  <> (phase = "Returned")

====
