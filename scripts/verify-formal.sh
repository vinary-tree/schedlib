#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
verification_target="${1:-all}"
evidence_directory="$repository_root/target/verification"
temporary_directory="$evidence_directory/tmp"
tlaps_directory="$evidence_directory/tlaps"

mkdir -p "$temporary_directory"

if [[ "${SCHEDLIB_FORMAL_SCOPED:-0}" != "1" ]]; then
  exec systemd-run --user --scope \
    -p MemoryMax=4G \
    -p MemorySwapMax=0 \
    -p CPUQuota=400% \
    -p TasksMax=64 \
    --setenv=SCHEDLIB_FORMAL_SCOPED=1 \
    --setenv=TMPDIR="$temporary_directory" \
    --setenv=JAVA_TOOL_OPTIONS="-Xmx1024m -Djava.awt.headless=true -Djava.io.tmpdir=$temporary_directory" \
    -- "$repository_root/scripts/verify-formal.sh" "$verification_target"
fi

export TMPDIR="$temporary_directory"
export JAVA_TOOL_OPTIONS="${JAVA_TOOL_OPTIONS:--Xmx1024m -Djava.awt.headless=true -Djava.io.tmpdir=$temporary_directory}"

require_tool() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "required formal-verification tool is unavailable: $1" >&2
    exit 1
  fi
}

verify_registry() {
  require_tool awk
  require_tool rg
  require_tool sort

  local registry="$repository_root/formal/refinement-map.tsv"
  local log="$evidence_directory/refinement-registry.log"

  {
    awk -F '\t' '
      NR == 1 {
        if (NF != 8) {
          print "registry header must have exactly 8 fields"
          failed = 1
        }
        next
      }
      {
        if (NF != 8) {
          print "registry row " NR " must have exactly 8 fields"
          failed = 1
        }
        if ($1 == "" || $4 == "" || $6 == "" || $7 == "") {
          print "registry row " NR " has an empty normative field"
          failed = 1
        }
        if (seen[$1]++) {
          print "duplicate registry identifier: " $1
          failed = 1
        }
        if ($8 != "accepted") {
          print "refinement registry row is not accepted: " $1
          failed = 1
        }
        count++
      }
      END {
        print "registry obligations: " count
        if (count != 32) {
          print "registry must contain exactly 32 extracted obligations"
          failed = 1
        }
        exit failed
      }
    ' "$registry"

    mapfile -t configured_predicates < <(
      rg --no-filename '^(INVARIANT|PROPERTY) ' \
        "$repository_root/formal/tla"/*.cfg |
        awk '{print $2}' |
        sort -u
    )

    local predicate
    for predicate in "${configured_predicates[@]}"; do
      if ! rg -Fq "$predicate" "$registry"; then
        echo "configured formal predicate is absent from registry: $predicate"
        return 1
      fi
      echo "registered configured predicate: $predicate"
    done

    if ! rg -Fq 'EffectIndependenceKernelIsSymmetric' "$registry"; then
      echo "TLAPS theorem is absent from registry"
      return 1
    fi
    echo "registered TLAPS theorem: EffectIndependenceKernelIsSymmetric"
  } 2>&1 | tee "$log"
}

verify_tla() {
  require_tool tla2sany
  require_tool tlc

  local syntax_log="$evidence_directory/tla-syntax.log"
  (
    cd "$repository_root/formal/tla"
    tla2sany SchedulerKernels.tla
    tla2sany DeterministicScheduler.tla
  ) 2>&1 | tee "$syntax_log"

  local scenario
  for scenario in Empty BatchOrder Dependencies Effects Resources Outcomes; do
    local scenario_directory="$evidence_directory/tlc-$scenario"
    local scenario_log="$evidence_directory/tlc-$scenario.log"
    rm -rf "$scenario_directory"
    mkdir -p "$scenario_directory"

    set +e
    (
      cd "$repository_root/formal/tla"
      tlc -workers 1 \
        -metadir "$scenario_directory" \
        -config "$scenario.cfg" \
        DeterministicScheduler.tla
    ) 2>&1 | tee "$scenario_log"
    local status="${PIPESTATUS[0]}"
    set -e

    rm -rf "$scenario_directory"
    if [[ "$status" -ne 0 ]]; then
      return "$status"
    fi

    rg -q '^Model checking completed\. No error has been found\.$' "$scenario_log"
    rg -q '^The depth of the complete state graph search is ' "$scenario_log"
    rg -q ' distinct states found, 0 states left on queue\.$' "$scenario_log"
  done
}

verify_tlaps() {
  require_tool tlapm

  local log="$evidence_directory/tlaps.log"
  rm -rf "$tlaps_directory"
  mkdir -p "$tlaps_directory"

  set +e
  (
    cd "$tlaps_directory"
    tlapm -I "$repository_root/formal/tla" \
      "$repository_root/formal/tla/SchedulerKernels.tla"
  ) 2>&1 | tee "$log"
  local status="${PIPESTATUS[0]}"
  set -e

  rm -rf "$tlaps_directory"
  if [[ "$status" -ne 0 ]]; then
    return "$status"
  fi

  rg -q 'All 1 obligation(s)? proved\.' "$log"
}

case "$verification_target" in
  tla)
    verify_registry
    verify_tla
    ;;
  tlaps)
    verify_registry
    verify_tlaps
    ;;
  all)
    verify_registry
    verify_tla
    verify_tlaps
    ;;
  *)
    echo "usage: $0 {tla|tlaps|all}" >&2
    exit 2
    ;;
esac
