//! Native `egui` user interface layer for `gitRUST`.
//!
//! # Stage 00 Scope
//! This crate renders a clean, provisional desktop view representing the actual
//! state of Stage 00 (Project Foundation). It deliberately avoids mock Git controls,
//! placeholder repository trees, or continuous animations so that:
//! 1. Only genuinely implemented capabilities are presented.
//! 2. The event loop remains idle (`0%` CPU target) when there is no user input.
//! 3. Static diagnostic labels are formatted once at initialization rather than
//!    allocating fresh heap strings on every UI pass.
//!
//! The definitive visual identity and component system will be built in
//! **Stage 01 (Design System)**.

use egui::{
    Align, Color32, Context, CornerRadius, Frame, Layout, Margin, Panel, RichText, ScrollArea,
    Stroke, Ui,
};
use gitrust_core::{AppEvent, AppState, CrateStageStatus, WORKSPACE_CRATES};

/// Applies a clean, legible baseline visual configuration for Stage 00.
pub fn configure_provisional_visuals(ctx: &Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = Color32::from_rgb(18, 20, 24);
    visuals.window_fill = Color32::from_rgb(24, 27, 33);
    visuals.faint_bg_color = Color32::from_rgb(26, 30, 37);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(44, 50, 62));
    ctx.set_visuals(visuals);
}

/// Stack-allocated batch of up to two [`AppEvent`]s emitted during a single UI pass.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct FrameEvents {
    resize: Option<AppEvent>,
    action: Option<AppEvent>,
}

impl FrameEvents {
    /// Iterates over emitted events in deterministic order without heap allocation.
    pub fn into_iter_events(self) -> impl Iterator<Item = AppEvent> {
        self.resize.into_iter().chain(self.action)
    }
}

/// Cached presentation state for the Stage 00 provisional foundation view.
///
/// Pre-computes static labels once on startup and recomputes the status footer
/// only when the viewport dimensions actually change.
#[derive(Debug, Clone)]
pub struct FoundationView {
    version_badge: String,
    stage_badge: String,
    license_label: String,
    os_label: String,
    backend_label: String,
    adapter_label: String,
    cached_viewport_size: [f32; 2],
    footer_label: String,
}

impl FoundationView {
    /// Initializes cached display labels from [`AppState`].
    #[must_use]
    pub fn new(state: &AppState) -> Self {
        let footer_label = format_footer_label(state.metadata.package_name, state.viewport_size);
        Self {
            version_badge: format!("v{}", state.metadata.version),
            stage_badge: format!(
                "Stage {} — {}",
                state.metadata.stage_id, state.metadata.stage_name
            ),
            license_label: format!("License: {}", state.metadata.license),
            os_label: format!(
                "{} ({})",
                state.host.operating_system, state.host.architecture
            ),
            backend_label: format!(
                "{} ({})",
                state.graphics.renderer, state.graphics.backend_api
            ),
            adapter_label: format!(
                "{} [{}]",
                state.graphics.adapter_name, state.graphics.device_type
            ),
            cached_viewport_size: state.viewport_size,
            footer_label,
        }
    }

