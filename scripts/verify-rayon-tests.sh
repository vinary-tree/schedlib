#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
registry="$repository_root/formal/rayon-refinement-map.tsv"
test_source="$repository_root/tests/rayon_refinement.rs"
evidence_directory="$repository_root/target/testing"
mkdir -p "$evidence_directory"

awk -F '\t' '
  NR > 1 {
    count += 1
    split($7, names, ";")
    for (name_index in names) {
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", names[name_index])
      if (names[name_index] != "") {
        print names[name_index]
      }
    }
  }
  END {
    if (count != 8) {
      print "expected 8 Rayon refinement rows, found " count > "/dev/stderr"
      exit 1
    }
  }
' "$registry" | sort -u > "$evidence_directory/required-rayon-test-names.txt"

missing=0
while IFS= read -r test_name; do
  if ! rg --quiet "fn[[:space:]]+$test_name[[:space:]]*\(" "$test_source"; then
    echo "missing Rayon refinement test: $test_name"
    missing=1
  fi
done < "$evidence_directory/required-rayon-test-names.txt"

if [[ "$missing" -ne 0 ]]; then
  exit 1
fi

required_count="$(wc -l < "$evidence_directory/required-rayon-test-names.txt")"
echo "registered $required_count unique tests for all 8 Rayon refinement rows"
