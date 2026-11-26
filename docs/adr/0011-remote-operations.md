# ADR-0011 — Explicit network operations and authentication

**Status** Proposed

**Stage** 06

## Context

gitRUST already controls Git metadata trust, subprocesses and writes by common Git directory. Remote support needs an explicit transport boundary that preserves those invariants and works with multiple hosting services.

## Decision

Keep the Git CLI behind typed requests with a separate environment policy for an explicitly selected HTTPS or SSH endpoint. Clear inherited Git configuration and restrict protocols, arguments, credential providers, and SSH invocation. Validate TLS and SSH host identities. Accept user-approved system credential helpers or one-operation credentials for HTTPS. Delegate private-key operations to system OpenSSH and its agent.

Fetch an explicit branch mapping under the selected remote's namespace. Push a reviewed object ID and a single branch destination. Pull fetches that remote, rechecks the workspace, then integrates a known commit ID under the local write policy. Use the Stage 05 merge path and a restricted non-interactive rebase that checks its own recovery metadata before continuation or abort.

The worker scheduler serializes operations that alter refs or config by common Git directory. The desktop coordinator scopes snapshots, progress, and results to the authorized workspace.

## Alternatives considered

- Passing through the caller's Git, helper and OpenSSH configuration would support a broader set of installations but permit shell-backed helpers, URL rewrites and transport programs that bypass the review boundary.
- Provider APIs or a second Git implementation would expand the dependency and credential footprint of the MVP. The standard transports work independently of a provider API.

## Consequences

A user's custom SSH aliases, proxies or Git helpers require an equivalent supported endpoint and authentication configuration. The app retains no manually entered credentials. Config edits composed of individual Git commands have Git's per-command lock guarantees. An external writer may still race a check or cause a partial config update. A lost server response can leave a push outcome unknown.

[Remote operations](../REMOTE_OPERATIONS.md) · [Authentication](../AUTHENTICATION.md)
