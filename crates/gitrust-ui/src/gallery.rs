//! Opt-in component examples and synthetic stress cases.

use egui::{Id, RichText, ScrollArea, Ui};

use crate::components::{self as c, Icon, ListSelection, Row};
use crate::demo::Fixtures;
use crate::diff::{Diff, DiffMode, DiffView};
use crate::i18n::{Language, Text};
use crate::shell::{UiOptions, preferences};
use crate::theme::{FileStatus, Palette, size, weight};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Page {
    #[default]
    Controls,
    Tokens,
    States,
    Lists,
    Diff,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Scenario {
    #[default]
    Empty,
    Loading,
    Working,
    Error,
    Conflict,
    Binary,
    Confirmation,
}

#[derive(Debug, Default)]
struct ControlExamples {
    toggle: bool,
    check: bool,
    radio: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Gallery {
    page: Page,
    examples: ControlExamples,
    field: String,
    multiline: String,
    search: String,
    fixture: Option<Fixtures>,
    count: usize,
    list: ListSelection,
    entries: Vec<String>,
    mode: DiffMode,
    stress: bool,
    diff: DiffView,
    scenario: Scenario,
    notification: bool,
}

impl Gallery {
    pub(crate) fn open_stress(&mut self, diff: bool) {
        self.page = if diff { Page::Diff } else { Page::Lists };
        self.stress = diff;
        if diff {
            self.diff.focus_on_open();
        }
        self.count = 2;
    }

    pub(crate) fn show(&mut self, ui: &mut Ui, options: &mut UiOptions, p: Palette) {
        let l = options.language;
        ui.horizontal_wrapped(|ui| {
            ui.heading(l.text(Text::Gallery));
        });
        c::tabs(
            ui,
            &mut self.page,
            &[
                (Page::Controls, l.text(Text::Controls)),
                (Page::Tokens, l.text(Text::Colors)),
                (Page::States, l.text(Text::States)),
                (Page::Lists, l.text(Text::Lists)),
                (Page::Diff, l.text(Text::Diff)),
            ],
            p,
        );
        ui.separator();
        match self.page {
            Page::Controls => {
                ScrollArea::vertical()
                    .id_salt("gallery-controls")
                    .show(ui, |ui| self.controls(ui, options, p, l));
            }
            Page::Tokens => {
                ScrollArea::vertical()
                    .id_salt("gallery-tokens")
                    .show(ui, |ui| Self::tokens(ui, p, l));
            }
            Page::States => {
                ScrollArea::vertical()
                    .id_salt("gallery-states")
                    .show(ui, |ui| self.states(ui, p, l));
            }
            Page::Lists => self.lists(ui, p, l),
            Page::Diff => self.diffs(ui, p, l),
        }
    }

    fn controls(&mut self, ui: &mut Ui, options: &mut UiOptions, p: Palette, l: Language) {
        preferences(ui, options);
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            if c::primary(ui, l.text(Text::Example), p).clicked() {
                self.notification = !self.notification;
            }
            if c::icon_button(ui, Icon::Settings, l.text(Text::Settings), p).clicked() {
                self.notification = !self.notification;
            }
            ui.add_enabled(false, egui::Button::new(l.text(Text::Commit)))
                .on_disabled_hover_text(l.text(Text::NoGit));
            ui.menu_button(l.text(Text::Menu), |ui| {
                if ui.button(l.text(Text::Example)).clicked() {
                    self.notification = !self.notification;
                    ui.close();
                }
            });
        });
        ui.horizontal_wrapped(|ui| {
            ui.toggle_value(&mut self.examples.toggle, l.text(Text::Toggle));
            ui.checkbox(&mut self.examples.check, l.text(Text::Checkbox));
            ui.radio_value(&mut self.examples.radio, true, l.text(Text::Radio));
            ui.radio_value(&mut self.examples.radio, false, l.text(Text::Example));
        });
        ui.set_max_width(540.0);
        c::search(ui, Id::unique("gallery-search"), l, p, &mut self.search);
        c::text_field(
            ui,
            Id::unique("gallery-field"),
            l.text(Text::Summary),
            &mut self.field,
        );
        c::text_area(
            ui,
            Id::unique("gallery-area"),
            l.text(Text::Description),
            &mut self.multiline,
        );
        let example = ui.button(l.text(Text::ContextHelp));
        example.context_menu(|ui| {
            if ui.button(l.text(Text::Copy)).clicked() {
                ui.ctx().copy_text(self.field.clone());
                ui.close();
            }
        });
        if self.notification {
            c::notice(ui, l.text(Text::Example), l.text(Text::DemoHint), p.info, p);
            if ui.button(l.text(Text::Close)).clicked() {
                self.notification = false;
            }
        }
    }

    fn tokens(ui: &mut Ui, p: Palette, l: Language) {
        ui.label(RichText::new(l.text(Text::Typography)).heading());
        ui.label(RichText::new("gitRUST").family(weight()).size(19.0));
        ui.label(l.text(Text::Intro));
        ui.label(RichText::new("Aa Bb Çç Éé 123").small().color(p.muted));
        ui.label(RichText::new("6c718ad  fn ingest(frame: &Frame)").monospace());
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            for (key, color) in [
                (Text::Background, p.background),
                (Text::Surface, p.surface),
                (Text::Elevated, p.elevated),
                (Text::Selected, p.selected),
                (Text::Border, p.border),
                (Text::Divider, p.divider),
                (Text::PrimaryForeground, p.text),
                (Text::SecondaryForeground, p.muted),
                (Text::DisabledForeground, p.disabled),
                (Text::Accent, p.accent),
                (Text::OnAccent, p.on_accent),
                (Text::Focus, p.focus),
                (Text::Hover, p.hover),
                (Text::Active, p.active),
                (Text::Selection, p.selection),
                (Text::Success, p.success),
                (Text::Warning, p.warning),
                (Text::ErrorTone, p.error),
                (Text::Information, p.info),
                (Text::AddedLine, p.added_line),
                (Text::RemovedLine, p.removed_line),
                (Text::AddedEdit, p.added_edit),
                (Text::RemovedEdit, p.removed_edit),
            ] {
                c::panel(p.surface, size::SM)
                    .corner_radius(size::RADIUS)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (rect, _) = ui.allocate_exact_size(
                                egui::Vec2::splat(size::ICON),
                                egui::Sense::hover(),
                            );
                            ui.painter().rect(
                                rect,
                                size::RADIUS,
                                color,
                                egui::Stroke::new(1.0, p.border),
                                egui::StrokeKind::Inside,
                            );
                            ui.label(l.text(key));
                        });
                        ui.label(
                            RichText::new(format!(
                                "#{:02X}{:02X}{:02X}",
                                color.r(),
                                color.g(),
                                color.b()
                            ))
                            .monospace(),
                        );
                    });
            }
        });
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            for status in FileStatus::ALL {
                c::badge(ui, l.text(status.label()), p.status(status), p.surface);
            }
        });
    }

    fn states(&mut self, ui: &mut Ui, p: Palette, l: Language) {
        c::dropdown(
            ui,
            "gallery-scenario",
            &mut self.scenario,
            &[
                (Scenario::Empty, l.text(Text::NoChanges)),
                (Scenario::Loading, l.text(Text::Loading)),
                (Scenario::Working, l.text(Text::Working)),
                (Scenario::Error, l.text(Text::Error)),
                (Scenario::Conflict, l.text(Text::Conflict)),
                (Scenario::Binary, l.text(Text::Binary)),
                (Scenario::Confirmation, l.text(Text::Destructive)),
            ],
        );
        ui.add_space(f32::from(size::SM));
        match self.scenario {
            Scenario::Empty => c::empty(
                ui,
                Icon::Files,
                l.text(Text::NoChanges),
                l.text(Text::DemoHint),
                p,
            ),
            Scenario::Loading => c::loading(ui, l.text(Text::Loading), None),
            Scenario::Working => c::loading(ui, l.text(Text::Working), Some(0.62)),
            Scenario::Error => {
                c::notice(ui, l.text(Text::Error), l.text(Text::DemoHint), p.error, p);
                if ui.button(l.text(Text::Retry)).clicked() {
                    self.scenario = Scenario::Empty;
                }
            }
            Scenario::Conflict => c::notice(
                ui,
                l.text(Text::Conflict),
                l.text(Text::ConflictDetails),
                p.error,
                p,
            ),
            Scenario::Binary => {
                self.diff.show(
                    ui,
                    Id::unique("gallery-binary"),
                    Diff::Binary,
                    &mut self.mode,
                    l,
                    p,
                );
            }
            Scenario::Confirmation => {
                if c::confirmation(ui, p, l).is_some() {
                    self.scenario = Scenario::Empty;
                }
            }
        }
    }

    fn lists(&mut self, ui: &mut Ui, p: Palette, l: Language) {
        ui.horizontal(|ui| {
            ui.label(l.text(Text::Count));
            c::dropdown(
                ui,
                "gallery-count",
                &mut self.count,
                &[
                    (0, "100"),
                    (1, l.text(Text::Thousand)),
                    (2, l.text(Text::TenThousand)),
                ],
            );
        });
        let count = [100, 1_000, 10_000][self.count];
        if self.list.index.is_some_and(|key| key >= count) {
            self.list = ListSelection::default();
        }
        if self.entries.len() < count {
            self.entries
                .extend((self.entries.len()..count).map(|i| format!("sample_{i:05}.rs")));
        }
        let height = ui.available_height();
        self.list.show(
            ui,
            Id::unique("gallery-list"),
            height,
            count,
            p,
            l,
            |index| index,
            |index| Row {
                name: &self.entries[index],
                detail: "src/generated",
                status: Some(FileStatus::ALL[index % FileStatus::ALL.len()]),
            },
        );
    }

    fn diffs(&mut self, ui: &mut Ui, p: Palette, l: Language) {
        ui.checkbox(&mut self.stress, l.text(Text::StressDiff));
        let fixture = self.fixture.get_or_insert_with(Fixtures::default);
        let source = if self.stress {
            Diff::Stress(100_000)
        } else {
            fixture.file(0)
        };
        self.diff.show(
            ui,
            Id::unique(("gallery-diff", self.stress)),
            source,
            &mut self.mode,
            l,
            p,
        );
    }
}
