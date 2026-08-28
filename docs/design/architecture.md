# Architecture

## Boundary rule

schedlib owns deterministic schedule planning and synchronous serial
execution. Parallel runtimes are adapters over the same immutable plan and
ordered commit contract. Optimizer-specific rewrite semantics, parser
semantics, and graph-analysis algorithms remain in their owning crates.

![Component diagram of the formal contract, planner, executors, ordered commit, and consumers.](figures/contract-architecture.svg)

[PlantUML source for the contract architecture](figures/contract-architecture.puml)

## Components

Formal contract
: TLA+ state machine, TLAPS kernel theorem, exhaustive configurations, and the
  refinement registry. It is normative and contains no production callbacks.

Input snapshot
: Immutable tasks, stable identifiers, dependencies, effects, costs, budget,
  outcomes supplied by execution, and cancellation policy.

Plan builder
: Performs exact validation, canonical topology, dependency-floor calculation,
  and deterministic first-fit placement. It publishes either one immutable plan
  or one rejection.

Serial executor
: Normative production executor. It visits batches and tasks with iterative
  cursors, captures results, and drives ordered commit synchronously.

Parallel adapter
: Optional executor that dispatches one independent batch to a runtime. It may
  receive completions in any order but cannot bypass ordered commit. The first
  adapter is planned as a Rayon-backed feature after the serial core is
  accepted.

Ordered commit machine
: The sole publication boundary. It converts an unordered completed-result set
  into the canonical prefix, applies fail-fast policy, and acknowledges
  cancellation at commit boundaries.

Consumer
: lling-llang or another pipeline component that supplies tasks and observes
  the terminal outcome. schedlib does not embed lling-llang policy.

## Data ownership

The plan is immutable after validation. Execution state contains only indices,
an internal result store, and cancellation state. Production APIs must prevent
workers from mutating plan or input metadata. Results move exactly once from
the internal store to the ordered commit sink.

## Why schedlib is independent

Keeping schedlib standalone provides one verified scheduler contract to
lling-llang, Replete, libvgraph, and other Vinary crates without introducing an
optimizer dependency into foundational libraries. Runtime adapters depend on
schedlib, never the reverse. This produces a synchronous-first core that works
without a thread pool and a parallel extension that cannot change semantics.

## Stack-safety architecture

The dependency graph and plan are flat finite structures, so an iterative
topological machine and explicit execution cursors are the optimal abstraction.
A PDA would add unnecessary control and storage for these operations. If a
future task language introduces mutual recursion over nested terms, that
extension must refine its denotation to a specialized iterative PDA outside the
flat scheduler core.
