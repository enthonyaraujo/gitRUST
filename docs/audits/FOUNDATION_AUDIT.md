# Stage 00.1 — Independent Foundation Audit and Hardening Report

- **Audit Date:** `2026-11-24` to `2026-11-25`
- **Audited Branch:** `feat/project-foundation` (baseline commit `fad1832`)
- **Audit Host:** Fedora Linux 45 Workstation (`7.3.4-300.fc45.x86_64`, `x86_64-unknown-linux-gnu`), GNOME/Mutter (`Wayland` + `XWayland`), Intel Raptor Lake-P iGPU (`Mesa 26.2.7`) + NVIDIA GeForce RTX 3050 Laptop dGPU (`620.70.06`), `rustc 1.99.1` & `rustc 1.97.1`, `git 2.56.1`.
- **Scope:** Architecture, dependencies and licenses, toolchain/MSRV, graphics backends (Wayland, X11, Intel iGPU, NVIDIA dGPU, Vulkan, OpenGL ES), reproducible performance benchmarks, security posture, GitHub Actions CI, and real visual execution evidence.

---

## 1. Executive Summary

The Stage 00 foundation established a clean 100% native Rust (`egui`/`eframe` + `wgpu`) Cargo Workspace with `unsafe_code = "forbid"` and zero web/WebView dependencies. However, independent verification uncovered **1 High**, **4 Medium**, **3 Low**, and **1 Informational** finding—most critically that `WINIT_UNIX_BACKEND=x11` had been removed in `winit 0.29+` (causing false X11 reporting while running on Wayland), that `WGPU_POWER_PREFERENCE` was misnamed (`wgpu` reads `WGPU_POWER_PREF`), that MSRV `1.95` was declared without validation (`1.97.1` is locally validated and now enforced in CI), and that third-party embedded font attributions (`epaint_default_fonts`) and SHA-pinned GitHub Actions were missing.

All **High**, **Medium**, and **Low** defects confirmed within Stage 00 scope were resolved and revalidated with `--locked` builds across Rust `1.99.1` and MSRV `1.97.1`, real GPU screenshots across 6 runtime configurations, and 60-second settled idle benchmarks. Two external/environmental limitations remain transparently documented: local absence of `cargo-deny`/`cargo-audit` binaries (and no Windows/macOS local hosts for live GUI execution prior to remote CI execution) and an upstream Mesa Intel ANV Vulkan + XWayland swapchain reconfiguration behavior (`5.48%` idle CPU under XWayland Intel Vulkan vs. `0.000%` under native Wayland, X11 Intel GLES, and X11 NVIDIA Vulkan).

---

## 2. Findings Registry

### `FA-001` — False X11 Detection via Removed `WINIT_UNIX_BACKEND` Environment Variable

- **Severity:** `High`
- **Description:** `gitrust-app` read `WINIT_UNIX_BACKEND` and `WAYLAND_DISPLAY`/`DISPLAY` to infer the active windowing protocol, and `docs/BUILD.md` / `ADR-0005` instructed testers to use `WINIT_UNIX_BACKEND=x11`. Upstream `winit` removed `WINIT_UNIX_BACKEND` in `v0.29.0`.
- **Objective Evidence:**
  - `winit-0.30.13/src/changelog/v0.29.md:134`: `Breaking: WINIT_UNIX_BACKEND was removed in favor of standard WAYLAND_DISPLAY and DISPLAY variables.`
  - Previous Stage 00 runs with `WINIT_UNIX_BACKEND=x11` logged `session=X11 (forced via WINIT_UNIX_BACKEND)` while `xprop -root _NET_CLIENT_LIST` found zero X11 windows because `winit` had opened a native Wayland surface.
- **Cause:** Relying on deprecated environment variables instead of inspecting the OS display handle actually created by `winit`/`eframe`.
- **Fix Applied:**
  - Replaced `HostDiagnostics::detect` string heuristics in `crates/gitrust-core/src/lib.rs` with typed, allocation-free `WindowSessionProtocol` (`Wayland`, `X11`, `Win32`, `AppKit`, `HeadlessOrUnknown`).
  - Implemented `detect_verified_session_protocol(cc)` in `crates/gitrust-app/src/main.rs` reading `cc.display_handle().map(|h| h.as_raw())` via `eframe::wgpu::rwh::HasDisplayHandle` (`RawDisplayHandle`).
  - Updated `docs/BUILD.md`, `docs/ARCHITECTURE.md`, and `docs/adr/0005-cross-platform-strategy.md` to document `env -u WAYLAND_DISPLAY` for X11/XWayland testing.
