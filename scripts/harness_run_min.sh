#!/usr/bin/env bash
set -euo pipefail

# Run a minimal harness scenario and shellcheck scripts first if available.

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

# Normalize line endings and ensure executables
sed -i 's/\r$//' scripts/test_run.sh scripts/summarize_results.py || true
chmod +x scripts/test_run.sh scripts/summarize_results.py || true

# Shellcheck if available
if command -v shellcheck >/dev/null 2>&1; then
  shellcheck scripts/test_run.sh || true
fi

# Minimal run (release binary, small scenario)
./scripts/test_run.sh --scenarios small --runs 1 --build release \
  --k 4 --pct 25 --chunk 4096 --interleave off


