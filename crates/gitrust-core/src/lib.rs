//! Core state, metadata, runtime diagnostics, and event primitives for `gitRUST`.
//!
//! `gitrust-core` has zero external dependencies and no coupling to UI, storage,
//! or OS process execution layers. In Stage 00 (Project Foundation), it models
//! only verified application metadata, runtime graphics/session diagnostics, and
//! basic lifecycle events.

/// Official user-facing product name.
pub const APP_NAME: &str = "gitRUST";

/// Technical package and binary identifier.
pub const PACKAGE_NAME: &str = "gitrust";

/// SPDX license identifier for the project.
pub const APP_LICENSE: &str = "MIT";

/// Current development stage identifier.
pub const CURRENT_STAGE_ID: &str = "00";
/// Current development stage name.
pub const CURRENT_STAGE_NAME: &str = "Project Foundation";

/// Default logical window dimensions `(width, height)` in points.
///
/// Sized to fit comfortably within `1920×1080` displays running at `200%` `HiDPI`
/// scaling (`960×540` logical desktop) without compositor clamping, as well as
/// standard `100%`–`150%` desktop monitors.
pub const DEFAULT_WINDOW_SIZE: [f32; 2] = [880.0, 500.0];

/// Minimum logical window dimensions `(width, height)` in points.
pub const MIN_WINDOW_SIZE: [f32; 2] = [640.0, 400.0];

/// Build and identity metadata injected by the binary crate at startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppMetadata {
    /// Official application name (`gitRUST`).
    pub name: &'static str,
    /// Technical package/executable name (`gitrust`).
    pub package_name: &'static str,
    /// `SemVer` version defined in `Cargo.toml`.
    pub version: &'static str,
    /// SPDX license expression.
    pub license: &'static str,
    /// Current roadmap stage identifier.
    pub stage_id: &'static str,
    /// Current roadmap stage human-readable name.
    pub stage_name: &'static str,
}

impl AppMetadata {
    /// Constructs metadata using the binary crate's `Cargo.toml` version string.
    #[must_use]
    pub const fn new(version: &'static str) -> Self {
        Self {
            name: APP_NAME,
            package_name: PACKAGE_NAME,
            version,
            license: APP_LICENSE,
            stage_id: CURRENT_STAGE_ID,
            stage_name: CURRENT_STAGE_NAME,
        }
    }

    /// Formats the canonical window title for the desktop shell.
    #[must_use]
    pub fn window_title(&self) -> String {
        format!("{} {}", self.name, self.version)
    }
}

/// Graphics adapter and rendering backend diagnostics captured at startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphicsDiagnostics {
    /// Active `eframe` renderer (`wgpu`).
    pub renderer: String,
    /// Underlying graphics API (`Vulkan`, `Metal`, `Dx12`, `Gl`, etc.).
    pub backend_api: String,
    /// Adapter human-readable name reported by the driver.
    pub adapter_name: String,
    /// Adapter classification (`IntegratedGpu`, `DiscreteGpu`, `Cpu`, `VirtualGpu`, `Other`).
    pub device_type: String,
    /// Driver name and version string when reported by the platform.
    pub driver_description: String,
}

impl Default for GraphicsDiagnostics {
    fn default() -> Self {
        Self {
            renderer: String::from("wgpu"),
            backend_api: String::from("Unavailable (headless or uninitialized)"),
            adapter_name: String::from("Unavailable"),
            device_type: String::from("Unknown"),
            driver_description: String::from("Not reported"),
        }
    }
}

/// Windowing session protocol verified directly from the OS display handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowSessionProtocol {
    /// Native Wayland display handle (`RawDisplayHandle::Wayland`).
    Wayland,
    /// X11 (`Xlib` or `XCB`) display handle, either native Xorg or `XWayland`.
    X11,
    /// Microsoft Windows `Win32` display handle.
    Win32,
    /// Apple macOS `AppKit` display handle.
    AppKit,
    /// Headless test context or unrecognized platform handle.
    HeadlessOrUnknown,
}

impl WindowSessionProtocol {
    /// Human-readable protocol name without heap allocation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Wayland => "Wayland",
            Self::X11 => "X11",
            Self::Win32 => "Win32",
            Self::AppKit => "AppKit",
            Self::HeadlessOrUnknown => "Headless / Unattached",
        }
    }
}

/// Host operating system and verified display session information.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostDiagnostics {
    /// Target operating system (`linux`, `windows`, `macos`, etc.).
    pub operating_system: &'static str,
    /// Target CPU architecture (`x86_64`, `aarch64`, etc.).
    pub architecture: &'static str,
    /// Active windowing protocol verified from the runtime display handle.
    pub session_protocol: WindowSessionProtocol,
    /// Telemetry state (always disabled by default per project requirements).
    pub telemetry_enabled: bool,
}

impl HostDiagnostics {
    /// Constructs host diagnostics from the runtime-verified [`WindowSessionProtocol`].
    #[must_use]
    pub const fn from_protocol(session_protocol: WindowSessionProtocol) -> Self {
        Self {
            operating_system: std::env::consts::OS,
            architecture: std::env::consts::ARCH,
            session_protocol,
            telemetry_enabled: false,
        }
    }
}

