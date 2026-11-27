# Set up the gitRUST GitHub App

This guide covers the Device Flow option for GitHub App installations. For the main browser sign-in, see [OAuth App setup](GITHUB_OAUTH_SETUP.md).

A maintainer supplies the Client ID of a GitHub App owned by the gitRUST project. The application supports GitHub.com. Use an isolated account or an organization with disposable repositories for the first installation.

1. Open **Settings → Developer settings → GitHub Apps → New GitHub App** while signed in as the intended owner. On an organization, open its settings with permission to register apps.
2. Enter the app name, description and homepage for gitRUST. Use the public project homepage and explain which features require repository access. Leave the OAuth callback and webhook disabled when using only Device Flow.
3. Enable **Device Flow** and leave expiring user access tokens enabled. Keep the app private during maintainer testing. To distribute later, review the public listing and choose **Any account** for installation availability.
4. Set the repository permissions below. Save, then copy the **Client ID** from the app's general settings. The App ID, private key, and client secret are separate.
5. Install the app on the intended personal account and choose specific test repositories. A private app installs only on its owner account. To test another account or organization, make the app public for installations and obtain that account owner's approval. Select only the disposable repositories authorized for the test. Installing the App and authorizing it as a user are separate steps.
6. Open **Settings → GitHub Accounts → GitHub App (Device Flow)** and choose **Sign in to GitHub.com**. Enter the Client ID when asked. For a distribution build, set `GITRUST_GITHUB_CLIENT_ID` in the build environment to the public Client ID of the owned app. Never supply a client secret or private key. gitRUST opens the official authorization page in the system default browser once GitHub supplies the code. Copy the displayed code to authorize there. After pairing, use **Refresh account and access** to view installations and repositories.

| Repository permission | Level | Workflow |
| --- | --- | --- |
| Metadata | Read | Repository selection and permissions |
| Contents | Write | Git HTTPS clone/fetch/push and updating a fork |
| Repository creation | Write | Create a personal or organization repository without broad repository administration |
| Pull requests | Write | List and create pull requests, including drafts |
| Checks | Read | Read check runs |
| Commit statuses | Read | Read legacy commit statuses |
| Actions | Read, optional | Display workflow runs |
| Administration | Write, optional | GitHub's fork endpoint requires this repository permission with Contents read. Enable only on an installation that needs forks |

Organization administration and write access for workflows or secrets are outside this integration. The account must also independently have permission for every operation. A selected-repositories installation can require adding a newly created repository or fork before Git push becomes possible. When that happens, use **Resume publication** after granting the app access.

Changing an app's permissions can require installation owners to approve the update. The first public distribution should use the same owned app identity after review, with appropriate privacy and support information in the listing.

[Registering a GitHub App](https://docs.github.com/en/apps/creating-github-apps/registering-a-github-app/registering-a-github-app) · [GitHub App permissions](https://docs.github.com/en/apps/creating-github-apps/registering-a-github-app/choosing-permissions-for-a-github-app) · [Repository creation endpoints](https://docs.github.com/en/rest/repos/repos) · [Fork permissions](https://docs.github.com/en/rest/repos/forks)
