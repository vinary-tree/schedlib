#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
evidence_directory="$repository_root/target/durable-resume"
log_directory="$evidence_directory/logs"
temporary_directory="$evidence_directory/tmp"
coq_build_directory="$evidence_directory/coq"

mkdir -p "$log_directory" "$temporary_directory" "$coq_build_directory"

if [[ "${SCHEDLIB_DURABLE_FORMAL_SCOPED:-0}" != "1" ]]; then
  exec systemd-run --user --scope \
    -p MemoryMax=4G \
    -p MemorySwapMax=0 \
    -p CPUQuota=400% \
    -p TasksMax=64 \
    --setenv=SCHEDLIB_DURABLE_FORMAL_SCOPED=1 \
    --setenv=TMPDIR="$temporary_directory" \
    --setenv=JAVA_TOOL_OPTIONS="-Xmx1024m -XX:+UseParallelGC -Djava.awt.headless=true -Djava.io.tmpdir=$temporary_directory" \
    -- "$repository_root/scripts/verify-durable-resume-formal.sh"
fi

export TMPDIR="$temporary_directory"
export JAVA_TOOL_OPTIONS="${JAVA_TOOL_OPTIONS:--Xmx1024m -XX:+UseParallelGC -Djava.awt.headless=true -Djava.io.tmpdir=$temporary_directory}"

require_tool() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "required durable-resume formal tool is unavailable: $1" >&2
    exit 1
  fi
}

for tool in awk cargo coqc coqchk diff python3 rg rustfmt sort tail tla2sany tlc z3; do
  require_tool "$tool"
done

