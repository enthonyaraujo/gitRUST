use egui::{Align, Button, Frame, Layout, Margin, RichText, ScrollArea, Stroke, Ui, vec2};
use gitrust_core::workspaces::{Activity, RepositoryAction, RepositoryView};

use super::{Command, FormKind, Language, ManagerUi, Palette, Text, c, weight};

const CARD_WIDTH: f32 = 600.0;

fn centered_column<R>(ui: &mut Ui, content: impl FnOnce(&mut Ui) -> R) -> R {
    let width = ui.available_width().min(CARD_WIDTH);
    ui.vertical_centered(|ui| {
        ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
            ui.set_width(width);
            content(ui)
        })
        .inner
    })
    .inner
}

fn shortcut_keys(ui: &mut Ui, command: Command, p: Palette) {
    if let Some(shortcut) = command.shortcut() {
        let shortcut = ui.ctx().format_shortcut(&shortcut);
        for (i, key) in shortcut.split('+').enumerate() {
            if i > 0 {
                ui.label(RichText::new("+").small().color(p.muted));
            }
            Frame::new()
                .fill(p.elevated)
                .stroke(Stroke::new(1.0, p.divider))
                .corner_radius(3)
                .inner_margin(Margin::symmetric(4, 0))
                .show(ui, |ui| ui.label(RichText::new(key).small().color(p.text)));
        }
    }
}

fn outlined_button(label: &str, p: Palette) -> Button<'_> {
    Button::new(label)
        .frame(true)
        .fill(egui::Color32::TRANSPARENT)
        .stroke(Stroke::new(1.0, p.border))
}

pub(super) fn clean_workspace(
    ui: &mut Ui,
    p: Palette,
    l: Language,
    view: &RepositoryView,
) -> Option<Command> {
    let top = (ui.available_height() * 0.07).clamp(4.0, 48.0);
    ScrollArea::vertical()
        .id_salt("clean-workspace")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(top);
            centered_column(ui, |ui| {
                let width = ui.available_width();
                // Leave room on wide layouts for a future project-owned worktree illustration.
                ui.allocate_ui_with_layout(
                    vec2(if width >= 450.0 { width - 80.0 } else { width }, 0.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        ui.heading(RichText::new(l.local("No local changes", "Sem alterações locais"))
                            .family(egui::FontFamily::Proportional).size(32.0));
                        ui.colored_label(p.muted, l.local(
                            "There are no uncommitted changes in this repository. Here are a few things you can do next.",
                            "Não há alterações não commitadas neste repositório. Veja algumas sugestões do que fazer a seguir.",
                        ));
                    },
                );
                ui.add_space(24.0);
                let mut clicked = None;
                Frame::new()
                    .fill(p.surface)
                    .stroke(Stroke::new(1.0, p.divider))
                    .corner_radius(6)
                    .show(ui, |ui| {
                        ui.set_width((width - 2.0).max(0.0));
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for (index, (action, title, available)) in [
                            (Command::Editor, l.local("Open the repository in your external editor", "Abrir o repositório no seu editor externo"), cfg!(target_os = "linux")),
                            (Command::FileManager, l.local("Browse the repository in your file manager", "Ver os arquivos do repositório no gerenciador de arquivos"), true),
                            (Command::Github, l.local("Open the repository page on GitHub in your browser", "Abrir a página do repositório no GitHub no navegador"), view.github.workspace_repo.is_some()),
                        ].into_iter().enumerate() {
                            if index > 0 {
                                ui.add(egui::Separator::default().spacing(1.0));
                            }
                            if let Some(command) = suggestion(ui, action, title, available, l, p) {
                                clicked = Some(command);
                            }
                        }
                    });
                clicked
            })
        })
        .inner
}

fn suggestion(
    ui: &mut Ui,
    action: Command,
    title: &str,
    available: bool,
    l: Language,
    p: Palette,
) -> Option<Command> {
    Frame::new()
        .inner_margin(Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            let mut clicked = None;
            let mut copy = |ui: &mut Ui| {
                ui.label(RichText::new(title).family(weight()));
                if action == Command::Editor {
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(
                            p.muted,
                            l.local("Editor: Visual Studio Code", "Editor: Visual Studio Code"),
                        );
                        ui.visuals_mut().hyperlink_color = p.accent;
                        if ui.link(l.local("Options", "Opções")).clicked() {
                            clicked = Some(Command::Settings);
                        }
                    });
                }
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    ui.label(
                        RichText::new(l.local("Repository menu or", "Menu Repositório ou"))
                            .small()
                            .color(p.muted),
                    );
                    shortcut_keys(ui, action, p);
                });
            };
            let button = |ui: &mut Ui| {
                ui.add_enabled(available, outlined_button(action.label(l), p))
                    .clicked()
            };
            let pressed = if ui.available_width() >= 448.0 {
                let width = ui.available_width();
                let height = if action == Command::Editor {
                    64.0
                } else {
                    50.0
                };
                ui.allocate_ui_with_layout(
                    vec2(width, height),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.allocate_ui_with_layout(
                            vec2(width - 214.0, 0.0),
                            Layout::top_down(Align::Min),
                            copy,
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), button)
                            .inner
                    },
                )
                .inner
            } else {
                copy(ui);
                ui.add_space(5.0);
                button(ui)
            };
            if pressed { Some(action) } else { clicked }
        })
        .inner
}

impl ManagerUi {
    pub(super) fn empty_landing(&mut self, ui: &mut Ui, p: Palette, l: Language) {
        ui.add_space((ui.ctx().content_rect().height() * 0.07).clamp(4.0, 48.0));
        centered_column(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                c::Icon::Repo.show(ui, 30.0, p.accent);
                ui.heading(RichText::new(l.text(Text::NoRepository)).size(28.0));
            });
            ui.add_space(8.0);
            if self.view.activity != Activity::Starting && !self.view.git_available {
                if ui
                    .button(l.local("Retry Git discovery", "Buscar Git novamente"))
                    .clicked()
                {
                    self.send(RepositoryAction::RetryGit);
                }
            } else {
                ui.add_enabled_ui(self.usable(), |ui| {
                    for (kind, command, icon, label) in [
                        (
                            FormKind::Create,
                            Command::NewRepository,
                            c::Icon::Branch,
                            l.local("Create a new repository", "Criar novo repositório"),
                        ),
                        (
                            FormKind::Open,
                            Command::Open,
                            c::Icon::Repo,
                            l.local("Add local repository", "Adicionar repositório local"),
                        ),
                        (
                            FormKind::Clone,
                            Command::CloneRepository,
                            c::Icon::Remote,
                            l.local("Clone a repository", "Clonar repositório"),
                        ),
                    ] {
                        ui.horizontal_wrapped(|ui| {
                            icon.show(ui, 18.0, p.muted);
                            let clicked = if kind == FormKind::Open {
                                c::primary(ui, label, p)
                            } else {
                                ui.add(outlined_button(label, p))
                            }
                            .clicked();
                            if clicked {
                                self.open_form(kind);
                            }
                            ui.spacing_mut().item_spacing.x = 3.0;
                            shortcut_keys(ui, command, p);
                        });
                    }
                });
            }
            if !self.view.recent.paths.is_empty() {
                ui.add_space(16.0);
                ui.separator();
                ui.label(
                    RichText::new(l.local("Recent repositories", "Repositórios recentes"))
                        .family(weight()),
                );
                self.recents(ui, "", p, l, 190.0);
            }
        });
    }
}
