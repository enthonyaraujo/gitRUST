//! Native desktop entrypoint for `gitRUST` (Stage 01 — Design System).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod browser_oauth;
mod github_actor;
mod ime;
mod workspaces;

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use eframe::egui;
use eframe::wgpu::rwh::{HasDisplayHandle as _, RawDisplayHandle};
use gitrust_core::{
    AppMetadata, AppState, DEFAULT_WINDOW_SIZE, GraphicsDiagnostics, HostDiagnostics,
    MIN_WINDOW_SIZE, PACKAGE_NAME, WindowSessionProtocol,
};
use gitrust_ui::{Desktop, DiagnosticView, Language, UiOptions};

/// Build-time package version sourced directly from `Cargo.toml`.
const CARGO_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default resize target exercised during `--smoke-test` when no custom size is supplied.
///
/// Chosen to stay within `960×540` logical desktop bounds (`1920×1080` at `200%` `HiDPI`).
const DEFAULT_SMOKE_RESIZE: [f32; 2] = [940.0, 520.0];

/// Configuration for automated desktop window lifecycle verification (`--smoke-test`).
#[derive(Debug, Clone, PartialEq)]
struct SmokeTestOptions {
    target_size: Option<[f32; 2]>,
    initial_size: Option<[f32; 2]>,
    screenshot_ppm_path: Option<PathBuf>,
}

impl Default for SmokeTestOptions {
    fn default() -> Self {
        Self {
            target_size: Some(DEFAULT_SMOKE_RESIZE),
            initial_size: None,
            screenshot_ppm_path: None,
        }
    }
}

/// CLI execution mode parsed without pulling external argument-parser crates.
#[derive(Debug, Clone)]
enum LaunchMode {
    /// Normal interactive desktop window execution.
    Interactive(UiOptions),
    /// Opens the real native window, verifies render/resize lifecycle, and closes cleanly.
    SmokeTest(SmokeTestOptions, UiOptions),
    /// Prints version string to stdout and exits.
    PrintVersion,
    /// Prints usage summary to stdout and exits.
    PrintHelp,
}

fn parse_dimensions(raw: &str) -> Result<[f32; 2], String> {
    let Some((w_str, h_str)) = raw.split_once('x') else {
        return Err(format!(
            "invalid dimensions '{raw}'; expected '<width>x<height>' (for example, '640x400')"
        ));
    };
    let width: f32 = w_str
        .parse()
        .map_err(|_| format!("invalid width in '{raw}'"))?;
    let height: f32 = h_str
        .parse()
        .map_err(|_| format!("invalid height in '{raw}'"))?;
    if !width.is_finite() || !height.is_finite() || width < 100.0 || height < 100.0 {
        return Err(format!("dimensions '{raw}' must be finite and >= 100x100"));
    }
    Ok([width, height])
}

