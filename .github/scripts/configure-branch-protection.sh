#!/usr/bin/env bash
set -euo pipefail

repository="${1:-cmiami/BrickellStatus}"
branch="main"
# Every check that must pass before main accepts a merge. Each string must
# match a job's `name:` in ci.yml exactly: GitHub matches status contexts by
# that display name, so renaming a job silently un-gates it rather than
# failing loudly.
required_checks=(
  "Rust, console, and Tauri shell"
  "Fedora 44 build, tests, and RPM"
  "Android cross-compile"
)

if ! command -v gh >/dev/null 2>&1; then
  echo "GitHub CLI (gh) is required." >&2
  exit 1
fi

visibility="$(gh api "repos/${repository}" --jq '.visibility')"
default_branch="$(gh api "repos/${repository}" --jq '.default_branch')"

if [[ "${visibility}" != "public" ]]; then
  echo "Refusing to apply protection: ${repository} is ${visibility}, not public." >&2
  exit 1
fi

if [[ "${default_branch}" != "${branch}" ]]; then
  echo "Refusing to apply protection: default branch is ${default_branch}, not ${branch}." >&2
  exit 1
fi

if ! gh api "repos/${repository}/branches/${branch}" >/dev/null; then
  echo "Refusing to apply protection: ${repository}:${branch} does not exist yet." >&2
  exit 1
fi

contexts_json=""
for check in "${required_checks[@]}"; do
  contexts_json+="${contexts_json:+, }\"${check}\""
done

gh api \
  --method PUT \
  -H "Accept: application/vnd.github+json" \
  -H "X-GitHub-Api-Version: 2026-03-10" \
  "repos/${repository}/branches/${branch}/protection" \
  --input - <<JSON
{
  "required_status_checks": {
    "strict": true,
    "contexts": [${contexts_json}]
  },
  "enforce_admins": true,
  "required_pull_request_reviews": {
    "dismissal_restrictions": {
      "users": [],
      "teams": [],
      "apps": []
    },
    "dismiss_stale_reviews": true,
    "require_code_owner_reviews": true,
    "required_approving_review_count": 1,
    "require_last_push_approval": true,
    "bypass_pull_request_allowances": {
      "users": [],
      "teams": [],
      "apps": []
    }
  },
  "restrictions": null,
  "required_linear_history": false,
  "allow_force_pushes": false,
  "allow_deletions": false,
  "block_creations": false,
  "required_conversation_resolution": true,
  "lock_branch": false,
  "allow_fork_syncing": true
}
JSON

echo "Protected ${repository}:${branch}."
echo "Verify with: gh api repos/${repository}/branches/${branch}/protection"
