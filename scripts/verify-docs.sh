#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
evidence_directory="$repository_root/target/documentation"
temporary_directory="$evidence_directory/tmp"
mkdir -p "$evidence_directory"

if [[ "${SCHEDLIB_DOCS_SCOPED:-0}" != "1" ]]; then
  mkdir -p "$temporary_directory"
  exec systemd-run --user --scope \
    -p MemoryMax=2G \
    -p MemorySwapMax=0 \
    -p CPUQuota=200% \
    -p TasksMax=32 \
    --setenv=SCHEDLIB_DOCS_SCOPED=1 \
    --setenv=TMPDIR="$temporary_directory" \
    --setenv=JAVA_TOOL_OPTIONS="-Djava.awt.headless=true -Djava.io.tmpdir=$temporary_directory" \
    -- "$repository_root/scripts/verify-docs.sh"
fi

"$repository_root/scripts/render-diagrams.sh" 2>&1 |
  tee "$evidence_directory/render-diagrams.log"

if ! command -v vinary-doc-lint >/dev/null 2>&1; then
  echo "required documentation linter is unavailable: vinary-doc-lint" >&2
  exit 1
fi

(
  cd "$repository_root"
  vinary-doc-lint check .
) 2>&1 | tee "$evidence_directory/vinary-doc-lint.log"