fn parse_launch_mode<I, S>(args: I) -> Result<LaunchMode, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let mut iter = args.into_iter().skip(1);
    let mut smoke_enabled = false;
    let mut ui_options = UiOptions::default();
    let mut smoke_opts = SmokeTestOptions::default();

    while let Some(arg) = iter.next() {
        let Some(flag) = arg.as_ref().to_str() else {
            return Err(String::from("unrecognized non-UTF-8 command-line option"));
        };
        match flag {
            "--version" | "-V" => return Ok(LaunchMode::PrintVersion),
            "--help" | "-h" => return Ok(LaunchMode::PrintHelp),
            "--diagnostics" => ui_options.diagnostics = Some(DiagnosticView::Gallery),
            "--diagnostic-view" => {
                ui_options.diagnostics = Some(match option(&mut iter, flag)?.as_str() {
                    "gallery" => DiagnosticView::Gallery,
                    "unified" => DiagnosticView::Unified,
                    "split" => DiagnosticView::Split,
                    "history" => DiagnosticView::History,
                    "settings" => DiagnosticView::Settings,
                    "keyboard" => DiagnosticView::Keyboard,
                    "compose" => DiagnosticView::Compose,
                    "compose-summary" => DiagnosticView::ComposeSummary,
                    "compose-description" => DiagnosticView::ComposeDescription,
                    "repository" => DiagnosticView::Repository,
                    "branch" => DiagnosticView::Branch,
                    "long-diff" => DiagnosticView::LongDiff,
                    "binary" => DiagnosticView::Binary,
                    "stress-diff" => DiagnosticView::StressDiff,
                    "stress-list" => DiagnosticView::StressList,
                    _ => return Err("unknown diagnostic view".into()),
                });
            }
            "--theme" => {
                ui_options.theme = match option(&mut iter, flag)?.as_str() {
                    "system" => egui::ThemePreference::System,
                    "light" => egui::ThemePreference::Light,
                    "dark" => egui::ThemePreference::Dark,
                    _ => return Err("--theme expects system, light or dark".into()),
                };
            }
            "--language" => ui_options.language = Language::from_locale(&option(&mut iter, flag)?),
            "--scale" => {
                ui_options.zoom = match option(&mut iter, flag)?.as_str() {
                    "100" => 1.0,
                    "125" => 1.25,
                    "150" => 1.5,
                    "200" => 2.0,
                    _ => return Err("--scale expects 100, 125, 150 or 200".into()),
                };
            }
            "--smoke-test" => smoke_enabled = true,
            "--smoke-initial-size" => {
                smoke_enabled = true;
                smoke_opts.target_size = None;
                smoke_opts.initial_size = None;
            }
            "--smoke-size" => {
                smoke_enabled = true;
                let Some(val) = iter.next() else {
                    return Err(String::from(
                        "missing '<width>x<height>' after --smoke-size",
                    ));
                };
                let Some(val_str) = val.as_ref().to_str() else {
                    return Err(String::from(
                        "--smoke-size requires UTF-8 '<width>x<height>'",
                    ));
                };
                smoke_opts.target_size = Some(parse_dimensions(val_str)?);
                smoke_opts.initial_size = smoke_opts.target_size;
            }
            "--smoke-screenshot" => {
                smoke_enabled = true;
                let Some(path) = iter.next() else {
                    return Err(String::from("missing file path after --smoke-screenshot"));
                };
                if path.as_ref().is_empty() {
                    return Err(String::from("--smoke-screenshot path must not be empty"));
                }
                smoke_opts.screenshot_ppm_path = Some(PathBuf::from(path.as_ref()));
            }
            other => {
                return Err(format!(
                    "unknown option '{other}'. Run '{PACKAGE_NAME} --help' for supported flags."
                ));
            }
        }
    }

    if smoke_enabled {
        Ok(LaunchMode::SmokeTest(smoke_opts, ui_options))
    } else {
        Ok(LaunchMode::Interactive(ui_options))
    }
}

fn option<S: AsRef<std::ffi::OsStr>>(
    iter: &mut impl Iterator<Item = S>,
    flag: &str,
) -> Result<String, String> {
    iter.next()
        .and_then(|arg| arg.as_ref().to_str().map(ToOwned::to_owned))
        .ok_or_else(|| format!("missing UTF-8 value for {flag}"))
}

