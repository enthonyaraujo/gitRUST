# Performance Targets and Measurement Methodology

Paths under `target/` identify local audit artifacts available in the maintainer workspace. Git excludes these artifacts.

This document establishes the exploratory performance objectives for **gitRUST**, defines a reproducible cross-platform measurement methodology, and records the audited foundation baseline and the Stage 01, 01.1 and 01.2 desktop comparisons.

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

- **Stability and diagnostics come first:** Stage 00 uses standard Cargo `release` defaults (`panic = "unwind"`, unstripped symbol tables until Stage 11 packaging) so crash backtraces and profiling tools (`perf`, `samply`, `heaptrack`) remain accurate.
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

## 5. Stage 01 desktop comparison

Measured on 2026-11-25 on Fedora 45, GNOME/Mutter, kernel `7.3.4-300.fc45.x86_64`, Intel Core i5-13420H, Intel RPL-P graphics (Mesa 26.2.7), approximately 15.3 GiB RAM. The native window reports Wayland or X11 via its display handle. Framebuffer dimensions show Wayland captures at 1× and 1.25× and XWayland captures at 2×. Benchmarks use the uninstrumented release binaries from the integrated foundation (`a6340e6`) and Stage 01, built with Rust 1.99.1 and the same lockfile.

Each smoke timing below is the median of five launches. The settled capture uses the same built-in smoke delay in both builds and includes GPU readback and writing to a temporary file. For each idle run, the regular workspace settles for four seconds and then remains open for 60 seconds without scripted input. CPU uses the difference in `/proc/<pid>/stat` ticks divided by wall time and clock frequency. Memory comes from `status` and `smaps_rollup` at the end of that interval.

| Configuration | Window initialized (ms) | Settled capture (ms) | Idle CPU (%) |
| --- | ---: | ---: | ---: |
| Wayland / Vulkan | 160.2 → 144.8 | 467.4 → 432.9 | 0.0000 → 0.0000 |
| Wayland / GLES | 90.7 → 92.4 | 390.0 → 372.5 | 1.3832 → 0.0000 |
| XWayland / Vulkan | 179.0 → 140.7 | 520.1 → 438.7 | 0.0000 → 0.0000 |
| XWayland / GLES | 127.8 → 92.2 | 455.7 → 408.7 | 1.3666 → 0.3166 |

| Configuration | RSS (KiB) | PSS (KiB) | USS (KiB) | RssAnon (KiB) |
| --- | ---: | ---: | ---: | ---: |
| Wayland / Vulkan | 133,928 → 135,028 | 89,074 → 94,879 | 55,892 → 63,756 | 29,172 → 28,552 |
| Wayland / GLES | 117,564 → 118,180 | 78,537 → 81,689 | 50,820 → 55,360 | 26,060 → 25,440 |
| XWayland / Vulkan | 133,596 → 135,960 | 88,658 → 95,426 | 55,608 → 64,476 | 28,968 → 29,732 |
| XWayland / GLES | 117,408 → 119,744 | 77,917 → 82,779 | 50,260 → 56,512 | 25,644 → 26,408 |

The executable grew from **26,738,192 to 28,615,984 bytes** (+7.0%), below the 30 MB target. The three new TTFs total 1,067,412 bytes. Measured private memory rose on all four paths and stayed within the target. The final measured desktop run met the idle CPU target in each configuration. The contribution of UI, graphics driver and OS activity to the differences was not isolated. The historical Intel ANV/XWayland and Optimus observations remain applicable to their measured conditions.

### First framebuffer readback

Two temporary, identically instrumented release builds requested a screenshot from the first UI pass, bypassing the smoke test's settling delay when no resize was requested. Five warm launches per build and protocol were interleaved and timed using a PTY. This includes GPU readback and file writing. Presentation timing at the monitor requires compositor instrumentation.

