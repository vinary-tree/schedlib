# schedlib documentation

These documents move from vocabulary and theory to the formal state machine,
implementation refinement, operational use, and security boundaries. The
formal model is normative; prose and diagrams explain it but do not override
it.

## Reading path

1. [Glossary](GLOSSARY.md) defines every recurring term and symbol.
2. [Deterministic scheduling theory](theory/deterministic-scheduling.md)
   derives dependency, effect, budget, and commit semantics.
3. [Durable identity and resume theory](theory/durable-identity-and-resume.md)
   derives structural identity, journal-first publication, and crash recovery.
4. [Architecture](design/architecture.md) assigns responsibilities to the
   planner, executor, commit machine, and adapters.
5. [Durable resume protocol](design/durable-resume-protocol.md) defines the
   persistence boundary and iterative recovery algorithm.
6. [Formal state machine](design/formal-state-machine.md) documents every
   phase and transition.
7. [Evidence method](scientific/evidence-method.md) explains what the bounded
   exhaustive checks establish and what they do not.
8. [Durable resume evidence](scientific/durable-resume-evidence.md) records the
   proof, model-checking, oracle, mutation, and required-red ladder.
9. [Algorithms and complexity](engineering/algorithms-and-complexity.md)
   explains the selected stack-safe data structures and exact bounds.
10. [Refinement and testing](engineering/refinement-and-testing.md) translates
   every formal invariant into tests and acceptance gates.
11. [Durable resume implementation acceptance](engineering/durable-resume-verification.md)
   gives the implementation shapes, properties, and terminal gates.
12. [Cancellation and resources](security/cancellation-and-resources.md)
   defines containment guarantees.
13. [Durable resume threat model](security/durable-resume-threat-model.md)
   identifies trust boundaries and fail-closed behavior.
14. [Rust API](usage/rust-api.md) gives a complete construction and execution
   example.
15. [Durable committed-prefix resume](usage/durable-resume-contract.md)
   explains the implemented core, replay policy, and continuing verification.
16. [Formal workflow](usage/formal-workflow.md) gives reproducible commands and
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
- [Durable resume boundaries](design/figures/durable-resume-boundaries.svg)
  assigns semantic, codec, artifact, and consumer responsibilities.
- [Durable resume state machine](design/figures/durable-resume-state-machine.svg)
  shows validation, append, publication, crash, recovery, and terminal paths.
- [Durable crash-recovery sequence](design/figures/durable-crash-recovery-sequence.svg)
  shows the two durable monotone structures across crash windows.

The PlantUML source beside each SVG is authoritative. The render gate rebuilds
every SVG and rejects missing output.
