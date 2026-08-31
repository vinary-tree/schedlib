# ADR 0001: Separate durable scheduling semantics from portable codecs and artifacts

## Status

Accepted for implementation after the required-red contract is satisfied.

## Context

schedlib needs stable generic task keys, structural plan identity, ordered
events, checkpoint validation, crash recovery, and deterministic resume. A
single crate could also own JSON, digests, files, synchronization, and runtime
artifact policy, but that would add storage dependencies and failure modes to
the deterministic scheduling loop. Conversely, placing semantic validation in
the runtime would let different storage adapters disagree about plan identity
or safe resume.

## Decision

- schedlib owns external-key-to-dense mapping, structural plan identity, typed
  ordered events, checkpoint semantics, receipt semantics, replay classes,
  iterative resume, logical bounds, and work profiles.
- `schedlib-interop` owns canonical versioned plan/event/checkpoint encodings
  and domain-separated digests. It depends on schedlib.
- `vinary-runtime` owns durable artifact paths, atomic publication, file and
  directory synchronization, retention, and storage recovery. It may depend on
  `schedlib-interop`.
- A digest is an index and transport aid. Structural equality remains the
  semantic acceptance condition.
- The journal is authoritative and precedes logical publication. Receipts use
  the complete `(plan identity, one-based ordinal)` key.
- A pre-journal replay requires an explicit deterministic, idempotent, or
  transactional witness. Otherwise recovery fails closed.

## Consequences

The schedlib hot path stays synchronous-first, runtime neutral, stack safe, and
free of serialization or filesystem dependencies. Interop and runtime can
evolve their formats and durability mechanisms without changing core
observation semantics. The cost is an explicit conversion boundary and a need
for cross-crate refinement tests. That cost is accepted because it makes
invalid dependency direction and storage-driven semantic drift reviewable.

The decision does not authorize production implementation until the
[required-red contract](../../usage/durable-resume-contract.md) becomes green
for the intended reason and every invariant-ledger row changes state with
evidence.

## Rejected alternatives

### Put persistence in schedlib

Rejected because it couples core scheduling to codecs, hashing, and storage
failure policy and increases hot-loop dependency and allocation surfaces.

### Put checkpoint semantics in vinary-runtime

Rejected because plan identity, canonical ordering, terminal outcomes, and
replay admissibility are scheduler semantics, not storage semantics.

### Use only a digest as plan identity

Rejected because a finite digest cannot prove structural equality. Collision
resistance may support security policy but cannot replace exact semantic
comparison.

### Replay every pre-journal task

Rejected because arbitrary side effects cannot be made safe by scheduler
policy after a crash.
