#!/usr/bin/env bash
set -euo pipefail

# Usage: scripts/update_required_checks.sh [OWNER] [REPO] [BRANCH]

OWNER=${1:-}
REPO=${2:-}
BR=${3:-main}

if [[ -z "${OWNER}" || -z "${REPO}" ]]; then
  url=$(git remote get-url origin 2>/dev/null || true)
  if [[ -z "$url" ]]; then
    echo "Error: provide OWNER REPO or run in a git repo with an 'origin' remote." >&2
    exit 1
  fi
  owner_repo=$(sed -E 's#.*github.com[:/ ]([^/]+/[^/.]+)(\\.git)?$#\1#' <<<"$url")
  OWNER="${owner_repo%/*}"
  REPO="${owner_repo#*/}"
fi

echo "Updating required status checks for $OWNER/$REPO:$BR"

# Desired contexts (must match job/check names exactly)
contexts=(
  "fmt"
  "clippy"
  "tests"
  "cargo-deny"
  "Analyze (rust)"
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

printf '%s' "$json" | gh api -X PUT repos/$OWNER/$REPO/branches/$BR/protection -H "Accept: application/vnd.github+json" --input -
echo
echo "Done. Note: ensure jobs exist before making them required."

