---- MODULE SchedulerKernels ----
EXTENDS FiniteSets

(***************************************************************************)
(* Nonrecursive scheduling kernels shared by the TLC state-machine model    *)
(* and TLAPS. Keeping this module recursion-free works within TLAPS' current *)
(* front-end limits while ensuring both verifiers consume one definition.   *)
(***************************************************************************)

Independent(readSets, writeSets, left, right) ==
  /\ writeSets[left] \cap
       (readSets[right] \cup writeSets[right]) = {}
  /\ writeSets[right] \cap
       (readSets[left] \cup writeSets[left]) = {}

THEOREM EffectIndependenceKernelIsSymmetric ==
  \A leftReads, rightReads, leftWrites, rightWrites :
    ((leftWrites \cap (rightReads \cup rightWrites) = {}
      /\ rightWrites \cap (leftReads \cup leftWrites) = {})
     <=>
     (rightWrites \cap (leftReads \cup leftWrites) = {}
      /\ leftWrites \cap (rightReads \cup rightWrites) = {}))
<1>1. QED
  OBVIOUS

====
