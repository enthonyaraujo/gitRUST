# Architecture Decision Records (ADRs)

This directory records significant architectural decisions made in **gitRUST**, capturing context, evaluated alternatives, rationale, and trade-offs using the MADR format.

## Index

| ADR | Title | Status | Stage |
| --- | --- | --- | --- |
| [ADR-0001](0001-rust-as-primary-language.md) | Use Rust as the Sole Application Language | Accepted | 00 |
| [ADR-0002](0002-egui-and-eframe-ui-framework.md) | Adopt `egui`, `eframe`, and `wgpu` for Native Desktop UI | Accepted | 00 |
| [ADR-0003](0003-git-cli-initial-integration-strategy.md) | Use Git CLI Initially Behind an Abstract Engine Boundary | Accepted | 00 |
| [ADR-0004](0004-cargo-workspace-organization.md) | Organize Codebase as a Multi-Crate Acyclic Cargo Workspace | Accepted | 00 |
| [ADR-0005](0005-cross-platform-strategy.md) | Linux-First Cross-Platform Strategy (Wayland, X11, Windows, macOS) | Accepted | 00 |

## Lifecycle States

- **Proposed:** Under review before implementation.
- **Accepted:** Approved and active architectural constraint.
- **Deprecated:** No longer applicable due to scope changes.
- **Superseded:** Replaced by a newer ADR (the original ADR remains immutable except for its status link).
