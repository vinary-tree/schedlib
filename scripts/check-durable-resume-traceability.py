#!/usr/bin/env python3
"""Validate exact ledger-to-oracle-to-test-to-mutant traceability."""

from __future__ import annotations

import csv
import re
import subprocess
import sys
from pathlib import Path

sys.dont_write_bytecode = True

ROOT = Path(__file__).resolve().parents[1]
LEDGER = ROOT / "formal/durable-resume-invariants.tsv"
ORACLE = ROOT / "scripts/check-durable-resume-exhaustive.py"
MUTANTS = ROOT / "scripts/check-durable-resume-mutants.py"
CONTRACTS = ROOT / "formal/required-red/durable-resume/tests/contracts.rs"
EXPECTED_COLUMNS = (
    "id",
    "kind",
    "statement",
    "rocq-obligation",
    "tla-obligation",
    "smt-obligation",
    "exhaustive-oracle",
    "required-red-test",
    "causal-mutant",
    "acceptance-state",
)


def fail(message: str) -> None:
    print(f"ERROR: {message}", file=sys.stderr)
    raise SystemExit(1)


def unique(values: list[str], label: str) -> set[str]:
    if len(values) != len(set(values)):
        fail(f"duplicate {label} identifier")
    return set(values)


def main() -> None:
    with LEDGER.open(encoding="utf-8", newline="") as source:
        reader = csv.DictReader(source, delimiter="\t")
        if tuple(reader.fieldnames or ()) != EXPECTED_COLUMNS:
            fail(f"unexpected durable ledger columns: {reader.fieldnames}")
        rows = list(reader)
    if len(rows) != 35:
        fail(f"expected 35 durable ledger rows, found {len(rows)}")
    states = {row["acceptance-state"] for row in rows}
    preimplementation = states == {"required-before-implementation"}
    accepted_pattern = re.compile(r"accepted@[0-9a-f]{40}")
    postimplementation = all(
        accepted_pattern.fullmatch(row["acceptance-state"]) for row in rows
    )
    if not preimplementation and not postimplementation:
        fail(
            "durable ledger must be uniformly required-before-implementation "
            "or carry one exact implementation commit per accepted row"
        )
    if postimplementation:
        evidence_commits = {
            row["acceptance-state"].removeprefix("accepted@") for row in rows
        }
        for commit in evidence_commits:
            resolved = subprocess.run(
                ["git", "cat-file", "-e", f"{commit}^{{commit}}"],
                cwd=ROOT,
                check=False,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            if resolved.returncode != 0:
                fail(f"accepted implementation commit is unavailable: {commit}")

    expected_oracles = unique(
        [row["exhaustive-oracle"] for row in rows], "ledger oracle"
    )
    expected_tests = unique(
        [row["required-red-test"] for row in rows], "ledger test"
    )
    expected_mutants = unique(
        [row["causal-mutant"] for row in rows], "ledger mutant"
    )

    oracle_text = ORACLE.read_text(encoding="utf-8")
    actual_oracles = unique(
        re.findall(r"^def (oracle_[a-z0-9_]+)\(\) -> int:$", oracle_text, re.MULTILINE),
        "executable oracle",
    )
    contract_text = CONTRACTS.read_text(encoding="utf-8")
    actual_tests = unique(
        re.findall(r"^fn ((?:prop|small_stack)_[a-z0-9_]+)\(\) \{$", contract_text, re.MULTILINE),
        "required-red test",
    )
    mutant_text = MUTANTS.read_text(encoding="utf-8")
    actual_mutants = unique(
        re.findall(r'^    "(mutant_[a-z0-9_]+)": Mutation\($', mutant_text, re.MULTILINE),
        "causal mutant",
    )

    for label, expected, actual in (
        ("oracle", expected_oracles, actual_oracles),
        ("required-red test", expected_tests, actual_tests),
        ("causal mutant", expected_mutants, actual_mutants),
    ):
        if expected != actual:
            fail(
                f"{label} coverage differs from the ledger: "
                f"missing={sorted(expected - actual)}, extra={sorted(actual - expected)}"
            )

    test_attributes = re.findall(
        r"#\[test\]\nfn ((?:prop|small_stack)_[a-z0-9_]+)\(\) \{",
        contract_text,
        re.MULTILINE,
    )
    if set(test_attributes) != actual_tests or len(test_attributes) != len(actual_tests):
        fail("every required-red property must have exactly one #[test] attribute")

    print(
        "Durable-resume traceability validated "
        f"in {'preimplementation' if preimplementation else 'postimplementation'} mode: "
        f"{len(rows)} ledger rows, {len(actual_oracles)} oracles, "
        f"{len(actual_tests)} required-red tests, and {len(actual_mutants)} mutants."
    )


if __name__ == "__main__":
    main()
