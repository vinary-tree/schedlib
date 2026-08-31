# Using the durable resume preimplementation contract

## Current status

The durable API is intentionally absent. The formal model, executable oracle,
mutation controls, and required-red Rust suite define what must exist before
production implementation is accepted. Existing serial and Rayon schedlib APIs
remain unchanged.

An application must not copy the Python oracle into production or treat its
in-memory classes as a serialization format. Portable bytes will be provided by
`schedlib-interop`; durable files remain owned by `vinary-runtime`.

## Verify the contract

Run the complete bounded gate:

```sh
./scripts/verify-durable-resume-formal.sh
```

A valid preimplementation run ends with:

```text
Validated all 35 causal required-red durable-resume properties.
unfinished-marker audit passed
Durable resume formal verification completed successfully.
```

The Cargo diagnostic inside that successful gate must say that
`schedlib::durable` is absent. The wrapper rejects every other red cause.

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
- `durable-resume-required-red.log` for the reviewed compiler boundary.

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

## Handoff to implementation

An implementation agent begins with
[`formal/durable-resume-invariants.tsv`](../../formal/durable-resume-invariants.tsv),
not with an informal API sketch. For each row it must:

1. preserve the named Rocq, TLA+, SMT, and executable obligations;
2. make the exact required-red function compile against production code;
3. keep the mapped causal mutant dead;
4. add independent Rust example, property, malformed-input, crash, and
   small-stack evidence; and
5. record commit-linked acceptance before changing the row state.

The architecture decision is
[`ADR 0001`](../design/decisions/0001-durable-resume-boundary.md). The protocol
algorithm and complexity requirements are in the
[design document](../design/durable-resume-protocol.md).