- **Revalidation Result:** Confirmed real X11 window creation via `env -u WAYLAND_DISPLAY`: `RawDisplayHandle::Xlib` reported `session="X11"`, `xprop` matched window `0x1a00004` (`WM_NAME="gitRUST 0.1.0-alpha.0"`, `WM_CLASS="", "gitrust"`, `WM_NORMAL_HINTS`), and real GPU screenshot `capture_04_x11_native_880x500.png` displayed `Windowing Session: X11`.
- **Final Status:** `Resolved`

---

### `FA-002` — Declared MSRV `1.95` Not Validated Locally or Enforced in CI

- **Severity:** `Medium`
- **Description:** `Cargo.toml`, `README.md`, `docs/BUILD.md`, and `ADR-0001` declared MSRV `1.95`, while local tests ran only on `1.99.1` and `.github/workflows/ci.yml` tested only `@stable`.
- **Objective Evidence:**
  - `rustup show` lists installed toolchains `stable` (`1.99.1`) and `1.97.1`; `1.95.0` is absent locally (global installation prohibited without authorization).
  - `cargo metadata --locked` across Linux, Windows, and macOS confirms the highest dependency `rust-version` is `1.95` (`egui`/`eframe`/`epaint`/`emath` `0.37.0`).
- **Cause:** Setting `rust-version` to the upstream crate floor without verifying compilation on that compiler version or gating it in CI.
- **Fix Applied:**
  - Set `[workspace.package] rust-version = "1.97.1"` in `Cargo.toml`—matching the lowest Rust toolchain actually compiled and tested end-to-end.
  - Added a dedicated `msrv` job (`toolchain: "1.97.1"`) in `.github/workflows/ci.yml` running `cargo check --locked --workspace --all-targets` and `cargo test --locked --workspace`.
  - Documented the Toolchain & MSRV Update Policy in `docs/BUILD.md`, `README.md`, and `ADR-0001`, explicitly distinguishing upstream floor (`1.95.0`) from validated workspace MSRV (`1.97.1`).
- **Revalidation Result:** `cargo +1.97.1 check --locked --workspace --all-targets` and `cargo +1.97.1 test --locked --workspace` both passed (`14/14` tests) with exit code `0`.
- **Final Status:** `Resolved`

---

### `FA-003` — Incorrect GPU Power-Preference Environment Variable (`WGPU_POWER_PREFERENCE` vs. `WGPU_POWER_PREF`)

- **Severity:** `Medium`
- **Description:** Code comments (`crates/gitrust-app/src/main.rs`) and `docs/BUILD.md` documented `WGPU_POWER_PREFERENCE`, whereas `wgpu::PowerPreference::from_env()` reads `WGPU_POWER_PREF`.
- **Objective Evidence:**
  - `egui-wgpu-0.37.0/src/setup.rs:50` documents `WGPU_POWER_PREF: low, high or none`.
  - Running with `WGPU_POWER_PREFERENCE=high` selected `Intel(R) Graphics (RPL-P) [IntegratedGpu]`, whereas `WGPU_POWER_PREF=high` immediately selected `NVIDIA GeForce RTX 3050 6GB Laptop GPU [DiscreteGpu]`.
- **Cause:** Unverified environment variable name copied into docs/comments without checking `wgpu` source behavior.
- **Fix Applied:** Updated `crates/gitrust-app/src/main.rs`, `docs/BUILD.md`, `docs/ARCHITECTURE.md`, and `docs/adr/0005-cross-platform-strategy.md` to reference `WGPU_POWER_PREF`.
- **Revalidation Result:** Verified dedicated NVIDIA RTX 3050 execution (`capture_05_wayland_nvidia_dgpu.png` and `N=5` warm benchmarks on NVIDIA Vulkan).
- **Final Status:** `Resolved`

---

### `FA-004` — Unpinned Mutable Action Tags in GitHub Actions CI

- **Severity:** `Medium`
- **Description:** `.github/workflows/ci.yml` referenced external actions by mutable branch/major tags (`@v4`, `@v2`, `@stable`) instead of immutable 40-character commit SHAs.
- **Objective Evidence:** `.github/workflows/ci.yml` lines 27, 50, 55, 76, 100, 103, 120, 125, 128 prior to hardening.
- **Cause:** Initial workflow authoring used major tags instead of commit-SHA pinning recommended for supply-chain integrity.
- **Fix Applied:** Resolved verified upstream commit SHAs via `git ls-remote` and pinned all 4 actions by full 40-char SHA (`actions/checkout@11d5960a...`, `dtolnay/rust-toolchain@df2a88e9...` with explicit `toolchain:` inputs, `Swatinem/rust-cache@d1527c8f...`, `EmbarkStudios/cargo-deny-action@81891abc...`), enforced `--locked` on all Cargo commands, and preserved `permissions: contents: read` + `persist-credentials: false`.
- **Revalidation Result:** Workflow YAML syntax, SHAs, and minimal permissions inspected directly. **Note:** Remote GitHub Actions execution remains pending until branch push is authorized; CI is **not** claimed as executed remotely.
- **Final Status:** `Resolved Locally (Remote CI Execution Pending Push Authorization)`

