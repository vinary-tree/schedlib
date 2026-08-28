# schedlib

schedlib is the deterministic, synchronous-first scheduler foundation for the
Vinary categorical optimization pipeline. Its current revision is deliberately
formal-only: it defines and verifies the contract that the Rust implementation
must refine, but contains no production scheduler code.

The contract separates a deterministic immutable plan from potentially
parallel execution. Stable task identifiers determine a canonical topological
order and deterministic first-fit batches. Workers may complete in any order,
while ordered commit preserves the same observable result as serial execution.
Cycle rejection, resource exhaustion, fail-fast failure, and boundary-exact
cancellation are all decided by the verified state machine.

## Verification status

The formal baseline covers:

- the complete empty-schedule state machine;
- all 64 nonreflexive dependency relations over three tasks;
- all 256 read/write-set assignments in the complete two-task, two-resource
  effect-independence kernel;
- all 27 admissible-or-exhausted cost assignments for three tasks and budget
  two;
- all 27 success/failure/incomplete maps crossed with all 4 cancellation
  boundaries for the three-task outcome model; and
- one TLAPS theorem proving that the shared effect-independence kernel is
  symmetric.

The complete 32-obligation map is machine-readable in
[`formal/refinement-map.tsv`](formal/refinement-map.tsv). Production work may
begin only after this formal baseline is committed. Its first change must add
the exhaustive oracle and property tests named by that map; implementation
follows only after those tests demonstrably fail for the missing production
API.

## Documentation map

- [`docs/README.md`](docs/README.md) is the documentation index.
- [`formal/README.md`](formal/README.md) explains the executable models.
- [`docs/GLOSSARY.md`](docs/GLOSSARY.md) defines the vocabulary and symbols.
- [`docs/theory/deterministic-scheduling.md`](docs/theory/deterministic-scheduling.md)
  derives the scheduler semantics.
- [`docs/design/architecture.md`](docs/design/architecture.md) defines component
  boundaries and refinement targets.
- [`docs/design/formal-state-machine.md`](docs/design/formal-state-machine.md)
  specifies every state and transition.
- [`docs/scientific/evidence-method.md`](docs/scientific/evidence-method.md)
  explains the exhaustive evidence.
- [`docs/engineering/refinement-and-testing.md`](docs/engineering/refinement-and-testing.md)
  defines implementation and test acceptance.
- [`docs/security/cancellation-and-resources.md`](docs/security/cancellation-and-resources.md)
  covers cancellation and resource containment.
- [`docs/usage/formal-workflow.md`](docs/usage/formal-workflow.md) provides the
  operator workflow.

## Local gates

Run the bounded verification suite:

```sh
make verify
```

The scripts create all generated state, logs, and temporary files beneath the
ignored, persistent `target/` directory. Formal checks self-enter a
`systemd-run --user --scope` with a 4 GiB memory ceiling, no swap, bounded CPU,
and headless Java. No workflow uses a memory-backed temporary directory.