| Vulkan configuration | Foundation first readback median [min–max], ms | Stage 01 first readback median [min–max], ms |
| --- | ---: | ---: |
| Wayland | 234.3 [189.0–436.1] | 212.9 [193.0–315.2] |
| XWayland | 252.6 [227.0–264.2] | 251.6 [234.8–269.1] |

### Virtual viewport CPU cost

In release, 100 headless frames after 10 warmups at 640 × 400 exercised egui layout and CPU tessellation at a fixed viewport. Only visible rows and egui overscan are processed. These timings exclude compositor and GPU work.

| Fixture | Median (ms) | p95 (ms) |
| --- | ---: | ---: |
| 100 list entries | 0.098 | 0.179 |
| 1,000 list entries | 0.083 | 0.116 |
| 10,000 list entries | 0.083 | 0.095 |
| 100,000 diff lines | 0.100 | 0.108 |

Run this fixture benchmark with:

```sh
cargo test --locked --release -p gitrust-ui profile_virtualized_views -- --ignored --nocapture
```

The local comparison covers Intel under GNOME Wayland and XWayland. It leaves standalone Xorg, Windows, macOS, discrete GPUs, compositor presentation time and cold-cache sampling for separate validation.

---

## 6. Stage 01.1 native refinement

Measured on the Fedora 45 GNOME/Mutter host in Section 5, using Intel RPL-P with Mesa 26.2.7 and Vulkan in both protocols. The saved Stage 01 binary was captured before code changes. The final release is **28,553,824 bytes**, down from 28,615,984 bytes by 62,160 bytes (0.217%). Both builds use the same workspace lockfile. The main comparison used a 28,548,744-byte build before the final Alt shortcut fix. That fix applies only to demonstration tab input.

Settled CPU is process ticks per wall second over 60 seconds, after four seconds of startup settling. Memory is the end-of-window reading from `/proc`. USS sums private clean and private dirty memory. The diagnostic profiles are Unified and the 100,000-line diff gallery. The baseline stage lacked the latter startup option, so the stress profile has Stage 01.1 measurements only.

Warm launches use five samples. The initialization marker reports the time until the native window and Vulkan adapter have been created. Smoke time also includes the built-in 250 ms settling interval and window shutdown. Pipe output is timed to the observed marker. These figures do not measure display presentation.

| Vulkan configuration | Stage 01 smoke median (ms) | Stage 01.1 init median [min–max] (ms) | Stage 01.1 smoke median (ms) | Idle CPU (%) Stage 01 → 01.1 |
| --- | ---: | ---: | ---: | ---: |
| Wayland, empty workspace | 442.5 | 262.4 [219.1–507.4] | 570.8 | 0.1167 → 5.1996 |
| Wayland, Unified | 469.6 | 147.5 [143.7–165.9] | 447.4 | 1.4333 → 0.1500 |
| Wayland, 100,000-line diff | — | 150.7 [141.2–172.5] | 440.5 | — → 0.0000 |
| XWayland, empty workspace | 442.5 | 147.9 [122.3–187.6] | 461.3 | 0.5000 → 3.4997 |
| XWayland, Unified | 487.9 | 174.6 [164.7–191.7] | 497.4 | 0.3666 → 0.0000 |
| XWayland, 100,000-line diff | — | 165.6 [145.8–277.3] | 473.6 | — → 0.0000 |

| Vulkan configuration | VmRSS (KiB) Stage 01 → 01.1 | PSS (KiB) Stage 01 → 01.1 | USS (KiB) Stage 01 → 01.1 | RssAnon (KiB) Stage 01 → 01.1 |
| --- | ---: | ---: | ---: | ---: |
| Wayland, empty workspace | 136,168 → 137,376 | 97,778 → 98,952 | 68,500 → 69,616 | 28,600 → 30,868 |
| Wayland, Unified | 137,484 → 135,528 | 98,921 → 97,340 | 69,472 → 68,208 | 29,264 → 29,068 |
| Wayland, 100,000-line diff | — → 135,096 | — → 96,982 | — → 67,840 | — → 28,768 |
| XWayland, empty workspace | 144,900 → 139,316 | 106,034 → 100,821 | 76,752 → 71,668 | 36,412 → 33,288 |
| XWayland, Unified | 138,248 → 136,700 | 99,421 → 98,096 | 70,140 → 68,928 | 30,256 → 30,164 |
| XWayland, 100,000-line diff | — → 136,084 | — → 97,634 | — → 68,520 | — → 29,952 |

