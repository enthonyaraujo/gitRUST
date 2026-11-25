# Changelog

All notable changes to **gitRUST** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Stage 00 — Project Foundation (`v0.1.0-alpha.0`):**
  - Initialized multi-crate Cargo Workspace (`gitrust-app`, `gitrust-core`, `gitrust-ui`, `gitrust-git`, `gitrust-storage`, `gitrust-providers`) with Edition 2024, MSRV `1.95`, and workspace-wide `unsafe_code = "forbid"`.
  - Implemented minimal native desktop shell (`gitrust` executable) using `egui` `0.37` and `eframe` `0.37` over the `wgpu` backend (`Vulkan` / `Metal` / `DirectX 12` / `OpenGL ES`), with zero web/WebView dependencies.
  - Added Linux Wayland and X11 windowing support, hybrid GPU low-power default preference, minimum/initial window geometry constraints, clean shutdown handling, and reactive idle rendering.
  - Added shell-free, non-UTF-8-capable `GitCommandSpec` boundary invariants in `gitrust-git` and security baseline policies in `gitrust-storage` and `gitrust-providers`.
  - Added project governance and technical documentation (`README.md`, `AGENTS.md`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `SECURITY.md`, `LICENSE`, `docs/ARCHITECTURE.md`, `docs/ROADMAP.md`, `docs/BUILD.md`, `docs/SECURITY_ARCHITECTURE.md`, `docs/PERFORMANCE.md`).
  - Recorded initial Architecture Decision Records (`ADR-0001` through `ADR-0005`).
  - Configured multi-platform GitHub Actions CI for Linux, Windows, and macOS (formatting, Clippy, tests, release build, and `cargo-deny` license/advisory verification).