fn print_help(metadata: &AppMetadata) {
    println!(
        "{name} {version} — Stage {stage_id}: {stage_name}\n\n\
         USAGE:\n    {bin} [OPTIONS]\n\n\
         OPTIONS:\n    -h, --help                      Show this help message and exit\n    \
         -V, --version                   Show version information and exit\n        \
         --theme <system|light|dark> Set the initial theme\n\
         --language <en-US|pt-BR>    Set the initial interface language\n\
         --scale <100|125|150|200>   Set the initial interface scale\n\
         --diagnostics              Open the component gallery and enable the sample repository\n\
         --diagnostic-view <VIEW>   Open a sample view or stress case.\n\
                                     gallery, unified, split, history, settings, keyboard, compose,\n\
                                     compose-summary, compose-description,\n\
                                     repository, branch, long-diff, binary, stress-diff, stress-list\n\
         --smoke-test                Launch native window, verify render/resize, and exit cleanly\n        \
         --smoke-initial-size        Keep default window size during --smoke-test\n        \
         --smoke-size <WxH>          Request logical viewport size '<WxH>' during --smoke-test\n        \
         --smoke-screenshot <PATH>   Write captured GPU framebuffer as binary PPM (P6) during --smoke-test",
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

/// Inspects the actual OS display handle opened by `winit`/`eframe` rather than
/// inferring the windowing protocol from environment variables.
fn detect_verified_session_protocol(cc: &eframe::CreationContext<'_>) -> WindowSessionProtocol {
    match cc.display_handle().map(|handle| handle.as_raw()) {
        Ok(RawDisplayHandle::Wayland(_)) => WindowSessionProtocol::Wayland,
        Ok(RawDisplayHandle::Xlib(_) | RawDisplayHandle::Xcb(_)) => WindowSessionProtocol::X11,
        Ok(RawDisplayHandle::Windows(_)) => WindowSessionProtocol::Win32,
        Ok(RawDisplayHandle::AppKit(_)) => WindowSessionProtocol::AppKit,
        _ => WindowSessionProtocol::HeadlessOrUnknown,
    }
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

fn write_ppm_screenshot(path: &Path, image: &egui::ColorImage) -> std::io::Result<()> {
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    write!(writer, "P6\n{} {}\n255\n", image.width(), image.height())?;
    for pixel in &image.pixels {
        writer.write_all(&[pixel.r(), pixel.g(), pixel.b()])?;
    }
    writer.flush()
}

/// Smoke-test state machine used only when `--smoke-test` is passed on the command line.
#[derive(Debug, Clone)]
struct SmokeTestController {
    started_at: Instant,
    options: SmokeTestOptions,
    resize_dispatched: bool,
    screenshot_requested: bool,
    screenshot_saved: Arc<AtomicBool>,
    close_dispatched: bool,
}

impl SmokeTestController {
    fn new(options: SmokeTestOptions) -> Self {
        Self {
            started_at: Instant::now(),
            options,
            resize_dispatched: false,
            screenshot_requested: false,
            screenshot_saved: Arc::new(AtomicBool::new(false)),
            close_dispatched: false,
        }
    }
}

/// `eframe` application wrapper binding [`AppState`] and [`Desktop`] to the native window loop.
struct GitRustApp {
    state: AppState,
    view: Desktop,
    smoke_test: Option<SmokeTestController>,
    wayland_ime: Option<ime::WaylandIme>,
    workspaces: Option<workspaces::Workspaces>,
    focus: bool,
    last_preferences: gitrust_core::workspaces::Preferences,
    preference_overrides: [bool; 3],
}

impl GitRustApp {
    fn new(
        cc: &eframe::CreationContext<'_>,
        metadata: AppMetadata,
        smoke_test: Option<SmokeTestOptions>,
        ui_options: UiOptions,
        oauth: Option<browser_oauth::Inbox>,
    ) -> Self {
        let protocol = detect_verified_session_protocol(cc);
        let host = HostDiagnostics::from_protocol(protocol);
        let graphics = extract_graphics_diagnostics(cc);

        tracing::info!(
            version = metadata.version,
            os = host.operating_system,
            session = host.session_protocol.as_str(),
            backend = %graphics.backend_api,
            adapter = %graphics.adapter_name,
            device_type = %graphics.device_type,
            "Initialized gitRUST desktop window"
        );

        let state = AppState::new(metadata, host, graphics);
        let mut view = Desktop::new(&cc.egui_ctx, ui_options);
        let workspaces = (ui_options.diagnostics.is_none() && smoke_test.is_none()).then(|| {
            view.enable_repository_management();
            workspaces::Workspaces::start(&cc.egui_ctx, oauth)
        });
        let last_preferences = view.preferences();
        let preference_overrides = interactive_preference_overrides(std::env::args_os());
        let wayland_ime = (protocol == WindowSessionProtocol::Wayland)
            .then(|| ime::WaylandIme::install(&cc.egui_ctx));

        Self {
            state,
            view,
            smoke_test: smoke_test.map(SmokeTestController::new),
            wayland_ime,
            workspaces,
            focus: false,
            last_preferences,
            preference_overrides,
        }
    }

    fn drive_smoke_test(&mut self, ctx: &egui::Context) {
        let Some(smoke) = &mut self.smoke_test else {
            return;
        };

        let elapsed = smoke.started_at.elapsed();
        if !smoke.resize_dispatched {
            smoke.resize_dispatched = true;
            if let Some([width, height]) = smoke.options.target_size {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(
                    width, height,
                )));
            }
            ctx.request_repaint_after(Duration::from_millis(250));
            return;
        }

        if elapsed < Duration::from_millis(250) {
            ctx.request_repaint_after(Duration::from_millis(250).saturating_sub(elapsed));
            return;
        }

        if let Some(ref out_path) = smoke.options.screenshot_ppm_path {
            if !smoke.screenshot_requested {
                smoke.screenshot_requested = true;
                let path = out_path.clone();
                let done_flag = Arc::clone(&smoke.screenshot_saved);
                let wakeup_ctx = ctx.clone();
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                    egui::ScreenshotCallback::new(move |image| {
                        if let Err(err) = write_ppm_screenshot(&path, &image) {
                            tracing::error!(error = %err, "Failed to save smoke-test screenshot");
                        } else {
                            tracing::info!(
                                width = image.width(),
                                height = image.height(),
                                "Captured GPU framebuffer screenshot"
                            );
                        }
                        done_flag.store(true, Ordering::Release);
                        wakeup_ctx.request_repaint();
                    }),
                ));
                ctx.request_repaint();
                return;
            }

            if !smoke.screenshot_saved.load(Ordering::Acquire) && elapsed < Duration::from_secs(2) {
                ctx.request_repaint_after(Duration::from_millis(30));
                return;
            }
        }

        if !smoke.close_dispatched {
            smoke.close_dispatched = true;
            tracing::info!(
                viewport_width = self.state.viewport_size[0],
                viewport_height = self.state.viewport_size[1],
                "Smoke-test lifecycle verified; requesting clean window close"
            );
            self.state.apply(gitrust_core::AppEvent::RequestClose);
        }
    }
}