verify_invariant_ledger() {
  local ledger="$repository_root/formal/durable-resume-invariants.tsv"
  local expected="$repository_root/formal/smt/durable-resume.expected"
  local log="$log_directory/invariant-ledger.log"

  {
    awk -F '\t' '
      NR == 1 {
        if (NF != 10) {
          print "durable invariant ledger header must have exactly 10 fields"
          failed = 1
        }
        next
      }
      {
        if (NF != 10) {
          print "durable invariant row " NR " must have exactly 10 fields"
          failed = 1
        }
        for (field = 1; field <= 10; field++) {
          if ($field == "") {
            print "durable invariant row " NR " has empty field " field
            failed = 1
          }
        }
        if (seen_id[$1]++) {
          print "duplicate durable invariant identifier: " $1
          failed = 1
        }
        if (seen_test[$8]++) {
          print "duplicate required-red test identifier: " $8
          failed = 1
        }
        if (seen_mutant[$9]++) {
          print "duplicate causal mutant identifier: " $9
          failed = 1
        }
        if ($10 != "required-before-implementation") {
          print "durable invariant row is prematurely accepted: " $1
          failed = 1
        }
        count++
      }
      END {
        print "durable invariant obligations: " count
        if (count != 35) {
          print "durable invariant ledger must contain exactly 35 obligations"
          failed = 1
        }
        exit failed
      }
    ' "$ledger"

    mapfile -t configured_predicates < <(
      rg --no-filename '^(INVARIANT|PROPERTY) ' \
        "$repository_root/formal/tla"/DurableResume*.cfg |
        awk '{print $2}' |
        sort -u
    )

    local predicate
    for predicate in "${configured_predicates[@]}"; do
      if ! rg -Fq "$predicate" "$ledger"; then
        echo "configured durable predicate is absent from ledger: $predicate"
        return 1
      fi
      echo "registered durable predicate: $predicate"
    done

    local rocq_obligation_file="$temporary_directory/rocq-obligations.txt"
    awk -F '\t' '
      NR > 1 {
        count = split($4, values, /; */)
        for (i = 1; i <= count; i++) {
          if (values[i] != "not-applicable") print values[i]
        }
      }
    ' "$ledger" | sort -u > "$rocq_obligation_file"
    mapfile -t rocq_obligations < "$rocq_obligation_file"
    local obligation
    for obligation in "${rocq_obligations[@]}"; do
      if ! rg -Fq "$obligation" "$repository_root/formal/coq/DurableResume.v"; then
        echo "ledger Rocq obligation is absent from theory: $obligation"
        return 1
      fi
    done
    echo "registered Rocq obligations: ${#rocq_obligations[@]}"
    if [[ "${#rocq_obligations[@]}" -eq 0 ]]; then
      echo "durable invariant ledger registered no Rocq obligations"
      return 1
    fi

    local tla_obligation_file="$temporary_directory/tla-obligations.txt"
    awk -F '\t' '
      NR > 1 {
        count = split($5, values, /; */)
        for (i = 1; i <= count; i++) {
          if (values[i] != "not-applicable") print values[i]
        }
      }
    ' "$ledger" | sort -u > "$tla_obligation_file"
    mapfile -t tla_obligations < "$tla_obligation_file"
    for obligation in "${tla_obligations[@]}"; do
      if ! rg -Fq "$obligation" "$repository_root/formal/tla/DurableResume.tla"; then
        echo "ledger TLA+ obligation is absent from model: $obligation"
        return 1
      fi
    done
    echo "registered TLA+ obligations: ${#tla_obligations[@]}"
    if [[ "${#tla_obligations[@]}" -eq 0 ]]; then
      echo "durable invariant ledger registered no TLA+ obligations"
      return 1
    fi

    local smt_obligation_file="$temporary_directory/smt-obligations.txt"
    awk -F '\t' '
      NR > 1 {
        count = split($6, values, /; */)
        for (i = 1; i <= count; i++) {
          if (values[i] != "not-applicable") print values[i]
        }
      }
    ' "$ledger" | sort -u > "$smt_obligation_file"
    mapfile -t smt_obligations < "$smt_obligation_file"
    for obligation in "${smt_obligations[@]}"; do
      if ! rg -Fq "$obligation" "$repository_root/formal/smt/durable-resume.smt2"; then
        echo "ledger SMT obligation is absent from model: $obligation"
        return 1
      fi
    done
    echo "registered SMT obligations: ${#smt_obligations[@]}"
    if [[ "${#smt_obligations[@]}" -eq 0 ]]; then
      echo "durable invariant ledger registered no SMT obligations"
      return 1
    fi

    local expected_queries
    expected_queries="$(awk 'NR % 2 == 1 {count++} END {print count + 0}' "$expected")"
    echo "expected SMT queries: $expected_queries"
    if [[ "$expected_queries" -ne 21 ]]; then
      echo "durable SMT verdict ledger must contain exactly 21 queries"
      return 1
    fi
    mapfile -t expected_query_names < <(awk 'NR % 2 == 1' "$expected")
    for obligation in "${expected_query_names[@]}"; do
      if ! rg -Fq "$obligation" "$ledger"; then
        echo "expected SMT query is absent from invariant ledger: $obligation"
        return 1
      fi
    done
  } 2>&1 | tee "$log_directory/invariant-ledger.log"
}

verify_rocq() {
  local log="$log_directory/rocq.log"
  local assumptions_log="$log_directory/rocq-assumptions.log"
  local kernel_log="$log_directory/rocq-kernel.log"

  cp "$repository_root/formal/coq/DurableResume.v" "$coq_build_directory/DurableResume.v"
  cp "$repository_root/formal/coq/Assumptions.v" "$coq_build_directory/Assumptions.v"

  (
    cd "$coq_build_directory"
    coqc -Q . Schedlib DurableResume.v
  ) 2>&1 | tee "$log"

  (
    cd "$coq_build_directory"
    coqc -Q . Schedlib Assumptions.v
  ) 2>&1 | tee "$assumptions_log"

  if rg -n '\b(Axiom|Conjecture|Admitted|admit|Abort)\b' \
       "$repository_root/formal/coq/DurableResume.v" \
       "$repository_root/formal/coq/Assumptions.v"; then
    echo "proof escape found in durable Rocq source" >&2
    return 1
  fi

  local closed_count
  closed_count="$(rg -c '^Closed under the global context$' "$assumptions_log")"
  echo "closed Rocq assumption reports: $closed_count" | tee -a "$assumptions_log"
  if [[ "$closed_count" -ne 18 ]]; then
    echo "expected exactly 18 closed Rocq assumption reports" >&2
    return 1
  fi

  (
    cd "$coq_build_directory"
    coqchk -Q . Schedlib Schedlib.DurableResume
  ) >"$kernel_log" 2>&1
  tail -n 20 "$kernel_log"
  rg -q '^Modules were successfully checked$' "$kernel_log"
}

