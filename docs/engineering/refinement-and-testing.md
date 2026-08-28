# Refinement and testing

## No-code-before-contract rule

The formal baseline preceded production code. The test crate and independent
reference oracle were committed next and demonstrated a missing-production
compile failure. Only after that red baseline did the serial implementation
begin. The repository history preserves this ordering.

[`../../formal/refinement-map.tsv`](../../formal/refinement-map.tsv) names every
required test and implementation obligation. All 32 rows are now `accepted`:
the six TLC configurations reach an empty state queue without an error, TLAPS
proves the universal effect-kernel theorem, and every one of the 68 registered
test names passes in both debug and optimized-release builds. Six additional
regressions bring the executed implementation suite to 74 tests.

The optional Rayon adapter has its own eight-row registry at
[`../../formal/rayon-refinement-map.tsv`](../../formal/rayon-refinement-map.tsv).
All eight rows are now `accepted`. They advanced atomically only after the
parallel red baseline and every worker-count, timing, join, lifecycle,
small-stack, and equivalence test passed in debug and optimized-release builds.

## Test layers

Exhaustive oracle tests
: Enumerate every small dependency relation, effect map, cost map, outcome map,
  cancellation boundary, and worker-completion permutation. Compare production
  observations with a deliberately simple independent reference.

Property tests
: Generate larger DAGs, conflicts, budgets, and outcomes. Check invariants
  directly and apply metamorphic changes such as input permutation, stable-ID
  relabeling, addition of isolated tasks, and completion-order permutation.

Small-stack tests
: Launch dedicated threads with a deliberately small native stack. Exercise
  deep chains, wide DAGs, success, failure, cancellation, and destruction.
  Input size must not affect native-stack depth.

Complexity tests
: Instrument queue operations, edge visits, batch probes, conflict lookups,
  commits, and allocations. Assert analytical bounds rather than inferring
  complexity from elapsed time.

Benchmarks
: Preregister representative chains, antichains, layered DAGs, dense DAGs,
  sparse and dense effects, and mixed costs. Compare against the accepted
  baseline only after correctness gates pass.

## Stack-safe refinement

The model's recursive operators are finite denotational notation. Production
must use:

- adjacency arrays and an indegree vector for dependency traversal;
- a min-ready structure for canonical topology;
- preallocated batch metadata and explicit placement cursors;
- indexed result slots and batch-local remaining counters;
- iterative commit cursors; and
- explicitly iterative destruction for any depth-shaped owned structure.

These are finite-state graph operations, not a context-free recognition
problem, so a PDA would be less direct. A future mutually recursive nested-task
extension must introduce a dedicated PDA with finite control, an explicit value
stack, and a transition invariant. It may not emulate recursion with native
calls.

## Work and space contracts

Let $`V=|\mathcal{T}|`$, $`E=|D|`$, $`R`$ be effect-storage size, and
$`P`$ be plan-storage size. Stable topology must use
$`O((V+E)\log V)`$ work or better and $`O(V+E)`$ auxiliary storage.
Execution after planning must use $`O(V+E)`$ scheduler work plus caller task
work.

The accepted batch index combines a preallocated max-capacity segment tree with
sparse aggregate read/write hash sets. A segment query skips every contiguous
subtree whose maximum remaining capacity is too small. Only capacity-admissible
candidates incur sparse effect membership checks. The complete parameterized
bound and operation counters are specified in
[Algorithms and complexity](algorithms-and-complexity.md).

Overall scheduler auxiliary storage is bounded by:

```math
O(V+E+R+P),
```

excluding caller-owned task values and returned outcomes. Capacity planning
must use checked arithmetic, and allocation failure must not publish a partial
plan.

## Acceptance order

1. Formal syntax, model checking, and theorem proof.
2. Registry completeness audit.
3. Test-only failing baseline.
4. Production implementation.
5. Exhaustive, property, metamorphic, lifecycle, and small-stack tests.
6. Formatting, lints, API documentation, and minimum-supported-Rust-version
   checks.
7. Preregistered performance and allocation gates.
8. `vinary-doc-lint`, diagram rendering, and `pgmcp bug-gate`.
9. Exact generated-artifact cleanup and clean-worktree audit.

No later gate can compensate for an earlier failure.
