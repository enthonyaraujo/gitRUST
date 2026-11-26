# Architecture Overview

> **Document Scope:** Reflects the Stage 00 foundation and the Stage 01 native shell. [UI architecture](UI_ARCHITECTURE.md) details the presentation layer. Later roadmap stages describe the target architecture. Planned subsystems are explicitly labeled as not yet implemented.

---

## 1. Architectural Goals

1. **Native Desktop Performance & Predictability:** Written 100% in Rust with immediate-mode UI (`egui`/`eframe`) over `wgpu`, avoiding embedded browsers or JavaScript runtimes.
2. **Strict Separation of Concerns:** Acyclic Cargo Workspace crates separating pure application state (`gitrust-core`) from UI rendering (`gitrust-ui`), process execution (`gitrust-git`), persistence (`gitrust-storage`), and remote provider integrations (`gitrust-providers`).
3. **Linux First-Class Portability:** Seamless execution on Linux (Wayland and X11), Windows, and macOS, respecting integrated vs. discrete GPU power profiles.
4. **Security by Construction:** Workspace-wide `#![forbid(unsafe_code)]`, shell-free subprocess execution, non-UTF-8 path fidelity, untrusted repository isolation, and OS-native secret storage.

---

## 2. Cargo Workspace Dependency Graph

All workspace crates form a strict **Directed Acyclic Graph (DAG)**. Circular dependencies and lateral coupling between infrastructure crates are prohibited.

```mermaid
flowchart TD
    app["gitrust-app (binary: gitrust)"]
    ui["gitrust-ui (egui views & theme)"]
    git["gitrust-git (Git engine boundary — Stage 02)"]
    storage["gitrust-storage (local config & keychain policy — Stage 03)"]
    providers["gitrust-providers (GitHub/GitLab boundary — Stage 08)"]
    core["gitrust-core (pure state, models & events)"]

    app --> ui
    app --> core
    app -.->|"dev-dependencies (Stage 00 integration tests)"| git
    app -.->|"dev-dependencies (Stage 00 integration tests)"| storage
    app -.->|"dev-dependencies (Stage 00 integration tests)"| providers
    ui --> core
    git --> core
    storage --> core
    providers --> core
```

### Crate Responsibilities

| Crate | Status | Responsibility | External Dependencies |
| --- | --- | --- | --- |
| `gitrust-core` | **Implemented (Foundation)** | Pure domain models, metadata, runtime/host diagnostics, `AppState`, and deterministic `AppEvent` reducer. | None (`std` only) |
| `gitrust-ui` | **Implemented** | Native `egui` shell and design system decoupled from OS windowing and headlessly testable via `egui::Context`. | `egui` |
| `gitrust-app` | **Implemented (Foundation)** | Native binary (`gitrust`), CLI flag handling, `eframe` + `wgpu` initialization, Wayland/X11 integration, and clean shutdown lifecycle. | `eframe`, `egui`, `tracing`, `tracing-subscriber` |
| `gitrust-git` | **Stage 02 implementation** | Typed, local Git discovery and read-only inspection, asynchronous task scheduling and byte-oriented parsers. | `gitrust-core`, `process-wrap` |
| `gitrust-storage` | **Boundary Only (Planned Stage 03)** | Storage security baseline prohibiting plaintext credentials and telemetry by default. Persistent config and keychain adapters scheduled for **Stage 03 / Stage 08**. | None (`std` only) |
| `gitrust-providers` | **Boundary Only (Planned Stage 08)** | Strict TLS policy contract. GitHub, GitLab, and hosting API clients scheduled for **Stage 08**. | None (`std` only) |

### Documented Adjustments to the Reference Directory Layout

Per Stage 00 engineering guidelines, two intentional refinements were made relative to the reference directory skeleton:

1. **Workspace Integration Tests (`crates/gitrust-app/tests/` instead of root `tests/`):** Because the root `Cargo.toml` is a **virtual Cargo manifest** (`[workspace]` without a root `[package]`), Cargo only discovers and executes integration tests placed inside member crates (`crates/*/tests/`). Placing `workspace_integration.rs` inside `crates/gitrust-app/tests/` ensures `cargo test --workspace` compiles and executes cross-crate and binary CLI integration tests automatically.
2. **Asset and Benchmark Directories (`assets/` and `benches/`).** Empty directories with placeholder files were omitted in Stage 00. Stage 01 introduces licensed typography in `crates/gitrust-ui/assets` and first-party painted icons. Stage 02 adds an opt-in benchmark harness in `crates/gitrust-git/benches`. Stage 09 can expand measurements based on workloads observed in practice.
3. **Supply-Chain Policy (`deny.toml`):** Added at the workspace root to enforce MIT-compatible dependency licensing and vulnerability checks in CI.

---

## 3. Desktop Shell and Rendering Pipeline (Implemented)

- **Windowing:** `eframe` delegates native OS window management to `winit`, supporting Wayland (`libwayland-client`) and X11 (`x11` / `xcb`) on Linux, Win32 on Windows, and AppKit on macOS.
- **Graphics Backend (`wgpu`):** Selected as the primary backend (`Vulkan` on Linux/Windows, `DirectX 12` on Windows, `Metal` on macOS, with `OpenGL ES` fallback inside `wgpu`).
- **Hybrid GPU Power Policy:** `WgpuSetupCreateNew` defaults to `wgpu::PowerPreference::LowPower` (while honoring `WGPU_POWER_PREF=high|low|none`), preventing a desktop productivity tool from unnecessarily selecting power-hungry discrete GPUs on Optimus/hybrid laptops while still supporting dedicated GPUs seamlessly.
- **Verified Session Diagnostics:** Active windowing protocol (`Wayland`, `X11`, `Win32`, `AppKit`) is verified directly from the runtime `RawDisplayHandle` rather than inferred from environment variables.
- **Event-Driven Repaint:** Views render in response to OS/input events (`ControlFlow::Wait`) and never call `Context::request_repaint()` unconditionally, keeping idle CPU consumption near `0%`.

---

## 4. Git Engine Architecture (Stage 02)

System Git supplies the initial implementation behind a typed `gitrust-git` boundary. [ADR-0003](adr/0003-git-cli-initial-integration-strategy.md) records the adapter choice. [ADR-0007](adr/0007-local-git-inspection.md) defines the narrower local-read trust policy.

Discovery and authorization are separate. Allowlisted requests run in a bounded worker pool and return typed models through a completion channel. The engine constructs every command, using the absolute Git executable, discrete native arguments, literal file selectors and a cleared subprocess environment. Machine formats preserve bytes for Git paths and commit content. The engine keeps subprocess supervision and decoding independent of presentation.

The canonical common Git directory is the scheduler identity for a working tree and its linked worktrees. Read-only operations may coexist. Reserved write classes require exclusive access with writer ordering. The UI can use completion notifications when the repository and changes stages connect the engine. [Git Engine contract](GIT_ENGINE.md)
