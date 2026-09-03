#!/usr/bin/env python3
"""Exhaust the finite durable-identity and committed-prefix contract."""

from __future__ import annotations

import argparse
import itertools
import sys
from collections.abc import Iterable, Sequence
from dataclasses import dataclass, replace
from enum import Enum

sys.dont_write_bytecode = True


class ContractViolation(AssertionError):
    """A named executable-contract violation."""


class Outcome(str, Enum):
    SUCCESS = "success"
    FAILURE = "failure"
    INCOMPLETE = "incomplete"


class EventKind(str, Enum):
    SUCCESS = "success"
    FAILURE = "failure"
    INCOMPLETE = "incomplete"
    CANCELLED = "cancelled"
    RESOURCE_LIMITED = "resource-limited"
    COMPLETED = "completed"


class Phase(str, Enum):
    VALIDATE = "validate"
    READY = "ready"
    COMPUTED = "computed"
    JOURNALED = "journaled"
    PUBLISHING = "publishing"
    CRASHED = "crashed"
    COMPLETED = "completed"
    FAILED = "failed"
    INCOMPLETE = "incomplete"
    CANCELLED = "cancelled"
    RESOURCE_LIMITED = "resource-limited"
    REJECTED = "rejected"


class ReplayClass(str, Enum):
    DETERMINISTIC = "deterministic"
    IDEMPOTENT = "idempotent"
    TRANSACTIONAL = "transactional"
    UNSAFE = "unsafe"


PHASES = frozenset(Phase)
TERMINAL_KINDS = frozenset(
    {
        EventKind.FAILURE,
        EventKind.INCOMPLETE,
        EventKind.CANCELLED,
        EventKind.RESOURCE_LIMITED,
        EventKind.COMPLETED,
    }
)


@dataclass(frozen=True, order=True)
class PlanIdentity:
    schema: int
    keys: tuple[str, ...]
    dependencies: tuple[tuple[int, int], ...]
    effects: tuple[tuple[tuple[int, ...], tuple[int, ...]], ...]
    costs: tuple[int, ...]
    budget: int
    semantics: str


@dataclass(frozen=True, order=True)
class Event:
    plan: PlanIdentity
    ordinal: int
    task: int | None
    kind: EventKind


@dataclass(frozen=True, order=True)
class EventId:
    plan: PlanIdentity
    ordinal: int


@dataclass(frozen=True)
class Checkpoint:
    plan: PlanIdentity
    journal: tuple[Event, ...]
    receipts: tuple[EventId, ...]
    cursor: int
    integrity_valid: bool
    encoded_events: int


@dataclass(frozen=True)
class Snapshot:
    plan: PlanIdentity
    phase: Phase | str
    cursor: int
    journal: tuple[Event, ...]
    receipts: tuple[EventId, ...]
    effects: tuple[int, ...]
    staged: Event | None = None


@dataclass(frozen=True)
class RunResult:
    phase: Phase
    journal: tuple[Event, ...]
    receipts: tuple[EventId, ...]
    effects: tuple[int, ...]
    snapshots: tuple[Snapshot, ...]
    steps: int


@dataclass(frozen=True)
class CodecLimits:
    events: int
    bytes: int


@dataclass(frozen=True)
class CodecUsage:
    events: int
    bytes: int


@dataclass
class AllocationProbe:
    attempts: int = 0


def require(property_name: str, condition: bool, detail: str) -> None:
    if not condition:
        raise ContractViolation(f"PROPERTY FAILED: {property_name}: {detail}")


def canonical_plan(
    keys: Sequence[str],
    dependencies: Iterable[tuple[int, int]] = (),
    effects: Sequence[tuple[Sequence[int], Sequence[int]]] | None = None,
    costs: Sequence[int] | None = None,
    budget: int = 3,
    semantics: str = "schedlib-v1",
    schema: int = 1,
) -> PlanIdentity:
    key_tuple = tuple(keys)
    effect_source = effects or tuple(((), ()) for _ in key_tuple)
    cost_source = costs or tuple(1 for _ in key_tuple)
    return PlanIdentity(
        schema=schema,
        keys=key_tuple,
        dependencies=tuple(sorted(set(dependencies))),
        effects=tuple(
            (tuple(sorted(set(reads))), tuple(sorted(set(writes))))
            for reads, writes in effect_source
        ),
        costs=tuple(cost_source),
        budget=budget,
        semantics=semantics,
    )


def build_key_map(keys: Sequence[str]) -> tuple[dict[str, int], tuple[str, ...]]:
    if len(set(keys)) != len(keys):
        raise ValueError("external task keys must be unique")
    forward = {key: index for index, key in enumerate(keys)}
    reverse = tuple(keys)
    return forward, reverse


def state_is_typed(snapshot: Snapshot) -> bool:
    return (
        snapshot.phase in PHASES
        and 0 <= snapshot.cursor <= len(snapshot.plan.keys)
        and isinstance(snapshot.journal, tuple)
        and isinstance(snapshot.receipts, tuple)
        and isinstance(snapshot.effects, tuple)
        and (snapshot.staged is None or isinstance(snapshot.staged, Event))
    )


def event_id(event: Event) -> EventId:
    return EventId(event.plan, event.ordinal)


