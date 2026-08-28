# Formal scheduler contract

This directory is the normative contract that preceded and continues to govern
schedlib's production implementation.
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
| `Empty.cfg` | The complete zero-task state machine | Atomic empty completion with no work or publication |
| `BatchOrder.cfg` | The minimal four-task late-low-identifier counterexample | Sorted insertion within a deterministic first-fit batch |
| `Dependencies.cfg` | 64 nonreflexive relations on three tasks | Exact directed-acyclic-graph acceptance and rejection |
| `Effects.cfg` | 256 read/write assignments on two tasks and two resources | Complete pairwise independence truth kernel |
| `Resources.cfg` | 27 task-cost maps on three tasks with budget two | Exact resource admission and exhaustion |
| `Outcomes.cfg` | 27 outcome maps crossed with 4 cancellation counts | Failure, incomplete, cancellation, completion, and serial equivalence |

Two tasks are sufficient for the effect family because `Independent` is a
pairwise predicate. Two resources exhaust the empty, singleton, overlapping,
and disjoint set relationships. The state machine still checks full batch
pairwise independence through `PlanIsCanonicalAndLawful`.

`BatchOrder.cfg` factorizes planning from execution and closes a boundary that
the three-task families cannot expose. Its two-state planning transition checks
the same validation and plan-law invariants without redundantly multiplying
the already-exhaustive execution interleavings. Minimum-ready Kahn order is
canonical but not globally increasing. A task with a low identifier can become
ready late and fit an existing later batch. The
planner therefore inserts it at its sorted position rather than appending it.
The fixed scenario proves the resulting plan is `<<2, 4>, <1, 3>>` for the
dependency `4 -> 1` and conflict pairs `{1, 4}` and `{2, 3}`.

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
