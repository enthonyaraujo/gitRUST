# Configure the gitRUST OAuth App

The maintainer registers a GitHub.com OAuth App and provisions its credentials. Development builds can use a dedicated app and disposable repositories.

1. Open [GitHub Developer settings](https://github.com/settings/developers) and choose **OAuth Apps**, then **New OAuth App**.
2. Set **Application name** to `gitRUST`, **Homepage URL** to `https://github.com/enthonyaraujo/gitRUST`, and **Application description** to `Native Git desktop client for GitHub repository workflows`.
3. Set **Authorization callback URL** to `gitrust://oauth/callback` with wildcard matching disabled. A non-HTTP redirect is supported by the GitHub OAuth web flow. Use the exact URI in the registration and check the round trip on the registered app before publishing a build. [Redirect URL rules](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps#redirect-urls)
4. Copy the public **Client ID** and generate a **Client Secret**. Keep the secret in a credential manager.
5. With a running Linux Secret Service session, provision the developer secret using the command below. The command prompts for the secret through the terminal. Use the same public identifier when launching gitRUST. Run the executable from a stable location so the browser can activate it later.

```sh
export GITRUST_GITHUB_OAUTH_CLIENT_ID='your-public-client-id'
secret-tool store --label='gitRUST OAuth App (development)' \
  application gitRUST provider 'github.com OAuth client' \
  client_id "$GITRUST_GITHUB_OAUTH_CLIENT_ID"
cargo run --locked --release -p gitrust-app
```

Click **Sign in with GitHub** in the welcome screen or the GitHub accounts screen. gitRUST registers its Linux desktop handler, then opens the default browser. Select an account, authorize access, and approve the browser prompt to open gitRUST. In the application, select **Refresh account and access** to list repositories. After restarting, use **Load saved accounts** to allow the keyring to unlock when necessary.

Use the build environment variable `GITRUST_GITHUB_OAUTH_CLIENT_ID` to bake the public ID into a distribution build. The developer credential provisioning above is separate. Public distribution of the exchange secret requires an explicit maintainer decision about the public-client model and an approved provisioning mechanism. [ADR-0013](adr/0013-native-browser-oauth.md)

## Scope and token lifecycle

| Scope | Usage |
| --- | --- |
| `repo` | Public and private repositories available to the account, including Git HTTPS, publication, forks, pull requests, and CI reads. It is broader than the granular repository permissions of a GitHub App. Organization app restrictions and SSO still apply. |
| `read:user` | Account identity and profile. |
| `offline_access` | Requests expiring access tokens and refresh tokens. |

The app leaves email and workflow-file write permissions out of the request. Identity uses the numeric GitHub ID. Workflow runs can be read with repository access, while pushes involving workflow-file edits can require the separate `workflow` scope. gitRUST requests no organization administration or repository deletion scope. Organization repositories appear in the authenticated repository list when permitted. [GitHub OAuth scopes](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/scopes-for-oauth-apps)

To rotate the client secret, provision the replacement in the same keyring entry, verify login and refresh, and then revoke the previous secret in the OAuth App settings. Refresh preserves the account and public client ID and stores the rotated pair together. GitHub can issue a token without an expiration or refresh token. For a revoked or expired session, reconnect in the browser. To switch to a different OAuth App for testing, use a separate XDG profile. This preserves associations under the old client ID.

Disconnect removes the local session and its workspace associations. Revoke the authorization or grant organization access in [GitHub authorized applications](https://github.com/settings/applications).