def task_event(plan: PlanIdentity, task: int, outcome: Outcome) -> Event:
    kind = {
        Outcome.SUCCESS: EventKind.SUCCESS,
        Outcome.FAILURE: EventKind.FAILURE,
        Outcome.INCOMPLETE: EventKind.INCOMPLETE,
    }[outcome]
    return Event(plan=plan, ordinal=task + 1, task=task, kind=kind)


def boundary_event(plan: PlanIdentity, success_count: int, kind: EventKind) -> Event:
    return Event(plan=plan, ordinal=success_count + 1, task=None, kind=kind)


def terminal_phase(kind: EventKind) -> Phase:
    return {
        EventKind.FAILURE: Phase.FAILED,
        EventKind.INCOMPLETE: Phase.INCOMPLETE,
        EventKind.CANCELLED: Phase.CANCELLED,
        EventKind.RESOURCE_LIMITED: Phase.RESOURCE_LIMITED,
        EventKind.COMPLETED: Phase.COMPLETED,
    }.get(kind, Phase.READY)


def successful_prefix_length(journal: Sequence[Event]) -> int:
    count = 0
    for event in journal:
        if event.kind is not EventKind.SUCCESS:
            break
        count += 1
    return count


def reference_journal(
    plan: PlanIdentity,
    outcomes: Sequence[Outcome],
    cancel_after: int,
    resource_after: int,
) -> tuple[Event, ...]:
    journal: list[Event] = []
    for task in range(len(plan.keys)):
        if cancel_after == task:
            journal.append(boundary_event(plan, task, EventKind.CANCELLED))
            return tuple(journal)
        if resource_after == task:
            journal.append(boundary_event(plan, task, EventKind.RESOURCE_LIMITED))
            return tuple(journal)
        event = task_event(plan, task, outcomes[task])
        journal.append(event)
        if event.kind is not EventKind.SUCCESS:
            return tuple(journal)
    journal.append(boundary_event(plan, len(plan.keys), EventKind.COMPLETED))
    return tuple(journal)


def journal_is_canonical(plan: PlanIdentity, journal: Sequence[Event]) -> bool:
    if len(journal) > len(plan.keys) + 1:
        return False
    terminal_seen = False
    success_count = 0
    for index, event in enumerate(journal):
        if event.plan != plan or event.ordinal != index + 1 or terminal_seen:
            return False
        if event.kind is EventKind.SUCCESS:
            if event.task != success_count:
                return False
            success_count += 1
            continue
        terminal_seen = True
        if index != len(journal) - 1:
            return False
        if event.kind in {EventKind.FAILURE, EventKind.INCOMPLETE}:
            if event.task != success_count or success_count >= len(plan.keys):
                return False
        elif event.kind in {EventKind.CANCELLED, EventKind.RESOURCE_LIMITED}:
            if event.task is not None or success_count >= len(plan.keys):
                return False
        elif event.kind is EventKind.COMPLETED:
            if event.task is not None or success_count != len(plan.keys):
                return False
        else:
            return False
    return True


def receipts_are_prefix(journal: Sequence[Event], receipts: Sequence[EventId]) -> bool:
    expected = tuple(event_id(event) for event in journal[: len(receipts)])
    return tuple(receipts) == expected


def checkpoint_is_valid(active: PlanIdentity, checkpoint: Checkpoint) -> bool:
    return (
        checkpoint.plan == active
        and checkpoint.integrity_valid
        and checkpoint.encoded_events == len(checkpoint.journal)
        and checkpoint.cursor == successful_prefix_length(checkpoint.journal)
        and journal_is_canonical(active, checkpoint.journal)
        and receipts_are_prefix(checkpoint.journal, checkpoint.receipts)
    )


def make_checkpoint(
    plan: PlanIdentity,
    journal: Sequence[Event],
    published_count: int | None = None,
) -> Checkpoint:
    journal_tuple = tuple(journal)
    count = len(journal_tuple) if published_count is None else published_count
    return Checkpoint(
        plan=plan,
        journal=journal_tuple,
        receipts=tuple(event_id(event) for event in journal_tuple[:count]),
        cursor=successful_prefix_length(journal_tuple),
        integrity_valid=True,
        encoded_events=len(journal_tuple),
    )


def add_receipt(receipts: Sequence[EventId], identifier: EventId) -> tuple[EventId, ...]:
    if identifier in receipts:
        return tuple(receipts)
    return tuple(receipts) + (identifier,)


def publish_event(
    journal: Sequence[Event], receipts: Sequence[EventId], event: Event
) -> tuple[EventId, ...]:
    if event not in journal:
        raise ValueError("publication requires a durable journal event")
    return add_receipt(receipts, event_id(event))


def first_unpublished(
    journal: Sequence[Event], receipts: Sequence[EventId]
) -> Event | None:
    receipt_set = frozenset(receipts)
    for event in journal:
        if event_id(event) not in receipt_set:
            return event
    return None


def recover_event(
    journal: Sequence[Event], receipts: Sequence[EventId]
) -> Event | None:
    return first_unpublished(journal, receipts)


def append_journal(journal: Sequence[Event], event: Event) -> tuple[Event, ...]:
    if journal and journal[-1].kind in TERMINAL_KINDS:
        raise ValueError("a terminal journal cannot grow")
    return tuple(journal) + (event,)


