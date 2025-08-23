#!/usr/bin/env bash
set -euo pipefail

# Summarize the latest harness results via Python summarizer

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

sed -i 's/\r$//' scripts/summarize_results.py || true
chmod +x scripts/summarize_results.py || true

# Find latest results.jsonl by modification time
latest=""
while IFS= read -r -d '' f; do
  latest="$f"
  break
done < <(find _tgt/tests -type f -name results.jsonl -printf '%T@\t%p\0' 2>/dev/null | sort -zr | cut -zf2-)

if [[ -z "$latest" ]]; then
  echo "No results.jsonl found under _tgt/tests" >&2
  exit 2
fi

python3 scripts/summarize_results.py --path "$latest"


