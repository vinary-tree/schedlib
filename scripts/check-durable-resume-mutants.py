#!/usr/bin/env python3
"""Kill one causal durable-resume fault for every invariant-ledger row."""

from __future__ import annotations

import csv
import shutil
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

sys.dont_write_bytecode = True

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "scripts/check-durable-resume-exhaustive.py"
LEDGER = ROOT / "formal/durable-resume-invariants.tsv"
MUTANT_ROOT = ROOT / "target/durable-resume/mutants"


@dataclass(frozen=True)
class Mutation:
    needle: str
    replacement: str
    oracle: str
    occurrences: int = 1


MUTATIONS = {
    "mutant_unchecked_state_discriminant": Mutation(
        "        snapshot.phase in PHASES\n",
        "        True\n",
        "oracle_protocol_state_is_typed",
    ),
    "mutant_dense_alias": Mutation(
        "    forward = {key: index for index, key in enumerate(keys)}\n",
        "    forward = {key: index // 2 for index, key in enumerate(keys)}\n",
        "oracle_key_map_injective",
    ),
    "mutant_dense_orphan": Mutation(
        "    reverse = tuple(keys)\n",
        "    reverse = tuple(keys[:-1])\n",
        "oracle_key_map_bijection",
    ),
    "mutant_omit_dependency_from_plan_identity": Mutation(
        "        dependencies=tuple(sorted(set(dependencies))),\n",
        "        dependencies=(),\n",
        "oracle_structural_plan_identity",
    ),
    "mutant_mutable_plan_after_validation": Mutation(
        "    plan = active\n",
        "    plan = replace(active, semantics=active.semantics + '-mutated')\n",
        "oracle_input_snapshot_immutability",
    ),
    "mutant_shift_event_ordinal": Mutation(
        "    return Event(plan=plan, ordinal=task + 1, task=task, kind=kind)\n",
        "    return Event(plan=plan, ordinal=task + 2, task=task, kind=kind)\n",
        "oracle_canonical_journal_ordinals",
    ),
    "mutant_duplicate_event_id": Mutation(
        "    return EventId(event.plan, event.ordinal)\n",
        "    return EventId(event.plan, 1)\n",
        "oracle_unique_event_ids",
    ),
    "mutant_append_after_terminal": Mutation(
        "    if journal and journal[-1].kind in TERMINAL_KINDS:\n"
        "        raise ValueError(\"a terminal journal cannot grow\")\n",
        "    if False and journal and journal[-1].kind in TERMINAL_KINDS:\n"
        "        raise ValueError(\"a terminal journal cannot grow\")\n",
        "oracle_journal_length_bound",
    ),
    "mutant_terminal_reason_promotion": Mutation(
        "def terminal_phase(kind: EventKind) -> Phase:\n"
        "    return {\n"
        "        EventKind.FAILURE: Phase.FAILED,\n",
        "def terminal_phase(kind: EventKind) -> Phase:\n"
        "    return {\n"
        "        EventKind.FAILURE: Phase.COMPLETED,\n",
        "oracle_terminal_event_exactness",
    ),
    "mutant_rewrite_committed_event": Mutation(
        "    return tuple(journal) + (event,)\n",
        "    return tuple(journal[:-1]) + (event,)\n",
        "oracle_journal_prefix_monotonicity",
    ),
    "mutant_accept_foreign_plan": Mutation(
        "        checkpoint.plan == active\n",
        "        checkpoint.plan.schema == active.schema\n",
        "oracle_checkpoint_plan_binding",
    ),
    "mutant_ignore_plan_mismatch": Mutation(
        "    if not checkpoint_is_valid(active, checkpoint):\n",
        "    if not checkpoint_is_valid(checkpoint.plan, checkpoint):\n",
        "oracle_stale_checkpoint_rejection",
    ),
    "mutant_trust_checkpoint_cursor": Mutation(
        "        and checkpoint.cursor == successful_prefix_length(checkpoint.journal)\n",
        "        and checkpoint.cursor >= 0\n",
        "oracle_malformed_checkpoint_matrix",
    ),
    "mutant_skip_resume_task": Mutation(
        "def resume_cursor(checkpoint: Checkpoint) -> int:\n"
        "    return successful_prefix_length(checkpoint.journal)\n",
        "def resume_cursor(checkpoint: Checkpoint) -> int:\n"
        "    return successful_prefix_length(checkpoint.journal) + 1\n",
        "oracle_checkpoint_cursor_prefix",
    ),
    "mutant_execute_before_validation": Mutation(
        "    effects: tuple[int, ...] = ()\n",
        "    effects: tuple[int, ...] = (0,)\n",
        "oracle_rejection_is_effect_free",
    ),
    "mutant_publish_before_append": Mutation(
        "    if event not in journal:\n"
        "        raise ValueError(\"publication requires a durable journal event\")\n",
        "    if False and event not in journal:\n"
        "        raise ValueError(\"publication requires a durable journal event\")\n",
        "oracle_journal_before_publish",
    ),
    "mutant_multiset_receipts": Mutation(
        "    if identifier in receipts:\n"
        "        return tuple(receipts)\n",
        "    if False and identifier in receipts:\n"
        "        return tuple(receipts)\n",
        "oracle_idempotent_receipt_set",
    ),
    "mutant_publish_out_of_order": Mutation(
        "def first_unpublished(\n"
        "    journal: Sequence[Event], receipts: Sequence[EventId]\n"
        ") -> Event | None:\n"
        "    receipt_set = frozenset(receipts)\n"
        "    for event in journal:\n",
        "def first_unpublished(\n"
        "    journal: Sequence[Event], receipts: Sequence[EventId]\n"
        ") -> Event | None:\n"
        "    receipt_set = frozenset(receipts)\n"
        "    for event in reversed(journal):\n",
        "oracle_receipt_prefix",
    ),
    "mutant_fabricate_recovery_event": Mutation(
        "    return first_unpublished(journal, receipts)\n",
        "    event = first_unpublished(journal, receipts)\n"
        "    return None if event is None else replace(event, ordinal=event.ordinal + 1)\n",
        "oracle_receipt_membership",
    ),
    "mutant_skip_lagging_receipt": Mutation(
        "        if lagging is not None:\n",
        "        if False and lagging is not None:\n",
        "oracle_lagging_publication_recovery",
    ),
    "mutant_delete_receipt_on_crash": Mutation(
        "    return tuple(journal), tuple(receipts)\n",
        "    return tuple(journal), tuple(receipts[:-1])\n",
        "oracle_receipts_monotone",
    ),
    "mutant_erase_journal_on_crash": Mutation(
        "    return tuple(journal), tuple(receipts)\n",
        "    return (), tuple(receipts)\n",
        "oracle_crash_persistence",
    ),
    "mutant_replay_arbitrary_effect": Mutation(
        "    return in_flight is None or replay_class is not ReplayClass.UNSAFE\n",
        "    return True\n",
        "oracle_unsafe_replay_matrix",
    ),
    "mutant_reject_all_replay": Mutation(
        "    return in_flight is None or replay_class is not ReplayClass.UNSAFE\n",
        "    return in_flight is None\n",
        "oracle_safe_replay_admission",
    ),
    "mutant_resume_from_zero": Mutation(
        "    return tuple(prefix) + tuple(complete[len(prefix) :])\n",
        "    return tuple(prefix) + tuple(complete)\n",
        "oracle_all_journal_prefixes",
    ),
    "mutant_duplicate_prefix_result": Mutation(
        "    return tuple(dict.fromkeys(event_id(event) for event in journal))\n",
        "    return tuple(event_id(event) for event in journal)\n",
        "oracle_serial_resume_observation",
    ),
    "mutant_journal_physical_completion_order": Mutation(
        "    return tuple(task for task in canonical if task in physical_set)\n",
        "    return tuple(physical)\n",
        "oracle_completion_permutations",
    ),
    "mutant_incomplete_as_success": Mutation(
        "        Outcome.INCOMPLETE: EventKind.INCOMPLETE,\n",
        "        Outcome.INCOMPLETE: EventKind.SUCCESS,\n",
        "oracle_terminal_outcome_partition",
    ),
    "mutant_complete_early": Mutation(
        "        if cursor == len(plan.keys):\n",
        "        if cursor >= len(plan.keys) - 1:\n",
        "oracle_completion_totality",
    ),
    "mutant_allocate_before_limit_check": Mutation(
        "    ):\n"
        "        return None\n"
        "    probe.attempts += 1\n"
        "    return CodecUsage(usage.events + 1, usage.bytes + event_bytes)\n",
        "    ):\n"
        "        probe.attempts += 1\n"
        "        return None\n"
        "    probe.attempts += 1\n"
        "    return CodecUsage(usage.events + 1, usage.bytes + event_bytes)\n",
        "oracle_codec_boundary_sweep",
    ),
    "mutant_rescan_prefix_per_event": Mutation(
        "    return 9 * (input_cells + 1)\n",
        "    return 9 * (input_cells + 1) * (input_cells + 1)\n",
        "oracle_operation_accounting",
    ),
    "mutant_clone_journal_per_step": Mutation(
        "    return 7 * (input_cells + 1)\n",
        "    return 7 * (input_cells + 1) * (input_cells + 1)\n",
        "oracle_heap_accounting",
    ),
    "mutant_recursive_replay": Mutation(
        "    return visited, 1\n",
        "    return visited, depth + 1\n",
        "oracle_explicit_pda_lifecycle",
    ),
    "mutant_nonadvancing_replay_cursor": Mutation(
        "    return cursor + 1\n",
        "    return cursor\n",
        "oracle_replay_cursor_measure",
    ),
    "mutant_recovery_livelock": Mutation(
        "    return remaining - 1\n",
        "    return remaining\n",
        "oracle_finite_protocol_traces",
    ),
}