def crash_durable_state(
    journal: Sequence[Event], receipts: Sequence[EventId]
) -> tuple[tuple[Event, ...], tuple[EventId, ...]]:
    return tuple(journal), tuple(receipts)


def replay_admissible(in_flight: int | None, replay_class: ReplayClass) -> bool:
    return in_flight is None or replay_class is not ReplayClass.UNSAFE


def resume_cursor(checkpoint: Checkpoint) -> int:
    return successful_prefix_length(checkpoint.journal)


def resume_journal(prefix: Sequence[Event], complete: Sequence[Event]) -> tuple[Event, ...]:
    if tuple(complete[: len(prefix)]) != tuple(prefix):
        raise ValueError("checkpoint journal is not a committed prefix")
    return tuple(prefix) + tuple(complete[len(prefix) :])


def observed_events(journal: Sequence[Event]) -> tuple[EventId, ...]:
    return tuple(dict.fromkeys(event_id(event) for event in journal))


def normalize_completions(
    canonical: Sequence[int], physical: Sequence[int]
) -> tuple[int, ...]:
    physical_set = frozenset(physical)
    return tuple(task for task in canonical if task in physical_set)


def append_codec(
    limits: CodecLimits,
    usage: CodecUsage,
    event_bytes: int,
    probe: AllocationProbe,
) -> CodecUsage | None:
    if (
        usage.events + 1 > limits.events
        or usage.bytes + event_bytes > limits.bytes
    ):
        return None
    probe.attempts += 1
    return CodecUsage(usage.events + 1, usage.bytes + event_bytes)


def operation_accounting(input_cells: int) -> int:
    return 9 * (input_cells + 1)


def heap_accounting(input_cells: int) -> int:
    return 7 * (input_cells + 1)


def explicit_lifecycle(depth: int) -> tuple[int, int]:
    worklist = list(range(depth))
    visited = 0
    while worklist:
        worklist.pop()
        visited += 1
    return visited, 1


def replay_cursor_step(cursor: int, length: int) -> int:
    if cursor >= length:
        raise ValueError("replay cursor is already terminal")
    return cursor + 1


def recovery_transition(remaining: int) -> int:
    if remaining <= 0:
        return 0
    return remaining - 1


def finite_recovery(remaining: int) -> tuple[int, int]:
    steps = 0
    bound = remaining + 1
    while remaining > 0 and steps < bound:
        remaining = recovery_transition(remaining)
        steps += 1
    return remaining, steps


def candidate_run(
    active: PlanIdentity,
    checkpoint: Checkpoint,
    outcomes: Sequence[Outcome],
    cancel_after: int,
    resource_after: int,
    replay_classes: Sequence[ReplayClass],
    crash_points: frozenset[tuple[int, str]] = frozenset(),
) -> RunResult:
    plan = active
    journal = checkpoint.journal
    receipts = checkpoint.receipts
    effects: tuple[int, ...] = ()
    snapshots: list[Snapshot] = [
        Snapshot(plan, Phase.VALIDATE, checkpoint.cursor, journal, receipts, effects)
    ]
    steps = 0
    consumed_crashes: set[tuple[int, str]] = set()
    if not checkpoint_is_valid(active, checkpoint):
        snapshots.append(
            Snapshot(plan, Phase.REJECTED, checkpoint.cursor, journal, receipts, effects)
        )
        return RunResult(
            Phase.REJECTED, journal, receipts, effects, tuple(snapshots), steps
        )

    cursor = resume_cursor(checkpoint)
    snapshots.append(Snapshot(plan, Phase.READY, cursor, journal, receipts, effects))
    while steps < 256:
        steps += 1
        lagging = recover_event(journal, receipts)
        if lagging is not None:
            receipts = publish_event(journal, receipts, lagging)
            snapshots.append(
                Snapshot(plan, Phase.PUBLISHING, cursor, journal, receipts, effects, lagging)
            )
            cursor = successful_prefix_length(journal)
            continue

        cursor = successful_prefix_length(journal)

        if journal and journal[-1].kind in TERMINAL_KINDS:
            phase = terminal_phase(journal[-1].kind)
            snapshots.append(Snapshot(plan, phase, cursor, journal, receipts, effects))
            return RunResult(phase, journal, receipts, effects, tuple(snapshots), steps)

        if cursor == len(plan.keys):
            staged = boundary_event(plan, cursor, EventKind.COMPLETED)
        elif cancel_after == cursor:
            staged = boundary_event(plan, cursor, EventKind.CANCELLED)
        elif resource_after == cursor:
            staged = boundary_event(plan, cursor, EventKind.RESOURCE_LIMITED)
        else:
            staged = task_event(plan, cursor, outcomes[cursor])
            effects = effects + (cursor,)

        snapshots.append(
            Snapshot(plan, Phase.COMPUTED, cursor, journal, receipts, effects, staged)
        )
        crash_key = (staged.ordinal, "computed")
        if crash_key in crash_points and crash_key not in consumed_crashes:
            consumed_crashes.add(crash_key)
            if staged.task is not None and not replay_admissible(
                staged.task, replay_classes[staged.task]
            ):
                snapshots.append(
                    Snapshot(plan, Phase.REJECTED, cursor, journal, receipts, effects)
                )
                return RunResult(
                    Phase.REJECTED,
                    journal,
                    receipts,
                    effects,
                    tuple(snapshots),
                    steps,
                )
            snapshots.append(
                Snapshot(plan, Phase.CRASHED, cursor, journal, receipts, effects)
            )
            continue

        journal = append_journal(journal, staged)
        snapshots.append(
            Snapshot(plan, Phase.JOURNALED, cursor, journal, receipts, effects, staged)
        )
        crash_key = (staged.ordinal, "journaled")
        if crash_key in crash_points and crash_key not in consumed_crashes:
            consumed_crashes.add(crash_key)
            journal, receipts = crash_durable_state(journal, receipts)
            snapshots.append(
                Snapshot(plan, Phase.CRASHED, cursor, journal, receipts, effects)
            )
            continue

        receipts = publish_event(journal, receipts, staged)
        snapshots.append(
            Snapshot(plan, Phase.PUBLISHING, cursor, journal, receipts, effects, staged)
        )
        crash_key = (staged.ordinal, "publishing")
        if crash_key in crash_points and crash_key not in consumed_crashes:
            consumed_crashes.add(crash_key)
            journal, receipts = crash_durable_state(journal, receipts)
            snapshots.append(
                Snapshot(plan, Phase.CRASHED, cursor, journal, receipts, effects)
            )
            continue

        cursor = successful_prefix_length(journal)

    return RunResult(
        Phase.REJECTED, journal, receipts, effects, tuple(snapshots), steps
    )


