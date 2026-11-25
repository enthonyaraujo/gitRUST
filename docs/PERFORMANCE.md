# Performance Targets and Measurement Methodology

This document establishes the exploratory performance objectives for **gitRUST**, defines a reproducible cross-platform measurement methodology, and records empirical Stage 00 / Stage 00.1 baselines.

---

## 1. Exploratory Performance Targets

The following values are **exploratory engineering targets**, not guarantees across every operating system, compositor, or GPU driver:

| Metric | Exploratory Target | Scope / Condition |
| --- | --- | --- |
| **Idle RAM (`USS` / `RssAnon` vs. `VmRSS`)** | `<= 100 MB` | Settled desktop window in idle state (see memory accounting breakdown below) |
| **Idle CPU** | `< 1%` | Settled desktop window with no user input over a `60`-second observation window |
| **Startup Latency** | `<= 1.0 s` | Warm launch from process invocation to initialized desktop window / first frame |
| **Release Executable** | `<= 30 MB` | Compiled with `cargo build --locked --workspace --release` |

### Memory Accounting (`VmRSS` vs. `PSS` vs. `USS` / `RssAnon`)

Hardware-accelerated desktop applications using Vulkan (`wgpu`), Metal, or DirectX 12 map shared GPU driver libraries (`libvulkan_*`, Mesa/LLVM shader compilers, or proprietary NVIDIA ICDs) into the process address space. On Linux, raw `VmRSS` over-counts shared read-only `.so` mappings (`RssFile` / `Shared_Clean`). Every Linux measurement therefore reports:

- **`VmRSS`:** Total resident pages (private + full size of shared libraries).
- **`PSS` (Proportional Set Size, `/proc/<pid>/smaps_rollup`):** Private pages plus shared pages divided by the number of sharing processes.
- **`USS` (Unique Set Size = `Private_Clean + Private_Dirty`):** Physical memory uniquely attributable to the `gitrust` process.
- **`RssAnon` / `Private_Dirty`:** Heap/stack and writable anonymous buffers allocated by `gitrust` and its GPU command queues.
- **`RssFile` / `RssShmem`:** File-backed executable/library mappings and shared compositor/SHM buffers.

---

## 2. Anti-Premature-Optimization Policy

- **Stability and diagnostics come first:** Stage 00 uses standard Cargo `release` defaults (`panic = "unwind"`, unstripped symbol tables until Stage 10 packaging) so crash backtraces and profiling tools (`perf`, `samply`, `heaptrack`) remain accurate.
- **Architectural efficiency over micro-tricks:** Zero idle CPU is achieved structurally through event-driven `egui`/`eframe` rendering (`ControlFlow::Wait`), zero per-frame static string allocations (`FoundationView` label caching), and defaulting `wgpu` to `PowerPreference::LowPower` (overridable via `WGPU_POWER_PREF=high`).

---

## 3. Reproducible Measurement Protocol

Every benchmark report must record:

1. **System & OS:** Distribution/version, kernel, CPU, total RAM.
2. **Windowing & Graphics:** Active protocol verified via `RawDisplayHandle` (`Wayland`, `X11`, `Win32`, `AppKit`), compositor, display scale factor, active GPU adapter + driver, `wgpu` backend (`Vulkan`, `Dx12`, `Metal`, `Gl`), and discrete GPU PCIe runtime PM status (`active` vs. `suspended`).
3. **Build Metadata:** `rustc --version`, Cargo profile (`--release --locked`), target triple, and exact binary byte size (`stat -c %s target/release/gitrust`).
4. **Sampling:** Multiple warm runs (`N >= 5`, reporting median, min, and max) using a PTY harness (`pty.openpty()`) to avoid pipe block-buffering distortion on startup/shutdown timestamps, plus a minimum **`60`-second** settled idle period (after `5` seconds of startup settling) for CPU and memory drift.

---

## 4. Stage 00.1 Audited Reference Host Baselines

Recorded on `2026-11-24` / `2026-11-25` (`rustc 1.99.1`, `cargo build --locked --workspace --release`, `feat/project-foundation` after Stage 00.1 hardening):

