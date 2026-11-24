//! Native `egui` user interface layer for `gitRUST`.
//!
//! # Stage 00 Scope
//! This crate renders a clean, provisional desktop view representing the actual
//! state of Stage 00 (Project Foundation). It deliberately avoids mock Git controls,
//! placeholder repository trees, or continuous animations so that:
//! 1. Only genuinely implemented capabilities are presented.
//! 2. The event loop remains idle (`0%` CPU target) when there is no user input.
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

/// Renders the Stage 00 Foundation view inside the root `Ui` and returns any [`AppEvent`]s.
///
/// This function never calls `Context::request_repaint`, preserving reactive,
/// event-driven rendering when the desktop window is idle.
pub fn render_foundation_view(ui: &mut Ui, state: &AppState) -> Vec<AppEvent> {
    let mut events = Vec::new();

    let rect = ui.ctx().input(egui::InputState::content_rect);
    let observed_size = [rect.width(), rect.height()];
    if (observed_size[0] - state.viewport_size[0]).abs() > 0.5
        || (observed_size[1] - state.viewport_size[1]).abs() > 0.5
    {
        events.push(AppEvent::ViewportResized {
            width: observed_size[0],
            height: observed_size[1],
        });
    }

    render_top_bar(ui, state, &mut events);
    render_status_footer(ui, state);

    egui::CentralPanel::default()
        .frame(
            Frame::new()
                .fill(Color32::from_rgb(18, 20, 24))
                .inner_margin(Margin::symmetric(24, 20)),
        )
        .show(ui, |ui| {
            ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    render_intro_card(ui);
                    ui.add_space(16.0);
                    render_runtime_card(ui, state);
                    ui.add_space(16.0);
                    render_workspace_card(ui);
                });
        });

    events
}

fn render_top_bar(ui: &mut Ui, state: &AppState, events: &mut Vec<AppEvent>) {
    Panel::top("gitrust_top_bar")
        .frame(
            Frame::new()
                .fill(Color32::from_rgb(23, 26, 32))
                .inner_margin(Margin::symmetric(20, 14))
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
                    &format!("v{}", state.metadata.version),
                    Color32::from_rgb(35, 52, 78),
                    Color32::from_rgb(168, 205, 255),
                );
                badge(
                    ui,
                    &format!(
                        "Stage {} — {}",
                        state.metadata.stage_id, state.metadata.stage_name
                    ),
                    Color32::from_rgb(31, 61, 47),
                    Color32::from_rgb(154, 230, 180),
                );

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Close Window").clicked() {
                        events.push(AppEvent::RequestClose);
                    }
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(format!("License: {}", state.metadata.license))
                            .size(12.5)
                            .color(Color32::from_rgb(156, 164, 178)),
                    );
                });
            });
        });
}

fn render_status_footer(ui: &mut Ui, state: &AppState) {
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
                    RichText::new(format!(
                        "Package: {}  •  Viewport: {:.0}×{:.0} pt  •  Rendering: Event-driven (idle repaint off)  •  Telemetry: Disabled",
                        state.metadata.package_name,
                        state.viewport_size[0],
                        state.viewport_size[1],
                    ))
                    .size(12.0)
                    .color(Color32::from_rgb(142, 151, 166)),
                );
            });
        });
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

fn render_runtime_card(ui: &mut Ui, state: &AppState) {
    section_card(ui, "Verified Runtime Environment", |ui| {
        egui::Grid::new("gitrust_runtime_grid")
            .num_columns(2)
            .spacing([24.0, 8.0])
            .striped(false)
            .show(ui, |ui| {
                kv_row(
                    ui,
                    "Operating System",
                    &format!(
                        "{} ({})",
                        state.host.operating_system, state.host.architecture
                    ),
                );
                kv_row(ui, "Windowing Session", &state.host.session_protocol);
                kv_row(
                    ui,
                    "Graphics Backend",
                    &format!(
                        "{} ({})",
                        state.graphics.renderer, state.graphics.backend_api
                    ),
                );
                kv_row(
                    ui,
                    "GPU Adapter",
                    &format!(
                        "{} [{}]",
                        state.graphics.adapter_name, state.graphics.device_type
                    ),
                );
                kv_row(ui, "Graphics Driver", &state.graphics.driver_description);
                kv_row(
                    ui,
                    "Web / WebView Stack",
                    "None (100% native Rust + egui/eframe)",
                );
            });
    });
}

fn render_workspace_card(ui: &mut Ui) {
    section_card(ui, "Cargo Workspace Architecture", |ui| {
        egui::Grid::new("gitrust_workspace_grid")
            .num_columns(3)
            .spacing([18.0, 8.0])
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
        .inner_margin(Margin::same(16))
        .stroke(Stroke::new(1.0, Color32::from_rgb(40, 46, 58)))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(title)
                    .size(15.5)
                    .strong()
                    .color(Color32::from_rgb(235, 239, 247)),
            );
            ui.add_space(10.0);
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
    };

    use super::*;

    #[test]
    fn foundation_view_renders_without_continuous_repaint() {
        let ctx = Context::default();
        configure_provisional_visuals(&ctx);

        let mut state = AppState::new(
            AppMetadata::new("0.1.0-alpha.0"),
            HostDiagnostics::detect(true, false, None),
            GraphicsDiagnostics::default(),
        );

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
                for ev in render_foundation_view(ui, &state) {
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
            for ev in render_foundation_view(ui, &state) {
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
            HostDiagnostics::detect(false, true, Some("x11")),
            GraphicsDiagnostics::default(),
        );

        let raw_input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::Vec2::new(MIN_WINDOW_SIZE[0], MIN_WINDOW_SIZE[1]),
            )),
            ..Default::default()
        };

        let mut output = ctx.run_ui(raw_input, |ui| {
            for ev in render_foundation_view(ui, &state) {
                state.apply(ev);
            }
        });
        output.textures_delta.clear();

        assert_eq!(state.viewport_size, MIN_WINDOW_SIZE);
    }
}