All six profiles in the main comparison stayed within the warm initialization, private memory and executable targets. Both empty workspace profiles exceeded the existing 1% idle CPU target in the main round. A preliminary build previously showed 7.4661% in Unified on Wayland and a 2,371.4 ms smoke outlier on XWayland. The main round showed 0.15% and 0% in Unified, with 0% in both 100,000-line profiles. The headless empty and diagnostic views settle without a pending repaint. Process measurements leave the cause of the native variability open, including the historical ANV/XWayland finding. Stage 01 repeat (`target/stage01.1-evidence/performance-before.log`) · Stage 01.1 Wayland (`target/stage01.1-evidence/performance-final-wayland.log`) · Stage 01.1 XWayland (`target/stage01.1-evidence/performance-final-x11.log`)

A second 60-second empty-workspace run with five seconds of settling used the same binaries in reverse order. It measured `0.6000%` after refinement versus `0.0500%` on Stage 01 under Wayland, and `0.0667%` versus `1.2833%` under XWayland. The XWayland baseline spent most of those ticks in the first 10 seconds of the sample. Swapchain worker threads also accumulated ticks. The repeats establish variability across runs. They leave consistent compliance with the idle target unverified. Wayland repeat (`target/stage01.1-evidence/performance-idle-recheck.log`) · XWayland repeat (`target/stage01.1-evidence/performance-idle-recheck-x11.log`)

The exact final release repeated the default empty workspace over 60 seconds after five seconds of settling. It used `0.0667%` CPU on Wayland and `0.0167%` on XWayland. Both samples met the idle target. The earlier spikes remain in the record. Final release logs (`target/stage01.1-evidence/performance-release-idle.log`)

Stage 01 headless results remain in Section 5. Stage 01.1 uses the same test viewport, frames and warmups. The benchmark measures egui layout and CPU tessellation. It excludes GPU rasterization and compositor presentation. The runtime lists still mount visible rows with egui overscan. The diff mounts only the rows intersecting its viewport.

| Fixture | Median (ms) Stage 01 → 01.1 | p95 (ms) Stage 01 → 01.1 |
| --- | ---: | ---: |
| 100 list entries | 0.098 → 0.101 | 0.179 → 0.217 |
| 1,000 list entries | 0.083 → 0.069 | 0.116 → 0.074 |
| 10,000 list entries | 0.083 → 0.069 | 0.095 → 0.076 |
| 100,000 diff lines | 0.100 → 0.097 | 0.108 → 0.110 |

A sample taken alongside compilation had a 1.465 ms p95 for 100 entries. The table uses the subsequent quiet run. A final-build repeat measured a 0.110 ms median and 0.176 ms p95 on the 100,000-line diff, and medians of 0.155, 0.072 and 0.098 ms on the three lists. Quiet run (`target/stage01.1-evidence/virtualized-cpu-quiet.log`) · Final run (`target/stage01.1-evidence/virtualized-cpu-final.log`)

Native continuous scrolling used the XWayland application showing the 100,000-line synthetic diff. The input driver sent 120 Page Down events in 5.431 seconds with 1.040 seconds of process CPU. The before and after viewports show advancement through the fixture. The driver also exercised horizontal movement on a long line. This run measures process cost during input and does not establish frame latency, GPU time or compositor throughput. Input log (`target/stage01.1-evidence/native-input.log`) · Initial and scrolled viewports (`target/stage01.1-evidence/after/scroll-review.png`)

