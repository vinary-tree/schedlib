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
- Structural plan identity, $`I=\langle v,K,D,E,C,B,S\rangle`$: exact schema,
  external keys, dependencies, effects, costs, budget, and semantic profile.
- Durable journal, $`J=\langle e_1,\ldots,e_m\rangle`$: the authoritative
  append-only sequence of canonical task and terminal events.
- Publication receipts, $`R`$: the canonical prefix of journal event
  identifiers already exposed logically.
- Event identifier, $`\mathrm{id}(e)=\langle I,o(e)\rangle`$: structural plan
  identity paired with a one-based event ordinal.
- Replay measure, $`\mu(m,c)=m-c`$: journal events remaining after cursor
  $`c`$ in a journal of length $`m`$.

## Terms

Canonical
: Uniquely determined by semantic input. Stable task identifiers break all
  otherwise arbitrary topological ties.

Commit
: Publication of one completed task outcome to the caller-visible ordered
  result. Worker completion alone is not observable commit.

Committed-prefix checkpoint
: Immutable structural plan identity, canonical journal, canonical publication
  prefix, successful-task cursor, integrity result, and bounded representation
  counts sufficient to validate and resume one run.

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

Journal-first publication
: Ordering rule requiring a canonical event to be durable in the authoritative
  journal before its identifier can be published logically.

Native-stack safety
: Scheduler-controlled stack consumption is bounded independently of task,
  edge, batch, or nesting depth.

Logical exactly-once publication
: Idempotent caller-visible observation keyed by structural plan and event
  ordinal. Physical delivery may retry after a crash.

Pushdown automaton (PDA)
: An automaton with finite control and an explicit unbounded stack. schedlib
  requires a specialized iterative PDA only when semantics are genuinely
  pushdown-shaped, such as mutual recursion over nested syntax.

Refinement
: A production implementation relation that preserves every observable
  behavior and invariant of the formal model while adding concrete data
  structures and performance bounds.

Replay class
: Immutable evidence declaring a task deterministic, idempotent,
  transactional, or unsafe when a crash occurs before journal append.

Structural identity
: Exact equality over every semantic manifest field. A finite digest may index
  this value but does not replace structural comparison.

Stable task identifier
: A unique totally ordered identifier supplied before planning. It is the only
  tie breaker used by the canonical order.

Terminal phase
: Exactly one of completed, failed, incomplete, cancelled, resource limited,
  rejected, rejected cycle, or exhausted, as determined by the applicable
  scheduler protocol.
