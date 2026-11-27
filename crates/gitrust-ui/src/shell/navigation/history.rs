use egui::{
    Button, FontId, Id, Label, Margin, Panel, RichText, ScrollArea, Stroke, Ui, pos2, vec2,
};
use gitrust_core::navigation::{BranchBase, Details, HistoryRow, Merge, Pagination, SignatureKind};

use super::super::repositories::{WorkspaceLayout, single_pane};
use super::{
    Arc, Command, Form, MenuCommand, NavigationIntent, NavigationUi, NavigationView, Pane, intent,
};
use crate::{
    components as c,
    i18n::{Language, Text},
    theme::{Palette, size, weight},
};

const HISTORY_ROW_HEIGHT: f32 = 48.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Search {
    #[default]
    Closed,
    Open,
    Focus,
}

impl NavigationUi {
    pub(in crate::shell) fn history(
        &mut self,
        ui: &mut Ui,
        view: &NavigationView,
        writable: bool,
        l: Language,
        p: Palette,
        layout: &mut WorkspaceLayout,
    ) -> Option<NavigationIntent> {
        let anchor = view
            .root
            .as_ref()
            .map(|root| (view.identity, root.clone(), view.all_refs));
        if self.anchor != anchor || view.pagination == Pagination::Initial {
            self.anchor = anchor;
            self.selected = None;
            self.previewed = None;
            self.pane = Pane::Revisions;
        }
        if view.root.is_none() {
            layout.tabs(ui, l, p);
            if !view.refs.is_empty()
                && ui
                    .button(l.local("Show all branches", "Mostrar todas as branches"))
                    .clicked()
            {
                return Some(intent(view, Command::UseAllRefs(true)));
            }
            ui.label(l.text(Text::NoHistory));
            return None;
        }
        let mut command = None;
        if single_pane(ui) {
            layout.tabs(ui, l, p);
            c::tabs(
                ui,
                &mut self.pane,
                &[
                    (Pane::Revisions, l.local("Commits", "Commits")),
                    (Pane::Details, l.local("Details", "Detalhes")),
                    (Pane::Diff, l.local("Diff", "Diff")),
                ],
                p,
            );
            let chosen = match self.pane {
                Pane::Revisions => self.revisions(ui, view, l, p),
                Pane::Details => self.details(ui, view, writable, l),
                Pane::Diff => {
                    self.patch(ui, view, l, p);
                    None
                }
            };
            command = chosen;
        } else {
            layout.sidebar_width = Some(c::repository_sidebar(ui, p, layout.delta, |ui| {
                layout.tabs(ui, l, p);
                command = self.revisions(ui, view, l, p);
            }));
            let inspector = if ui.available_width() >= 590.0 && ui.available_height() >= 245.0 {
                c::panel(p.background, 0)
                    .show(ui, |ui| {
                        self.inspector(
                            ui,
                            view,
                            writable,
                            l,
                            p,
                            layout.readable && command.is_none(),
                        )
                    })
                    .inner
            } else {
                let mut show_patch = self.pane == Pane::Diff;
                c::tabs(
                    ui,
                    &mut show_patch,
                    &[
                        (false, l.local("Commit details", "Detalhes do commit")),
                        (true, "Diff"),
                    ],
                    p,
                );
                if show_patch {
                    self.pane = Pane::Diff;
                    self.patch(ui, view, l, p);
                    None
                } else {
                    self.details(ui, view, writable, l)
                }
            };
            command = command.or(inspector);
        }
        if !layout.readable || view.busy {
            return None;
        }
        if command.is_none() {
            if view.pagination == Pagination::Initial {
                command = Some(Command::More);
            } else if self.selected.is_none() && view.details.is_none() && !view.history.is_empty()
            {
                let revision = &view.history[0].revision;
                self.selected = Some(revision.id.clone());
                command = Some(Command::Details(revision.id.clone(), None));
            }
        }
        if let Some(Command::Details(id, _)) = &command {
            self.selected = Some(id.clone());
        }
        command.map(|command| intent(view, command))
    }

