# ADR-0001: Use Rust as the Sole Application Language

## Status

Accepted

## Context

`gitRUST` aims to provide a modern, lightweight, secure, and cross-platform open-source desktop Git client inspired functionally by GitHub Desktop, with Linux as a first-class platform alongside Windows and macOS.

Many existing desktop Git clients rely on Electron/WebView stacks (high idle RAM and large distribution bundles) or platform-locked native toolkits (divergent codebases or C/C++ memory-safety exposure when parsing untrusted repository data).

## Decision Drivers

- Memory safety without garbage-collection pauses or runtime bloat.
- Strong static type system for modeling repository states, non-UTF-8 OS paths (`OsString`/`PathBuf`), and explicit error handling (`Result`).
- Single native codebase compiling to Linux, Windows, and macOS binaries without bundling a browser engine or language VM.
- Compile-time enforcement of safety invariants (`unsafe_code = "forbid"`).
- Mature ecosystem for native graphics (`wgpu`, `egui`) and future pure-Rust Git tooling (`gitoxide`).

## Considered Options

### Option 1: 100% Rust (Selected)
- **Pros:** Compile-time memory and thread safety, predictable low resource footprint, native OS process/path primitives, unified Cargo build/test/lint toolchain across Linux/Windows/macOS, strong supply-chain auditing (`cargo-deny`).
- **Cons:** Steeper compile times than interpreted languages; requires deliberate ownership modeling across async tasks and UI state.

### Option 2: Electron / Tauri + TypeScript/React
- **Pros:** Rapid UI layout prototyping with HTML/CSS.
- **Cons:** Violates core project requirements; introduces WebView/Chromium memory overhead, multi-ecosystem supply-chain surface (`npm` + native), and Linux WebKitGTK/Wayland quirks (in Tauri) or high RAM footprint (in Electron).

### Option 3: C++ with Qt or GTK
- **Pros:** Mature retained-mode desktop widgets.
- **Cons:** Manual memory-safety risks when parsing untrusted Git output/diffs; complex cross-platform C++ dependency and build-system maintenance across MSVC, Clang, and GCC.

## Decision

Develop `gitRUST` **entirely in Rust** (Edition 2024, validated MSRV `1.97.1` above upstream crate floor `1.95`), prohibiting web stacks (Electron, Tauri, HTML/CSS/JS, WebView) and enforcing `unsafe_code = "forbid"` across all workspace crates.

## Consequences

### Positive
- Eliminates entire classes of memory corruption and data-race bugs in first-party code.
- Produces a single, self-contained native executable with fast startup and minimal idle memory overhead.
- Unifies formatting (`rustfmt`), static analysis (`clippy`), testing (`cargo test`), and license auditing (`cargo-deny`) under one toolchain.

### Negative / Risks
- Clean builds of GPU crates (`wgpu`, `naga`) take longer in CI, mitigated with `Swatinem/rust-cache` and modular workspace crates so edits to `gitrust-core` or `gitrust-git` recompile rapidly.

## Related Decisions

- [ADR-0002](0002-egui-and-eframe-ui-framework.md)
- [ADR-0004](0004-cargo-workspace-organization.md)
