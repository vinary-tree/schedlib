# Algorithms and complexity

## Parameters

Let $`V`$ be the number of tasks, $`E`$ the number of distinct dependency
edges, $`R`$ the total read/write effect occurrences, $`P`$ the number of plan
batches, and $`Q`$ the number of capacity-admissible batch candidates examined
by exact first fit. Let $`e_t`$ be the effect count of task $`t`$.

The work profiles report semantic events rather than elapsed time. This makes
complexity regressions reproducible across machines and separates scheduler
work from caller task work.

## Canonical graph and topology

libvgraph constructs stable forward and reverse compressed sparse row (CSR)
arrays. Arbitrary stable identifiers are canonicalized once; dependency pairs
are deduplicated with fixed-width radix passes. schedlib retains a randomized
stable-ID index and moves payloads into libvgraph's canonical dense order in
linear work. It does not sort payloads a second time.

Kahn's algorithm stores every indegree in a flat vector and every ready vertex
in a binary min-heap. Each vertex is pushed and popped exactly once; each edge
is inspected exactly once:

```math
T_{\mathrm{topo}}(V,E)=O((V+E)\log V),
\qquad S_{\mathrm{topo}}(V,E)=O(V+E).
```

The implementation is one iterative loop. It does not invoke Tarjan's
algorithm because schedule acceptance needs only a topological order and cycle
rejection; SCC materialization would compute strictly more information.

## Exact first-fit placement

For each task, predecessor batch locations determine the first legal batch
index. A max segment tree over remaining batch capacities then finds the first
capacity-admissible candidate at or after that floor. Tree search uses a reused
heap-resident frame vector; no call-stack recursion occurs.

Each batch stores aggregate read and write resources in sparse hash sets. For a
candidate, writes are checked against aggregate reads and writes, and reads are
checked against aggregate writes. This is equivalent to scanning all batch
members pairwise, but membership work depends on the new task's effects rather
than the number of earlier members.

The exact policy-sensitive bound is:

```math
T_{\mathrm{place}}
=O\!\left(\sum_t q_t(\log P+e_t)\right),
\qquad Q=\sum_t q_t,
```

where $`q_t`$ is the number of capacity-admissible candidates inspected for
task $`t`$. Exact first fit may need to reject several conflicting candidates;
the segment tree skips every capacity-inadmissible subtree without scanning its
batches or members. `batch_probes`, `conflict_lookups`, and
`capacity_tree_nodes` expose these terms directly.

After assignment, each batch's member vector is canonicalized in place with an
iterative binary heap. This implements the formally verified sorted-insertion
result without quadratic vector shifting. Across all batches its bound is
$`O(V\log V)`$ and it allocates no second member buffer.

## Execution and commit

The immutable stable-ID index maps each completion to a canonical dense task
and its batch-local result slot in expected constant time. Each valid
completion is stored once; each published outcome is removed once and appended
once. Serial dispatch, completion validation, cancellation polling, commit, and
destruction are iterative:

```math
T_{\mathrm{execute}}=O(V),
\qquad S_{\mathrm{execute}}=O(V+B_{\max}),
```

where $`B_{\max}`$ is the largest batch. The $`O(V)`$ committed prefix is a
returned observation; current-batch scratch is $`O(B_{\max})`$.

## Stack and storage guarantees

All scheduler-controlled collections are flat vectors, hash tables, heaps, or
CSR arrays. Segment-tree traversal uses explicit frames. Standard container
destruction iterates their flat storage, so task, edge, batch, and dependency
depth do not increase native call-stack depth.

The complete planning bound, excluding caller payloads and returned outputs,
is:

```math
S_{\mathrm{plan}}=O(V+E+R+P).
```

`PlanWorkProfile` records exact graph and probe events, a conservative heap-sort
work bound, logical allocation sites, and a conservative temporary-slot bound.
`ExecutionWorkProfile` records dispatch, completion, commit, current-batch
scratch, and execution allocation events.
