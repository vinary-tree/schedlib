# Glossary

This glossary defines schedlib's terminology before it appears in formulas,
tables, or diagrams.

## Symbols

- Task set, $`\mathcal{T}`$: the finite set of stable task identifiers.
- Resource set, $`\mathcal{R}`$: the finite universe named by effect sets.
- Dependency relation, $`D \subseteq \mathcal{T}\times\mathcal{T}`$: an edge
  $`(u,v)`$ means task $`u`$ must commit before task $`v`$.
- Read set, $`R(t)`$: resources read by task $`t`$.
- Write set, $`W(t)`$: resources written by task $`t`$.
- Cost, $`c(t) \in \mathbb{N}_{>0}`$: the modeled resource cost of task
  $`t`$.
- Budget, $`B \in \mathbb{N}_{>0}`$: the maximum aggregate cost in one
  batch.
- Plan, $`P = \langle P_1,\ldots,P_k\rangle`$: an immutable sequence of
  ordered task batches.
- Flattening, $`\mathrm{flat}(P)`$: concatenation of every batch in a
  plan.
- Committed prefix, $`C`$: the observable prefix of
  $`\mathrm{flat}(P)`$.

## Terms

Canonical
: Uniquely determined by semantic input. Stable task identifiers break all
  otherwise arbitrary topological ties.

Commit
: Publication of one completed task outcome to the caller-visible ordered
  result. Worker completion alone is not observable commit.

Directed acyclic graph (DAG)
: A directed graph with no directed cycle. An accepted dependency relation must
  be a DAG.

Effect independence
: A symmetric relation between two tasks: neither task writes a resource read
  or written by the other.

Exhaustive bounded model
: Enumeration of every input and reachable execution state within declared
  finite bounds. It is exhaustive for those bounds, not a proof over all input
  sizes.

Fail-fast
: Termination immediately after publishing the first failure in canonical
  commit order. Results later in that order remain unpublished.

First-fit batch
: The earliest batch after every dependency predecessor that can accept a task
  without effect conflict or budget overflow.

Incomplete outcome
: A task result that is neither success nor semantic failure because the
  injected executor could not finish. Its caller-defined reason is preserved,
  committed at canonical position, and terminates the run.

Fairness
: The TLA+ weak-fairness assumption that an action continuously enabled by the
  scheduler is eventually taken.

Native-stack safety
: Scheduler-controlled stack consumption is bounded independently of task,
  edge, batch, or nesting depth.

Pushdown automaton (PDA)
: An automaton with finite control and an explicit unbounded stack. schedlib
  requires a specialized iterative PDA only when semantics are genuinely
  pushdown-shaped, such as mutual recursion over nested syntax.

Refinement
: A production implementation relation that preserves every observable
  behavior and invariant of the formal model while adding concrete data
  structures and performance bounds.

Stable task identifier
: A unique totally ordered identifier supplied before planning. It is the only
  tie breaker used by the canonical order.

Terminal phase
: Exactly one of completed, failed, incomplete, cancelled, rejected-cycle, or
  exhausted.
