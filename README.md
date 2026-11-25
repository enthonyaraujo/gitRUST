# gitRUST

**gitRUST** is an independent, free, and open-source native desktop Git client written entirely in Rust using [`egui`](https://github.com/emilk/egui) and [`eframe`](https://github.com/emilk/egui/tree/main/crates/eframe), with [`wgpu`](https://github.com/gfx-rs/wgpu) as its primary graphics backend.

Inspired by the approachable workflow of [GitHub Desktop](https://github.com/desktop/desktop), gitRUST is designed from scratch with its own architecture, visual identity, and security model—delivering a lightweight, responsive, cross-platform Git experience with **Linux (Wayland and X11) as a first-class citizen**, alongside planned compatibility for Windows and macOS.

---

## Current Project Status

> **Current Stage:** `Stage 00 — Project Foundation` (`v0.1.0-alpha.0`)

gitRUST is at the foundational stage of development. Only capabilities verified for Stage 00 are currently implemented:

- **Implemented in Stage 00:**
  - Multi-crate Cargo Workspace with acyclic dependency boundaries and `#![forbid(unsafe_code)]` via workspace lints.
  - Native desktop window shell (`gitrust` binary) built with `egui` + `eframe` (`0.37`) and `wgpu` (`30`), with zero web/WebView dependencies.
  - Resizable desktop window with minimum/default size constraints, normal window close handling, and event-driven rendering (no continuous idle repainting).
  - Real-time runtime diagnostics view reporting the active OS session (Wayland/X11/Win32/AppKit), GPU adapter, graphics backend API, and workspace crate boundaries.
  - Architectural documentation, preliminary security model, performance measurement methodology, Architecture Decision Records (ADR-0001 through ADR-0005), and multi-OS GitHub Actions CI.
- **Not Yet Implemented (Planned for Subsequent Stages):**
  - Definitive visual identity and component library (`Stage 01 — Design System`).
  - Functional Git CLI execution engine (`Stage 02 — Git Engine`).
  - Repository discovery, cloning, and local persistence (`Stage 03 — Repository Management`).
  - Working-tree diffing, staging, commits, branches, history, remotes, and provider integrations (`Stages 04–08`).

See [`docs/ROADMAP.md`](docs/ROADMAP.md) for the complete stage breakdown toward `v0.1.0`.

---

## Core Principles

- **100% Native Rust:** No Electron, Tauri, React, TypeScript, JavaScript, HTML, CSS, or WebView components.
- **Linux First-Class:** Native Wayland and X11 support out of the box, with careful handling of integrated and discrete GPUs.
- **Privacy by Default:** Zero telemetry by default and no hidden background network calls.
- **Security First:** Untrusted repositories are treated defensively; shell command interpolation, plaintext credential files, and TLS verification bypasses are strictly prohibited.
- **Maintainability over Haste:** Clear crate boundaries, explicit error handling, reproducible benchmarks, and conservative dependency selection.

---

## Workspace Overview

```text
gitRUST/
├── crates/
│   ├── gitrust-app/        # Native binary (`gitrust`), eframe/wgpu bootstrap, CLI flags
│   ├── gitrust-core/       # Pure-Rust state, metadata, diagnostics, and event model
│   ├── gitrust-ui/         # Provisional egui foundation view (Design System in Stage 01)
│   ├── gitrust-git/        # Shell-free Git command spec boundary (Stage 02 target)
│   ├── gitrust-storage/    # Storage & OS keychain policy boundary (Stage 03 target)
│   └── gitrust-providers/  # Hosting provider security boundary (Stage 08 target)
├── docs/                   # Architecture, roadmap, build, security, performance, and ADRs
├── .github/                # Multi-platform CI workflows and issue templates
├── Cargo.toml              # Workspace manifest, shared dependencies, and strict lints
└── deny.toml               # License and advisory policy for cargo-deny
```

---

## Building and Running

### Prerequisites

- **Rust:** Stable toolchain (`rustc >= 1.97.1` validated MSRV; upstream dependency floor `1.95.0`) with `rustfmt` and `clippy`.
- **Git:** System `git` installation (required from Stage 02 onward for repository operations).
- **Platform Libraries:**
  - **Linux:** Wayland and/or X11 client libraries, `libxkbcommon`, `fontconfig`, and a Vulkan (recommended) or OpenGL ES driver.
  - **Windows:** Microsoft C++ Build Tools (MSVC) and Windows 10/11 SDK (DirectX 12 or Vulkan).
  - **macOS:** Xcode Command Line Tools (Metal).

Detailed per-distribution package commands are documented in [`docs/BUILD.md`](docs/BUILD.md).

### Quick Start

```bash
# Check formatting, types, lints, and tests
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Build optimized release binary
cargo build --workspace --release

# Launch the desktop application
cargo run -p gitrust-app --release
```

---

## Documentation Index

- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — System architecture, crate DAG, and Git engine design.
- [`docs/ROADMAP.md`](docs/ROADMAP.md) — Official development stages (`00` through `10`) and `v0.1.0` scope.
- [`docs/BUILD.md`](docs/BUILD.md) — Platform dependencies, compilation guide, and runtime environment variables.
- [`docs/SECURITY_ARCHITECTURE.md`](docs/SECURITY_ARCHITECTURE.md) — Threat model, untrusted repository defenses, and secret policies.
- [`docs/PERFORMANCE.md`](docs/PERFORMANCE.md) — Exploratory resource targets and reproducible benchmark methodology.
- [`docs/adr/README.md`](docs/adr/README.md) — Architecture Decision Records (`ADR-0001` to `ADR-0005`).
- [`CONTRIBUTING.md`](CONTRIBUTING.md) — Contribution guidelines and quality gates.
- [`SECURITY.md`](SECURITY.md) — Vulnerability reporting and disclosure policy.
- [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md) — Third-party dependency elections and embedded font attributions.

---

## License

Licensed under the [MIT License](LICENSE).
