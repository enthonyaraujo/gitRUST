# ADR-0012 — Native GitHub App integration

**Status** Proposed

**Stage** 07

## Decision

Use GitHub App Device Flow from a GitHub.com-specific native provider client. The provider owns REST, pagination, OAuth, session refresh and wire decoding. `gitrust-storage` owns OS vault access and per-workspace account associations. `gitrust-app` owns the bounded provider worker and binds the effects of an explicit GitHub action to the reviewed local workspace. `gitrust-ui` consumes typed, credential-free snapshots.

Reuse the Git Engine for HTTPS/SSH clone, remote configuration, fetch, pull, push and checkout. A provider token is restricted to the exact authorized HTTPS repository URL. Publication starts by creating a remote repository after confirmation, then verifies authorization before any local remote or Git push. Retry an incomplete publication against the existing repository.

## Alternatives and consequences

A WebView or a client-secret exchange would add a browser runtime or private credential to the desktop distribution. GitHub App Device Flow uses a public Client ID and can refresh user access tokens directly. The fixed transport and repository boundary make it possible to add independently scoped clients for other providers later. The maintainer owns app registration and live validation. An unavailable Linux vault yields a temporary session; native Windows and macOS credential adapters are Stage 08 work.

[Setup](../GITHUB_APP_SETUP.md) · [Authentication](../GITHUB_AUTHENTICATION.md)