fn interactive_preference_overrides(
    args: impl IntoIterator<Item = std::ffi::OsString>,
) -> [bool; 3] {
    let mut overrides = [false; 3];
    for arg in args {
        match arg.to_str() {
            Some("--theme") => overrides[0] = true,
            Some("--language") => overrides[1] = true,
            Some("--scale") => overrides[2] = true,
            _ => {}
        }
    }
    overrides
}

impl eframe::App for GitRustApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(ime) = &self.wayland_ime {
            ime.preserve_during_logic(ctx);
        }
        if let Some(workspaces) = &self.workspaces {
            if let Some(update) = workspaces.take_update() {
                if let Some(preferences) = update.preferences {
                    self.view
                        .apply_saved_preferences(ctx, preferences, self.preference_overrides);
                    self.last_preferences = self.view.preferences();
                }
                if let Some(view) = update.view {
                    self.view.set_repository_view(view);
                }
                if let Some(path) = update.directory {
                    self.view.set_repository_destination(path);
                }
            }
            let focused = ctx.input(|input| input.viewport().focused.unwrap_or(self.focus));
            if focused && !self.focus {
                workspaces.send(gitrust_core::workspaces::RepositoryAction::Focus);
            }
            self.focus = focused;
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let events = self.view.show(ui, &self.state);
        if let Some(workspaces) = &self.workspaces {
            if let Some(action) = self.view.take_repository_action() {
                workspaces.send(action);
            }
            if events.show_logs() {
                let host = self.state.host;
                let graphics = &self.state.graphics;
                let log = format!(
                    "gitRUST {}\nOS: {} ({})\nSession: {}\nRenderer: {}\nBackend: {:?}\nAdapter: {:?}\nDevice type: {:?}\nDriver: {:?}\n",
                    self.state.metadata.version,
                    host.operating_system,
                    host.architecture,
                    host.session_protocol.as_str(),
                    graphics.renderer,
                    graphics.backend_api,
                    graphics.adapter_name,
                    graphics.device_type,
                    graphics.driver_description,
                );
                workspaces.send(gitrust_core::workspaces::RepositoryAction::ShowRuntimeLog(
                    log,
                ));
            }
            let preferences = self.view.preferences();
            if preferences != self.last_preferences {
                self.last_preferences = preferences;
                workspaces.send(gitrust_core::workspaces::RepositoryAction::SavePreferences(
                    preferences,
                ));
            }
        }
        if let Some(ime) = &mut self.wayland_ime {
            ime.remember_ui_output(ui.ctx());
        }
        for event in events.into_iter_events() {
            self.state.apply(event);
        }

        let ctx = ui.ctx().clone();
        self.drive_smoke_test(&ctx);

        if self.state.should_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn build_native_options(initial_size_override: Option<[f32; 2]>) -> eframe::NativeOptions {
    let metadata = AppMetadata::new(CARGO_VERSION);

    let mut wgpu_options = eframe::egui_wgpu::WgpuConfiguration::default();
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(ref mut create_new) = wgpu_options.wgpu_setup {
        // Default to LowPower on hybrid laptops (e.g., Intel/AMD iGPU + discrete GPU) while
        // honoring user overrides via WGPU_POWER_PREF=high|low|none.
        create_new.power_preference = eframe::wgpu::PowerPreference::from_env()
            .unwrap_or(eframe::wgpu::PowerPreference::LowPower);
    }

    let initial_size = initial_size_override.unwrap_or(DEFAULT_WINDOW_SIZE);

    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(metadata.window_title())
            .with_app_id(PACKAGE_NAME)
            .with_inner_size(initial_size)
            .with_min_inner_size(MIN_WINDOW_SIZE)
            .with_resizable(true),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options,
        run_and_return: true,
        ..Default::default()
    }
}