def fail(message: str) -> None:
    print(f"ERROR: {message}", file=sys.stderr)
    raise SystemExit(1)


def ledger_pairs() -> dict[str, str]:
    with LEDGER.open(encoding="utf-8", newline="") as source:
        rows = tuple(csv.DictReader(source, delimiter="\t"))
    return {row["causal-mutant"]: row["exhaustive-oracle"] for row in rows}


def inject(source: str, name: str, mutation: Mutation) -> str:
    observed = source.count(mutation.needle)
    if observed != mutation.occurrences:
        fail(
            f"mutant {name} expected {mutation.occurrences} injection site(s), "
            f"found {observed}"
        )
    return source.replace(mutation.needle, mutation.replacement, mutation.occurrences)


def validate_registry() -> None:
    expected = ledger_pairs()
    if expected != {name: mutation.oracle for name, mutation in MUTATIONS.items()}:
        fail(
            "mutation registry differs from invariant ledger: "
            f"missing={sorted(set(expected) - set(MUTATIONS))}, "
            f"extra={sorted(set(MUTATIONS) - set(expected))}, "
            "or property mapping changed"
        )


def main() -> None:
    validate_registry()
    source = SOURCE.read_text(encoding="utf-8")
    if MUTANT_ROOT.exists():
        shutil.rmtree(MUTANT_ROOT)
    MUTANT_ROOT.mkdir(parents=True)

    try:
        for name, mutation in MUTATIONS.items():
            path = MUTANT_ROOT / f"{name}.py"
            path.write_text(inject(source, name, mutation), encoding="utf-8")
            completed = subprocess.run(
                [sys.executable, str(path), "--property", mutation.oracle],
                cwd=ROOT,
                capture_output=True,
                text=True,
                timeout=30,
                check=False,
            )
            output = completed.stdout + completed.stderr
            expected = f"PROPERTY FAILED: {mutation.oracle}:"
            if completed.returncode == 0:
                fail(f"mutant {name} survived {mutation.oracle}")
            if expected not in output:
                fail(
                    f"mutant {name} died for an unexpected reason; "
                    f"wanted {expected!r}, got:\n{output}"
                )
            print(f"killed: {name}: {mutation.oracle}")
    finally:
        if MUTANT_ROOT.exists():
            shutil.rmtree(MUTANT_ROOT)

    print(f"Killed all {len(MUTATIONS)} durable-resume causal mutants.")


if __name__ == "__main__":
    main()
