#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
evidence_directory="$repository_root/target/documentation"
temporary_directory="$evidence_directory/tmp"

mkdir -p "$temporary_directory"

if [[ "${SCHEDLIB_DOCS_SCOPED:-0}" != "1" ]]; then
  exec systemd-run --user --scope \
    -p MemoryMax=2G \
    -p MemorySwapMax=0 \
    -p CPUQuota=200% \
    -p TasksMax=32 \
    --setenv=SCHEDLIB_DOCS_SCOPED=1 \
    --setenv=TMPDIR="$temporary_directory" \
    --setenv=JAVA_TOOL_OPTIONS="-Djava.awt.headless=true -Djava.io.tmpdir=$temporary_directory" \
    -- "$repository_root/scripts/render-diagrams.sh"
fi

export TMPDIR="$temporary_directory"
export JAVA_TOOL_OPTIONS="${JAVA_TOOL_OPTIONS:--Djava.awt.headless=true -Djava.io.tmpdir=$temporary_directory}"

if ! command -v plantuml >/dev/null 2>&1; then
  echo "required diagram compiler is unavailable: plantuml" >&2
  exit 1
fi

mapfile -d '' diagrams < <(
  find "$repository_root/docs" -type f -name '*.puml' -print0 | sort -z
)

if [[ "${#diagrams[@]}" -eq 0 ]]; then
  echo "no PlantUML diagrams found" >&2
  exit 1
fi

plantuml -failfast2 -charset UTF-8 -tsvg "${diagrams[@]}"

for source in "${diagrams[@]}"; do
  output="${source%.puml}.svg"
  if [[ ! -s "$output" ]]; then
    echo "diagram output is missing or empty: $output" >&2
    exit 1
  fi
done

echo "rendered ${#diagrams[@]} PlantUML diagrams"
