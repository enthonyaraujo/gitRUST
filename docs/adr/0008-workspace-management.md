# ADR-0008 — Session-authorized workspaces and local provisioning

## Status

Accepted. Stage 03 passed functional validation on Linux and was integrated into `develop` at `dc9a6eb`. Desktop validation across all supported platforms is scheduled for Stage 07. [PR #6](https://github.com/enthonyaraujo/gitRUST/pull/6) · [Merge CI](https://github.com/enthonyaraujo/gitRUST/actions/runs/45848058298)

## Decision

The core crate defines workspace actions and snapshots. The native view renders snapshots and sends bounded action messages. The application coordinator owns the engine, profile, session authorization map, and active watcher. Git task completions and progress wake the UI when they yield a new snapshot. Directory selection uses the native dialog backend. Repositories can be identified and displayed for approval before inspection.

Persist only preferences and paths in bounded, versioned binaries and replace each file atomically in its own directory. Paths remain in native encoding. Resolve platform directories through a platform adapter. Keep authorization in memory and use canonical workspace paths for recents. Linked worktrees may share a Git scheduling key without sharing a catalog entry.

Init and public HTTPS clone prepare a private sibling and publish atomically without replacement. Use a dedicated clone environment and command. Read-only operations retain the Stage 02 transport deny-all policy. Clone disables redirects, credentials, external templates, hooks, and submodule recursion, and forces TLS verification. Validate local metadata before materializing the tree with restrictive filename and symlink options. Authorize the published workspace separately.

## Alternatives

Periodic status polling would detect changes without platform watchers and wake the application continuously. The native watcher and event-driven fallback preserve idle behavior. Libraries for native dialog, notification and platform directories reduce first-party OS bindings. The bounded binary format preserves non-UTF-8 paths without a third-party serialization dependency. Checkout directly into the destination would make partial failures visible and compete with user files. Private staging with no-replace publication keeps those failure modes separate.

## Consequences

Redirection, embedded credentials, shallow or partial clone, external filters, LFS materialization and automatic submodule checkout remain outside the supported policy. Local paths, the selected Git installation and its certificate roots are trusted prerequisites. Deadlines are cooperative around filesystem operations. Authorization is required after restart, and a new catalog record becomes durable only after successful profile replacement.
