# schedlib

schedlib is the deterministic, synchronous-first scheduler foundation for the
Vinary categorical optimization pipeline. The production serial core refines
the repository's TLA+ state machine and TLAPS theorem. It constructs immutable
plans over libvgraph canonical CSR, accepts injected task or batch executors,
and normalizes arbitrary batch completion order through stable ordered commit.
The core requires no asynchronous runtime or thread pool.

The contract separates a deterministic immutable plan from potentially
parallel execution. Stable task identifiers determine a canonical topological
order and deterministic first-fit batches. Workers may complete in any order,
while ordered commit preserves the same observable result as serial execution.
Cycle rejection, resource exhaustion, fail-fast failure, and boundary-exact
cancellation are all decided by the verified state machine.

## Verification status

The formal baseline covers:

- the complete empty-schedule state machine;
- the minimal four-task case proving stable sorted insertion when a low task
  identifier becomes ready late;
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
[`formal/refinement-map.tsv`](formal/refinement-map.tsv). Its 68 required test
names are audited mechanically. The production suite also crosses validation
precedence, sparse stable identifiers, duplicate edges, maximum-width costs,
and effect canonicalization. Deep-chain, wide-DAG, success, failure,
incomplete, cancellation, and destruction paths run on 64 KiB native stacks.

## Use the serial core

[`docs/usage/rust-api.md`](docs/usage/rust-api.md) contains a complete compiling
example and defines the executor, cancellation, and commit contracts. In
outline:

1. construct `TaskSpec` values with stable `TaskId`, `TaskEffects`, and `Cost`;
2. call `PlanBuilder::build` with dependencies and a positive `Budget`;
3. inject `SerialExecutor` or another complete-batch executor; and
4. observe only the stable prefix delivered through `CommitSink` and
   `ExecutionReport`.

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
- [`docs/usage/rust-api.md`](docs/usage/rust-api.md) documents production API
  construction and execution.
- [`docs/engineering/algorithms-and-complexity.md`](docs/engineering/algorithms-and-complexity.md)
  specifies the selected data structures and exact complexity parameters.

## Local gates

Run the bounded verification suite:

```sh
make verify
```

Run production acceptance with one Cargo job inside a bounded systemd scope as
described in the [formal workflow](docs/usage/formal-workflow.md). The required
gates include `cargo test --all-targets`, release tests, strict Clippy, Rustdoc,
diagram rendering, `vinary-doc-lint`, and `pgmcp bug-gate`.

The scripts create all generated state, logs, and temporary files beneath the
ignored, persistent `target/` directory. Formal checks self-enter a
`systemd-run --user --scope` with a 4 GiB memory ceiling, no swap, bounded CPU,
and headless Java. No workflow uses a memory-backed temporary directory.