---

## 7. Stage 01.2 idle revalidation

The locked release build used for the visual evidence was also used for the primary series. Each of the six cells has five independent launches with Intel RPL-P and Vulkan on GNOME/Mutter 51.1. The matrix crosses native Wayland and XWayland with the empty workspace, the Unified fixture workspace and the 100,000-line diff. It rotates profile order across rounds and runs one application at a time with no injected input. Fedora 45, Mesa 26.2.7 and XWayland 24.1.15 were recorded on this host. The harness verifies the display handle and GPU in each application's startup log.

The driver waits five seconds after native initialization and measures 60 seconds per launch. CPU is the process user and system tick rate divided by elapsed wall time, expressed as a percentage of one logical core. The individual ledger preserves five-second intervals, per-thread ticks, main-thread context switch deltas and memory at both endpoints. RSS comes from `/proc/<pid>/status`. PSS and USS use `/proc/<pid>/smaps_rollup`, with USS calculated as private clean plus private dirty pages. Table memory values use end-of-window medians in KiB.

An early session lock interrupted the desktop preflight. Those attempts and the isolated Mutter virtual-monitor preflight are retained separately. The counted series uses the unlocked physical desktop after the maintainer unlocked the session. A session-scoped GNOME idle inhibitor and lock-state checks kept the sample windows available. The inhibitor ended with the benchmark. The procedure issued no desktop settings writes. The observed `idle-delay` value was 300 seconds before the lock and 0 at the final readback. The source of that session preference change was outside the observed audit operations. Final settings readback (`target/stage01.2-evidence/desktop-settings-after.log`)

The production binary does not expose frame counters. The investigation uses a separate audited copy built under `target/stage01.2-evidence`. It counts native frames, egui immediate and delayed repaint notifications, window focus and input events, AccessKit updates, surface configuration, suboptimal and other acquisition results, and present submissions. It leaves the WGPU integration callback in place and records its own five-second counter snapshots. These probe results explain observed runtime activity without altering the 30-sample release series or treating present submission as display timing.

The application repaint calls in `main.rs` are in the opt-in smoke controller. Standard launch does not start that controller. The UI style uses zero animation duration for normal transitions. The spinner is confined to the Gallery loading example. The headless empty and synthetic views settle with no pending repaint. A separate focused-filter control distinguishes normal caret blinking from unexpected redraws of a settled window. The Orca probe runs on a private AT-SPI bus to observe accessibility updates independently.

### Primary measurements

All 30 full 60-second samples met the existing `< 1%` idle CPU target. The maximum was 0.4333% in the Wayland 100,000-line diff. The memory columns report end-of-window medians in KiB. The native XWayland scale guess was 1× in all 15 primary launches.

| Protocol | Profile | Mean CPU % | Median % | Min % | Max % | RSS | PSS | USS | RssAnon |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Wayland | Empty workspace | 0.0133 | 0.0000 | 0.0000 | 0.0500 | 135072 | 81552 | 55844 | 28536 |
| Wayland | Unified fixture workspace | 0.0067 | 0.0000 | 0.0000 | 0.0167 | 135872 | 82459 | 56828 | 28972 |
| Wayland | 100,000-line diff | 0.0933 | 0.0167 | 0.0000 | 0.4333 | 135532 | 82248 | 56552 | 28780 |
| XWayland | Empty workspace | 0.0067 | 0.0000 | 0.0000 | 0.0167 | 135364 | 84693 | 59036 | 28596 |
| XWayland | Unified fixture workspace | 0.0800 | 0.0167 | 0.0000 | 0.3667 | 135824 | 81970 | 56320 | 29008 |
| XWayland | 100,000-line diff | 0.0100 | 0.0167 | 0.0000 | 0.0167 | 135512 | 81695 | 56040 | 28800 |

