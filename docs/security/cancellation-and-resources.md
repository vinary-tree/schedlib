# Cancellation and resource containment

## Threat model

schedlib must remain predictable under malformed dependency graphs,
over-budget tasks, adversarial completion order, late cancellation, deep input,
and allocation pressure. It does not attempt to sandbox caller-provided task
code; runtime isolation of that code belongs to the adapter or host.

## Validation before effects

A cycle or individually over-budget task is rejected while the phase is still
unvalidated. No plan, worker callback, result, or commit is published. This
prevents invalid input from causing partial external effects.

Batch cost is also checked during deterministic placement:

```math
\sum_{t\in P_i}c(t)\leq B.
```

The modeled cost is a scheduling admission unit, not a memory allocator limit.
Adapters must separately bound threads, queues, memory, and runtime-specific
resources.

## Cancellation boundary

Cancellation is defined by a canonical committed-prefix length. When the
configured boundary becomes current, the machine first records a request and
then acknowledges it without publishing another task. Thus cancellation cannot
split a single commit or produce a timing-dependent prefix.

Workers already completing inside a batch may have internal results that never
become observable. An adapter must stop launching new work once cancellation is
requested, join or safely detach work according to its runtime contract, and
drop unpublished results without recursive destruction.

## Failure ordering

A worker finishing with failure or an incomplete reason does not immediately
publish that non-success if an earlier canonical task remains uncommitted.
Ordered commit publishes the first non-success in canonical order and
terminates immediately afterward. This prevents faster later failures or
incomplete results from changing the observable outcome.

## Resource limits for verification

The verification wrapper uses a 4 GiB memory ceiling, disables swap, caps CPU
and task count, gives Java a 1 GiB heap, and forces headless execution. All logs
and temporary state use persistent repository storage beneath `target/`.
Documentation rendering also forces headless Java.

## Required production hardening

- checked arithmetic for task counts, edge counts, costs, and capacities;
- preallocation only after validation of representable sizes;
- no unbounded channel or runtime queue;
- no task-count-dependent native recursion;
- cancellation checks at documented launch and commit boundaries;
- exact cleanup on success, failure, rejection, and cancellation; and
- adapter-specific tests for panic, shutdown, and abandoned workers.
