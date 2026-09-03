# Formal scheduler contract

This directory is the normative contract that preceded and continues to govern
schedlib's production implementation.
[`tla/DeterministicScheduler.tla`](tla/DeterministicScheduler.tla) specifies
validation, planning, arbitrary worker completion, stable commit, failure,
cancellation, and terminal outcomes. [`tla/SchedulerKernels.tla`](tla/SchedulerKernels.tla)
contains the shared nonrecursive effect-independence definition and its TLAPS
theorem.
[`tla/RayonAdapter.tla`](tla/RayonAdapter.tla) separately specifies the
optional parallel adapter's bounded worker set, nondeterministic physical
completion, join barrier, and canonical returned completion vector.
[`tla/DurableResume.tla`](tla/DurableResume.tla),
[`coq/DurableResume.v`](coq/DurableResume.v), and
[`smt/durable-resume.smt2`](smt/durable-resume.smt2) jointly specify the
durable-identity, journal-first publication, crash, and
committed-prefix resume contract.

## Why the contract is split

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
| `RayonAdapter.cfg` | Every dispatch/completion interleaving for four tasks at worker limits 1, 2, and 4 | Exact-once bounded execution, join-before-return, worker-count-independent order, and liveness |

The durable model has eight additional configurations. `Crash`, `Resume`,
`Outcomes`, and `Resources` vary protocol behavior. `Stale`, `KeyCollision`,
`Malformed`, and `UnsafeReplay` isolate fail-closed rejection. All check the
same safety set, including structural input immutability, canonical journal,
durable-before-publish ordering, receipt monotonicity, typed terminal outcome,
deterministic cancellation-before-resource priority, and eventual terminal
progress.

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

`RayonAdapter.cfg` begins at an already accepted nonempty batch. A task moves
monotonically from pending to active to completed. Physical completion order
is recorded but never returned. The join transition is enabled only after all
tasks complete and returns the canonical stable-task sequence independently of
the selected worker limit.

## Commands

```sh
./scripts/verify-formal.sh rayon
./scripts/verify-formal.sh tla
./scripts/verify-formal.sh tlaps
./scripts/verify-formal.sh all
./scripts/verify-durable-resume-formal.sh
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

[`rayon-refinement-map.tsv`](rayon-refinement-map.tsv) applies the same rule to
the optional Rayon adapter. All eight rows are `accepted`: repository history
preserves the test-only missing-adapter failure, and the completed implementation
passes every named test in debug and optimized-release builds. The model assumes
finite nonpanicking task callbacks: Rust panics are outside `TaskExecution` and
therefore propagate according to Rayon rather than being misreported as typed
success, failure, or incompleteness.

[`durable-resume-invariants.tsv`](durable-resume-invariants.tsv) has 35 rows.
Each row names its Rocq theorem, configured TLA+ predicate, SMT control,
independent executable oracle, required-red Rust property, and causal mutant.
Every row names accepted implementation commit
`086bfb5d6a240ccc7c4e5f3bbaae1e7ed9a4cea1`. The integrated verifier
resolves that commit and requires exact bidirectional name coverage, 18 closed assumption reports,
successful `coqchk`, eight complete TLC searches, 21 expected SMT verdicts,
4,695 oracle cases, 35 killed mutants, and a Cargo failure caused only by the
reviewed missing `schedlib::durable` module before implementation or all 35
passing Rust properties after acceptance.
