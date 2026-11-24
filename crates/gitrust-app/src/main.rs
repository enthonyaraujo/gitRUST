//! Native desktop entrypoint for `gitRUST` (Stage 00 — Project Foundation).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;
use std::time::{Duration, Instant};

use eframe::egui;
use gitrust_core::{
    AppMetadata, AppState, DEFAULT_WINDOW_SIZE, GraphicsDiagnostics, HostDiagnostics,
    MIN_WINDOW_SIZE, PACKAGE_NAME,
};
use gitrust_ui::{configure_provisional_visuals, render_foundation_view};

/// Build-time package version sourced directly from `Cargo.toml`.
const CARGO_VERSION: &str = env!("CARGO_PKG_VERSION");

/// CLI execution mode parsed without pulling external argument-parser crates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LaunchMode {
    /// Normal interactive desktop window execution.
    Interactive,
    /// Opens the real native window, verifies render + resize + idle behavior, and closes cleanly.
    SmokeTest,
    /// Prints version string to stdout and exits.
    PrintVersion,
    /// Prints usage summary to stdout and exits.
    PrintHelp,
}

fn parse_launch_mode<I, S>(args: I) -> Result<LaunchMode, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let mut mode = LaunchMode::Interactive;
    for arg in args.into_iter().skip(1) {
        let Some(flag) = arg.as_ref().to_str() else {
            return Err(String::from("unrecognized non-UTF-8 command-line option"));
        };
        match flag {
            "--version" | "-V" => return Ok(LaunchMode::PrintVersion),
            "--help" | "-h" => return Ok(LaunchMode::PrintHelp),
            "--smoke-test" => mode = LaunchMode::SmokeTest,
            other => {
                return Err(format!(
                    "unknown option '{other}'. Run '{PACKAGE_NAME} --help' for supported flags."
                ));
            }
        }
    }
    Ok(mode)
}

fn print_help(metadata: &AppMetadata) {
    println!(
        "{name} {version} — Stage {stage_id}: {stage_name}\n\n\
         USAGE:\n    {bin} [OPTIONS]\n\n\
         OPTIONS:\n    -h, --help          Show this help message and exit\n    \
         -V, --version       Show version information and exit\n        \
         --smoke-test    Launch native window, verify render/resize lifecycle, and exit cleanly",
        name = metadata.name,
        version = metadata.version,
        stage_id = metadata.stage_id,
        stage_name = metadata.stage_name,
        bin = metadata.package_name,
    );
}

fn init_logging() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .without_time()
        .try_init();
}

fn detect_host_diagnostics() -> HostDiagnostics {
    let wayland_present = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|val| !val.is_empty());
    let x11_present = std::env::var_os("DISPLAY").is_some_and(|val| !val.is_empty());
    let forced_backend = std::env::var("WINIT_UNIX_BACKEND").ok();

    HostDiagnostics::detect(wayland_present, x11_present, forced_backend.as_deref())
}

fn extract_graphics_diagnostics(cc: &eframe::CreationContext<'_>) -> GraphicsDiagnostics {
    let Some(render_state) = cc.wgpu_render_state.as_ref() else {
        return GraphicsDiagnostics::default();
    };

    let info = render_state.adapter.get_info();
    let driver_description = match (info.driver.is_empty(), info.driver_info.is_empty()) {
        (true, true) => String::from("System default driver"),
        (false, true) => info.driver,
        (true, false) => info.driver_info,
        (false, false) => format!("{} ({})", info.driver, info.driver_info),
    };

    GraphicsDiagnostics {
        renderer: String::from("wgpu"),
        backend_api: format!("{:?}", info.backend),
        adapter_name: info.name,
        device_type: format!("{:?}", info.device_type),
        driver_description,
    }
}

/// Smoke-test state machine used only when `--smoke-test` is passed on the command line.
#[derive(Debug, Clone, Copy)]
struct SmokeTestController {
    started_at: Instant,
    resized_dispatched: bool,
    close_dispatched: bool,
}

impl SmokeTestController {
    fn new() -> Self {
        Self {
            started_at: Instant::now(),
            resized_dispatched: false,
            close_dispatched: false,
        }
    }
}

/// `eframe` application wrapper binding [`AppState`] to the native window loop.
struct GitRustApp {
    state: AppState,
    smoke_test: Option<SmokeTestController>,
}

