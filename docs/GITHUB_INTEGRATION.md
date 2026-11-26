# GitHub integration

The native GitHub workspace provides **Accounts**, **Repositories** and **Pull requests**. The application also works independently with local repositories, SSH agents and the supported credential helpers.

## First launch

New local profiles offer an optional GitHub.com sign-in. Continue without an account to open the local workspace immediately. This choice is stored locally and does not prevent connecting later from **Settings → GitHub Accounts**. Profiles with an existing repository catalog or GitHub configuration go directly to the workspace.

Sign-in starts Device Flow and displays the code within gitRUST. Copy the code, open GitHub authorization in the system browser and approve the connection there. After authorization, gitRUST opens Accounts. Creating a GitHub.com account also opens the browser. A release can provide the public Client ID at build time. Builds without an ID show the setup field when sign-in is selected. GitHub Enterprise Server and other dedicated hosts are outside the current GitHub.com integration.

## Workflows

- Open [GitHub App setup](GITHUB_APP_SETUP.md) before connecting for the first time. Refresh the profile and available installations. Select a GitHub account or associate the current local workspace with an account. Remote dialogs use the association only when **Linked GitHub account** is explicitly selected.
- Search the current repository page, or look up a repository by its owner and name. Filter by installation and visibility and sort the loaded page. Refresh and page manually. An accessible public project can be cloned via HTTPS without a token. Private projects require an authorized account or SSH identity.
- Clone using the native destination picker, then authorize the resulting workspace through the usual trust dialog. A confirmed fork clone adds the verified parent as `upstream`. Use the Remote menu to fetch it or **Sync fork** to request GitHub's upstream sync on the selected branch.
- Publish with private visibility selected by default. Choose the target account, owner, repository name and local remote name. The Git Engine respects an existing remote when both directions match the reviewed endpoint. When API creation succeeds but installation access or Git push fails, use **Resume publication** or **Publish to this repository** after checking access. The application retains a created repository and retries only the explicitly reviewed Git operation.
- Fetch PR pages for a selected repository, filter the loaded page, and view the author, source, base and draft status. Create a regular or draft PR with explicit source and destination fields. Checking out a PR requires its matching local repository, a clean worktree and a free local branch name. The engine downloads the reviewed commit and switches without force. Use the browser for review and merge.
- Query checks and workflow runs explicitly. Results show the queried repository and commit. A pending status is separate from an empty result or a failed request. The browser actions open repositories, branches, commits, PRs and Actions on GitHub.com.

## API surface

| Service | Endpoints |
| --- | --- |
| OAuth | `POST /login/device/code`, `POST /login/oauth/access_token` |
| Accounts | `GET /user`, `GET /user/installations` |
| Repositories | `GET /user/repos`, `GET /user/installations/{id}/repositories`, `GET /repos/{owner}/{repo}` |
| Create and fork | `POST /user/repos`, `POST /orgs/{org}/repos`, `POST /repos/{owner}/{repo}/forks`, `POST /repos/{owner}/{repo}/merge-upstream` |
| Pull requests | `GET/POST /repos/{owner}/{repo}/pulls`, `GET /repos/{owner}/{repo}/pulls/{number}` |
| Checks | `GET /repos/{owner}/{repo}/commits/{sha}/check-runs`, `GET /repos/{owner}/{repo}/commits/{sha}/status`, `GET /repos/{owner}/{repo}/actions/runs?head_sha={sha}` |
| Avatar | Public image from `avatars.githubusercontent.com` after explicit account refresh, without authentication |

API responses are limited to 4 MiB. A single background worker performs requests with timeouts and cancellation between requests. A started HTTPS call may continue until its deadline. Device Flow cancellation also interrupts the wait between polls. Idempotent reads can retry once for transient gateway errors. Writes are never retried automatically. For a lost response, inspect the remote resource before retrying. Page navigation reconstructs the request on the fixed API origin. Checks and installation queries have bounded page counts.

## Validation

```bash
cargo test --workspace --locked
cargo test --locked -p gitrust-providers
cargo test --locked -p gitrust-git --test inspection remotes::
```

Standard tests use loopback fixtures. The ignored Secret Service integration test requires an isolated D-Bus session and a disposable keyring. Real Device Flow and resource creation require the maintainer's registered GitHub App and approval to use disposable repositories. Cross-platform credential stores and graphics validation are scheduled for Stage 08.

[Authentication](GITHUB_AUTHENTICATION.md) · [Setup](GITHUB_APP_SETUP.md) · [ADR-0012](adr/0012-github-integration.md)