- **Host:** Fedora Linux 45 (Workstation Edition), Kernel `7.3.4-300.fc45.x86_64`, GNOME/Mutter (`Wayland` native @ `1.25x` scale; `XWayland` @ `2.0x` scale).
- **GPUs:** `Intel(R) Graphics (RPL-P)` (Mesa `26.2.7`, default `LowPower`) and `NVIDIA GeForce RTX 3050 6GB Laptop GPU` (NVIDIA `620.70.06`, selected via `WGPU_POWER_PREF=high`).
- **Release Binary Size:** **`26,738,192` bytes (`26.74 MB` / `25.50 MiB`)** — within `<= 30 MB` target.

### 4.1 Startup & Clean Shutdown Latency (`N = 5` Warm Runs, PTY-Timed)

| Configuration | Init Median (ms) | Init Min–Max (ms) | Shutdown Median (ms) | Shutdown Min–Max (ms) |
| --- | --- | --- | --- | --- |
| **Wayland + Intel iGPU (`Vulkan`)** *(default)* | **`273.7`** | `253.1 – 286.1` | **`69.6`** | `68.0 – 73.2` |
| **X11 (`env -u WAYLAND_DISPLAY`) + Intel iGPU (`Vulkan`)** | **`305.9`** | `278.4 – 429.1` | **`83.5`** | `52.8 – 3033.8`* |
| **Wayland + NVIDIA RTX 3050 (`Vulkan`, `WGPU_POWER_PREF=high`)** | **`411.7`** | `389.0 – 513.2` | **`126.4`** | `121.3 – 169.0` |
| **Wayland + Intel iGPU (`OpenGL ES` via `WGPU_BACKEND=gl`)** | **`167.7`** | `133.2 – 190.6` | **`38.6`** | `31.2 – 52.7` |

- **First / Cold Launch (Wayland + Intel Vulkan):** `427.1 ms` init, `237.7 ms` clean shutdown when the discrete NVIDIA PCIe device is already `active`. When the NVIDIA PCIe device (`0000:01:00.0`) is in `D3cold` (`suspended`), upstream Vulkan ICD enumeration (`nvidia_icd.json`) adds a one-time `~3.2 s` PCIe wakeup penalty on Linux Optimus hosts.
- \* The single `3033.8 ms` shutdown outlier occurred exclusively under `X11 (XWayland) + Mesa Intel ANV Vulkan` during active swapchain reconfiguration (see Section 4.3).

### 4.2 Settled Idle CPU and Memory Decomposition (`60 s` Vulkan / `15 s` GLES)

| Configuration | Idle Window | Idle CPU (%) | `RssAnon` (MB) | `USS` Private (MB) | `PSS` (MB) | `VmRSS` (MB) | `RssFile` (MB) | `RssShmem` (MB) | `Shared` (MB) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| **Wayland + Intel iGPU (`Vulkan`)** | `60.00 s` | **`0.0000%`** (`0` ticks) | **`36.44`** | **`99.02`** | `115.14` | `155.65` | `118.64` | `0.57` | `56.62` |
| **Wayland + NVIDIA RTX 3050 (`Vulkan`)** | `6.00 s` | **`0.000%`** (`0` ticks) | **`34.73`** | **`88.18`** | `123.28` | `186.18` | `133.60` | `17.86` | `80.28` |
| **Wayland + Intel iGPU (`WGPU_BACKEND=gl`)** | `15.00 s` | **`0.0000%`** (`0` ticks) | **`25.74`** | **`89.25`** | `95.53` | `116.84` | `90.58` | `0.52` | `27.58` |
| **X11 + Intel iGPU (`WGPU_BACKEND=gl`)** | `6.00 s` | **`0.000%`** (`0` ticks) | — | — | — | — | — | — | — |
| **X11 + NVIDIA RTX 3050 (`Vulkan`)** | `6.00 s` | **`0.000%`** (`0` ticks) | — | — | — | — | — | — | — |
| **X11 (`XWayland`) + Intel iGPU (`Vulkan`)** | `60.00 s` | **`5.4833%`** (`329` ticks) | `37.58 -> 41.05` | `99.87 -> 103.34` | `115.70 -> 121.05` | `156.66 -> 160.14` | `119.02` | `0.07` | `56.79` |

*(Values in MB use SI `1,000 kB = 1 MB` from `/proc/<pid>/status` and `/proc/<pid>/smaps_rollup`; memory remained byte-identical across the entire 60 s idle window on Wayland Vulkan.)*

### 4.3 Controlled Isolation of X11 CPU Anomaly and `wgpu` vs. `glow` Evaluation

