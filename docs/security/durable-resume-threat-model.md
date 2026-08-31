# Durable resume threat model

## Protected properties

The protocol protects exact plan binding, canonical event order, monotone
durable state, non-invented publication, fail-closed replay, bounded resource
use, and typed terminal outcomes. It assumes the runtime supplies durable bytes
faithfully or reports corruption; it does not assume storage is always
available.

## Trust boundaries

| Boundary | Untrusted or fallible input | Required response |
| --- | --- | --- |
| Caller to schedlib | External keys, semantic profile, dependency/effect/cost metadata | Canonicalize and validate before identity publication |
| Runtime to checkpoint validator | Bytes, integrity result, counts, cursor, receipts | Reject stale, malformed, corrupt, oversized, or foreign state before new task effects |
| Task to recovery policy | Replay-class claim | Require an explicit immutable witness; never infer arbitrary safety |
| Journal to publication sink | Event identifier and payload | Publish only durable canonical events and deduplicate by structural plan plus ordinal |
| Parallel executor to ordered commit | Arbitrary physical completion order | Validate exact batch cover and normalize before durable observation |

## Adversary cases

### Identity substitution

An attacker or stale process presents a checkpoint from a plan differing only
in dependency, effect, cost, budget, semantic profile, key, or schema. Exact
structural comparison rejects it. A digest match is insufficient without
structural confirmation.

### Hash collision

Two structural plans produce the same finite digest. The collision may cause an
extra exact comparison but cannot merge plans, accept a checkpoint, or suppress
a task. Domain separation prevents plan, event, and checkpoint bytes from
sharing an undifferentiated digest namespace.

### Journal surgery

An input removes, reorders, duplicates, renumbers, edits, or appends after a
terminal event. Canonical ordinal, event count, task position, terminal shape,
integrity, and receipt-prefix validation reject it before execution.

### Receipt fabrication

A receipt names an event absent from the journal or skips an earlier event.
Membership and canonical-prefix checks reject it. Recovery constructs staging
only from an existing durable journal event.

### Replay of unsafe effects

A crash occurs after an external effect but before journal append. Without a
deterministic, idempotent, or transactional witness, recovery returns a typed
unsafe-replay rejection. The scheduler does not attempt compensating actions
whose semantics it cannot know.

### Resource exhaustion

Encoded event count, byte count, work, heap, and native-stack bounds are checked
before allocation or iteration. Limit exhaustion is a typed limited or rejected
state, never a completed result or an empty checkpoint.

### Crash storm

Repeated crashes preserve journal and receipt monotonicity. Every replay cursor
step decreases a finite measure, but liveness assumes a fair interval in which
an enabled transition eventually executes. An environment that crashes before
every durable action can prevent completion; it cannot cause false completion
or fabricated publication.

## Availability and confidentiality

schedlib's contract is about integrity and deterministic observation. Encryption
at rest, key management, file permissions, multi-process leases, anti-rollback
storage, and durable media synchronization belong to the runtime threat model.
The interop format must avoid embedding secrets in diagnostic text and must
bound all decoded lengths before allocation.

## Security acceptance

Security review must retain the stale-plan, malformed-checkpoint,
publish-before-append, fabricated-receipt, unsafe-replay, early-completion,
preallocation, superlinear-work, superlinear-heap, and recursive-replay mutants.
Each must continue to fail for its mapped property rather than for an unrelated
reason.