def sample_plan(task_count: int = 3) -> PlanIdentity:
    keys = tuple(f"task-{index}" for index in range(task_count))
    dependencies = tuple((index, index + 1) for index in range(task_count - 1))
    effects = tuple(((index,), (index + 1,)) for index in range(task_count))
    costs = tuple(index + 1 for index in range(task_count))
    return canonical_plan(keys, dependencies, effects, costs, max(1, sum(costs)))


def run_for(
    plan: PlanIdentity,
    outcomes: Sequence[Outcome],
    prefix: int = 0,
    published: int | None = None,
    cancel_after: int | None = None,
    resource_after: int | None = None,
    crash_points: frozenset[tuple[int, str]] = frozenset(),
    replay_classes: Sequence[ReplayClass] | None = None,
) -> tuple[tuple[Event, ...], RunResult]:
    complete = reference_journal(
        plan,
        outcomes,
        len(plan.keys) + 1 if cancel_after is None else cancel_after,
        len(plan.keys) + 1 if resource_after is None else resource_after,
    )
    initial = complete[:prefix]
    checkpoint = make_checkpoint(plan, initial, published)
    classes = replay_classes or tuple(
        ReplayClass.DETERMINISTIC for _ in plan.keys
    )
    result = candidate_run(
        plan,
        checkpoint,
        outcomes,
        len(plan.keys) + 1 if cancel_after is None else cancel_after,
        len(plan.keys) + 1 if resource_after is None else resource_after,
        classes,
        crash_points,
    )
    return complete, result


def oracle_protocol_state_is_typed() -> int:
    plan = sample_plan(2)
    event = task_event(plan, 0, Outcome.SUCCESS)
    valid = Snapshot(plan, Phase.READY, 0, (), (), (), event)
    require("oracle_protocol_state_is_typed", state_is_typed(valid), "valid state rejected")
    invalid = replace(valid, phase="invented")
    require(
        "oracle_protocol_state_is_typed",
        not state_is_typed(invalid),
        "unknown phase accepted",
    )
    return len(PHASES) + 1


def oracle_key_map_injective() -> int:
    checked = 0
    for length in range(4):
        for keys in itertools.permutations(("a", "b", "c"), length):
            forward, _ = build_key_map(keys)
            require(
                "oracle_key_map_injective",
                len(set(forward.values())) == len(keys),
                f"dense alias for {keys}",
            )
            checked += 1
    return checked


def oracle_key_map_bijection() -> int:
    checked = 0
    for length in range(4):
        for keys in itertools.permutations(("a", "b", "c"), length):
            forward, reverse = build_key_map(keys)
            require(
                "oracle_key_map_bijection",
                len(reverse) == len(keys)
                and all(reverse[dense] == key for key, dense in forward.items()),
                f"key round trip failed for {keys}",
            )
            checked += 1
    return checked


def oracle_structural_plan_identity() -> int:
    base = sample_plan(2)
    variants = (
        replace(base, schema=2),
        replace(base, keys=("task-0", "other")),
        canonical_plan(base.keys, (), base.effects, base.costs, base.budget, base.semantics),
        replace(base, effects=(((9,), ()), base.effects[1])),
        replace(base, costs=(2, 2)),
        replace(base, budget=base.budget + 1),
        replace(base, semantics="schedlib-v2"),
    )
    require(
        "oracle_structural_plan_identity",
        all(base != variant for variant in variants),
        "a semantic manifest field was omitted from identity",
    )
    return len(variants)


def oracle_input_snapshot_immutability() -> int:
    plan = sample_plan(2)
    complete, result = run_for(plan, (Outcome.SUCCESS, Outcome.SUCCESS))
    require(
        "oracle_input_snapshot_immutability",
        result.journal == complete and all(snapshot.plan == plan for snapshot in result.snapshots),
        "validated input changed during execution",
    )
    return len(result.snapshots)