---

### `FA-005` — License Count Discrepancy (`263` vs. `253`), Unused `deny.toml` Allowances, and Missing Third-Party Font Notices

- **Severity:** `Medium`
- **Description:**
  1. The Stage 00 report cited `263` Linux packages while its manual prose summary totaled `253` (`6` internal workspace crates were merged into `MIT`, and `10` external crates with slash/Zlib SPDX expressions were omitted in manual summation).
  2. `deny.toml` permitted `MPL-2.0`, `CC0-1.0`, and `Unicode-DFS-2016` despite zero dependencies requiring them.
  3. Embedded default fonts in `epaint_default_fonts 0.37.0` (`Ubuntu-font-1.0`, `OFL-1.1`, `MIT`) and dual-licensed crates (`self_cell` under `Apache-2.0 OR GPL-2.0-only`; `r-efi` under `MIT OR Apache-2.0 OR LGPL-2.1-or-later`) lacked a repository-level attribution and license-election record.
- **Objective Evidence:** Reproducible `cargo metadata --locked --filter-platform <target>` and `cargo tree --locked --target <target>` inventory:
  - `x86_64-unknown-linux-gnu`: `263` metadata packages (`6` internal + `257` external; `245` active = `6` internal + `232` runtime + `7` build-only; `12` duplicate crate names).
  - `x86_64-pc-windows-msvc`: `172` metadata packages (`6` internal + `166` external; `152` active = `6` internal + `140` runtime + `6` build-only; `5` duplicate crate names).
  - `aarch64-apple-darwin` & `x86_64-apple-darwin`: `181` metadata packages (`6` internal + `175` external; `150` active = `6` internal + `141` runtime + `3` build-only; `11` duplicate crate names).
- **Cause:** Manual grouping arithmetic error in the Stage 00 report, conservative initial `deny.toml` template, and omission of a binary redistribution attribution file.
- **Fix Applied:**
  - Removed unused `MPL-2.0`, `CC0-1.0`, and `Unicode-DFS-2016` from `deny.toml`.
  - Created `THIRD_PARTY_NOTICES.md` documenting embedded font copyrights/licenses (`Ubuntu-Light.ttf`, `NotoEmoji-Regular.ttf`, `Hack-Regular.ttf`, `emoji-icon-font.ttf`) and explicit permissive license elections (`Apache-2.0` for `self_cell`, `MIT` for `r-efi`).
  - Reconciled exact per-target inventories with `sum(licenses) == total` verified programmatically.
- **Revalidation Result:** Programmatic SPDX check confirms `100%` of external crates across Linux, Windows, and macOS satisfy `deny.toml`. Local vulnerability scanning via `cargo-deny`/`cargo-audit` could not run because neither tool is installed locally (no claim of zero CVEs is made without advisory DB execution).
- **Final Status:** `Resolved (Local cargo-deny/cargo-audit Tooling Pending Authorization or CI Execution)`

---

### `FA-006` — `GitCommandSpec::new` Permitted Leading-Dash Option Injection in Subcommand Slot

- **Severity:** `Low`
- **Description:** `GitCommandSpec::new` in `crates/gitrust-git/src/lib.rs` validated only non-empty and NUL-free subcommands, allowing inputs like `"-c"` or `"--upload-pack=..."` as the subcommand name. Additionally, `repository_path()` returned `&PathBuf` instead of `&Path` and did not check the repository path for embedded NUL bytes.
- **Objective Evidence:** `crates/gitrust-git/src/lib.rs:89-135` prior to hardening.
- **Cause:** Overly permissive subcommand typing (`impl AsRef<OsStr>` without lexical command validation).
- **Fix Applied:** Restricted `subcommand` to `&str` validated against `^[a-z][a-z0-9-]*$` (`CommandSpecError::InvalidSubcommand`), added NUL-byte validation on `repository_path`, changed `repository_path(&self) -> &Path`, and added unit tests covering malicious subcommands (`-c`, `--upload-pack=evil`, `--exec-path=/tmp`, `status;id`, `../git`).
- **Revalidation Result:** `gitrust-git` unit tests (`4/4`) pass on Rust `1.99.1` and `1.97.1`.
- **Final Status:** `Resolved`

