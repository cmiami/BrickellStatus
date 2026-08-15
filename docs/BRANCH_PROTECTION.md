# `main` branch protection

This repository is public, but `main` is not a public write surface. Changes
land through reviewed pull requests after the first approved source push.

## Intended policy

| Control | Required value |
|---|---|
| Repository visibility | Public |
| Pull request before merge | Required |
| Approving reviews | 1 |
| Code-owner review | Required |
| Dismiss stale approvals | Yes |
| Approval after the most recent push | Required from someone other than the pusher |
| Required check | `Rust, console, and Tauri shell` |
| Require branch up to date | Yes |
| Resolve conversations | Required |
| Apply to administrators | Yes |
| Force pushes | Blocked |
| Branch deletion | Blocked |

There is no auto-merge policy for dependency pull requests. Dependabot may
open delayed version-update proposals, but they travel through the same review
and CI gate as human changes. GitHub exempts security updates from the
cooldown; branch review and checks still apply.

## Apply after the first reviewed push

Do **not** run the script while the remote repository is empty: GitHub cannot
protect a branch that does not exist. After the reviewed local tree has been
committed and pushed to `main`, authenticate `gh` as a repository
administrator and run:

```sh
bash .github/scripts/configure-branch-protection.sh cmiami/PuenteGonorrea
```

The script follows GitHub's [protected-branch REST
contract](https://docs.github.com/en/rest/branches/branch-protection#update-branch-protection)
and is deliberately fail-closed. It refuses to continue unless the
repository is already public, the default branch is exactly `main`, and the
remote `main` branch exists. It does not change visibility, push code, create a
release, enable auto-merge, or weaken any repository setting.

The caller needs repository administration permission. For a fine-grained
token, grant repository **Administration: write** and **Contents: read**.

## Verify

Read the resulting rule without mutating it:

```sh
gh api repos/cmiami/PuenteGonorrea/branches/main/protection \
  --jq '{checks: .required_status_checks, reviews: .required_pull_request_reviews, admins: .enforce_admins, conversations: .required_conversation_resolution, force_pushes: .allow_force_pushes, deletions: .allow_deletions}'
```

Then open a disposable pull request and confirm that GitHub blocks merge until
the named CI job passes, one code-owner review is present, the final pusher is
not the approving reviewer, and every conversation is resolved. Close the
test pull request without merging it.

If the CI job name changes in [`.github/workflows/ci.yml`](../.github/workflows/ci.yml),
update the check context in both this document and the script in the same
reviewed pull request.