def oracle_canonical_journal_ordinals() -> int:
    checked = 0
    for task_count in range(4):
        plan = sample_plan(task_count)
        for outcomes in itertools.product(tuple(Outcome), repeat=task_count):
            journal = reference_journal(plan, outcomes, task_count + 1, task_count + 1)
            require(
                "oracle_canonical_journal_ordinals",
                [event.ordinal for event in journal] == list(range(1, len(journal) + 1)),
                f"noncanonical ordinals for {outcomes}",
            )
            checked += 1
    return checked


def oracle_unique_event_ids() -> int:
    plan = sample_plan(3)
    journal = tuple(task_event(plan, task, Outcome.SUCCESS) for task in range(3))
    identifiers = tuple(event_id(event) for event in journal)
    require(
        "oracle_unique_event_ids",
        len(set(identifiers)) == len(identifiers),
        "event identifiers collided",
    )
    other = replace(plan, semantics="other")
    require(
        "oracle_unique_event_ids",
        event_id(journal[0]) != event_id(task_event(other, 0, Outcome.SUCCESS)),
        "plan identity was absent from event identity",
    )
    return 4


def oracle_journal_length_bound() -> int:
    checked = 0
    for task_count in range(4):
        plan = sample_plan(task_count)
        for outcomes in itertools.product(tuple(Outcome), repeat=task_count):
            journal = reference_journal(plan, outcomes, task_count + 1, task_count + 1)
            require(
                "oracle_journal_length_bound",
                len(journal) <= task_count + 1 and journal_is_canonical(plan, journal),
                "journal exceeded its plan bound",
            )
            try:
                append_journal(journal, boundary_event(plan, len(journal), EventKind.COMPLETED))
            except ValueError:
                pass
            else:
                require(
                    "oracle_journal_length_bound", False, "terminal journal accepted an append"
                )
            checked += 1
    return checked


def oracle_terminal_event_exactness() -> int:
    plan = sample_plan(2)
    exact_phases = {
        EventKind.FAILURE: Phase.FAILED,
        EventKind.INCOMPLETE: Phase.INCOMPLETE,
        EventKind.CANCELLED: Phase.CANCELLED,
        EventKind.RESOURCE_LIMITED: Phase.RESOURCE_LIMITED,
        EventKind.COMPLETED: Phase.COMPLETED,
    }
    checked = 0
    for outcomes in itertools.product(tuple(Outcome), repeat=2):
        for cancel_after in range(4):
            for resource_after in range(4):
                journal = reference_journal(plan, outcomes, cancel_after, resource_after)
                require(
                    "oracle_terminal_event_exactness",
                    journal[-1].kind in TERMINAL_KINDS
                    and terminal_phase(journal[-1].kind)
                    is exact_phases[journal[-1].kind]
                    and journal_is_canonical(plan, journal),
                    "terminal event or phase is inexact",
                )
                checked += 1
    return checked


def oracle_journal_prefix_monotonicity() -> int:
    plan = sample_plan(3)
    _, result = run_for(
        plan,
        (Outcome.SUCCESS,) * 3,
        crash_points=frozenset({(1, "journaled"), (2, "publishing")}),
    )
    journals = [snapshot.journal for snapshot in result.snapshots]
    require(
        "oracle_journal_prefix_monotonicity",
        all(right[: len(left)] == left for left, right in zip(journals, journals[1:])),
        "durable journal was rewritten",
    )
    return len(journals)


def oracle_checkpoint_plan_binding() -> int:
    plan = sample_plan(2)
    other = replace(plan, semantics="other")
    checkpoint = make_checkpoint(other, ())
    require(
        "oracle_checkpoint_plan_binding",
        not checkpoint_is_valid(plan, checkpoint),
        "foreign plan checkpoint was accepted",
    )
    return 1


def oracle_stale_checkpoint_rejection() -> int:
    plan = sample_plan(2)
    other = replace(plan, budget=plan.budget + 1)
    result = candidate_run(
        plan,
        make_checkpoint(other, ()),
        (Outcome.SUCCESS,) * 2,
        3,
        3,
        (ReplayClass.DETERMINISTIC,) * 2,
    )
    require(
        "oracle_stale_checkpoint_rejection",
        result.phase is Phase.REJECTED and not result.effects,
        "stale checkpoint started task effects",
    )
    return 1


def oracle_malformed_checkpoint_matrix() -> int:
    plan = sample_plan(2)
    event = task_event(plan, 0, Outcome.SUCCESS)
    valid = make_checkpoint(plan, (event,))
    malformed = (
        replace(valid, integrity_valid=False),
        replace(valid, encoded_events=2),
        replace(valid, cursor=0),
        replace(valid, journal=(replace(event, ordinal=2),)),
        replace(valid, journal=(event, event), encoded_events=2, cursor=2),
        replace(
            valid,
            journal=(boundary_event(plan, 0, EventKind.CANCELLED), event),
            encoded_events=2,
            cursor=0,
            receipts=(),
        ),
    )
    require(
        "oracle_malformed_checkpoint_matrix",
        all(not checkpoint_is_valid(plan, candidate) for candidate in malformed),
        "malformed checkpoint was accepted",
    )
    return len(malformed)


