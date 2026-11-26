use egui::{Button, Id, RichText, ScrollArea, ThemePreference, Ui};
use gitrust_core::AppState;

use super::{BRANCHES, Demo, Desktop, Dialog, Screen, UiOptions};
use crate::components as c;
use crate::i18n::{Language, Text};
use crate::theme::{Palette, size, weight};

impl Desktop {
    pub(super) fn dialogs(&mut self, ui: &mut Ui, state: &AppState, p: Palette, l: Language) {
        match self.dialog {
            Dialog::None => {}
            Dialog::Settings => {
                let r = c::modal(ui, "settings", p, l.text(Text::Settings), |ui| {
                    let max_height = (ui.ctx().content_rect().height() - 100.0).max(size::CONTROL);
                    ScrollArea::vertical()
                        .max_height(max_height)
                        .show(ui, |ui| preferences(ui, &mut self.options));
                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        let accounts = self.managed.is_some()
                            && ui
                                .button(l.local("GitHub Accounts", "Contas do GitHub"))
                                .clicked();
                        (ui.button(l.text(Text::Close)).clicked(), accounts)
                    })
                    .inner
                });
                if r.inner.1 {
                    if let Some(managed) = &mut self.managed {
                        managed.github.accounts();
                    }
                    self.screen = Screen::Github;
                }
                if r.inner.0 || r.inner.1 || r.should_close() {
                    self.dialog = Dialog::None;
                }
            }
            Dialog::About => {
                let r = c::modal(ui, "about", p, l.text(Text::About), |ui| {
                    ui.label(format!(
                        "{}  •  {}",
                        state.metadata.version, state.metadata.license
                    ));
                    ui.separator();
                    ui.label(RichText::new(l.text(Text::Runtime)).family(weight()));
                    ui.label(format!(
                        "{} / {}",
                        state.host.session_protocol.as_str(),
                        state.graphics.backend_api
                    ));
                    ui.label(&state.graphics.adapter_name);
                    ui.label(l.text(Text::Telemetry));
                    ui.button(l.text(Text::Close)).clicked()
                });
                if r.inner || r.should_close() {
                    self.dialog = Dialog::None;
                }
            }
            Dialog::Compose => {
                let Some(demo) = &mut self.demo else {
                    return;
                };
                let r = c::modal(ui, "commit-draft", p, l.text(Text::Compose), |ui| {
                    ScrollArea::vertical()
                        .max_height((ui.ctx().content_rect().height() - 120.0).max(30.0))
                        .show(ui, |ui| {
                            let discard = composer(ui, demo, p, l, "modal-draft");
                            if let Some(id) = self.initial_compose_focus.take() {
                                ui.memory_mut(|memory| memory.request_focus(id));
                                ui.ctx().request_repaint();
                            }
                            if discard {
                                Dialog::Discard {
                                    return_to_compose: true,
                                }
                            } else if ui.button(l.text(Text::Close)).clicked() {
                                Dialog::None
                            } else {
                                Dialog::Compose
                            }
                        })
                        .inner
                });
                self.dialog = if r.should_close() {
                    Dialog::None
                } else {
                    r.inner
                };
            }
            Dialog::Discard { return_to_compose } => {
                if let Some(confirmed) = c::confirmation(ui, p, l) {
                    if confirmed && let Some(demo) = &mut self.demo {
                        demo.summary.clear();
                        demo.description.clear();
                    }
                    self.dialog = if return_to_compose {
                        Dialog::Compose
                    } else {
                        Dialog::None
                    };
                }
            }
        }
    }
}

pub(super) fn composer(
    ui: &mut Ui,
    demo: &mut Demo,
    p: Palette,
    l: Language,
    scope: &'static str,
) -> bool {
    ui.push_id(scope, |ui| {
        ui.label(
            RichText::new(format!(
                "{} · {}",
                l.text(Text::Branch),
                BRANCHES[demo.branch]
            ))
            .small()
            .color(p.muted),
        );
        c::text_field(
            ui,
            Id::unique((scope, "summary")),
            l.text(Text::Summary),
            &mut demo.summary,
        );
        c::text_area(
            ui,
            Id::unique((scope, "description")),
            l.text(Text::OptionalDescription),
            &mut demo.description,
        );
        let discard = ui
            .horizontal_wrapped(|ui| {
                ui.add_enabled(false, Button::new(l.text(Text::Commit)).frame(true))
                    .on_disabled_hover_text(l.text(Text::NoGit));
                ui.add_enabled(
                    !demo.summary.is_empty() || !demo.description.is_empty(),
                    Button::new(l.text(Text::Discard)),
                )
                .accessible_name(l.text(Text::DiscardDraft))
                .on_hover_text(l.text(Text::DiscardDraft))
                .clicked()
            })
            .inner;
        ui.label(
            RichText::new(l.text(Text::DraftOnly))
                .small()
                .color(p.muted),
        );
        discard
    })
    .inner
}

pub(crate) fn preferences(ui: &mut Ui, options: &mut UiOptions) {
    let l = options.language;
    let previous = *options;
    egui::Grid::new("preferences-grid")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            let label = ui.label(RichText::new(l.text(Text::Theme)).family(weight()));
            c::dropdown(
                ui,
                "theme-preference",
                &mut options.theme,
                &[
                    (ThemePreference::System, l.text(Text::System)),
                    (ThemePreference::Light, l.text(Text::Light)),
                    (ThemePreference::Dark, l.text(Text::Dark)),
                ],
            )
            .labelled_by(label.id);
            ui.end_row();
            let label = ui.label(RichText::new(l.text(Text::Language)).family(weight()));
            c::dropdown(
                ui,
                "interface-language",
                &mut options.language,
                &[
                    (Language::English, "English"),
                    (Language::Portuguese, "Português (Brasil)"),
                ],
            )
            .labelled_by(label.id);
            ui.end_row();
            let label = ui.label(RichText::new(l.text(Text::Scale)).family(weight()));
            c::dropdown(
                ui,
                "interface-scale",
                &mut options.zoom,
                &[(1.0, "100%"), (1.25, "125%"), (1.5, "150%"), (2.0, "200%")],
            )
            .labelled_by(label.id);
            ui.end_row();
        });
    if previous.theme != options.theme {
        ui.ctx().set_theme(options.theme);
    }
    if (previous.zoom - options.zoom).abs() > f32::EPSILON {
        ui.ctx().set_zoom_factor(options.zoom);
    }
}