    fn revisions(
        &mut self,
        ui: &mut Ui,
        view: &NavigationView,
        l: Language,
        p: Palette,
    ) -> Option<Command> {
        let (changed, command) = self.history_controls(ui, view, l, p);
        if command.is_some() {
            return command;
        }
        let changed_rows = self
            .rows
            .as_ref()
            .is_none_or(|rows| !Arc::ptr_eq(rows, &view.history));
        if changed || changed_rows {
            self.rows = Some(view.history.clone());
            let needle = self.query.trim().to_lowercase();
            self.visible = view
                .history
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    let commit = &entry.revision;
                    commit.subject.to_lowercase().contains(&needle)
                        || commit.author.name.to_lowercase().contains(&needle)
                        || commit.id.contains(&needle)
                })
                .map(|(index, _)| index)
                .collect();
        }
        if changed_rows {
            let lanes = view
                .history
                .iter()
                .map(|row| {
                    row.edges
                        .iter()
                        .fold(row.node, |index, &(from, to)| index.max(from).max(to))
                })
                .max()
                .unwrap_or(0);
            self.graph_width = match lanes {
                0 => 18.0,
                1..=3 => 32.0,
                _ => 66.0,
            };
        }
        let mut action = None;
        if view.pagination == Pagination::More {
            action = Panel::bottom("history-pagination")
                .frame(c::panel(p.surface, 0))
                .show(ui, |ui| {
                    ui.add_enabled(
                        !view.busy,
                        Button::new(l.local("Load more commits", "Carregar mais commits")),
                    )
                    .clicked()
                    .then_some(Command::More)
                })
                .inner;
        } else if view.pagination == Pagination::Limit {
            ui.colored_label(
                p.muted,
                l.local(
                    "History limit reached for this view",
                    "Limite de histórico atingido nesta visão",
                ),
            );
        }
        if self.visible.is_empty() && view.pagination != Pagination::Initial {
            ui.label(l.local(
                "No matching loaded commits",
                "Nenhum commit carregado corresponde à busca",
            ));
        }
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ScrollArea::vertical()
                .id_salt("real-history-rows")
                .auto_shrink([false; 2])
                .show_rows(ui, HISTORY_ROW_HEIGHT, self.visible.len(), |ui, range| {
                    for index in range {
                        let entry = &view.history[self.visible[index]];
                        ui.push_id(&entry.revision.id, |ui| {
                            if self.revision_row(ui, view, entry, self.visible[index], l, p)
                                && !view.busy
                            {
                                action = Some(Command::Details(entry.revision.id.clone(), None));
                                self.pane = Pane::Details;
                            }
                        });
                    }
                });
        });
        action
    }

    fn history_controls(
        &mut self,
        ui: &mut Ui,
        view: &NavigationView,
        l: Language,
        p: Palette,
    ) -> (bool, Option<Command>) {
        let mut all_refs = view.all_refs;
        let (compare, ()) = egui::Sides::new().shrink_left().truncate().show(
            ui,
            |ui| {
                ui.add_enabled(
                    Self::menu_available(Some(view), MenuCommand::Compare),
                    Button::new(l.local("Compare with branch…", "Comparar com branch…"))
                        .fill(p.background)
                        .stroke(Stroke::new(1.0, p.divider))
                        .min_size(vec2(ui.available_width(), 0.0))
                        .truncate(),
                )
                .clicked()
            },
            |ui| {
                let tools = c::icon_button(
                    ui,
                    c::Icon::Settings,
                    l.local("History options", "Opções do histórico"),
                    p,
                );
                egui::Popup::menu(&tools).show(|ui| {
                    if ui
                        .button(l.local("Filter loaded commits…", "Filtrar commits carregados…"))
                        .clicked()
                    {
                        self.search = Search::Focus;
                        ui.close();
                    }
                    if ui
                        .checkbox(&mut all_refs, l.local("All branches", "Todas as branches"))
                        .changed()
                    {
                        ui.close();
                    }
                });
            },
        );
        if compare {
            self.open_menu(view, MenuCommand::Compare);
        }
        if all_refs != view.all_refs && !view.busy {
            return (false, Some(Command::UseAllRefs(all_refs)));
        }
        let search = Id::unique("history-search");
        if self.search == Search::Focus {
            self.search = Search::Open;
            ui.memory_mut(|memory| memory.request_focus(search));
        }
        let mut changed = false;
        if self.search == Search::Open {
            egui::Sides::new().shrink_left().show(
                ui,
                |ui| {
                    changed = c::search(ui, search, l, p, &mut self.query)
                        .on_hover_text(
                            l.local("Search loaded commits", "Pesquisar commits carregados"),
                        )
                        .changed();
                },
                |ui| {
                    if c::icon_button(
                        ui,
                        c::Icon::Close,
                        l.local("Clear commit filter", "Limpar filtro de commits"),
                        p,
                    )
                    .clicked()
                    {
                        self.search = Search::Closed;
                    }
                },
            );
        }
        if self.search == Search::Closed && !self.query.is_empty() {
            self.query.clear();
            changed = true;
        }
        (changed, None)
    }

    fn revision_row(
        &self,
        ui: &mut Ui,
        view: &NavigationView,
        entry: &HistoryRow,
        index: usize,
        l: Language,
        p: Palette,
    ) -> bool {
        let (rect, response) = ui.allocate_exact_size(
            vec2(ui.available_width(), HISTORY_ROW_HEIGHT),
            egui::Sense::click(),
        );
        let commit = &entry.revision;
        let selected = self.selected.as_ref() == Some(&commit.id);
        let summary = if view.head.as_ref() == Some(&commit.id) {
            format!("HEAD · {}", commit.subject)
        } else if commit.parents.len() > 1 {
            format!("{} · {}", l.local("Merge", "Merge"), commit.subject)
        } else {
            commit.subject.clone()
        };
        let painter = ui.painter_at(rect);
        if selected || response.hovered() {
            painter.rect_filled(rect, 0.0, if selected { p.selected } else { p.hover });
        }
        if selected {
            painter.vline(
                rect.left() + 1.0,
                rect.y_range(),
                Stroke::new(2.0, p.accent),
            );
        }
        if response.has_focus() {
            painter.rect_stroke(
                rect,
                0.0,
                Stroke::new(1.0, p.focus),
                egui::StrokeKind::Inside,
            );
        }
        painter.hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            Stroke::new(1.0, p.divider),
        );
        let unfiltered = self.query.trim().is_empty();
        draw_graph(
            ui,
            egui::Rect::from_min_size(rect.min, vec2(self.graph_width, rect.height())),
            (unfiltered && index > 0).then(|| &view.history[index - 1]),
            entry,
            unfiltered,
            p,
        );
        let mut meta = format!(
            "{} · {}",
            commit.author.name,
            commit.author.time.split(' ').next().unwrap_or("")
        );
        if let Some(reference) = view
            .refs
            .iter()
            .find(|reference| reference.commit == commit.id)
        {
            meta.push_str(" · ");
            meta.push_str(&reference.label);
        }
        let x = rect.left() + self.graph_width + 2.0;
        for (caption, y, font, color) in [
            (&summary, 4.0, FontId::new(14.0, weight()), p.text),
            (&meta, 25.0, FontId::proportional(13.0), p.muted),
        ] {
            let mut job = egui::text::LayoutJob::simple(
                caption.clone(),
                font,
                color,
                (rect.right() - x - 6.0).max(0.0),
            );
            job.wrap.max_rows = 1;
            painter.galley(pos2(x, rect.top() + y), painter.layout_job(job), color);
        }
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, &summary)
        });
        response
            .on_hover_ui(|ui| {
                ui.strong(&commit.subject);
                ui.label(format!("{} · {}", commit.author.name, commit.author.time));
                ui.monospace(&commit.id);
                for reference in view
                    .refs
                    .iter()
                    .filter(|reference| reference.commit == commit.id)
                {
                    ui.colored_label(p.accent, &reference.label);
                }
            })
            .clicked()
    }

    fn details(
        &mut self,
        ui: &mut Ui,
        view: &NavigationView,
        writable: bool,
        l: Language,
    ) -> Option<Command> {
        let Some(detail) = &view.details else {
            ui.label(l.local(
                "Select a commit to inspect it",
                "Selecione um commit para inspecionar",
            ));
            return None;
        };
        ScrollArea::vertical()
            .id_salt("real-commit-details")
            .show(ui, |ui| {
                let mut action = commit_metadata(ui, detail, l);
                self.branch_from_commit(ui, view, detail, writable, l);
                ui.separator();
                let p = Palette::for_theme(ui.ctx().theme());
                if let Some(patch) = commit_files(ui, detail, view, l, 195.0, p) {
                    action = Some(patch);
                    self.pane = Pane::Diff;
                }
                if view.busy {
                    c::loading(ui, l.local("Loading…", "Carregando…"), None);
                }
                action
            })
            .inner
    }

    fn inspector(
        &mut self,
        ui: &mut Ui,
        view: &NavigationView,
        writable: bool,
        l: Language,
        p: Palette,
        allow_initial_patch: bool,
    ) -> Option<Command> {
        let Some(detail) = &view.details else {
            return self.details(ui, view, writable, l);
        };
        let metadata = self.inspector_header(ui, view, detail, writable, l, p);
        let files = Panel::left("history-files")
            .resizable(true)
            .default_size(250.0)
            .size_range(143.0..=(ui.available_width() * 0.46))
            .frame(c::panel(p.surface, 0))
            .show(ui, |ui| {
                commit_files(ui, detail, view, l, ui.available_height(), p)
            })
            .inner;
        ui.scope(|ui| {
            if detail.files.is_empty() {
                ui.label(l.local(
                    "No changed files in this comparison",
                    "Nenhum arquivo alterado nesta comparação",
                ));
            } else if view.busy && view.patch.is_none() {
                c::loading(ui, l.local("Loading…", "Carregando…"), None);
            } else {
                self.patch(ui, view, l, p);
            }
        });
        let action = metadata.or(files);
        if allow_initial_patch
            && action.is_none()
            && !view.busy
            && view.patch.is_none()
            && self
                .previewed
                .as_ref()
                .is_none_or(|previous| !Arc::ptr_eq(previous, detail))
            && let Some(file) = detail.files.first()
        {
            self.previewed = Some(detail.clone());
            return Some(Command::Patch(
                detail.revision.id.clone(),
                detail.parent.clone(),
                file.path.clone(),
            ));
        }
        action
    }

    fn inspector_header(
        &mut self,
        ui: &mut Ui,
        view: &NavigationView,
        detail: &Details,
        writable: bool,
        l: Language,
        p: Palette,
    ) -> Option<Command> {
        Panel::top("history-commit-header")
            .frame(c::panel(p.surface, 0).inner_margin(Margin::symmetric(10, 4)))
            .show(ui, |ui| {
                egui::Sides::new().shrink_left().truncate().show(
                    ui,
                    |ui| {
                        ui.add(
                            Label::new(
                                RichText::new(&detail.revision.subject)
                                    .family(weight())
                                    .size(16.0),
                            )
                            .truncate(),
                        )
                        .on_hover_text(&detail.revision.subject);
                    },
                    |ui| {
                        let label = l.local("Commit details", "Detalhes do commit");
                        if c::icon_button(ui, c::Icon::Chevron, label, p).clicked() {
                            self.show_details = !self.show_details;
                        }
                    },
                );
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(&detail.revision.author.name).small());
                    ui.label(
                        RichText::new(&detail.revision.author.time)
                            .small()
                            .color(p.muted),
                    );
                    let hash = &detail.revision.id[..detail.revision.id.len().min(8)];
                    ui.label(RichText::new(hash).small().monospace())
                        .on_hover_text(&detail.revision.id);
                    if c::icon_button(ui, c::Icon::Copy, l.local("Copy hash", "Copiar hash"), p)
                        .clicked()
                    {
                        ui.ctx().copy_text(detail.revision.id.clone());
                    }
                    let (added, removed) = detail
                        .files
                        .iter()
                        .filter_map(|file| file.lines)
                        .fold((0_u64, 0_u64), |(a, r), (plus, minus)| {
                            (a.saturating_add(plus), r.saturating_add(minus))
                        });
                    if detail.files.iter().any(|file| file.lines.is_some()) {
                        ui.label(RichText::new(format!("+{added}")).small().color(p.success));
                        ui.label(RichText::new(format!("−{removed}")).small().color(p.error));
                    }
                });
                if self.show_details {
                    ScrollArea::vertical()
                        .max_height(130.0)
                        .show(ui, |ui| {
                            let action = commit_metadata(ui, detail, l);
                            self.branch_from_commit(ui, view, detail, writable, l);
                            action
                        })
                        .inner
                } else {
                    None
                }
            })
            .inner
    }

    fn branch_from_commit(
        &mut self,
        ui: &mut Ui,
        view: &NavigationView,
        detail: &Details,
        writable: bool,
        l: Language,
    ) {
        if ui
            .add_enabled(
                writable && !view.busy && view.merge == Merge::None,
                Button::new(l.local("New branch from commit", "Nova branch a partir do commit")),
            )
            .clicked()
        {
            self.open(
                view,
                Form::Create(BranchBase::Commit(detail.revision.id.clone())),
            );
        }
    }

    fn patch(&mut self, ui: &mut Ui, view: &NavigationView, l: Language, p: Palette) {
        let Some(patch) = &view.patch else {
            ui.label(l.local(
                "Select a changed file in commit details",
                "Selecione um arquivo alterado nos detalhes do commit",
            ));
            return;
        };
        self.diff.show_commit(
            ui,
            Id::unique(("history-patch", &patch.key)),
            patch,
            &mut self.mode,
            l,
            p,
        );
    }
}

