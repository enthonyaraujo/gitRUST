# ADR-0004: Organize Codebase as a Multi-Crate Acyclic Cargo Workspace

## Status

Accepted

## Context

Desktop applications frequently degrade into tightly coupled monoliths where UI widgets invoke subprocesses directly, persistence leaks into domain models, or optional remote API dependencies bloat core test compilation. Setting clear module and crate boundaries in Stage 00 prevents architectural drift across Stages 01–10.

## Decision Drivers

- Compile-time enforcement of dependency direction (preventing circular dependencies and UI-to-Git coupling).
- Fast incremental compilation and sub-second headless unit tests in core/domain crates.
- Centralized dependency versions and workspace-wide safety lints (`unsafe_code = "forbid"`).
- Honest separation between active Stage 00 crates and future stage boundaries without adding unused third-party crates prematurely.

## Considered Options

### Option 1: Six-Crate Virtual Cargo Workspace (Selected)
Crates under `crates/`:
- `gitrust-core`: pure domain state, metadata, diagnostics, and events (`std` only in Stage 00).
- `gitrust-ui`: `egui` views and future Stage 01 Design System (`egui` + `gitrust-core`).
- `gitrust-git`: Git engine boundary (`std` + `gitrust-core` in Stage 00; full engine in Stage 02).
- `gitrust-storage`: local persistence and secret-storage policy boundary (Stage 03 target).
- `gitrust-providers`: hosting provider integration boundary (Stage 08 target).
- `gitrust-app`: binary entrypoint (`gitrust`) integrating `eframe`, `wgpu`, logging, and workspace crates.

- **Pros:** Compiler-verified DAG; `gitrust-core`, `gitrust-git`, `gitrust-storage`, and `gitrust-providers` compile and test in milliseconds without touching GPU crates; `gitrust-ui` tests headlessly without `eframe`/`winit`; shared `[workspace.dependencies]` and `[workspace.lints]` eliminate configuration drift.
- **Cons:** Slightly more `Cargo.toml` manifests to maintain than a single crate.

### Option 2: Single Monolithic Crate with Internal Modules
- **Pros:** Fewer `Cargo.toml` files initially.
- **Cons:** Rust module visibility alone does not prevent accidental intra-crate cycles or prevent heavy GUI/network dependencies from slowing down domain unit tests.

## Decision

Adopt the **six-crate virtual Cargo Workspace** (`resolver = "3"`, Edition 2024) with centralized `[workspace.dependencies]` and `[workspace.lints]`.

Additionally:
- Place cross-crate and binary CLI integration tests in `crates/gitrust-app/tests/workspace_integration.rs` because Cargo virtual workspaces execute integration tests from member crates rather than an unattached root `tests/` directory.
- Defer creating `assets/` until Stage 01 (when licensed fonts/icons are added) and `benches/` until Stage 02 (when `gitrust-git` parsers exist to benchmark), avoiding empty placeholder directories.

## Consequences

### Positive
- Architectural layer violations fail at `cargo check` time.
- Zero unnecessary external dependencies in foundation boundary crates.
- Every crate automatically inherits `unsafe_code = "forbid"` and Clippy `pedantic` lints.

### Negative / Risks
- Shared domain types consumed by multiple crates must live in `gitrust-core`; contributors must keep `gitrust-core` lean and free of infrastructure dependencies.

## Related Decisions

- [ADR-0001](0001-rust-as-primary-language.md)
- [ADR-0003](0003-git-cli-initial-integration-strategy.md)