- **Root Cause of `5.48%` Idle CPU under X11 (XWayland) + Mesa Intel ANV Vulkan:** Under the exact same binary and X11 (`env -u WAYLAND_DISPLAY`) session, switching either the GPU (`X11 + NVIDIA Vulkan` -> `0.000%`) or the backend (`X11 + Intel GLES` -> `0.000%`) eliminates 100% of idle CPU and memory drift. Native Wayland (`Wayland + Intel Vulkan` -> `0.000%` over `60 s`) is completely unaffected. This isolates the behavior to an upstream **Mesa Intel ANV Vulkan WSI + GNOME XWayland (`2x` HiDPI)** swapchain status interaction (`Suboptimal`/`Outdated` triggering `egui-wgpu` surface reconfiguration).
- **Comparative `wgpu` vs. `eframe/glow` Assessment:** `wgpu` already ships a built-in OpenGL ES fallback backend (`wgpu-hal` `gles`, selectable via `WGPU_BACKEND=gl` and activated automatically when Vulkan ICDs are absent). In measurements, `WGPU_BACKEND=gl` achieved lower startup latency (`167.7 ms` vs. `273.7 ms`), lower private memory (`25.74 MB` `RssAnon` / `95.53 MB` `PSS` vs. `36.44 MB` / `115.14 MB`), and `0.000%` idle CPU on both Wayland and X11. However, keeping **`wgpu` as the single primary renderer** remains architecturally superior because:
  1. It provides native Vulkan (Linux/Windows), DirectX 12 (Windows), and Metal (macOS, where OpenGL is deprecated) plus automatic OpenGL ES fallback without maintaining two separate `eframe::Renderer` stacks or pulling `glutin`/`glutin-winit`/`egui_glow`.
  2. It provides explicit `PowerPreference` control (`LowPower` default vs. `HighPerformance`) on hybrid-GPU laptops.

### 4.4 Documented Measurement Scope and Limitations

- **XWayland + Mesa Intel ANV Vulkan CPU Anomaly:** Settled idle CPU measured `5.4833%` over `60 s` exclusively under XWayland + Mesa Intel Vulkan (`0.000%` on native Wayland, X11 Intel GLES, and X11 NVIDIA Vulkan).
- **Discrete NVIDIA `D3cold` Cold-Wake Latency:** When the Optimus discrete GPU (`0000:01:00.0`) is suspended in `D3cold`, upstream Vulkan ICD probing (`nvidia_icd.json`) adds a one-time `~3.2 s` PCIe wakeup latency before `LowPower` selects the integrated Intel GPU.
- **Memory Accounting Distinction:** `VmRSS` (`155.65 MB` Wayland Vulkan) includes shared read-only system graphics libraries (`RssFile = 118.64 MB`), whereas `PSS` (`115.14 MB`), `USS` (`99.02 MB`), and `RssAnon` (`36.44 MB`) separate shared driver pages from private process allocations.
- **Sub-60-Second Idle Windows on Secondary Configurations:** Full `60.00 s` settled idle measurements were executed for the two primary Linux Vulkan paths (native Wayland and X11 via XWayland); comparative secondary runs (`Wayland GLES`, `Wayland NVIDIA`, `X11 GLES`, `X11 NVIDIA`) used shorter `6 s`–`15 s` idle windows.
- **Incomplete Cold-Start Evaluation:** First-launch timing (`427.1 ms`) was measured without clearing OS page caches (`drop_caches`) or GPU shader caches; a full multi-sample cold-start suite remains pending.
- **Platform & Display Coverage Limits:** Live graphical benchmarks ran only on Fedora 45 GNOME/Mutter (`Wayland` and `XWayland`); standalone `Xorg` sessions, native `Windows`, and native `macOS` graphical executions have not yet been benchmarked locally.

---

## 5. Future Micro- and Macro-Benchmarks (Stage 02 & Stage 09)

When `gitrust-git` and `gitrust-ui` diff virtualization are implemented:
- `benches/` will use `criterion` to measure deterministic non-UI workloads (`status --porcelain=v2 -z` parsing, unified diff chunking, commit graph indexing) against synthetic repositories of `1k`, `10k`, and `100k` tracked files.
- UI frame-time percentiles (`p50`, `p95`, `p99`) will be measured during continuous scrolling of large diffs and commit histories.