fn main() -> ExitCode {
    let mut arguments = std::env::args_os().skip(1);
    if arguments
        .next()
        .is_some_and(|flag| flag == "--oauth-callback")
    {
        if arguments
            .next()
            .and_then(|url| url.into_string().ok())
            .filter(|_| arguments.next().is_none())
            .is_some_and(|url| browser_oauth::forward(&url))
        {
            return ExitCode::SUCCESS;
        }
        eprintln!("gitRUST did not accept the browser authorization callback.");
        return ExitCode::FAILURE;
    }
    if let Some(code) = gitrust_git::ssh_bridge() {
        return code;
    }
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
        LaunchMode::Interactive(options) => {
            init_logging();
            let instance = if options.diagnostics.is_some() {
                None
            } else {
                match browser_oauth::start() {
                    Ok(Some(instance)) => Some(instance),
                    Ok(None) => return ExitCode::SUCCESS,
                    Err(_) => {
                        tracing::warn!("GitHub browser callback is unavailable in this session");
                        None
                    }
                }
            };
            #[cfg(target_os = "linux")]
            let oauth = instance.as_ref().map(browser_oauth::Instance::inbox);
            #[cfg(not(target_os = "linux"))]
            let oauth = instance.and(None);
            let native_options = build_native_options(None);
            match eframe::run_native(
                PACKAGE_NAME,
                native_options,
                Box::new(move |cc| {
                    Ok(Box::new(GitRustApp::new(
                        cc, metadata, None, options, oauth,
                    )))
                }),
            ) {
                Ok(()) => ExitCode::SUCCESS,
                Err(err) => {
                    tracing::error!(error = %err, "Failed to run gitRUST desktop window");
                    ExitCode::FAILURE
                }
            }
        }
        LaunchMode::SmokeTest(smoke_opts, options) => {
            init_logging();
            // Wayland compositors may decline programmatic resize requests.
            let native_options = build_native_options(smoke_opts.initial_size);
            match eframe::run_native(
                PACKAGE_NAME,
                native_options,
                Box::new(move |cc| {
                    Ok(Box::new(GitRustApp::new(
                        cc,
                        metadata,
                        Some(smoke_opts),
                        options,
                        None,
                    )))
                }),
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
    fn parses_supported_cli_flags_and_rejects_invalid_combinations() {
        assert!(matches!(
            parse_launch_mode(["gitrust"]),
            Ok(LaunchMode::Interactive(UiOptions {
                diagnostics: None,
                ..
            }))
        ));
        assert!(matches!(
            parse_launch_mode(["gitrust", "--version"]),
            Ok(LaunchMode::PrintVersion)
        ));
        assert!(matches!(
            parse_launch_mode(["gitrust", "-h"]),
            Ok(LaunchMode::PrintHelp)
        ));
        assert!(matches!(
            parse_launch_mode(["gitrust", "--smoke-test"]),
            Ok(LaunchMode::SmokeTest(_, _))
        ));
        let mode = parse_launch_mode([
            "gitrust",
            "--smoke-size",
            "640x400",
            "--smoke-screenshot",
            "/tmp/out.ppm",
            "--diagnostic-view",
            "history",
            "--theme",
            "dark",
            "--language",
            "pt-BR",
        ])
        .expect("valid launch options");
        let LaunchMode::SmokeTest(smoke, options) = mode else {
            panic!("expected smoke mode");
        };
        assert_eq!(
            smoke,
            SmokeTestOptions {
                target_size: Some([640.0, 400.0]),
                initial_size: Some([640.0, 400.0]),
                screenshot_ppm_path: Some(PathBuf::from("/tmp/out.ppm"))
            }
        );
        assert_eq!(options.diagnostics, Some(DiagnosticView::History));
        assert_eq!(options.theme, egui::ThemePreference::Dark);
        assert_eq!(options.language, Language::Portuguese);
        assert!(parse_launch_mode(["gitrust", "--smoke-size", "bad"]).is_err());
        assert!(parse_launch_mode(["gitrust", "--smoke-size", "50x50"]).is_err());
        assert!(parse_launch_mode(["gitrust", "--smoke-screenshot"]).is_err());
        assert!(parse_launch_mode(["gitrust", "--scale", "101"]).is_err());
        assert!(parse_launch_mode(["gitrust", "--unknown"]).is_err());
    }
}