def oracle_checkpoint_cursor_prefix() -> int:
    plan = sample_plan(3)
    for count in range(4):
        journal = tuple(task_event(plan, task, Outcome.SUCCESS) for task in range(count))
        checkpoint = make_checkpoint(plan, journal)
        require(
            "oracle_checkpoint_cursor_prefix",
            checkpoint.cursor == count and resume_cursor(checkpoint) == count,
            "resume skipped or repeated a committed task",
        )
    terminal = make_checkpoint(
        plan,
        (
            task_event(plan, 0, Outcome.SUCCESS),
            task_event(plan, 1, Outcome.FAILURE),
        ),
    )
    require(
        "oracle_checkpoint_cursor_prefix",
        terminal.cursor == 1,
        "terminal event was counted as a successful task",
    )
    return 5


def oracle_rejection_is_effect_free() -> int:
    plan = sample_plan(2)
    malformed = replace(make_checkpoint(plan, ()), integrity_valid=False)
    result = candidate_run(
        plan,
        malformed,
        (Outcome.SUCCESS,) * 2,
        3,
        3,
        (ReplayClass.DETERMINISTIC,) * 2,
    )
    require(
        "oracle_rejection_is_effect_free",
        result.phase is Phase.REJECTED and not result.effects and not result.journal,
        "invalid input produced observable work",
    )
    return 1


def oracle_journal_before_publish() -> int:
    plan = sample_plan(1)
    event = task_event(plan, 0, Outcome.SUCCESS)
    try:
        publish_event((), (), event)
    except ValueError:
        pass
    else:
        require(
            "oracle_journal_before_publish", False, "event published before durable append"
        )
    receipts = publish_event((event,), (), event)
    require(
        "oracle_journal_before_publish",
        receipts == (event_id(event),),
        "durable event did not publish",
    )
    return 2


def oracle_idempotent_receipt_set() -> int:
    plan = sample_plan(1)
    identifier = event_id(task_event(plan, 0, Outcome.SUCCESS))
    once = add_receipt((), identifier)
    twice = add_receipt(once, identifier)
    require(
        "oracle_idempotent_receipt_set",
        once == twice and len(twice) == 1,
        "duplicate publication was observable",
    )
    return 2


def oracle_receipt_prefix() -> int:
    plan = sample_plan(3)
    journal = tuple(task_event(plan, task, Outcome.SUCCESS) for task in range(3))
    receipts: tuple[EventId, ...] = ()
    for expected_count in range(1, 4):
        event = first_unpublished(journal, receipts)
        require("oracle_receipt_prefix", event is not None, "missing publication cursor")
        receipts = publish_event(journal, receipts, event)
        require(
            "oracle_receipt_prefix",
            receipts_are_prefix(journal, receipts) and len(receipts) == expected_count,
            "receipts are not a canonical prefix",
        )
    return 3


def oracle_receipt_membership() -> int:
    plan = sample_plan(1)
    durable = task_event(plan, 0, Outcome.SUCCESS)
    fabricated = boundary_event(plan, 1, EventKind.COMPLETED)
    try:
        publish_event((durable,), (), fabricated)
    except ValueError:
        pass
    else:
        require(
            "oracle_receipt_membership", False, "recovery fabricated a receipt"
        )
    recovered = recover_event((durable,), ())
    require(
        "oracle_receipt_membership",
        recovered == durable and recovered in (durable,),
        "recovery selected an event absent from the journal",
    )
    return 2


def oracle_lagging_publication_recovery() -> int:
    plan = sample_plan(2)
    journal = tuple(task_event(plan, task, Outcome.SUCCESS) for task in range(2))
    checkpoint = make_checkpoint(plan, journal, published_count=0)
    result = candidate_run(
        plan,
        checkpoint,
        (Outcome.SUCCESS,) * 2,
        3,
        3,
        (ReplayClass.DETERMINISTIC,) * 2,
    )
    first_effect_snapshot = next(
        (index for index, snapshot in enumerate(result.snapshots) if snapshot.effects),
        len(result.snapshots),
    )
    publication_prefix = result.snapshots[:first_effect_snapshot]
    require(
        "oracle_lagging_publication_recovery",
        result.receipts == tuple(event_id(event) for event in result.journal)
        and any(len(snapshot.receipts) == 2 for snapshot in publication_prefix),
        "lagging receipts were not drained before new work",
    )
    return len(result.snapshots)


def oracle_receipts_monotone() -> int:
    plan = sample_plan(2)
    _, result = run_for(
        plan,
        (Outcome.SUCCESS,) * 2,
        crash_points=frozenset({(1, "publishing"), (2, "journaled")}),
    )
    receipts = [snapshot.receipts for snapshot in result.snapshots]
    require(
        "oracle_receipts_monotone",
        all(set(left).issubset(right) for left, right in zip(receipts, receipts[1:])),
        "crash deleted a durable receipt",
    )
    return len(receipts)


def oracle_crash_persistence() -> int:
    plan = sample_plan(2)
    journal = (task_event(plan, 0, Outcome.SUCCESS),)
    receipts = (event_id(journal[0]),)
    crashed = crash_durable_state(journal, receipts)
    require(
        "oracle_crash_persistence",
        crashed == (journal, receipts),
        "crash changed durable state",
    )
    return 1


def oracle_unsafe_replay_matrix() -> int:
    checked = 0
    for replay_class in ReplayClass:
        accepted = replay_admissible(0, replay_class)
        require(
            "oracle_unsafe_replay_matrix",
            accepted == (replay_class is not ReplayClass.UNSAFE),
            f"incorrect replay decision for {replay_class.value}",
        )
        checked += 1
    require(
        "oracle_unsafe_replay_matrix",
        replay_admissible(None, ReplayClass.UNSAFE),
        "no-in-flight checkpoint was rejected",
    )
    return checked + 1