The strongest five-second interval was 4.2000% in the Wayland 100,000-line diff, from seconds 10 to 15 of round 3. Of 21 process ticks, 19 were accounted for by the main thread. Two 1.6000% intervals in the first XWayland Unified run included WSI swapchain worker ticks. The primary sampling attributes CPU time to threads but cannot attribute it to specific function calls. All 30 rows (`target/stage01.2-evidence/idle-samples.md`) · CSV (`target/stage01.2-evidence/idle-samples.csv`) · Raw samples (`target/stage01.2-evidence/idle/samples.ndjson`)

### Instrumented controls and findings

| Control | Settled behavior |
| --- | --- |
| Six ordinary scenarios, 20 seconds each | Zero native frames, repaint notifications, AccessKit updates and surface reconfigurations after settling. Process CPU ranged from 0.0000% to 0.0500%. |
| Wayland 100,000-line diff, 60-second targeted repeat | 0.0000% CPU and zero frames, input events, repaint notifications or surface reconfigurations. The strongest short release burst was not reproduced. |
| XWayland empty workspace, 60 seconds at a process-local 2× scale | 0.0000% CPU and zero settled frames, repaint requests or surface reconfigurations. One run leaves the historical XWayland finding open. |
| XWayland Unified with private Orca, 20 seconds | Three AccessKit updates before the first counter snapshot. Zero settled AccessKit updates, frames, repaint requests or surface reconfigurations. |
| XWayland focused filter, 20 seconds | 0.6500% CPU, 40 delayed notifications and 40 native frames. The source was the text caret timer. |
| Native Wayland focused filter, exact release, 60 seconds | 30.6333% CPU with no injected input. The original 20-second instrumented control measured 32.1999%. |
| Native Wayland focused filter, input-category probe, 30 seconds | 30.7666% CPU. The probe counted 3,591 empty IME preedits, immediate repaint notifications and native frames. Pointer, keyboard, resize, AccessKit update and surface-reconfiguration counts stayed flat. |
| Native Wayland focused filter, experimental A/B control, 30 seconds | 0.5667% CPU, zero settled empty IME preedits and immediate notifications, 60 delayed notifications and 61 frames. The only intervention was withholding empty preedit events from the egui pass in the scratch control. |

The installed winit 0.30.13 Wayland implementation produces an empty preedit when it handles a text-input `done` without a pending preedit. The installed egui-winit 0.37.0 treats IME events as a reason to repaint and sends an IME cursor-area update whenever the input queue contains an event, even when the cursor rectangle is unchanged. The event counts and the A/B intervention support that feedback loop in the tested session. The source of the separate short idle bursts remains undetermined. No production IME filtering was introduced. A production fix needs to preserve legitimate preedit cancellation and composition across input methods.

Probe counters (`target/stage01.2-evidence/probe-summary.json`) · All control samples (`target/stage01.2-evidence/supplemental-samples.md`) · Release focus reproduction (`target/stage01.2-evidence/focus-follow.log`) · Scratch A/B intervention (`target/stage01.2-evidence/prepare-ime-control.py`)

Build and host details (`target/stage01.2-evidence/host-graphics.log`) · Release sample procedure (`target/stage01.2-evidence/idle.py`) · Compositor progress (`target/stage01.2-evidence/measurement-follow.log`) · Probe implementation (`target/stage01.2-evidence/instrument.py`)

---

## 8. Stage 01.3 Wayland IME feedback

With egui, eframe and egui-winit 0.37.0 and winit 0.30.13, a native Wayland cursor-area update commits text-input state. Mutter acknowledges it with `done`, which winit can turn into a preedit even when the cursor has not moved. A non-empty preedit can also be repeated by IBus during composition. egui-winit requests a frame for the IME event and ordinarily treats its presence as a reason to send the same rectangle again.

