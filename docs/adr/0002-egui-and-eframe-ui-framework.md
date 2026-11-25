# ADR-0002: Adopt `egui`, `eframe`, and `wgpu` for Native Desktop UI

## Status

Accepted

## Context

`gitRUST` requires a 100% pure-Rust, cross-platform graphical user interface without WebViews, supporting Linux (Wayland and X11), Windows, and macOS on both integrated and dedicated GPUs, while maintaining `< 1%` CPU usage when idle and enabling a cohesive original Design System in Stage 01.

## Decision Drivers

- Pure-Rust native rendering with zero HTML/CSS/WebView dependencies.
- First-class Linux support on both Wayland and X11 via `winit`.
- Modern, safe GPU abstraction supporting Vulkan, DirectX 12, Metal, and OpenGL ES fallback.
- Screen-reader accessibility path (`AccessKit` integration).
- Event-driven idle behavior (no continuous 60 FPS game-loop spinning when idle).
- Permissive MIT/Apache-2.0 licensing.

## Considered Options

### Option 1: `egui` + `eframe` (`0.37`) with `wgpu` Backend (Selected)
- **Pros:** Mature, actively maintained, MIT/Apache-2.0 licensed, built-in `AccessKit` integration, crisp custom widget/theme control for diff views and commit graphs, headless UI testability via `egui::Context::run_ui`, and `wgpu` as default safe Rust GPU layer across Vulkan/Metal/DX12/GLES.
- **Cons:** Immediate-mode paradigm requires discipline to avoid accidental unconditional `request_repaint()` calls and requires virtualized list helpers (`show_rows`) for large histories/diffs.

### Option 2: `egui` + `eframe` with `glow` (OpenGL) as Primary Backend
- **Pros:** Slightly smaller binary footprint than `wgpu`.
- **Cons:** OpenGL is deprecated on macOS, less consistent across modern Wayland compositors and Windows ARM devices, and lacks `wgpu`'s explicit `PowerPreference` control for hybrid iGPU/dGPU laptops.

### Option 3: GTK4-rs / Relm4
- **Pros:** Native Linux GNOME integration.
- **Cons:** Heavy C library packaging complexity on Windows and macOS; non-uniform cross-platform styling.

## Decision

Adopt **`egui` `0.37.0` and `eframe` `0.37.0`** with **`wgpu` (`30.x`) as the primary rendering backend**, configured with:

1. `default-features = false` on `eframe`, explicitly enabling `accesskit`, `clipboard`, `default_fonts`, `wayland`, `wgpu`, and `x11`—excluding web-oriented features (`web_screen_reader`, `links`), redundant `glow` dependencies, and `system_fonts` (which scans host font directories and adds multi-second first-frame latency; curated fonts will be bundled in Stage 01).
2. `wgpu::PowerPreference::LowPower` by default (overridable via `WGPU_POWER_PREFERENCE`), ensuring hybrid laptops use the integrated GPU by default while supporting dedicated GPUs seamlessly.
3. Strict prohibition of unconditional `Context::request_repaint()` in idle views, verified by automated headless UI tests asserting `repaint_delay == Duration::MAX` on settled frames.
4. Separation of `gitrust-ui` (depends only on `egui` + `gitrust-core`) from `gitrust-app` (`eframe` window bootstrap).

## Consequences

### Positive
- Consistent rendering and custom Design System capability (Stage 01) across Linux (Wayland/X11), Windows (DX12/Vulkan), and macOS (Metal).
- Headless UI tests execute in milliseconds during `cargo test --workspace` without requiring a display server.
- Near-zero CPU usage when idle.

### Negative / Risks
- Vulkan/Metal/DX12 driver initialization maps shared GPU driver libraries into process RSS (`VmRSS`); documented transparently in `docs/PERFORMANCE.md`.
- Stage 01 must establish accessible focus traversal and high-density virtualized list patterns early.

## Related Decisions

- [ADR-0001](0001-rust-as-primary-language.md)
- [ADR-0005](0005-cross-platform-strategy.md)