fn commit_files(
    ui: &mut Ui,
    detail: &Details,
    view: &NavigationView,
    l: Language,
    max_height: f32,
    p: Palette,
) -> Option<Command> {
    c::panel(p.elevated, 0).show(ui, |ui| {
        ui.allocate_ui_with_layout(
            vec2(ui.available_width(), size::HISTORY_BAR),
            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
            |ui| {
                ui.label(if detail.files.len() == 1 {
                    l.local("1 changed file", "1 arquivo alterado").to_owned()
                } else {
                    format!(
                        "{} {}",
                        detail.files.len(),
                        l.local("changed files", "arquivos alterados")
                    )
                });
            },
        );
    });
    let mut action = None;
    ScrollArea::vertical()
        .id_salt("commit-paths")
        .max_height(max_height.min(ui.available_height()).max(0.0))
        .show_rows(
            ui,
            crate::theme::size::CONTROL,
            detail.files.len(),
            |ui, visible| {
                for index in visible {
                    let file = &detail.files[index];
                    let stats = file.lines.map_or_else(
                        || l.text(Text::Binary).to_owned(),
                        |(add, remove)| format!("+{add} −{remove}"),
                    );
                    let path = file.path.to_string_lossy();
                    let title = file.previous.as_ref().map_or_else(
                        || path.to_string(),
                        |old| format!("{} → {path}", old.display()),
                    );
                    let selected = view
                        .patch
                        .as_ref()
                        .is_some_and(|patch| patch.key.path == file.path);
                    if ui
                        .add_enabled_ui(!view.busy, |ui| {
                            c::workspace_row(ui, &title, Some(&stats), selected, p)
                        })
                        .inner
                        .on_hover_text(title)
                        .clicked()
                    {
                        action = Some(Command::Patch(
                            detail.revision.id.clone(),
                            detail.parent.clone(),
                            file.path.clone(),
                        ));
                    }
                }
            },
        );
    action
}