    /// Renders the Stage 00 Foundation view inside the root `Ui` without scheduling
    /// continuous repaints or performing unnecessary per-frame string allocations.
    pub fn show(&mut self, ui: &mut Ui, state: &AppState) -> FrameEvents {
        let mut events = FrameEvents::default();

        let rect = ui.ctx().input(egui::InputState::content_rect);
        let observed_size = [rect.width(), rect.height()];
        if (observed_size[0] - state.viewport_size[0]).abs() > 0.5
            || (observed_size[1] - state.viewport_size[1]).abs() > 0.5
        {
            events.resize = Some(AppEvent::ViewportResized {
                width: observed_size[0],
                height: observed_size[1],
            });
        }

        if (state.viewport_size[0] - self.cached_viewport_size[0]).abs() > 0.5
            || (state.viewport_size[1] - self.cached_viewport_size[1]).abs() > 0.5
        {
            self.cached_viewport_size = state.viewport_size;
            self.footer_label =
                format_footer_label(state.metadata.package_name, state.viewport_size);
        }

        self.render_top_bar(ui, state, &mut events);
        self.render_status_footer(ui);

        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(Color32::from_rgb(18, 20, 24))
                    .inner_margin(Margin::symmetric(24, 18)),
            )
            .show(ui, |ui| {
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        render_intro_card(ui);
                        ui.add_space(14.0);
                        self.render_runtime_card(ui, state);
                        ui.add_space(14.0);
                        render_workspace_card(ui);
                    });
            });

        events
    }

    fn render_top_bar(&self, ui: &mut Ui, state: &AppState, events: &mut FrameEvents) {
        Panel::top("gitrust_top_bar")
            .frame(
                Frame::new()
                    .fill(Color32::from_rgb(23, 26, 32))
                    .inner_margin(Margin::symmetric(20, 12))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(40, 46, 56))),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(state.metadata.name)
                            .size(20.0)
                            .strong()
                            .color(Color32::from_rgb(240, 243, 249)),
                    );
                    ui.add_space(6.0);
                    badge(
                        ui,
                        &self.version_badge,
                        Color32::from_rgb(35, 52, 78),
                        Color32::from_rgb(168, 205, 255),
                    );
                    badge(
                        ui,
                        &self.stage_badge,
                        Color32::from_rgb(31, 61, 47),
                        Color32::from_rgb(154, 230, 180),
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Close Window").clicked() {
                            events.action = Some(AppEvent::RequestClose);
                        }
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(&self.license_label)
                                .size(12.5)
                                .color(Color32::from_rgb(156, 164, 178)),
                        );
                    });
                });
            });
    }

    fn render_status_footer(&self, ui: &mut Ui) {
        Panel::bottom("gitrust_status_footer")
            .frame(
                Frame::new()
                    .fill(Color32::from_rgb(20, 23, 28))
                    .inner_margin(Margin::symmetric(20, 8))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(36, 41, 50))),
            )
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(&self.footer_label)
                            .size(12.0)
                            .color(Color32::from_rgb(142, 151, 166)),
                    );
                });
            });
    }

    fn render_runtime_card(&self, ui: &mut Ui, state: &AppState) {
        section_card(ui, "Verified Runtime Environment", |ui| {
            egui::Grid::new("gitrust_runtime_grid")
                .num_columns(2)
                .spacing([24.0, 6.0])
                .striped(false)
                .show(ui, |ui| {
                    kv_row(ui, "Operating System", &self.os_label);
                    kv_row(
                        ui,
                        "Windowing Session",
                        state.host.session_protocol.as_str(),
                    );
                    kv_row(ui, "Graphics Backend", &self.backend_label);
                    kv_row(ui, "GPU Adapter", &self.adapter_label);
                    kv_row(ui, "Graphics Driver", &state.graphics.driver_description);
                    kv_row(
                        ui,
                        "Web / WebView Stack",
                        "None (100% native Rust + egui/eframe)",
                    );
                });
        });
    }
}

fn format_footer_label(package_name: &str, viewport_size: [f32; 2]) -> String {
    format!(
        "Package: {package_name}  •  Viewport: {:.0}×{:.0} pt  •  Rendering: Event-driven (idle repaint off)  •  Telemetry: Disabled",
        viewport_size[0], viewport_size[1],
    )
}

fn render_intro_card(ui: &mut Ui) {
    section_card(ui, "Project Foundation Status", |ui| {
        ui.label(
            RichText::new(
                "gitRUST is an independent, native, open-source desktop Git client written entirely in Rust with egui and eframe.",
            )
            .size(14.0)
            .color(Color32::from_rgb(220, 225, 235)),
        );
        ui.add_space(6.0);
        ui.label(
            RichText::new(
                "This provisional window validates the Stage 00 desktop shell, native wgpu rendering pipeline, window resizing constraints, and workspace crate boundaries. Git repository management, commit workflows, and the definitive Design System (Stage 01) are intentionally not yet active.",
            )
            .size(13.0)
            .color(Color32::from_rgb(165, 174, 190)),
        );
    });
}

fn render_workspace_card(ui: &mut Ui) {
    section_card(ui, "Cargo Workspace Architecture", |ui| {
        egui::Grid::new("gitrust_workspace_grid")
            .num_columns(3)
            .spacing([18.0, 6.0])
            .striped(true)
            .show(ui, |ui| {
                ui.label(
                    RichText::new("Crate")
                        .strong()
                        .color(Color32::from_rgb(205, 212, 224)),
                );
                ui.label(
                    RichText::new("Stage 00 Status")
                        .strong()
                        .color(Color32::from_rgb(205, 212, 224)),
                );
                ui.label(
                    RichText::new("Responsibility")
                        .strong()
                        .color(Color32::from_rgb(205, 212, 224)),
                );
                ui.end_row();

                for item in WORKSPACE_CRATES {
                    ui.monospace(item.name);
                    match item.status {
                        CrateStageStatus::ActiveInFoundation => {
                            ui.label(
                                RichText::new("Active (Stage 00)")
                                    .color(Color32::from_rgb(138, 224, 168)),
                            );
                        }
                        CrateStageStatus::Planned { target_stage } => {
                            ui.label(
                                RichText::new(format!("Boundary only (Stage {target_stage:02})"))
                                    .color(Color32::from_rgb(180, 190, 208)),
                            );
                        }
                    }
                    ui.label(
                        RichText::new(item.responsibility).color(Color32::from_rgb(174, 182, 196)),
                    );
                    ui.end_row();
                }
            });
    });
}