---

### `FA-007` — Artificial Runtime Coupling in `gitrust-app` and Per-Frame UI String Allocations

- **Severity:** `Low`
- **Description:**
  1. `crates/gitrust-app/Cargo.toml` listed `gitrust-git`, `gitrust-storage`, and `gitrust-providers` under `[dependencies]` with a dead `let _ = (...)` tuple in `main.rs` despite Stage 00 executing no Git, storage, or provider runtime code.
  2. `crates/gitrust-ui/src/lib.rs` formatted 7 `String`s and allocated a `Vec<AppEvent>` container signature on every UI pass.
- **Objective Evidence:** `crates/gitrust-app/Cargo.toml:20-26`, `crates/gitrust-app/src/main.rs:138-143`, and `crates/gitrust-ui/src/lib.rs:33-195` prior to hardening.
- **Cause:** Over-eager workspace wiring and stateless formatting inside the immediate-mode render loop.
- **Fix Applied:**
  - Moved `gitrust-git`, `gitrust-storage`, and `gitrust-providers` to `[dev-dependencies]` in `crates/gitrust-app/Cargo.toml` (still verified by `workspace_integration.rs`) and removed the dead `let _ = (...)` tuple from `main.rs`.
  - Replaced stateless `render_foundation_view` with `FoundationView` caching static display labels once at startup, updating `footer_label` only on viewport dimension change, and returning stack-allocated `FrameEvents` (`Option<AppEvent>` slots, zero heap allocation).
- **Revalidation Result:** `cargo clippy --locked --workspace --all-targets -- -D warnings` and `cargo test --locked --workspace` pass cleanly.
- **Final Status:** `Resolved`

---

### `FA-008` — Default Window Height Clamped on `1080p @ 200%` HiDPI Screens

- **Severity:** `Low`
- **Description:** `DEFAULT_WINDOW_SIZE` (`960×620` pt) exceeded the `960×540` pt logical desktop height of `1920×1080` displays at `200%` scaling, causing GNOME Mutter to clamp initial geometry to `960×540` and ignore `1024×660` smoke-test resizes.
- **Objective Evidence:** Smoke-test logs prior to hardening showed `viewport_width=960.0 viewport_height=540.0` despite requesting `960×620` and resizing to `1024×660`.
- **Cause:** Choosing `620` pt initial height without accounting for `200%` HiDPI 1080p laptop viewports.
- **Fix Applied:** Adjusted `DEFAULT_WINDOW_SIZE` to `[880.0, 500.0]` and `MIN_WINDOW_SIZE` to `[640.0, 400.0]`, and tuned `DEFAULT_SMOKE_RESIZE` to `[940.0, 520.0]`.
- **Revalidation Result:** Both `880×500` initial, `640×400` minimum, and `940×520` resized (X11) rendered without clamping and were visually verified via real GPU screenshots.
- **Final Status:** `Resolved`

---

### `FA-009` — Upstream Mesa Intel ANV Vulkan + XWayland Idle CPU (`5.48%`) and Optimus Cold-Wake Latency

- **Severity:** `Informational` *(External Driver / Compositor Interaction)*
- **Description:** Under the specific combination `X11 (via GNOME XWayland @ 2x) + Mesa Intel ANV Vulkan (Mesa 26.2.7)`, idle CPU measured `5.4833%` over `60 s` with `+3.47 MB` `RssAnon` growth and a `3033.8 ms` single shutdown outlier. Additionally, when the host's NVIDIA RTX 3050 is in PCIe `D3cold` (`suspended`), Vulkan loader ICD probing (`nvidia_icd.json`) adds a one-time `~3.2 s` cold wakeup latency.
- **Objective Evidence:** Controlled matrix on the identical binary:
  - `Wayland + Intel Vulkan`: **`0.0000%`** CPU (`60.00 s`, `0` ticks, `0 kB` drift), `69.6 ms` median shutdown.
  - `Wayland + Intel GLES (WGPU_BACKEND=gl)`: **`0.0000%`** CPU (`15.00 s`, `0` ticks), `38.6 ms` median shutdown.
  - `Wayland + NVIDIA Vulkan`: **`0.000%`** CPU (`6.00 s`, `0` ticks), `126.4 ms` median shutdown.
  - `X11 + Intel GLES (WGPU_BACKEND=gl)`: **`0.000%`** CPU (`6.00 s`, `0` ticks).
  - `X11 + NVIDIA Vulkan (WGPU_POWER_PREF=high)`: **`0.000%`** CPU (`6.00 s`, `0` ticks).
  - `X11 (XWayland) + Intel Vulkan`: **`5.4833%`** CPU (`60.00 s`, `329` ticks).
