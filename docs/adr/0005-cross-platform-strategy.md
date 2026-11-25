# ADR-0005: Linux-First Cross-Platform Strategy (Wayland, X11, Windows, macOS)

## Status

Accepted

## Context

While GitHub Desktop officially targets Windows and macOS, Linux developers often rely on community forks, Electron wrappers, or terminal-only tools. `gitRUST` prioritizes **Linux as a first-class platform** (across both modern Wayland compositors and X11/XWayland sessions) while maintaining clean architectural portability and CI verification for **Windows** and **macOS**.

## Decision Drivers

- Native execution on both Wayland and X11 on Linux without requiring XWayland when Wayland is available, and without breaking pure X11 environments.
- Proper window metadata (`app_id = "gitrust"`) for Linux desktop environments (GNOME, KDE Plasma, sway/wlroots).
- Predictable behavior across integrated GPUs (Intel/AMD/Apple Silicon), discrete GPUs (NVIDIA/AMD), and software fallbacks (`llvmpipe`).
- OS-idiomatic paths, process handling, non-UTF-8 filename support, and credential storage in future stages.
- Continuous multi-OS build and test verification in GitHub Actions from Stage 00.

## Considered Options

### Option 1: Unified Native Stack (`winit` + `eframe` + `wgpu`) with Multi-OS CI Matrix (Selected)
- **Pros:** Single Rust codebase supporting Wayland + X11 simultaneously in the same Linux binary, DirectX 12/Vulkan on Windows, and Metal on macOS; CI validates compilation and unit/integration tests on Ubuntu, Windows, and macOS on every PR.
- **Cons:** Headless CI runners do not run interactive Wayland/macOS/Windows compositors by default, so interactive window lifecycle checks must be explicitly separated from headless `cargo test` runs and honest platform validation boundaries must be reported.

### Option 2: Linux-Only Toolkit First, Port Later
- **Pros:** Simpler initial scope.
- **Cons:** Accumulates Unix-only assumptions (`/` path separators, Unix-only process signals, XDG-only paths) that are expensive to untangle later for Windows and macOS.

## Decision

1. **Linux Windowing:** Enable both `wayland` and `x11` features in `eframe` so a single Linux binary runs natively on Wayland when `WAYLAND_DISPLAY` is active and falls back seamlessly to X11 (for example on pure X11 sessions or via `env -u WAYLAND_DISPLAY` under XWayland). Verify the active windowing protocol directly from `RawDisplayHandle` rather than environment heuristics, and set `ViewportBuilder::with_app_id("gitrust")` for desktop shell integration.
2. **GPU Portability:** Use `wgpu` across all three operating systems (Vulkan on Linux, DX12/Vulkan on Windows, Metal on macOS, plus built-in OpenGL ES fallback) with `PowerPreference::LowPower` default (`WGPU_POWER_PREF=high` override) to respect laptop battery and thermal budgets on dual-GPU systems.
3. **Filesystem & Process Portability:** Use `std::path::PathBuf` and `std::ffi::OsString` everywhere; gate Unix-specific raw byte path tests with `#[cfg(unix)]` while keeping cross-platform APIs portable across Windows WTF-8 / UTF-16 (`OsString`) semantics; suppress extra console windows on Windows release builds via `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`.
4. **Continuous Integration:** Validate Linux (`ubuntu-latest`), Windows (`windows-latest`), and macOS (`macos-latest`) in `.github/workflows/ci.yml` on every push to `main` and pull request, with minimal `contents: read` permissions.

## Consequences

### Positive
- Cross-platform compile/test regressions on Windows or macOS are caught immediately in CI even when primary development occurs on Linux.
- Both Wayland and X11 can be verified directly on dual-capable Linux workstations.

### Negative / Risks
- Local interactive GUI verification is bounded by the developer's available host OS (e.g., Linux Wayland + X11 validated locally; Windows and macOS runtime GUI validation requires native hosts or CI runners once remote CI executes). These validation boundaries must always be documented transparently.

## Related Decisions

- [ADR-0002](0002-egui-and-eframe-ui-framework.md)
- [ADR-0003](0003-git-cli-initial-integration-strategy.md)
