# schedlib documentation

These documents move from vocabulary and theory to the formal state machine,
implementation refinement, operational use, and security boundaries. The
formal model is normative; prose and diagrams explain it but do not override
it.

## Reading path

1. [Glossary](GLOSSARY.md) defines every recurring term and symbol.
2. [Deterministic scheduling theory](theory/deterministic-scheduling.md)
   derives dependency, effect, budget, and commit semantics.
3. [Architecture](design/architecture.md) assigns responsibilities to the
   planner, executor, commit machine, and adapters.
4. [Formal state machine](design/formal-state-machine.md) documents every
   phase and transition.
5. [Evidence method](scientific/evidence-method.md) explains what the bounded
   exhaustive checks establish and what they do not.
6. [Algorithms and complexity](engineering/algorithms-and-complexity.md)
   explains the selected stack-safe data structures and exact bounds.
7. [Refinement and testing](engineering/refinement-and-testing.md) translates
   every formal invariant into preimplementation tests and acceptance gates.
8. [Cancellation and resources](security/cancellation-and-resources.md)
   defines containment guarantees.
9. [Rust API](usage/rust-api.md) gives a complete construction and execution
   example.
10. [Formal workflow](usage/formal-workflow.md) gives reproducible commands and
   evidence locations.

## Diagram index

- [Contract architecture](design/figures/contract-architecture.svg) shows the
  boundary between mathematical specification and production refinement.
- [Scheduler state machine](design/figures/scheduler-state-machine.svg) shows
  validation, execution, ordered commit, and all terminal paths.
- [Parallel execution sequence](design/figures/parallel-execution-sequence.svg)
  shows arbitrary worker completion becoming deterministic observation.
- [Verification ladder](design/figures/verification-ladder.svg) shows why model
  checking, theorem proving, tests, and performance gates are all required.

The PlantUML source beside each SVG is authoritative. The render gate rebuilds
every SVG and rejects missing output.
