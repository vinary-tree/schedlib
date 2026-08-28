# Formal scheduler contract

This directory is the normative pre-implementation contract for schedlib.
[`tla/DeterministicScheduler.tla`](tla/DeterministicScheduler.tla) specifies
validation, planning, arbitrary worker completion, stable commit, failure,
cancellation, and terminal outcomes. [`tla/SchedulerKernels.tla`](tla/SchedulerKernels.tla)
contains the shared nonrecursive effect-independence definition and its TLAPS
theorem.

## Why two modules

TLC accepts finite recursive operators used as denotational definitions.
TLAPS' current front end does not accept recursive operator definitions in a
module, even when the proof itself does not depend on them. The kernel module is
therefore recursion-free and imported by the state-machine module. This is a
tool boundary, not semantic duplication: TLC and TLAPS consume the same
`Independent` operator.

The recursive specification operators are mathematical definitions only. They
do not authorize recursive Rust. Every corresponding production operation must
use an explicit heap-resident queue, arena, cursor, or—only for genuinely
pushdown semantics—a specialized iterative pushdown automaton (PDA).

## Scenario families

Each TLC configuration holds irrelevant dimensions fixed and exhaustively
varies one semantic boundary:

| Configuration | Exhaustive input family | Purpose |
| --- | --- | --- |
| `Dependencies.cfg` | 64 nonreflexive relations on three tasks | Exact directed-acyclic-graph acceptance and rejection |
| `Effects.cfg` | 256 read/write assignments on two tasks and two resources | Complete pairwise independence truth kernel |
| `Resources.cfg` | 27 task-cost maps on three tasks with budget two | Exact resource admission and exhaustion |
| `Outcomes.cfg` | 8 outcome maps crossed with 4 cancellation counts | Failure, cancellation, completion, and serial equivalence |

Two tasks are sufficient for the effect family because `Independent` is a
pairwise predicate. Two resources exhaust the empty, singleton, overlapping,
and disjoint set relationships. The state machine still checks full batch
pairwise independence through `PlanIsCanonicalAndLawful`.

## Commands

```sh
./scripts/verify-formal.sh tla
./scripts/verify-formal.sh tlaps
./scripts/verify-formal.sh all
```

The wrapper rejects incomplete runs: each TLC log must contain the no-error
verdict, a complete graph depth, and a zero-length queue. It also requires the
TLAPS one-obligation success verdict. Output remains available under
`target/verification/` until the evidence has been reviewed.

## Refinement rule

[`refinement-map.tsv`](refinement-map.tsv) is exhaustive and normative. A
production symbol is acceptable only when every row naming that symbol has:

1. its exhaustive oracle test;
2. its property or metamorphic test;
3. its small-stack or lifecycle test where required;
4. its complexity instrumentation where required; and
5. an evidence-backed status change from `required-before-implementation` to
   `accepted`.