impl GitRustApp {
    fn new(cc: &eframe::CreationContext<'_>, metadata: AppMetadata, smoke_test: bool) -> Self {
        configure_provisional_visuals(&cc.egui_ctx);
        let host = detect_host_diagnostics();
        let graphics = extract_graphics_diagnostics(cc);

        // Verify compile-time linkage of architectural boundary crates without executing fake work.
        let _ = (
            gitrust_git::STAGE_STATUS,
            gitrust_storage::STAGE_STATUS,
            gitrust_providers::STAGE_STATUS,
        );

        tracing::info!(
            version = metadata.version,
            os = host.operating_system,
            session = %host.session_protocol,
            backend = %graphics.backend_api,
            adapter = %graphics.adapter_name,
            device_type = %graphics.device_type,
            "Initialized gitRUST desktop window"
        );

        Self {
            state: AppState::new(metadata, host, graphics),
            smoke_test: smoke_test.then(SmokeTestController::new),
        }
    }
}

impl eframe::App for GitRustApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let events = render_foundation_view(ui, &self.state);
        for event in events {
            self.state.apply(event);
        }

        let ctx = ui.ctx();
        if let Some(smoke) = &mut self.smoke_test {
            let elapsed = smoke.started_at.elapsed();
            if !smoke.resized_dispatched {
                smoke.resized_dispatched = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(
                    1024.0, 660.0,
                )));
                ctx.request_repaint_after(Duration::from_millis(250));
            } else if elapsed >= Duration::from_millis(250) && !smoke.close_dispatched {
                smoke.close_dispatched = true;
                tracing::info!(
                    viewport_width = self.state.viewport_size[0],
                    viewport_height = self.state.viewport_size[1],
                    "Smoke-test lifecycle verified; requesting clean window close"
                );
                self.state.apply(gitrust_core::AppEvent::RequestClose);
            } else if !smoke.close_dispatched {
                ctx.request_repaint_after(Duration::from_millis(250).saturating_sub(elapsed));
            }
        }

        if self.state.should_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn build_native_options() -> eframe::NativeOptions {
    let metadata = AppMetadata::new(CARGO_VERSION);

    let mut wgpu_options = eframe::egui_wgpu::WgpuConfiguration::default();
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(ref mut create_new) = wgpu_options.wgpu_setup {
        // Default to LowPower on hybrid laptops (e.g., Intel/AMD iGPU + discrete GPU) while
        // honoring user overrides via WGPU_POWER_PREFERENCE=high|low and supporting discrete GPUs.
        create_new.power_preference = eframe::wgpu::PowerPreference::from_env()
            .unwrap_or(eframe::wgpu::PowerPreference::LowPower);
    }

    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(metadata.window_title())
            .with_app_id(PACKAGE_NAME)
            .with_inner_size(DEFAULT_WINDOW_SIZE)
            .with_min_inner_size(MIN_WINDOW_SIZE)
            .with_resizable(true),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options,
        run_and_return: true,
        ..Default::default()
    }
}

fn main() -> ExitCode {
    let metadata = AppMetadata::new(CARGO_VERSION);

    let mode = match parse_launch_mode(std::env::args_os()) {
        Ok(mode) => mode,
        Err(message) => {
            eprintln!("error: {message}");
            return ExitCode::from(2);
        }
    };

    match mode {
        LaunchMode::PrintVersion => {
            println!("{} {}", metadata.name, metadata.version);
            ExitCode::SUCCESS
        }
        LaunchMode::PrintHelp => {
            print_help(&metadata);
            ExitCode::SUCCESS
        }
        LaunchMode::Interactive | LaunchMode::SmokeTest => {
            init_logging();
            let is_smoke_test = mode == LaunchMode::SmokeTest;
            let native_options = build_native_options();

            match eframe::run_native(
                PACKAGE_NAME,
                native_options,
                Box::new(move |cc| Ok(Box::new(GitRustApp::new(cc, metadata, is_smoke_test)))),
            ) {
                Ok(()) => ExitCode::SUCCESS,
                Err(err) => {
                    tracing::error!(error = %err, "Failed to run gitRUST desktop window");
                    ExitCode::FAILURE
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_cli_flags_and_rejects_unknown_ones() {
        assert_eq!(parse_launch_mode(["gitrust"]), Ok(LaunchMode::Interactive));
        assert_eq!(
            parse_launch_mode(["gitrust", "--version"]),
            Ok(LaunchMode::PrintVersion)
        );
        assert_eq!(
            parse_launch_mode(["gitrust", "-h"]),
            Ok(LaunchMode::PrintHelp)
        );
        assert_eq!(
            parse_launch_mode(["gitrust", "--smoke-test"]),
            Ok(LaunchMode::SmokeTest)
        );
        assert!(parse_launch_mode(["gitrust", "--unknown"]).is_err());
    }
}
