# Using durable committed-prefix resume

## Current status

The durable semantic core is implemented by `schedlib::durable` at evidence
commit `086bfb5d6a240ccc7c4e5f3bbaae1e7ed9a4cea1`. The formal model,
executable oracle, mutation controls, and 35-property Rust suite continue to
govern it. Existing serial and Rayon schedlib APIs remain unchanged.

An application must not copy the Python oracle into production or treat its
in-memory classes as a serialization format. Portable bytes will be provided by
`schedlib-interop`; durable files remain owned by `vinary-runtime`.

## Verify the contract

Run the complete bounded gate:

```sh
./scripts/verify-durable-resume-formal.sh
```

A valid postimplementation run ends with:

```text
Validated all 35 causal postimplementation durable-resume properties.
unfinished-marker audit passed
Durable resume formal verification completed successfully.
```

The same harness operated as a required-red boundary before implementation.
Its mode now comes from the invariant ledger: every property must compile and
pass when all rows carry commit-linked `accepted@…` evidence.

To inspect one executable property without running the full suite:

```sh
python3 scripts/check-durable-resume-exhaustive.py \
  --property oracle_lagging_publication_recovery
```

Run all causal fault controls with:

```sh
python3 scripts/check-durable-resume-mutants.py
```

## Read the evidence

Persistent logs are under `target/durable-resume/logs/`. The most useful files
are:

- `invariant-ledger.log` for exact formal symbol registration;
- `rocq-assumptions.log` and `rocq-kernel.log` for proof closure;
- `tlc-*.log` for scenario state counts and liveness results;
- `z3-durable-resume.log` for exact SMT verdicts;
- `executable-oracle.log` for all bounded property counts;
- `causal-mutants.log` for property-specific fault kills; and
- `durable-resume-required-red.log` for the 35-property Rust result.

## Interpret the checkpoint schema

The semantic checkpoint fields are fixed even though portable syntax is not:

| Field | Meaning | Validation rule |
| --- | --- | --- |
| Structural plan identity | Schema, keys, dependencies, effects, costs, budget, semantic profile | Exact equality with active plan |
| Journal | Canonical ordered task and terminal events | One-based ordinals, exact task positions, at most one terminal suffix |
| Publication receipts | Logical publications keyed by plan and ordinal | Exact canonical prefix of the journal |
| Cursor | Next zero-based task index | Length of leading successful task events |
| Integrity result | Interop/runtime verification outcome | Must be valid before semantic acceptance |
| Encoded event count | Declared record count | Equal to journal length |
| Encoded byte usage | Bounded portable representation size | At or below configured limit before allocation |

## Select a replay class

Replay class is part of immutable task semantics.

| Class | Use only when | Pre-journal crash behavior |
| --- | --- | --- |
| Deterministic | Same immutable input yields the same external effect and result | Re-execution allowed |
| Idempotent | Repeating the operation is observationally equivalent to one execution | Re-execution allowed |
| Transactional | External system supplies atomic commit, idempotency key, or rollback boundary | Recovery follows that boundary |
| Unsafe | None of the above is proved | Recovery rejects without inventing an event |

Do not label an operation idempotent merely because duplicate success is rare
or usually harmless. The witness must cover every externally observable effect.

## Construct the semantic core

Begin with an exact `PlanIdentity`. Its constructor canonicalizes caller keys,
dependency pairs, effect-resource sets, and aligned costs. `ExternalKeyMap`
then exposes the bijection between those stable keys and dense internal
`TaskId` values. A checkpoint stores the exact structural identity and typed
event variants, while task-result payloads are rehydrated from the immutable
caller input only after full validation.

Run `ResumeMachine::run` with a new or resumed `ProtocolInput`. Configure every
task that can crash before journal append with an explicit `ReplayClass`.
Cancellation and resource boundaries are zero-based success counts;
cancellation has deterministic priority when both occur at the same boundary.

## Bridge exact semantics to portable codecs

An interoperability implementation must borrow semantics through
`PlanIdentity::view` or `Checkpoint::view`. `PlanIdentityView` exposes all
seven exact identity fields: schema, canonical keys, canonical dependencies,
canonical effects, aligned costs, budget, and semantic profile.
`CheckpointView` adds the canonical event-kind sequence, published prefix, and
derived next-task cursor. Both views borrow existing storage and allocate
nothing.

The reverse direction is intentionally narrower. Decode the complete plan into
`PlanIdentity::new`, then pass only the decoded event kinds and published
prefix to `Checkpoint::from_event_kinds`. The constructor derives every
redundant field and rejects a noncanonical event language. A codec cannot
inject an ordinal, dense task identifier, external task key, or resume cursor.

This function illustrates the read-only boundary with a key type that need not
implement `Copy`:

```rust
use schedlib::durable::{Checkpoint, CheckpointEventKind};

fn semantic_event_kinds<K>(checkpoint: &Checkpoint<K>) -> Vec<CheckpointEventKind>
where
    K: Clone + Ord,
{
    checkpoint.view().event_kinds().collect()
}
```

The responsibility diagram linked below remains normative: schedlib owns these
semantic projections and validation, `schedlib-interop` owns their canonical
wire representation, and `vinary-runtime` owns durable artifacts.

For each ledger row, maintenance changes must:

1. preserve the named Rocq, TLA+, SMT, and executable obligations;
2. keep the exact normative Rust property green against production code;
3. keep the mapped causal mutant dead;
4. add independent Rust example, property, malformed-input, crash, and
   small-stack evidence; and
5. replace acceptance evidence only after the new commit passes the complete
   gate.

Portable syntax is deliberately absent from schedlib. `schedlib-interop` must
define versioned, bounded canonical bytes and domain-separated digests under a
separate formal contract. Durable paths, atomic replacement, synchronization,
and retention remain `vinary-runtime` responsibilities.

The architecture decision is
[`ADR 0001`](../design/decisions/0001-durable-resume-boundary.md). The protocol
algorithm and complexity requirements are in the
[design document](../design/durable-resume-protocol.md).