def oracle_safe_replay_admission() -> int:
    safe = (
        ReplayClass.DETERMINISTIC,
        ReplayClass.IDEMPOTENT,
        ReplayClass.TRANSACTIONAL,
    )
    require(
        "oracle_safe_replay_admission",
        all(replay_admissible(0, replay_class) for replay_class in safe),
        "a witnessed replay class was rejected",
    )
    return len(safe)


def oracle_all_journal_prefixes() -> int:
    plan = sample_plan(3)
    checked = 0
    for outcomes in itertools.product(tuple(Outcome), repeat=3):
        complete = reference_journal(plan, outcomes, 4, 4)
        for prefix_count in range(len(complete) + 1):
            prefix = complete[:prefix_count]
            require(
                "oracle_all_journal_prefixes",
                resume_journal(prefix, complete) == complete,
                "resume did not reconstruct the serial journal",
            )
            checked += 1
    return checked


def oracle_serial_resume_observation() -> int:
    plan = sample_plan(3)
    complete = reference_journal(plan, (Outcome.SUCCESS,) * 3, 4, 4)
    for prefix_count in range(len(complete) + 1):
        resumed = resume_journal(complete[:prefix_count], complete)
        physical_replay = complete[:prefix_count] + resumed
        require(
            "oracle_serial_resume_observation",
            observed_events(physical_replay) == observed_events(complete),
            "resumed observation duplicated or omitted an event",
        )
    return len(complete) + 1


def oracle_completion_permutations() -> int:
    canonical = (0, 1, 2, 3)
    checked = 0
    for physical in itertools.permutations(canonical):
        require(
            "oracle_completion_permutations",
            normalize_completions(canonical, physical) == canonical,
            f"physical order leaked for {physical}",
        )
        checked += 1
    return checked


def oracle_terminal_outcome_partition() -> int:
    expected = {
        EventKind.FAILURE: Phase.FAILED,
        EventKind.INCOMPLETE: Phase.INCOMPLETE,
        EventKind.CANCELLED: Phase.CANCELLED,
        EventKind.RESOURCE_LIMITED: Phase.RESOURCE_LIMITED,
        EventKind.COMPLETED: Phase.COMPLETED,
    }
    require(
        "oracle_terminal_outcome_partition",
        len(set(expected.values())) == len(expected)
        and all(terminal_phase(kind) is phase for kind, phase in expected.items()),
        "terminal outcomes were promoted or conflated",
    )
    plan = sample_plan(1)
    collision = reference_journal(plan, (Outcome.SUCCESS,), 0, 0)
    require(
        "oracle_terminal_outcome_partition",
        collision[-1].kind is EventKind.CANCELLED,
        "cancellation-before-resource priority is not deterministic",
    )
    require(
        "oracle_terminal_outcome_partition",
        task_event(plan, 0, Outcome.INCOMPLETE).kind is EventKind.INCOMPLETE,
        "incomplete task outcome was promoted to success",
    )
    return len(expected) + 2


def oracle_completion_totality() -> int:
    plan = sample_plan(3)
    for outcomes in itertools.product(tuple(Outcome), repeat=3):
        journal = reference_journal(plan, outcomes, 4, 4)
        _, result = run_for(plan, outcomes)
        completed = result.phase is Phase.COMPLETED
        all_success = all(outcome is Outcome.SUCCESS for outcome in outcomes)
        require(
            "oracle_completion_totality",
            completed == all_success
            and result.journal == journal
            and (not completed or len(result.journal) == len(plan.keys) + 1),
            "completion did not mean all tasks succeeded",
        )
    return 27


def oracle_codec_boundary_sweep() -> int:
    checked = 0
    for event_limit in range(4):
        for byte_limit in range(6):
            for used_events in range(event_limit + 1):
                for used_bytes in range(byte_limit + 1):
                    for event_bytes in range(4):
                        probe = AllocationProbe()
                        result = append_codec(
                            CodecLimits(event_limit, byte_limit),
                            CodecUsage(used_events, used_bytes),
                            event_bytes,
                            probe,
                        )
                        admitted = (
                            used_events + 1 <= event_limit
                            and used_bytes + event_bytes <= byte_limit
                        )
                        require(
                            "oracle_codec_boundary_sweep",
                            (result is not None) == admitted
                            and (admitted or probe.attempts == 0)
                            and (
                                result is None
                                or result.events <= event_limit
                                and result.bytes <= byte_limit
                            ),
                            "codec limit or preallocation ordering failed",
                        )
                        checked += 1
    return checked


def oracle_operation_accounting() -> int:
    for input_cells in range(257):
        require(
            "oracle_operation_accounting",
            operation_accounting(input_cells) <= 9 * (input_cells + 1),
            "work account is superlinear",
        )
    return 257


def oracle_heap_accounting() -> int:
    for input_cells in range(257):
        require(
            "oracle_heap_accounting",
            heap_accounting(input_cells) <= 7 * (input_cells + 1),
            "heap account is superlinear",
        )
    return 257


