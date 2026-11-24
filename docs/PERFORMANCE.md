# Performance Targets and Measurement Methodology

This document establishes the exploratory performance objectives for **gitRUST** and defines a reproducible methodology for measuring memory, CPU, startup latency, and binary size across platforms.

---

## 1. Exploratory Performance Targets

The following values are **exploratory engineering targets**, not hard guarantees across all GPU drivers or operating systems:

| Metric | Exploratory Target | Scope / Condition |
| --- | --- | --- |
| **Idle RAM (`RSS`)** | `<= 100 MB` | Settled desktop window in idle state (see GPU driver note below) |
| **Idle CPU** | `< 1%` | Settled desktop window with no user input over a 10-second observation window |
| **Startup Latency** | `<= 1.0 s` | Warm launch from process invocation to first rendered desktop frame |
| **Release Executable** | `<= 30 MB` | Compiled with `cargo build --workspace --release` |

### Important Note on GPU Driver Memory Accounting (`VmRSS` vs. Private Anonymous Memory)

Hardware-accelerated desktop applications using Vulkan (`wgpu`), Metal, or DirectX 12 map shared GPU driver libraries (`libvulkan_*`, Mesa/LLVM shader compilers, or proprietary NVIDIA ICDs) and shared VRAM/DMA-BUF pages into the process address space. On Linux systems with Mesa/LLVM or proprietary NVIDIA drivers, `VmRSS` includes shared read-only `.so` mappings (`RssFile` / `RssShmem`) loaded by the graphics driver, whereas gitRUST's own heap/anonymous allocation (`RssAnon`) is typically a small fraction of total `VmRSS`.

Therefore, **both total `VmRSS` and private anonymous memory (`RssAnon`)** must be recorded during Linux measurements so driver overhead is distinguished from application allocations.

---

## 2. Anti-Premature-Optimization Policy

Per project engineering principles:

- **Stability and diagnostics come first:** Stage 00 uses standard Cargo `release` defaults (`panic = "unwind"`, unstripped symbol tables unless explicitly packaged in Stage 10) so crash backtraces and profiling tools (`perf`, `samply`, `heaptrack`) remain accurate.
- **Architectural efficiency over micro-tricks:** Low idle CPU is achieved structurally by event-driven `egui`/`eframe` rendering (`ControlFlow::Wait`, zero unconditional `request_repaint()` calls), and laptop battery efficiency is preserved by defaulting `wgpu` to `PowerPreference::LowPower`.

---

## 3. Reproducible Measurement Protocol

Every benchmark report must record the complete test context:

1. **System & OS:** Distribution/version, kernel version, CPU model, total RAM.
2. **Windowing & Graphics:** Display protocol (`Wayland` vs. `X11` / `Win32` / `AppKit`), compositor, active GPU adapter name, driver version, and `wgpu` backend (`Vulkan`, `Dx12`, `Metal`, `Gl`).
3. **Build Metadata:** `rustc --version`, commit hash, Cargo profile (`--release`), target triple, and exact binary byte size (`stat -c %s target/release/gitrust` on Linux).
4. **Application State:** Cold start vs. warm start (after 1 warm-up run), window dimensions (`960x620` default), and idle duration before sampling (minimum 3 seconds after initial frame).

### Linux Reference Measurement Commands

```bash
# 1. Measure release binary size
stat -c "%s bytes" target/release/gitrust
ls -lh target/release/gitrust

# 2. Measure warm startup + frame/resize lifecycle via --smoke-test
#    (Subtract the intentional 250ms smoke-test dwell interval when evaluating pure init time,
#     or observe timestamp delta to "Initialized gitRUST desktop window")
/usr/bin/time -v ./target/release/gitrust --smoke-test

# 3. Measure settled idle RAM (VmRSS + RssAnon + RssFile) and idle CPU over 5 seconds
./target/release/gitrust &
APP_PID=$!
sleep 3
grep -E 'VmRSS|RssAnon|RssFile|RssShmem|Threads' /proc/"$APP_PID"/status
ps -p "$APP_PID" -o pid,%cpu,rss,vsz,comm
sleep 5
ps -p "$APP_PID" -o pid,%cpu,rss,vsz,comm
kill -TERM "$APP_PID"
wait "$APP_PID"
```

---

## 4. Stage 00 Reference Host Baseline (Measured)

Recorded on `2026-11-24` (`rustc 1.99.1`, `cargo build --workspace --release`, commit baseline on `feat/project-foundation`):

- **Host:** Fedora Linux 45 (Workstation Edition), Kernel `7.3.4-300.fc45.x86_64`, GNOME/Mutter Wayland session (`XWayland` also active).
- **GPU Adapter Selected (`LowPower` default):** `Intel(R) Graphics (RPL-P)` (`IntegratedGpu`), Mesa `26.2.7`, `wgpu` `Vulkan` backend (secondary discrete `NVIDIA GeForce RTX 3050 6GB Laptop GPU` also present on host).

| Metric | Target | Measured (Wayland Native) | Measured (X11 via XWayland) | Notes |
| --- | --- | --- | --- | --- |
| **Release Binary Size** | `<= 30 MB` | **`26.72 MB`** (`26,721,952` bytes / `25.48 MiB`) | Same binary | Unstripped `release` profile (diagnostics preserved) |
| **Window + Vulkan Init** | `<= 1.0 s` | **`0.130 s`** | **`0.141 s`** | Warm process start to initialized `wgpu` window; full `--smoke-test` teardown on hybrid Optimus host takes `~3.7 s` due to Vulkan ICD/PCIe probe |
| **Idle CPU (`5 s` window)** | `< 1%` | **`0.000%`** (`0` ticks / `5.01 s`) | `3.742%` (XWayland translation overhead) | Native Wayland achieves zero wakeups when idle |
| **Idle Memory (`RssAnon` / `VmRSS`)** | `<= 100 MB` | **`36.38 MB` `RssAnon`** (`154.12 MB` total `VmRSS`) | **`36.41 MB` `RssAnon`** (`155.40 MB` total `VmRSS`) | Private anonymous heap/stack is `36.4 MB`; `117.1 MB` of `VmRSS` is shared read-only `RssFile` pages from Vulkan/Mesa/LLVM system libraries |

---

## 5. Future Micro- and Macro-Benchmarks (Stage 02 & Stage 09)

When `gitrust-git` and `gitrust-ui` diff virtualization are implemented:
- `benches/` will use `criterion` to measure deterministic non-UI workloads (`status --porcelain=v2 -z` parsing, unified diff chunking, commit graph indexing) against synthetic repositories of `1k`, `10k`, and `100k` tracked files.
- UI frame-time percentiles (`p50`, `p95`, `p99`) will be measured during continuous scrolling of large diffs and commit histories.
