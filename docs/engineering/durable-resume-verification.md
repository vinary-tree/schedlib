# Durable resume implementation acceptance

## Gate condition

Production durability work may begin only after the complete formal gate is
green and the required-red suite fails solely because `schedlib::durable` is
absent. A later implementation is accepted only when that same suite compiles
and every property passes without weakening the ledger, deleting a negative
control, or changing a required property name.

The current invariant state is `required-before-implementation`. No formal-only
artifact may relabel a row `accepted`; acceptance requires production evidence.

## Mandatory implementation shapes

- External keys are caller-owned generic values with exact equality and a
  canonical total order. Dense `TaskId` values are internal cache-efficient
  indices and never portable identity.
- Structural identity stores all semantic fields. A digest is supplemental.
- Journal, receipt set, validation cursor, replay cursor, and worklists are flat
  heap-resident structures.
- Journal append returns a new monotone state or mutates an exclusively owned
  buffer only after all checked limits succeed.
- Public receipt iteration is journal ordered even if exact membership uses a
  hash index.
- Recovery drains lagging receipts before executing new tasks.
- Parallel workers may compute concurrently, but journal append and logical
  publication follow canonical ordered commit.
- Every terminal result is a distinct enum variant. No `bool`, nullable error,
  or sentinel integer may conflate completion, failure, incomplete,
  cancellation, resource limitation, and rejection.

## Stack-safety refinement

No operation may map task count, graph depth, journal length, encoded byte
length, or crash count to native call-stack depth. Construction, validation,
replay, formatting, equality, ordering, hashing, cloning, serialization,
deserialization, and destruction are all in scope.

The required deep gate runs 100,000 task/event lifecycle operations on a 64 KiB
thread stack. Semantic operations must also cover at least 20,000 events where
materializing 100,000 values would obscure the operation under test. Increasing
the thread stack, adding a depth cutoff, or catching stack overflow is not a
valid repair.

## Work and heap refinement

The model exposes logical counters so asymptotic claims do not depend on timing
noise. Let $`N=n+m+j+b+1`$ combine keys, dependency edges, journal events, and
encoded bytes. The implementation must preregister constants $`a`$ and $`h`$
such that

```math
\mathrm{work}\leq aN
\qquad\text{and}\qquad
\mathrm{heap\ cells}\leq hN.
```

These are refinement bounds, not permission to hide expensive constants.
Representative wall time, allocations, peak live memory, and RSS must also be
measured. Prefix rescanning and journal cloning mutants exist specifically to
prevent accidental quadratic implementations.

## Property and crash matrix

The 35 required-red functions are normative. In addition, production testing
must cross:

- empty, singleton, deep, and wide plans;
- sparse, non-numeric, Unicode, and maximum-size external keys;
- every semantic manifest field changed independently;
- every valid success prefix and every lagging receipt prefix;
- corrupt integrity, event count, byte count, cursor, ordinal, task key,
  terminal position, and receipt membership;
- crash before append, after append, and after publication;
- every replay class, including unsafe rejection;
- cancellation/resource boundary collisions;
- success, failure, incomplete, cancellation, resource limitation, and
  completion;
- serial order and arbitrary parallel completion permutations; and
- serialization round trip, version mismatch, digest collision controls, and
  malformed bounded input in `schedlib-interop`.

## Acceptance sequence

1. Run the formal gate unchanged and preserve its evidence.
2. Confirm the required-red compile failure and record its exact diagnostic.
3. Implement the smallest complete semantic core that satisfies the public
   contract; do not add codecs to schedlib.
4. Make one required-red property green at a time while keeping all earlier
   properties green.
5. Add independent production property, mutation, crash, small-stack, and
   differential tests rather than delegating correctness to the Python oracle.
6. Implement canonical codecs in `schedlib-interop` and storage integration in
   `vinary-runtime` under their own formal and failure-injection gates.
7. Run format, all-target/all-feature debug and release tests, strict Clippy,
   Rustdoc, documentation rendering, `vinary-doc-lint`, performance/RSS gates,
   and the pgmcp bug gate.
8. Change ledger states only with commit-linked evidence for every row.

## Failure interpretation

A red required-red run is accepted only before implementation and only when the
compiler reports that `schedlib::durable` does not exist. Dependency download,
offline resolution, formatting, syntax, unrelated type, timeout, out-of-memory,
or missing-tool failures are rejected. After implementation begins, any red
property is a product defect or contract mismatch until proved otherwise.
