# schedlib

schedlib is the deterministic, synchronous-first scheduler foundation for the
Vinary categorical optimization pipeline. The production serial core refines
the repository's TLA+ state machine and TLAPS theorem. It constructs immutable
plans over libvgraph canonical CSR, accepts injected task or batch executors,
and normalizes arbitrary batch completion order through stable ordered commit.
The core requires no asynchronous runtime or thread pool; the optional
`rayon` feature provides an owned, explicitly sized local pool.

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
- all 528 reachable bounded-dispatch, completion, and join states for four
  tasks at Rayon worker counts 1, 2, and 4; and
- one TLAPS theorem proving that the shared effect-independence kernel is
  symmetric.

The complete 32-obligation map is machine-readable in
[`formal/refinement-map.tsv`](formal/refinement-map.tsv). Its 68 required test
names are audited mechanically. The production suite also crosses validation
precedence, sparse stable identifiers, duplicate edges, maximum-width costs,
and effect canonicalization. Deep-chain, wide-DAG, success, failure,
incomplete, cancellation, and destruction paths run on 64 KiB native stacks.
The Rayon refinement adds 16 tests, including every four-task physical
completion permutation and a 20,000-task batch on 64 KiB caller and worker
stacks.

The durable-resume contract adds 35 invariant-ledger rows,
45 referenced Rocq obligations with 18 explicit no-assumption reports, 18
configured TLA+ predicates across eight scenario families, 21 exact SMT
controls, 4,695 independent executable-oracle cases, 35 causally killed
mutants, and 35 Rust refinement properties. The stack-safe semantic core is
accepted at commit `086bfb5d6a240ccc7c4e5f3bbaae1e7ed9a4cea1`; all 35
properties pass, including 100,000 tasks on a 64 KiB stack. Portable codecs
remain assigned to `schedlib-interop`, and runtime artifacts remain outside
schedlib. Borrowed semantic views and a checked event-kind constructor provide
that crate boundary without adding a codec, digest, or storage dependency to
schedlib.

## Use the serial core

[`docs/usage/rust-api.md`](docs/usage/rust-api.md) contains a complete compiling
example and defines the executor, cancellation, and commit contracts. In
outline:

1. construct `TaskSpec` values with stable `TaskId`, `TaskEffects`, and `Cost`;
2. call `PlanBuilder::build` with dependencies and a positive `Budget`;
3. inject `SerialExecutor` or another complete-batch executor; and
4. observe only the stable prefix delivered through `CommitSink` and
   `ExecutionReport`.

Enable `rayon` to use `RayonExecutor` with an exact positive worker count. The
parallel example is compile-checked and executed by the acceptance gate:

```sh
cargo run --release --features rayon --example rayon
```

## Documentation map

- [`docs/README.md`](docs/README.md) is the documentation index.
- [`formal/README.md`](formal/README.md) explains the executable models.
- [`docs/GLOSSARY.md`](docs/GLOSSARY.md) defines the vocabulary and symbols.
- [`docs/theory/deterministic-scheduling.md`](docs/theory/deterministic-scheduling.md)
  derives the scheduler semantics.
- [`docs/theory/durable-identity-and-resume.md`](docs/theory/durable-identity-and-resume.md)
  defines structural identity and crash-safe committed-prefix recovery.
- [`docs/design/architecture.md`](docs/design/architecture.md) defines component
  boundaries and refinement targets.
- [`docs/design/formal-state-machine.md`](docs/design/formal-state-machine.md)
  specifies every state and transition.
- [`docs/design/durable-resume-protocol.md`](docs/design/durable-resume-protocol.md)
  assigns persistence responsibilities and specifies the iterative protocol.
- [`docs/scientific/evidence-method.md`](docs/scientific/evidence-method.md)
  explains the exhaustive evidence.
- [`docs/scientific/durable-resume-evidence.md`](docs/scientific/durable-resume-evidence.md)
  records the durable proof, oracle, mutation, and required-red evidence.
- [`docs/engineering/refinement-and-testing.md`](docs/engineering/refinement-and-testing.md)
  defines implementation and test acceptance.
- [`docs/security/cancellation-and-resources.md`](docs/security/cancellation-and-resources.md)
  covers cancellation and resource containment.
- [`docs/security/durable-resume-threat-model.md`](docs/security/durable-resume-threat-model.md)
  covers checkpoint substitution, corruption, replay, and resource attacks.
- [`docs/usage/formal-workflow.md`](docs/usage/formal-workflow.md) provides the
  operator workflow.
- [`docs/usage/durable-resume-contract.md`](docs/usage/durable-resume-contract.md)
  provides the implemented core and verification workflow.
- [`docs/usage/rust-api.md`](docs/usage/rust-api.md) documents production API
  construction and execution.
- [`docs/engineering/algorithms-and-complexity.md`](docs/engineering/algorithms-and-complexity.md)
  specifies the selected data structures and exact complexity parameters.
- [`docs/engineering/durable-resume-verification.md`](docs/engineering/durable-resume-verification.md)
  defines implementation acceptance for all 35 durable invariants.

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
