# ADR-0007 — Local Git inspection with an explicit metadata trust boundary

## Status

Accepted in Stage 02. [Implementation PR](https://github.com/enthonyaraujo/gitRUST/pull/4)

## Context

[ADR-0003](0003-git-cli-initial-integration-strategy.md) chose system Git as the initial adapter. Even Git read operations can interpret local configuration, run a content filter or lazily fetch missing objects. A safe public interface must distinguish candidate discovery from permission to inspect. The executor also needs bounded cancellation across operating systems.

## Decision

1. Expose filesystem discovery and typed, local read requests. Require a caller-issued trust capability and preflight the repository layout and allowlisted configuration without applying includes. Reject unsafe or unsupported metadata and retain Git's own ownership checks.
2. Require Git 2.45.0 because it introduced `--no-lazy-fetch`. Apply that flag alongside an empty Git transport allowlist. Disable global/system configuration, optional writes and other external extension points. Recommend a patched host Git. [Git 2.45 release notes](https://github.com/git/git/blob/v2.45.0/Documentation/RelNotes/2.45.0.txt)
3. Use `std::process`, dedicated workers and bounded capture. Depend on `process-wrap` with only the synchronous Unix process-group and Windows Job Object features. The crate supplies safe wrappers for platform-specific termination APIs. Keep this behavior outside the GUI and leave workspace `unsafe_code = "forbid"` intact. [process-wrap API](https://docs.rs/process-wrap/10.0.2/process_wrap/std/)
4. Schedule by canonical common Git directory. Permit concurrent reads, serialize future writes and enforce ordering against waiting writers. Make write cancellation an operation-specific decision in the stages that introduce writes.

## Alternatives considered

- **Option flags alone.** Rejected because they do not neutralize all local config and worktree filter behavior.
- **Globally trusting local configuration.** Rejected because user authorization to inspect does not grant permission to execute an extension.
- **A general async runtime.** Deferred. A bounded, synchronous worker pool handles these short CLI tasks and gives the consumer a completion channel.
- **Hand-maintained platform termination bindings.** Rejected because portable process-tree management would introduce first-party FFI and maintenance. A pure Rust Git engine remains an option for a later evaluation.

## Consequences

Preflight adds filesystem work and small Git processes, and some conventional extensions are refused with explicit errors. The trusted binary, stable local metadata and filesystem remain part of the trust boundary. There is no cross-process locking guarantee. The allowlist requires separate review before any later stage enables mutations, broader config compatibility or transports.