verify_tla() {
  local syntax_log="$log_directory/tla-syntax.log"
  (
    cd "$repository_root/formal/tla"
    tla2sany DurableResume.tla
  ) 2>&1 | tee "$syntax_log"

  local scenario
  for scenario in Crash Resume Outcomes Resources Stale KeyCollision Malformed UnsafeReplay; do
    local metadata_directory="$evidence_directory/tlc-$scenario"
    local scenario_log="$log_directory/tlc-$scenario.log"
    rm -rf "$metadata_directory"
    mkdir -p "$metadata_directory"

    set +e
    (
      cd "$repository_root/formal/tla"
      tlc -workers 1 \
        -metadir "$metadata_directory" \
        -config "DurableResume$scenario.cfg" \
        DurableResume.tla
    ) 2>&1 | tee "$scenario_log"
    local status="${PIPESTATUS[0]}"
    set -e

    if [[ "$status" -ne 0 ]]; then
      return "$status"
    fi
    rg -q '^Model checking completed\. No error has been found\.$' "$scenario_log"
    rg -q '^The depth of the complete state graph search is ' "$scenario_log"
    rg -q ' distinct states found, 0 states left on queue\.$' "$scenario_log"
  done
}

verify_smt() {
  local raw_log="$log_directory/z3-durable-resume.log"
  local expected="$repository_root/formal/smt/durable-resume.expected"

  z3 -smt2 "$repository_root/formal/smt/durable-resume.smt2" 2>&1 | tee "$raw_log"
  diff -u "$expected" "$raw_log"

  local verdict_count
  verdict_count="$(rg -c '^(sat|unsat|unknown)$' "$raw_log")"
  if [[ "$verdict_count" -ne 21 ]]; then
    echo "expected exactly 21 SMT verdicts, observed $verdict_count" >&2
    return 1
  fi
  if rg -q '^unknown$' "$raw_log"; then
    echo "SMT solver returned unknown" >&2
    return 1
  fi
}

verify_executable_contract() {
  python3 "$repository_root/scripts/check-durable-resume-traceability.py" \
    2>&1 | tee "$log_directory/executable-traceability.log"

  python3 "$repository_root/scripts/check-durable-resume-exhaustive.py" \
    2>&1 | tee "$log_directory/executable-oracle.log"

  python3 "$repository_root/scripts/check-durable-resume-mutants.py" \
    2>&1 | tee "$log_directory/causal-mutants.log"

  "$repository_root/scripts/check-durable-resume-required-red.sh" \
    2>&1 | tee "$log_directory/required-red-gate.log"
}

verify_no_placeholders() {
  local log="$log_directory/placeholder-audit.log"
  local paths=(
    "$repository_root/formal/coq/DurableResume.v"
    "$repository_root/formal/coq/Assumptions.v"
    "$repository_root/formal/tla/DurableResume.tla"
    "$repository_root/formal/smt/durable-resume.smt2"
    "$repository_root/formal/durable-resume-invariants.tsv"
    "$repository_root/scripts/check-durable-resume-exhaustive.py"
    "$repository_root/scripts/check-durable-resume-mutants.py"
    "$repository_root/scripts/check-durable-resume-traceability.py"
    "$repository_root/scripts/check-durable-resume-required-red.sh"
    "$repository_root/formal/required-red/durable-resume/Cargo.toml"
    "$repository_root/formal/required-red/durable-resume/src/lib.rs"
    "$repository_root/formal/required-red/durable-resume/tests/contracts.rs"
  )

  if rg -n '\b(TODO|FIXME|HACK|XXX|placeholder|stub|workaround|unimplemented)\b' "${paths[@]}" |
       tee "$log"; then
    echo "unfinished marker found in durable-resume formal artifacts" >&2
    return 1
  fi
  echo "unfinished-marker audit passed" | tee "$log"
}

verify_invariant_ledger
verify_rocq
verify_tla
verify_smt
verify_executable_contract
verify_no_placeholders

echo "Durable resume formal verification completed successfully."
