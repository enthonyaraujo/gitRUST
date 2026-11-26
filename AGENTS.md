# AGENTS.md — Permanent Engineering Instructions for gitRUST

This file defines non-negotiable architectural, security, quality, and workflow rules for human developers and AI coding agents working in this repository.

## 1. Identity and Licensing

- **Official product name:** `gitRUST` (exact casing).
- **Technical package and binary prefix:** `gitrust` (`gitrust-*` workspace crates, `gitrust` binary).
- **Project license:** `MIT` only. Every newly added dependency must use an MIT-compatible permissive license verified against `deny.toml`.
- **Language policy:** All source code, comments, commit messages, pull requests, ADRs, and repository documentation must be written in **English**. Execution reports to Brazilian Portuguese-speaking maintainers may follow the user's requested language in conversation only.

## 2. Non-Negotiable Technical Constraints

- **100% Native Rust:** Never introduce Electron, Tauri, React, TypeScript, JavaScript, HTML, CSS, or any WebView engine into the application.
- **GUI Stack:** Use `egui` and `eframe` exclusively, with `wgpu` as the primary rendering backend. Disable web-specific feature flags (such as `web_screen_reader` or `web-sys` targets) when configuring GUI dependencies.
- **Platform Priority:** Linux (both Wayland and X11) is a first-class target alongside planned compatibility for Windows and macOS. Account for both integrated and discrete GPUs (defaulting to low-power adapter selection unless overridden).
- **Privacy:** Telemetry must remain disabled by default. Never add analytics beacons or unsolicited network requests.
- **Honest Scope:** Never implement mock UI screens, fake Git buttons, or stubbed workflows presented as working features. Document planned capabilities accurately according to `docs/ROADMAP.md`.

## 3. Architecture and Workspace Boundaries

Maintain a strict Directed Acyclic Graph (DAG) across Cargo workspace crates:

- `gitrust-core` -> zero external dependencies where practical; pure domain state, models, events, and rules. Must never depend on `egui`, `eframe`, OS keychains, or subprocess execution.
- `gitrust-git` -> depends on `gitrust-core` (and approved Git/async crates when implemented in Stage 02). Must never depend on `gitrust-ui`, `gitrust-storage`, or `gitrust-providers`.
- `gitrust-storage` -> depends on `gitrust-core`. Handles local settings and OS credential store boundaries.
- `gitrust-providers` -> depends on `gitrust-core`. Handles the GitHub API (Stage 07) and future hosting providers (Stage 10).
- `gitrust-ui` -> depends on `egui` and `gitrust-core`. Keep decoupled from `eframe` bootstrap so views remain headlessly testable via `egui::Context`.
- `gitrust-app` -> top-level binary wiring crates together with `eframe` and `wgpu`.

Never introduce circular dependencies or bypass `gitrust-core` event contracts.

## 4. Security Baseline

- **Unsafe Code:** `unsafe_code = "forbid"` is enforced at the workspace root. Do not weaken or override this lint without an approved ADR and documented soundness proof.
- **Subprocess Execution:** Never invoke Git or any external program through a shell (`sh -c`, `cmd /C`, `powershell`). Always invoke executables directly with discrete `OsStr`/`OsString` arguments, explicit `--` option terminators before paths/revisions, and sanitized environment variables.
- **Non-UTF-8 Paths:** Use `Path`, `PathBuf`, `OsStr`, and `OsString` across filesystem and Git boundaries. Never silently drop or panic on valid non-UTF-8 OS paths.
- **Untrusted Repositories:** Treat cloned or opened repositories as untrusted input. Guard against malicious `.git/config` directives (`core.fsmonitor`, `core.sshCommand`, `core.pager`, filter drivers) and unexpected hook execution.
- **Secrets & TLS:** Never store tokens or passwords in plaintext files, never log credentials or environment dumps, and never allow disabling TLS certificate verification.
- **Destructive Actions:** Operations that can discard uncommitted work or rewrite history must require explicit user confirmation and never run against real personal repositories in automated tests.
- **Dependencies:** Prefer the Rust standard library before adding third-party crates. Justify every new dependency for maintenance health, transitive footprint, advisories, and license compatibility.

## 5. Performance Discipline

- **Reactive Rendering:** Never call `egui::Context::request_repaint()` unconditionally in idle frames. Keep idle CPU near `0%`.
- **Non-Blocking UI:** Any future filesystem scan, Git subprocess, or network I/O must run off the UI thread with bounded channels and safe cancellation.
- **No Premature Optimization:** Prioritize correctness, observability, and clean diagnostics before micro-optimizations. Validate performance changes using the methodology in `docs/PERFORMANCE.md`.

## 6. Mandatory Validation Gate

Before considering any code change complete, execute and verify:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
```

When modifying UI or windowing behavior, launch the compiled binary on the available desktop environment (testing both Wayland and X11 on Linux when available) and report any platform validation limits honestly. Never claim multi-OS test completion when tests ran on a single OS.

## 7. Git Workflow Rules

- Do not automatically commit, push, merge, tag, create releases, or mutate remotes unless explicitly instructed by the maintainer.
- Never perform destructive Git operations (`reset --hard`, `clean -fd`, force pushes) without explicit authorization.