fn commit_metadata(ui: &mut Ui, detail: &Details, l: Language) -> Option<Command> {
    let commit = &detail.revision;
    ui.heading(&commit.subject);
    ui.horizontal_wrapped(|ui| {
        ui.monospace(&commit.id);
        if ui.button(l.local("Copy hash", "Copiar hash")).clicked() {
            ui.ctx().copy_text(commit.id.clone());
        }
    });
    for (label, person) in [
        (l.local("Author", "Autor"), &commit.author),
        (l.local("Committer", "Autor do commit"), &detail.committer),
    ] {
        ui.label(format!(
            "{label} · {} <{}> · {}",
            person.name, person.email, person.time
        ));
    }
    if let Some(signature) = detail.signature {
        let kind = match signature {
            SignatureKind::OpenPgp => "OpenPGP",
            SignatureKind::Ssh => "SSH",
            SignatureKind::Other => l.local("Other", "Outro"),
        };
        ui.label(format!(
            "{kind} · {}",
            l.local(
                "Signature present, unverified",
                "Assinatura presente, sem verificação"
            )
        ));
    }
    ui.add(egui::Label::new(&detail.message).wrap().selectable(true));
    let mut action = None;
    if !commit.parents.is_empty() {
        ui.strong(l.local("Compare with parent", "Comparar com o pai"));
        for parent in &commit.parents {
            ui.horizontal_wrapped(|ui| {
                if ui
                    .selectable_label(detail.parent.as_ref() == Some(parent), &parent[..12])
                    .clicked()
                {
                    action = Some(Command::Details(commit.id.clone(), Some(parent.clone())));
                }
                if ui.button(l.local("Open parent", "Abrir pai")).clicked() {
                    action = Some(Command::Details(parent.clone(), None));
                }
            });
        }
    }
    action
}