/// Implementation status of a workspace crate in the current roadmap stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrateStageStatus {
    /// Implemented and active in Stage 00.
    ActiveInFoundation,
    /// Architectural boundary defined; functional implementation scheduled for a later stage.
    Planned {
        /// Target roadmap stage number (`2`..=`10`).
        target_stage: u8,
    },
}

/// Honest descriptor of a workspace crate's responsibility and current readiness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceCrateSummary {
    /// Cargo package name.
    pub name: &'static str,
    /// Concise architectural responsibility.
    pub responsibility: &'static str,
    /// Real implementation state in Stage 00.
    pub status: CrateStageStatus,
}

/// Canonical list of workspace crates and their truthful Stage 00 status.
pub const WORKSPACE_CRATES: [WorkspaceCrateSummary; 6] = [
    WorkspaceCrateSummary {
        name: "gitrust-app",
        responsibility: "Desktop bootstrap, window lifecycle, and wgpu initialization",
        status: CrateStageStatus::ActiveInFoundation,
    },
    WorkspaceCrateSummary {
        name: "gitrust-core",
        responsibility: "Application state, metadata, diagnostics, and event model",
        status: CrateStageStatus::ActiveInFoundation,
    },
    WorkspaceCrateSummary {
        name: "gitrust-ui",
        responsibility: "Provisional egui foundation view (Design System in Stage 01)",
        status: CrateStageStatus::ActiveInFoundation,
    },
    WorkspaceCrateSummary {
        name: "gitrust-git",
        responsibility: "Shell-free Git CLI boundary and future Gitoxide evaluation",
        status: CrateStageStatus::Planned { target_stage: 2 },
    },
    WorkspaceCrateSummary {
        name: "gitrust-storage",
        responsibility: "Local configuration persistence and OS keychain policy",
        status: CrateStageStatus::Planned { target_stage: 3 },
    },
    WorkspaceCrateSummary {
        name: "gitrust-providers",
        responsibility: "GitHub, GitLab, and remote hosting provider integrations",
        status: CrateStageStatus::Planned { target_stage: 8 },
    },
];

/// Events emitted by the UI or window shell and processed by [`AppState`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AppEvent {
    /// Updates the observed logical viewport size `(width, height)`.
    ViewportResized {
        /// Logical width in points.
        width: f32,
        /// Logical height in points.
        height: f32,
    },
    /// Requests a normal, user-initiated window close.
    RequestClose,
}

/// Root state for the Stage 00 foundation application.
#[derive(Debug, Clone, PartialEq)]
pub struct AppState {
    /// Static build and stage metadata.
    pub metadata: AppMetadata,
    /// Host OS and verified display session diagnostics.
    pub host: HostDiagnostics,
    /// Graphics adapter and backend diagnostics.
    pub graphics: GraphicsDiagnostics,
    /// Current logical viewport dimensions `(width, height)`.
    pub viewport_size: [f32; 2],
    /// Whether a graceful window shutdown has been requested.
    pub should_close: bool,
}

impl AppState {
    /// Creates a new Stage 00 application state.
    #[must_use]
    pub fn new(
        metadata: AppMetadata,
        host: HostDiagnostics,
        graphics: GraphicsDiagnostics,
    ) -> Self {
        Self {
            metadata,
            host,
            graphics,
            viewport_size: DEFAULT_WINDOW_SIZE,
            should_close: false,
        }
    }

    /// Applies an [`AppEvent`] deterministically without side effects.
    pub fn apply(&mut self, event: AppEvent) {
        match event {
            AppEvent::ViewportResized { width, height } => {
                if width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0 {
                    self.viewport_size = [width, height];
                }
            }
            AppEvent::RequestClose => {
                self.should_close = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_uses_official_names_and_mit_license() {
        let meta = AppMetadata::new("0.1.0-alpha.0");
        assert_eq!(meta.name, "gitRUST");
        assert_eq!(meta.package_name, "gitrust");
        assert_eq!(meta.license, "MIT");
        assert_eq!(meta.window_title(), "gitRUST 0.1.0-alpha.0");
    }

    #[test]
    fn host_diagnostics_reflect_verified_protocol_without_telemetry() {
        let wayland = HostDiagnostics::from_protocol(WindowSessionProtocol::Wayland);
        assert_eq!(wayland.session_protocol.as_str(), "Wayland");
        assert!(!wayland.telemetry_enabled);

        let x11 = HostDiagnostics::from_protocol(WindowSessionProtocol::X11);
        assert_eq!(x11.session_protocol.as_str(), "X11");
    }

    #[test]
    fn state_transitions_update_viewport_and_close_flag() {
        let mut state = AppState::new(
            AppMetadata::new("0.1.0-alpha.0"),
            HostDiagnostics::from_protocol(WindowSessionProtocol::Wayland),
            GraphicsDiagnostics::default(),
        );
        assert_eq!(state.viewport_size, DEFAULT_WINDOW_SIZE);
        assert!(!state.should_close);

        state.apply(AppEvent::ViewportResized {
            width: 1280.0,
            height: 800.0,
        });
        assert_eq!(state.viewport_size, [1280.0, 800.0]);

        // Invalid dimensions must be ignored safely.
        state.apply(AppEvent::ViewportResized {
            width: -10.0,
            height: f32::NAN,
        });
        assert_eq!(state.viewport_size, [1280.0, 800.0]);

        state.apply(AppEvent::RequestClose);
        assert!(state.should_close);
    }
}
