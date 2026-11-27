# ADR-0013 — Native GitHub OAuth browser authorization

**Status** Proposed for review. Public distribution of an OAuth Client Secret requires a separate maintainer decision.

## Decision

Use Authorization Code with S256 PKCE and an external browser for a gitRUST-owned GitHub OAuth App. Keep GitHub App Device Flow as a separately selected method for existing accounts and installations. The provider binds each browser session to the OAuth Client ID and authenticated GitHub account. The Linux vault namespaces the two methods and reuses the existing credential adapter. Workspace associations use a versioned record that migrates old numeric bindings to Device Flow and retains the original public Client ID.

The app requests `repo`, `read:user` and `offline_access`. OAuth repository discovery uses `/user/repos`. Organization restrictions and SSO continue to apply. Organization publication and forks use owner names that the user enters and GitHub validates. The Git transport reuses the existing exact-repository HTTPS boundary with method-specific Basic authentication.

The desktop and its registered OAuth App form a public client. GitHub requires a Client Secret for authorization-code exchange and for refresh of tokens issued through that flow. In development, the maintainer provisions the secret externally in Secret Service. A common secret shipped in a public desktop client can be recovered by anyone who has the client. PKCE protects the individual authorization exchange, while the provisioned secret satisfies GitHub's endpoint contract. No secret is embedded by this implementation. A server-side exchange could keep the client secret confidential, with additional deployment, availability and privacy implications. This stage excludes a new service.

## Linux callback and IPC

Use `gitrust://oauth/callback`, registered with an exact match in the OAuth App. gitRUST writes an XDG Desktop Entry with a quoted absolute executable and a separate `%u` URL argument. The activation process forwards the URL over a Unix domain socket to the instance holding the runtime lock, then exits. It does not create a window. The initial GUI takes the lock before creating a window. The socket is confined to a private XDG runtime directory, and both ends check kernel peer UIDs. The lock guards stale socket removal and is never unlinked while in use.

The provider checks the exact URI, one state and exactly one successful code or denial. A pending authorization expires after five minutes. The instance claims a matching callback at most once, then the provider exchanges it using the in-memory verifier. Cancellation wakes the worker directly. A started HTTPS call remains subject to its network deadline. Callback receipt requests native window focus, which the compositor may decline. Wayland and XWayland share the IPC path.

A custom URI scheme is a local routing mechanism. Another handler can claim the scheme, and the callback URL temporarily appears in the activation process arguments on Linux. The authorization code is short-lived and cannot be exchanged without the verifier. Callback data, state and verifier are omitted from logs. The session and refresh tokens stay out of process arguments. Same-user compromise can affect the local browser, process memory and desktop registration and is beyond the IPC isolation boundary.

## Validation

Automated tests use local fixtures, including callback parsing, one-time delivery, client and account isolation, migration, code exchange, rotation, and revocation-related errors. The registered OAuth App and real browser authorization remain prerequisites for production verification. Platform activation on Windows and macOS remains Stage 08 work.

[GitHub OAuth flow](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps) · [RFC 8252](https://www.rfc-editor.org/rfc/rfc8252) · [RFC 7636](https://www.rfc-editor.org/rfc/rfc7636) · [Desktop Entry Specification](https://specifications.freedesktop.org/desktop-entry-spec/latest/)
