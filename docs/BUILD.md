# Build and Platform Guide

This guide documents toolchain requirements, operating-system dependencies, compilation commands, and runtime graphics configuration for **gitRUST**.

---

## 1. Toolchain Requirements

- **Rust Compiler & Cargo:** Minimum Supported Rust Version (MSRV) is **`1.95.0`** (Rust 2024 Edition, workspace resolver `3`). The repository includes [`rust-toolchain.toml`](../rust-toolchain.toml) tracking `stable` with `rustfmt` and `clippy`.
- **Git:** `git >= 2.38` recommended on host systems (required starting in Stage 02 for repository operations).

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
cargo check --workspace --all-targets

# 3. Lint all crates and targets treating warnings as errors
cargo clippy --workspace --all-targets -- -D warnings

# 4. Execute unit and integration tests across the workspace
cargo test --workspace

# 5. Build the optimized release executable (`target/release/gitrust` or `gitrust.exe`)
cargo build --workspace --release
```

---

## 4. Running the Desktop Application

```bash
# Run release build
./target/release/gitrust

# Display CLI version or help
./target/release/gitrust --version
./target/release/gitrust --help

# Run automated desktop window lifecycle smoke check (opens window, resizes, exits cleanly)
./target/release/gitrust --smoke-test
```

### Runtime Display and GPU Environment Variables

| Variable | Values | Purpose |
| --- | --- | --- |
| `WINIT_UNIX_BACKEND` | `wayland` \| `x11` | Forces a specific windowing backend on Linux when testing XWayland/X11 vs. native Wayland. |
| `WGPU_POWER_PREFERENCE` | `low` \| `high` | Overrides default `LowPower` GPU selection (e.g., to test discrete NVIDIA/AMD GPU vs. integrated GPU). |
| `WGPU_BACKEND` | `vulkan` \| `dx12` \| `metal` \| `gl` | Restricts `wgpu` backend selection for diagnostics or driver troubleshooting. |
| `WGPU_ADAPTER_NAME` | Substring match | Selects a specific GPU adapter by name (e.g., `Intel`, `NVIDIA`, `llvmpipe`). |
