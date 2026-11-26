# Build and Platform Guide

This guide documents toolchain requirements, operating-system dependencies, compilation commands, and runtime graphics configuration for **gitRUST**.

---

## 1. Toolchain Requirements and Update Policy

- **Rust Compiler & Cargo:** Validated Minimum Supported Rust Version (MSRV) is **`1.97.1`** (`[workspace.package] rust-version = "1.97.1"`, Rust 2024 Edition, workspace resolver `3`). While the upstream crate floor (`egui` / `eframe` `0.37.0`) declares `1.95.0`, `gitRUST` only declares an MSRV that has been compiled and tested in local/CI validation (`1.97.1` MSRV job + `stable` `1.99.x`).
- **Toolchain Update Policy:** [`rust-toolchain.toml`](../rust-toolchain.toml) tracks the `stable` channel for routine development while CI enforces both `stable` and the pinned `rust-version` (`1.97.1`). Raising `rust-version` requires updating `Cargo.toml`, the `msrv` job in `.github/workflows/ci.yml`, and verifying `--locked` builds.
- **Git:** Native `git >= 2.45.0` on trusted absolute PATH entries is required for Stage 02 repository operations, real-Git integration tests and benchmarks. The floor enables `--no-lazy-fetch`. Use a patched system package.

Verify your toolchain:

```bash
rustc --version
cargo --version
git --version
```

---

## 2. Platform System Dependencies

### Linux (First-Class Platform: Wayland & X11)

`gitRUST` compiles with native support for **both Wayland and X11** and uses **`wgpu` (Vulkan primary, OpenGL ES fallback)** for hardware-accelerated rendering on integrated and discrete GPUs.

#### Fedora / RHEL-compatible

```bash
sudo dnf install \
  gcc pkgconf-pkg-config \
  wayland-devel libxkbcommon-devel libxkbcommon-x11-devel \
  libX11-devel libxcb-devel libXcursor-devel libXi-devel libXrandr-devel \
  mesa-libGL-devel mesa-libEGL-devel vulkan-loader-devel \
  fontconfig-devel
```

#### Debian / Ubuntu

```bash
sudo apt-get update
sudo apt-get install \
  build-essential pkg-config \
  libwayland-dev libxkbcommon-dev libxkbcommon-x11-dev \
  libx11-dev libxcb1-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
  libgl1-mesa-dev libegl1-mesa-dev libvulkan-dev \
  libfontconfig1-dev
```

#### Arch Linux

```bash
sudo pacman -S --needed \
  base-devel pkgconf \
  wayland libxkbcommon libxkbcommon-x11 \
  libx11 libxcb libxcursor libxi libxrandr \
  mesa vulkan-icd-loader fontconfig
```

> **Vulkan Drivers on Linux:** Ensure the appropriate Vulkan ICD package is installed for your GPU (`mesa-vulkan-drivers` / `vulkan-intel` / `vulkan-radeon` for Intel/AMD, or the vendor driver for NVIDIA). In virtual machines or headless containers without a hardware GPU, Mesa's `llvmpipe` (`lvp`) software Vulkan driver can be used for testing.

### Windows

- **Windows 10 (version 1903+) or Windows 11** (`x86_64` or `aarch64`).
- **Build Tools for Visual Studio 2022+** with the *Desktop development with C++* workload and Windows 10/11 SDK (`x86_64-pc-windows-msvc` target).
- **Graphics:** DirectX 12 (built into Windows 10/11) or Vulkan 1.2+ driver. No third-party system packages are required.

### macOS

- **macOS 12 Monterey or newer** (`aarch64-apple-darwin` Apple Silicon or `x86_64-apple-darwin` Intel).
- **Xcode Command Line Tools:**
  ```bash
  xcode-select --install
  ```
- **Graphics:** Apple Metal (included with macOS).

---

## 3. Standard Verification and Build Commands

Run from the workspace root:

```bash
# 1. Verify code formatting
cargo fmt --all -- --check

# 2. Type-check all workspace crates, binaries, tests, and examples
cargo check --locked --workspace --all-targets

# 3. Lint all crates and targets treating warnings as errors
cargo clippy --locked --workspace --all-targets -- -D warnings

# 4. Execute unit and integration tests across the workspace
cargo test --locked --workspace

# 5. Build the optimized release executable (`target/release/gitrust` or `gitrust.exe`)
cargo build --locked --workspace --release
```

---

### Git integration tests and hosted runners

`cargo test --locked --workspace` runs real-Git integration tests in temporary repositories. Missing, invalid and older Git binaries fail the tests explicitly. The Linux MSRV job and Linux, Windows and macOS test jobs record `git --version` before running the suites. The currently published Linux, Windows and macOS images include versions above the floor. For a self-hosted runner, provision an updated native Git and prefer its installation directory in PATH before running the jobs. The test suite neither installs nor upgrades system Git. The Unix path fixture exercises tabs, newlines and literal pathspecs on Linux and macOS. Linux also exercises a non-UTF-8 filename on disk. The macOS fixture checks non-UTF-8 bytes at the parser boundary because the hosted filesystem rejects that filename. To verify the floor, prepend the absolute directory of a separately built Git 2.45.0 executable to the test process PATH and run `cargo test --locked -p gitrust-git --test inspection`. [Ubuntu runner](https://github.com/actions/runner-images/blob/main/images/ubuntu/Ubuntu2604-Readme.md) · [Windows runner](https://github.com/actions/runner-images/blob/main/images/windows/Windows2025-VS2026-Readme.md) · [macOS runner](https://github.com/actions/runner-images/blob/main/images/macos/macos-26-arm64-Readme.md)

## 4. Running the Desktop Application

```bash
# Run release build (uses Wayland natively when WAYLAND_DISPLAY is active)
./target/release/gitrust

# Force X11 / XWayland on a dual Wayland+X11 Linux session (winit 0.29+ honors WAYLAND_DISPLAY / DISPLAY)
env -u WAYLAND_DISPLAY ./target/release/gitrust

# Display CLI version or help
./target/release/gitrust --version
./target/release/gitrust --help

# Run automated desktop window lifecycle smoke check (opens window, resizes, exits cleanly)
./target/release/gitrust --smoke-test
```

### Runtime Display and GPU Environment Variables

> **Note on `winit` `0.30`:** Upstream `winit` removed the legacy `WINIT_UNIX_BACKEND` variable in `v0.29`. On Linux, protocol selection follows standard `WAYLAND_DISPLAY` and `DISPLAY` variables, and `gitRUST` verifies the active protocol directly from the OS `RawDisplayHandle`.

| Variable | Values | Purpose |
| --- | --- | --- |
| `WAYLAND_DISPLAY` / `DISPLAY` | Socket / X11 display | Standard Unix display selection (`env -u WAYLAND_DISPLAY` selects X11/XWayland when `DISPLAY` is set). |
| `WGPU_POWER_PREF` | `low` \| `high` \| `none` | Overrides default `LowPower` GPU selection (e.g., `WGPU_POWER_PREF=high` selects a discrete NVIDIA/AMD GPU). |
| `WGPU_BACKEND` | `vulkan` \| `dx12` \| `metal` \| `gl` | Restricts `wgpu` backend selection (for example, `WGPU_BACKEND=gl` tests the built-in OpenGL ES path). |
| `WGPU_ADAPTER_NAME` | Substring match | Filters adapter selection by substring when multiple GPUs are exposed. |