fn draw_graph(
    ui: &Ui,
    rect: egui::Rect,
    previous: Option<&HistoryRow>,
    row: &HistoryRow,
    show_edges: bool,
    p: Palette,
) {
    let canvas = ui.painter_at(rect);
    let point = |lane: usize, y| {
        pos2(
            rect.left() + 7.0 + f32::from(u16::try_from(lane.min(8)).unwrap_or(8)) * 7.0,
            y,
        )
    };
    let colors = [p.accent, p.info, p.success, p.warning, p.muted];
    let center = rect.center().y;
    if let Some(previous) = previous {
        for &(_, to) in previous.edges.iter().filter(|&&(_, to)| to < 9) {
            canvas.line_segment(
                [point(to, rect.top()), point(to, center)],
                Stroke::new(1.5, colors[to % colors.len()]),
            );
        }
    }
    if show_edges {
        for &(from, to) in row.edges.iter().filter(|&&(from, to)| from < 9 && to < 9) {
            canvas.line_segment(
                [point(from, center), point(to, rect.bottom())],
                Stroke::new(1.5, colors[to % colors.len()]),
            );
        }
    }
    if row.node < 9 {
        canvas.circle_filled(
            point(row.node, center),
            if row.revision.parents.len() > 1 {
                4.5
            } else {
                3.2
            },
            colors[row.node % colors.len()],
        );
    } else {
        canvas.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "…",
            egui::FontId::monospace(12.0),
            p.muted,
        );
    }
}
