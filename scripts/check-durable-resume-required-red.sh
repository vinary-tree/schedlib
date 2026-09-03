#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
evidence_directory="$repository_root/target/durable-resume"
log_directory="$evidence_directory/logs"
temporary_directory="$evidence_directory/tmp"
target_directory="$evidence_directory/required-red-target"
manifest="$repository_root/formal/required-red/durable-resume/Cargo.toml"
manifest_lock="$repository_root/formal/required-red/durable-resume/Cargo.lock"

mkdir -p "$log_directory" "$temporary_directory"

if [[ "${SCHEDLIB_DURABLE_REQUIRED_RED_SCOPED:-0}" != "1" \
      && "${SCHEDLIB_DURABLE_FORMAL_SCOPED:-0}" != "1" ]]; then
  exec systemd-run --user --scope \
    -p MemoryMax=4G \
    -p MemorySwapMax=0 \
    -p CPUQuota=400% \
    -p TasksMax=64 \
    --setenv=SCHEDLIB_DURABLE_REQUIRED_RED_SCOPED=1 \
    --setenv=CARGO_BUILD_JOBS=1 \
    --setenv=CARGO_TARGET_DIR="$target_directory" \
    --setenv=TMPDIR="$temporary_directory" \
    -- "$repository_root/scripts/check-durable-resume-required-red.sh"
fi

export CARGO_BUILD_JOBS=1
export CARGO_TARGET_DIR="$target_directory"
export TMPDIR="$temporary_directory"

python3 "$repository_root/scripts/check-durable-resume-traceability.py" \
  2>&1 | tee "$log_directory/durable-resume-traceability.log"

rustfmt --edition 2021 --check \
  "$repository_root/formal/required-red/durable-resume/src/lib.rs" \
  "$repository_root/formal/required-red/durable-resume/tests/contracts.rs" \
  2>&1 | tee "$log_directory/durable-resume-required-red-rustfmt.log"

log="$log_directory/durable-resume-required-red.log"
ledger="$repository_root/formal/durable-resume-invariants.tsv"
if rg -q $'\trequired-before-implementation$' "$ledger"; then
  expected_mode="red"
else
  expected_mode="green"
fi
set +e
if [[ "$expected_mode" == "red" ]]; then
  cargo test --offline --manifest-path "$manifest" --no-run \
    2>&1 | tee "$log"
else
  cargo test --release --offline --manifest-path "$manifest" \
    2>&1 | tee "$log"
fi
status="${PIPESTATUS[0]}"
set -e

if [[ "$expected_mode" == "red" ]]; then
  if [[ "$status" -ne 101 ]]; then
    echo "durable-resume properties must be red with Cargo status 101; got $status" >&2
    exit 1
  fi
  if rg -qi 'failed to download|network failure|could not resolve host|timed out while fetching' "$log"; then
    echo "durable-resume required-red failed because of dependency transport" >&2
    exit 1
  fi
  if ! rg -Fq 'could not find `durable` in `schedlib`' "$log"; then
    echo "durable-resume required-red is not caused by the reviewed missing API" >&2
    exit 1
  fi
elif [[ "$status" -ne 0 ]]; then
  echo "accepted durable-resume properties must all pass; got Cargo status $status" >&2
  exit 1
fi

case "$manifest_lock" in
  "$repository_root"/formal/required-red/durable-resume/Cargo.lock)
    rm -f "$manifest_lock"
    ;;
  *)
    echo "refusing to clean an unexpected required-red lock path" >&2
    exit 1
    ;;
esac

case "$target_directory" in
  "$repository_root"/target/durable-resume/required-red-target)
    rm -rf "$target_directory"
    ;;
  *)
    echo "refusing to clean an unexpected required-red target path" >&2
    exit 1
    ;;
esac

if [[ "$expected_mode" == "red" ]]; then
  echo "Validated all 35 causal required-red durable-resume properties."
else
  echo "Validated all 35 causal postimplementation durable-resume properties."
fi