def oracle_explicit_pda_lifecycle() -> int:
    for depth in (0, 1, 2, 31, 1024, 100_000):
        visited, frames = explicit_lifecycle(depth)
        require(
            "oracle_explicit_pda_lifecycle",
            visited == depth and frames == 1,
            "input depth reached the native stack",
        )
    return 6


def oracle_replay_cursor_measure() -> int:
    checked = 0
    for length in range(1, 65):
        for cursor in range(length):
            next_cursor = replay_cursor_step(cursor, length)
            require(
                "oracle_replay_cursor_measure",
                length - next_cursor < length - cursor,
                "replay cursor did not strictly decrease remaining work",
            )
            checked += 1
    return checked


def protocol_matrix() -> int:
    checked = 0
    for task_count in range(3):
        plan = sample_plan(task_count)
        boundaries = range(task_count + 2)
        for outcomes in itertools.product(tuple(Outcome), repeat=task_count):
            for cancel_after in boundaries:
                for resource_after in boundaries:
                    complete = reference_journal(
                        plan, outcomes, cancel_after, resource_after
                    )
                    success_prefix = successful_prefix_length(complete)
                    for prefix_count in range(success_prefix + 1):
                        for published_count in range(prefix_count + 1):
                            _, result = run_for(
                                plan,
                                outcomes,
                                prefix_count,
                                published_count,
                                cancel_after,
                                resource_after,
                            )
                            require(
                                "oracle_finite_protocol_traces",
                                result.phase is terminal_phase(complete[-1].kind)
                                and result.journal == complete
                                and result.receipts
                                == tuple(event_id(event) for event in complete)
                                and result.steps < 256,
                                "finite run disagreed with the independent serial denotation",
                            )
                            checked += 1
    plan = sample_plan(2)
    complete = reference_journal(plan, (Outcome.SUCCESS,) * 2, 3, 3)
    sites = tuple(
        (event.ordinal, point)
        for event in complete
        for point in ("computed", "journaled", "publishing")
    )
    for mask in range(1 << len(sites)):
        selected = frozenset(
            site for index, site in enumerate(sites) if mask & (1 << index)
        )
        _, result = run_for(
            plan,
            (Outcome.SUCCESS,) * 2,
            crash_points=selected,
        )
        require(
            "oracle_finite_protocol_traces",
            result.phase is Phase.COMPLETED
            and result.journal == complete
            and result.receipts == tuple(event_id(event) for event in complete)
            and result.steps < 256,
            "crash interleaving changed the durable observation: "
            f"selected={sorted(selected)!r}, phase={result.phase.value}, "
            f"journal={[event.kind.value for event in result.journal]!r}, "
            f"receipts={[identifier.ordinal for identifier in result.receipts]!r}, "
            f"steps={result.steps}",
        )
        checked += 1
    return checked


def oracle_finite_protocol_traces() -> int:
    remaining, steps = finite_recovery(128)
    require(
        "oracle_finite_protocol_traces",
        remaining == 0 and steps == 128,
        "recovery machine failed to terminate",
    )
    return protocol_matrix() + 1


ORACLES = {
    function.__name__: function
    for function in (
        oracle_protocol_state_is_typed,
        oracle_key_map_injective,
        oracle_key_map_bijection,
        oracle_structural_plan_identity,
        oracle_input_snapshot_immutability,
        oracle_canonical_journal_ordinals,
        oracle_unique_event_ids,
        oracle_journal_length_bound,
        oracle_terminal_event_exactness,
        oracle_journal_prefix_monotonicity,
        oracle_checkpoint_plan_binding,
        oracle_stale_checkpoint_rejection,
        oracle_malformed_checkpoint_matrix,
        oracle_checkpoint_cursor_prefix,
        oracle_rejection_is_effect_free,
        oracle_journal_before_publish,
        oracle_idempotent_receipt_set,
        oracle_receipt_prefix,
        oracle_receipt_membership,
        oracle_lagging_publication_recovery,
        oracle_receipts_monotone,
        oracle_crash_persistence,
        oracle_unsafe_replay_matrix,
        oracle_safe_replay_admission,
        oracle_all_journal_prefixes,
        oracle_serial_resume_observation,
        oracle_completion_permutations,
        oracle_terminal_outcome_partition,
        oracle_completion_totality,
        oracle_codec_boundary_sweep,
        oracle_operation_accounting,
        oracle_heap_accounting,
        oracle_explicit_pda_lifecycle,
        oracle_replay_cursor_measure,
        oracle_finite_protocol_traces,
    )
}


def source_audit() -> None:
    source = __file__
    text = open(source, encoding="utf-8").read()
    forbidden = ("TO" + "DO", "FIX" + "ME", "HA" + "CK", "pass" + "  #")
    for token in forbidden:
        if token in text:
            raise ContractViolation(f"executable oracle contains forbidden token {token!r}")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--property", choices=tuple(ORACLES))
    arguments = parser.parse_args()
    source_audit()
    selected = (
        ((arguments.property, ORACLES[arguments.property]),)
        if arguments.property
        else tuple(ORACLES.items())
    )
    total = 0
    try:
        for name, oracle in selected:
            count = oracle()
            total += count
            print(f"passed: {name}: {count} cases")
    except ContractViolation as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1) from error
    print(
        f"Durable-resume executable oracle passed {len(selected)} properties "
        f"across {total} finite cases."
    )


if __name__ == "__main__":
    main()
