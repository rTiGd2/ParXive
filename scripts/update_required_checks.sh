#!/usr/bin/env bash
set -euo pipefail

OWNER=${1:-rTiGd2}
REPO=${2:-ParXive}
BR=${3:-main}

echo "Updating required status checks for $OWNER/$REPO:$BR"

# Desired contexts
contexts=(
  "fmt"
  "clippy"
  "tests"
  "cargo-deny"
  "Analyze"
)

# Build JSON array of contexts
ctx_json=$(printf '%s\n' "${contexts[@]}" | jq -Rcs 'split("\n") | map(select(length>0))')

json=$(jq -n --argjson contexts "$ctx_json" '{
  required_status_checks: { strict: true, contexts: $contexts },
  enforce_admins: true,
  required_pull_request_reviews: { required_approving_review_count: 1 },
  restrictions: null,
  allow_force_pushes: false,
  allow_deletions: false,
  required_linear_history: true
}')

printf '%s' "$json" | gh api -X PUT repos/"$OWNER"/"$REPO"/branches/"$BR"/protection -H "Accept: application/vnd.github+json" --input -
echo
echo "Done. Note: ensure jobs exist before making them required."