The app installs a public egui end-of-pass callback only for a verified native Wayland display. Widgets process every preedit before the callback runs. The callback then removes preedits from the event list that egui-winit checks when deciding whether to resend an unchanged rectangle. Real rectangle changes, commits and focus events still reach the native integration. Upstream deduplicates at the rectangle cache and invalidates it on focus and IME activation. That change was merged after the pinned stable release. The preedit filter can be removed when the workspace updates to a stable release containing the fix. [Upstream fix](https://github.com/emilk/egui/pull/8906)

With libpinyin the isolated session also exposed an eframe fallback after a redraw missed its 250 ms deadline. The fallback ran app logic without drawing widgets and forwarded an empty IME output, cancelling an active composition in a focused window. gitRUST temporarily republishes the last editor output for a focused, non-minimized window during such a logic-only cycle. The next UI pass publishes fresh widget geometry. The fallback clears stale processed events from the prior pass. eframe retains new raw input for the next UI pass. Fallback trace (`target/stage01.3-evidence/native-ime-logic-trace/description.log`)

The official measurement uses a fresh release process for each 60-second interval and five seconds of settling. The 1920 × 1080 virtual monitor runs on a private GNOME Shell/Mutter instance on the same Intel GPU with wgpu/Vulkan. A private virtual keyboard and compositor focus checks isolate the samples from regular desktop input. The diagnostic views `keyboard`, `compose-summary` and `compose-description` give initial focus to the same text widgets used by the demo workspace. Separate instrumented launches check native and widget focus and distinguish caret repaint timers from immediate repaint notifications and IME cursor-area resends.

| Focused scenario | Samples | Mean CPU | Median | Min | Max |
| --- | ---: | ---: | ---: | ---: | ---: |
| Wayland filter | 5 | 0.5300% | 0.5333% | 0.4333% | 0.6000% |
| Wayland commit summary | 5 | 0.5833% | 0.5667% | 0.5500% | 0.6333% |
| Wayland commit description | 5 | 0.5467% | 0.5667% | 0.4667% | 0.6167% |
| XWayland filter | 5 | 0.6067% | 0.6000% | 0.5500% | 0.6667% |

All 20 focused samples met the 1% process CPU target. The largest five-second interval was 0.8000%. The earlier 30.6333% filter reproduction used the physical GNOME session. A separate 30-second check of the archived Stage 01.2 release in the private compositor measured 0.0000% and did not reproduce that physical-session loop. The archived probe also stayed below the CPU clock's resolution while reporting focus. The report contains individual results and all four memory measures for each launch. Results (`target/stage01.3-evidence/RESULTADOS.md`) · Raw samples (`target/stage01.3-evidence/official/samples.ndjson`) · Archived release control (`target/stage01.3-evidence/matched-baseline/samples.ndjson`) · Archived probe (`target/stage01.3-evidence/matched-baseline-probe/1-wayland-filter-1.log`)

In each separate 20-second focused probe, 41 native frames corresponded to 40 or 41 delayed notifications from the caret timer. The counters recorded zero immediate repaint notifications, IME events, IME cursor-area updates, AccessKit updates and surface reconfigurations after settling. Native IBus interaction was assessed separately with AccessKit active. Probe ledgers and logs (`target/stage01.3-evidence/probe/samples.ndjson`)

The six abbreviated 15-second controls recorded 0.0000% CPU at the process clock's resolution. They covered the empty workspace, Unified demo and 100,000-line diff on both protocols, with no focused text editor. Abbreviated controls (`target/stage01.3-evidence/unfocused/samples.ndjson`)

## 9. Git Engine benchmarks (Stage 02)

Run `cargo bench --locked -p gitrust-git --bench engine` on an otherwise idle host. The opt-in Rust harness uses two warm-ups and 21 timed samples per case. It reports min, median, p95, p99, max and throughput. On Linux it samples the harness RSS after each case. An optional external peak-RSS measurement should name the measurement tool, clarify its process scope and record the host CPU, OS, Git version, toolchain and filesystem.

The workload compares direct Git command output with typed engine end-to-end requests against the same temporary data. It also times standalone parsers. Status uses 100, 1,000, 10,000 and 100,000 tracked files with 1% modified, plus synthetic porcelain output at the same entry counts. A single-file patch yields 100,000 changed lines. History reads a 50-commit page from a 3,000-commit fixture. Throughput submits 32 concurrent status requests to pools of one, two and four workers.

Direct Git rows include subprocess startup and capture. Engine rows add policy preflight, scheduling and parsing. History direct rows use a pre-resolved HEAD and capture the same page commands. Parser rows are in-process only. Do not subtract separately aggregated percentile values as if they were paired runs.

Criterion was considered. This initial suite needs a few wall-clock samples and simple percentiles, so it uses `std::time` and the existing temporary-fixture dependency. Consider Criterion when statistically rigorous regression tracking justifies its additional development dependencies. UI frame-time benchmarks and historical GPU measurements are independent of this engine suite.


## 10. Stage 03 workspace measurements

Measure the release application with a stable repository selected over 60-second windows. Record actual windowing protocol, renderer and GPU, process CPU, private pages, time to an interactive empty workspace, time to a loaded workspace, switch time and filesystem-to-view latency. Measure a clean fixture separately from noisy repositories. Git completions and debounced filesystem notifications drive refresh, and startup, transfer progress, and active text editing may legitimately wake the application. Manual refresh and focus recovery require no polling. Audits and screenshots are stored in target/stage03-evidence.

A point check on a 1200 × 800 Mutter 51.1 virtual monitor used Intel RPL-P and wgpu/Vulkan. The release with a stable repository open stayed below the process CPU tick resolution for both 60-second intervals, after five seconds of settling. Private memory was 74,228 KiB in Wayland and 88,160 KiB in XWayland. In the same virtual compositor the archived Stage 01.4 empty workspace measured 0.0167% and 0.0000% respectively, with 71,692 KiB and 72,624 KiB of private memory. The workloads differ and the single sample per case gives no distribution estimate.

Time to the visible workspace via AT-SPI was 273 ms in Wayland and 664 ms in XWayland, with 257 ms and 353 ms for the archived binary. On XWayland the end-to-end opening flow took 1,409 ms including authorization, a trusted switch took 611 ms, an external change appeared after 349 ms, and the clone cancellation outcome appeared after 288 ms. These wall times include the harness's interaction and observation delays. The default locked release is 30,408,248 bytes, up from the archived release's 28,570,200 bytes and above the exploratory 30 MB target. See `target/stage03-evidence/RELATORIO.md` for the individual results and provenance.

### Stage 03.1 release size

With Rust 1.99.1 and the locked release settings, the Stage 02 base (`938c8b9`) built to 28,570,216 bytes and the reviewed Stage 03 release built to 30,426,280 bytes. The difference includes 1,009,152 bytes of executable code and 454,593 bytes of symbol and string tables. Linux linker maps attribute retained input sections of 327,703 bytes to `notify`, 114,214 to `rfd`, 58,377 to `url`, 7,248 to `crossbeam-channel`, and 4,096 to `directories`. They attribute another 159,763 bytes to the transitive `idna` crate and 286,653 bytes to the engine now linked into the desktop app. Inlining, shared code, alignment and linker-generated sections limit the precision of this attribution. The review kept the existing release profile and audited dependencies. Local measurement files are in `target/stage03.1-evidence/`.

The reviewed Linux release completed 18 process measurements of 60 seconds on Intel RPL-P and wgpu/Vulkan in private Mutter 51.1 sessions. Each protocol had three trials per scenario. Empty and open workspaces reached at most 0.0167% process CPU. The real repository search field with AT-SPI focus reached 0.5333% in Wayland and 0.6333% in XWayland. The highest five-second interval was 0.8000%. The raw samples and memory breakdown are recorded in `target/stage03.1-evidence/RELATORIO.md`.