fn section_card(ui: &mut Ui, title: &str, add_contents: impl FnOnce(&mut Ui)) {
    Frame::new()
        .fill(Color32::from_rgb(24, 27, 34))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(14))
        .stroke(Stroke::new(1.0, Color32::from_rgb(40, 46, 58)))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(title)
                    .size(15.0)
                    .strong()
                    .color(Color32::from_rgb(235, 239, 247)),
            );
            ui.add_space(8.0);
            add_contents(ui);
        });
}

fn kv_row(ui: &mut Ui, key: &str, value: &str) {
    ui.label(RichText::new(key).color(Color32::from_rgb(148, 158, 175)));
    ui.label(RichText::new(value).color(Color32::from_rgb(225, 230, 240)));
    ui.end_row();
}

fn badge(ui: &mut Ui, text: &str, bg: Color32, fg: Color32) {
    Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(12.0).strong().color(fg));
        });
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use gitrust_core::{
        AppMetadata, DEFAULT_WINDOW_SIZE, GraphicsDiagnostics, HostDiagnostics, MIN_WINDOW_SIZE,
        WindowSessionProtocol,
    };

    use super::*;

    #[test]
    fn foundation_view_renders_without_continuous_repaint() {
        let ctx = Context::default();
        configure_provisional_visuals(&ctx);

        let mut state = AppState::new(
            AppMetadata::new("0.1.0-alpha.0"),
            HostDiagnostics::from_protocol(WindowSessionProtocol::Wayland),
            GraphicsDiagnostics::default(),
        );
        let mut view = FoundationView::new(&state);

        let raw_input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::Vec2::new(DEFAULT_WINDOW_SIZE[0], DEFAULT_WINDOW_SIZE[1]),
            )),
            ..Default::default()
        };

        // Allow egui's multi-pass layout cache and Style::animation_time (0.2s = 12 frames
        // at 60 Hz, plus startup layout/settle passes) to converge completely.
        for step in 0..20_u8 {
            let mut step_input = raw_input.clone();
            step_input.time = Some(f64::from(step) * (1.0 / 60.0));
            let mut warmup_output = ctx.run_ui(step_input, |ui| {
                for ev in view.show(ui, &state).into_iter_events() {
                    state.apply(ev);
                }
            });
            assert_ne!(warmup_output.shapes.len(), 0);
            warmup_output.textures_delta.clear();
        }

        // Settled idle frame must NOT schedule continuous repaints.
        let mut idle_input = raw_input;
        idle_input.time = Some(20.0 * (1.0 / 60.0));
        let mut idle_output = ctx.run_ui(idle_input, |ui| {
            for ev in view.show(ui, &state).into_iter_events() {
                state.apply(ev);
            }
        });
        idle_output.textures_delta.clear();

        assert_eq!(idle_output.viewport_output.len(), 1);
        let causes = ctx.repaint_causes();
        for vp_out in idle_output.viewport_output.values() {
            assert_eq!(
                vp_out.repaint_delay,
                Duration::MAX,
                "idle frame must not request continuous repainting; causes: {causes:?}"
            );
        }
    }

    #[test]
    fn foundation_view_handles_minimum_viewport_resize() {
        let ctx = Context::default();
        let mut state = AppState::new(
            AppMetadata::new("0.1.0-alpha.0"),
            HostDiagnostics::from_protocol(WindowSessionProtocol::X11),
            GraphicsDiagnostics::default(),
        );
        let mut view = FoundationView::new(&state);

        let raw_input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::Vec2::new(MIN_WINDOW_SIZE[0], MIN_WINDOW_SIZE[1]),
            )),
            ..Default::default()
        };

        let mut output = ctx.run_ui(raw_input.clone(), |ui| {
            for ev in view.show(ui, &state).into_iter_events() {
                state.apply(ev);
            }
        });
        output.textures_delta.clear();

        let mut synced_output = ctx.run_ui(raw_input, |ui| {
            let _ = view.show(ui, &state);
        });
        synced_output.textures_delta.clear();

        assert_eq!(state.viewport_size, MIN_WINDOW_SIZE);
        assert!(view.footer_label.contains("640×400 pt"));
    }
}
