# Durable resume evidence method

## Evidence question

The campaign asks whether the proposed durable protocol preserves schedlib's
canonical serial observation across validation, publication, crash, replay,
parallel completion, and terminal outcomes. No single tool establishes every
part of that claim. The evidence is therefore intentionally heterogeneous and
linked by the 35-row
[`durable-resume-invariants.tsv`](../../formal/durable-resume-invariants.tsv)
ledger.

The temporal model follows the state-machine style of Lamport's
[“The Temporal Logic of Actions”](https://doi.org/10.1145/177492.177726).
Checkpoint reasoning is also informed by Chandy and Lamport's separation of a
consistent recorded state from ongoing computation in
[“Distributed Snapshots: Determining Global States of Distributed Systems”](https://doi.org/10.1145/214451.214456).
schedlib does not implement that distributed snapshot algorithm; the citation
explains why recorded state and volatile activity must be distinguished.

## Evidence ladder

| Layer | Artifact | Strength | Boundary |
| --- | --- | --- | --- |
| Constructive proof | `formal/coq/DurableResume.v` | Universal theorems over generic types and finite lists | Does not select concrete Rust representations |
| Temporal model checking | `formal/tla/DurableResume.tla` plus eight configurations | Every reachable state and fair trace within each declared finite scenario | Finite constants only |
| Decidable controls | `formal/smt/durable-resume.smt2` | Exact satisfiable/unsatisfiable verdicts for 21 bounded propositions | Projection, not the full transition system |
| Independent executable oracle | `scripts/check-durable-resume-exhaustive.py` | 35 named properties over 4,695 finite cases | Bounded Python reference domain |
| Mutation adequacy | `scripts/check-durable-resume-mutants.py` | Each of 35 causal faults is killed by its mapped oracle | Fault set is declared, not universal |
| Required-red Rust contract | `formal/required-red/durable-resume/tests/contracts.rs` | Freezes 35 future implementation properties before API construction | Intentionally cannot compile until the durable API exists |

## Finite scenario families

The eight TLA+ configurations factor unrelated dimensions while retaining the
full safety invariant set.

| Scenario | Varied dimension | Final distinct states |
| --- | --- | ---: |
| Crash | Crash before append, after append, and after publication | 36 |
| Resume | Every success-prefix length and every publication-prefix length | 320 |
| Outcomes | Every three-task success/failure/incomplete assignment | 170 |
| Resources | Cancellation and resource boundaries with deterministic collision priority | 367 |
| Stale | Foreign plan identity | 2 |
| Key collision | Invalid external-key map | 2 |
| Malformed | Noncanonical checkpoint shape | 2 |
| Unsafe replay | Unwitnessed pre-journal crash | 36 |

The executable oracle independently enumerates zero-, one-, and two-task
protocol cases across outcomes and boundary positions. It additionally checks
all $`2^9=512`$ subsets of the three crash points for each of three canonical
events in a two-task successful run.

## Counterexamples retained in the design

Counterexamples changed the model rather than being suppressed.

1. Checkpoint validation originally depended on evolving protocol state. It
   now depends only on the immutable input snapshot.
2. A boolean next-state expression admitted an unassigned branch because of
   operator precedence. Explicit parentheses closed the branch.
3. Recovery originally advanced past a durable event whose receipt lagged. The
   machine now publishes the earliest missing durable event before new work.
4. Rocq initially numbered events from zero while TLA+, SMT, and the ledger
   numbered them from one. The constructive journal now proves the one-based
   event language explicitly.
5. The Rocq checkpoint cursor initially counted every journal event, including
   terminal events. It now counts only the leading successful task events.
6. Publication proofs initially keyed receipts by ordinal alone. They now prove
   idempotency and non-invention for structural plan-and-ordinal identifiers.
7. Cancellation and resource limitation were both enabled at an equal
   boundary. The state machine now specifies cancellation-before-resource
   priority and checks it as `BoundaryPriorityIsDeterministic`.

## Non-vacuity and negative controls

Nineteen SMT bad-state queries are unsatisfiable. Two witness queries are
satisfiable: an unsafe pre-journal crash has a fail-closed state, and a durable
event can temporarily lack a receipt before recovery publishes it. These
satisfiable controls demonstrate that the safety formulas did not erase the
failure and recovery states they are meant to govern.

The mutation gate edits one unique semantic site per ledger row and invokes
only that row's mapped oracle. A mutant is counted as killed only when output
contains the expected property identifier. A syntax error, timeout, transport
failure, or different property failure is rejected as noncausal evidence.

## Reproducibility

Run:

```sh
./scripts/verify-durable-resume-formal.sh
```

The wrapper enters a user `systemd-run` scope with a 4 GiB memory ceiling, no
swap, one TLC worker, one Cargo job, bounded process count, and headless Java.
All generated state and logs are beneath `target/durable-resume/` on the
repository-backed filesystem. No evidence path uses a memory-backed temporary
directory.