- **Cause:** Upstream Mesa Intel ANV Vulkan WSI on GNOME XWayland returning `Suboptimal`/`Outdated` swapchain status (`egui-wgpu-0.37.0/src/winit.rs:608-617`), not an application-level repaint loop.
- **Mitigation / Recommendation:** Keep native Wayland as default on Wayland sessions (`0.0000%` CPU), document `WGPU_BACKEND=gl` as a verified user workaround for affected XWayland/Intel Mesa setups in `docs/PERFORMANCE.md`, and re-test against future Mesa/`wgpu` releases in Stage 09.
- **Final Status:** `Documented (Upstream Environment Limitation)`

---

## 3. Discarded Hypotheses

- **`DISC-001` — "Application UI causes continuous repaint on X11":** Discarded. Controlled testing proved `0.000%` idle CPU on `X11 + Intel GLES`, `X11 + NVIDIA Vulkan`, and all Wayland configurations, plus headless `egui` tests confirming `repaint_delay == Duration::MAX`.
- **`DISC-002` — "Application crashes/panics when graphics or display server initialization fails":** Discarded. Tested both headless launch (`env -u WAYLAND_DISPLAY -u DISPLAY`) and forced Vulkan with missing ICD (`VK_ICD_FILENAMES=/nonexistent.json WGPU_BACKEND=vulkan`); both returned clean structured error logs and exit code `1` with zero panics. When Vulkan ICDs are missing under default settings, `wgpu` automatically fell back to OpenGL ES (`backend=Gl`) and launched cleanly.
- **`DISC-003` — "Web/WebView crates are compiled into the native binary":** Discarded. `cargo tree --locked --target <native>` confirmed zero `web-sys`, `js-sys`, `wasm-bindgen`, `wry`, `tauri`, or `webkit2gtk` crates on Linux, Windows, and macOS.

---

## 4. Security Controls: Implemented vs. Documented Only

| Security Control | Status in Stage 00.1 | Objective Basis |
| --- | --- | --- |
| Workspace `unsafe_code = "forbid"` | **Implemented & Enforced (Compile-Time)** | Root `Cargo.toml` `[workspace.lints.rust]` inherited by all 6 crates. |
| Zero web/WebView stack | **Implemented & Enforced** | `eframe` `default-features = false`, verified by `cargo tree`. |
| Shell-free `GitCommandSpec` + subcommand injection rejection + non-UTF-8 `OsString`/`PathBuf` | **Implemented (Contract Layer Only; Execution in Stage 02)** | `crates/gitrust-git/src/lib.rs` + 4 unit tests. |
| Non-sensitive diagnostic logs (no env/secret dumps) | **Implemented** | `crates/gitrust-app/src/main.rs` logs only version, OS, protocol, and GPU adapter. |
| Telemetry disabled by default | **Implemented** | No network crates compiled; `telemetry_enabled = false`. |
| GitHub Actions minimal permissions & SHA pinning | **Implemented in Workflow (Pending Remote Run)** | `.github/workflows/ci.yml` (`contents: read`, `persist-credentials: false`, 40-char SHAs). |
| Untrusted repository `.git/config`, hook, filter, and helper defenses | **Documented Only (Target: Stage 02 / Stage 09)** | `docs/SECURITY_ARCHITECTURE.md`; no `git` subprocess execution exists yet. |
| OS Keychain credential storage & strict TLS HTTP client | **Declarative Policy Constants Only (Target: Stage 03 / Stage 08)** | `gitrust-storage` & `gitrust-providers` define policy types; no disk/network I/O exists yet. |
| Automated CVE / RustSec advisory verification (`cargo-audit` / `cargo-deny`) | **Configured in CI; Not Executed Locally** | `deny.toml` + `ci.yml`; binaries absent locally without installation authorization. |

---

## 5. Final Audit Verdict

**Conditionally Approved to Proceed to Stage 01 (Design System)** upon maintainer review and authorization of local commits/push:

- **Zero unresolved `Critical` or `High` issues.**
- All confirmed `High`, `Medium`, and `Low` engineering issues within Stage 00 scope are fixed and revalidated locally with `--locked` builds on Rust `1.99.1` and MSRV `1.97.1`.
- Remaining external gates—remote execution of `.github/workflows/ci.yml` on Linux/Windows/macOS runners and live `cargo-deny` / `cargo-audit` advisory database verification—will execute immediately upon maintainer-authorized push to GitHub (or locally if installation of `cargo-deny`/`cargo-audit` is authorized).
