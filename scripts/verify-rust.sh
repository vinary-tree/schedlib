#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
evidence_directory="$repository_root/target/acceptance"
temporary_directory="$evidence_directory/tmp"
cargo_target_directory="$evidence_directory/cargo"
msrv_target_directory="$evidence_directory/cargo-msrv"
mkdir -p "$temporary_directory" "$cargo_target_directory" "$msrv_target_directory"

if [[ "${SCHEDLIB_RUST_SCOPED:-0}" != "1" ]]; then
  exec systemd-run --user --scope \
    -p MemoryMax=3G \
    -p MemorySwapMax=0 \
    -p CPUQuota=200% \
    -p TasksMax=64 \
    --setenv=SCHEDLIB_RUST_SCOPED=1 \
    --setenv=CARGO_BUILD_JOBS=1 \
    --setenv=CARGO_TARGET_DIR="$cargo_target_directory" \
    --setenv=TMPDIR="$temporary_directory" \
    -- "$repository_root/scripts/verify-rust.sh"
fi

export CARGO_BUILD_JOBS=1
export CARGO_TARGET_DIR="$cargo_target_directory"
export TMPDIR="$temporary_directory"

run_gate() {
  local name="$1"
  shift
  set +e
  "$@" 2>&1 | tee "$evidence_directory/$name.log"
  local status="${PIPESTATUS[0]}"
  set -e
  if [[ "$status" -ne 0 ]]; then
    return "$status"
  fi
}

run_gate refinement-registry "$repository_root/scripts/verify-refinement-tests.sh"
run_gate rayon-refinement-registry "$repository_root/scripts/verify-rayon-tests.sh"
run_gate cargo-fmt cargo fmt --all -- --check
run_gate cargo-check cargo check --all-targets --all-features
run_gate cargo-check-msrv env CARGO_TARGET_DIR="$msrv_target_directory" \
  cargo +1.85.0 check --all-targets --all-features
run_gate cargo-clippy cargo clippy --all-targets --all-features -- -D warnings
run_gate cargo-test-debug cargo test --all-targets --all-features
run_gate cargo-test-release cargo test --all-targets --all-features --release
run_gate cargo-run-example cargo run --release --all-features --example serial
run_gate cargo-run-rayon-example cargo run --release --all-features --example rayon
run_gate cargo-test-doc cargo test --doc --all-features
run_gate cargo-doc env RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
run_gate cargo-package cargo package --locked
