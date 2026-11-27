use std::{path::PathBuf, sync::Arc};

use egui::{Button, Id, Panel, RichText, ScrollArea, Stroke, TextEdit, Ui, pos2, vec2};
use gitrust_core::{
    changes::{CommitAuthor, CommitDraft, CommitSigning},
    navigation::{
        BranchBase, Details, HistoryRow, Merge, NavigationCommand as Command, NavigationIntent,
        NavigationNotice, NavigationView, Pagination, Reference, SignatureKind,
    },
    workspaces::Checkout,
};

use super::{
    commands::Command as MenuCommand,
    repositories::{WorkspaceLayout, single_pane},
};

use crate::{
    components as c,
    diff::{Diff, DiffMode, DiffView},
    i18n::{Language, Text},
    theme::Palette,
};

const HISTORY_ROW_HEIGHT: f32 = 62.0;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum Tab {
    #[default]
    Changes,
    History,
    Overview,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Pane {
    #[default]
    Revisions,
    Details,
    Diff,
}

#[derive(Debug)]
enum Form {
    Create(BranchBase),
    Rename(Reference),
    Delete(Reference),
    Merge(Reference),
    Squash(Reference),
    Rebase(Reference),
    Stash,
    Finish(String),
    Abort,
    Choose(MenuCommand),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Signing {
    #[default]
    None,
    Gpg,
    Ssh,
}

#[derive(Debug, Default)]
pub(super) struct NavigationUi {
    pub tab: Tab,
    pane: Pane,
    branch_query: String,
    choose_query: String,
    compare_waiting: bool,
    stash_waiting: bool,
    focus_search: bool,
    branch: Option<Arc<[u8]>>,
    form: Option<(u64, Form)>,
    input: String,
    body: String,
    author: CommitAuthor,
    signing: Signing,
    key: String,
    query: String,
    rows: Option<Arc<Vec<HistoryRow>>>,
    visible: Vec<usize>,
    graph_width: f32,
    selected: Option<String>,
    diff: DiffView,
    mode: DiffMode,
    anchor: Option<(u64, String, bool)>,
    previewed: Option<Arc<Details>>,
}

impl NavigationUi {
    pub(super) fn focus_search(&mut self) {
        self.pane = Pane::Revisions;
        self.focus_search = true;
    }

    pub(super) fn menu_available(view: Option<&NavigationView>, command: MenuCommand) -> bool {
        let Some(view) = view.filter(|view| !view.busy) else {
            return false;
        };
        match command {
            MenuCommand::NewBranch => view.head.is_some(),
            MenuCommand::Rename => view.refs.iter().any(|branch| branch.current),
            MenuCommand::Delete => candidates(view, command).next().is_some(),
            MenuCommand::Merge | MenuCommand::Squash | MenuCommand::Rebase => {
                view.refs.iter().any(|branch| branch.current)
                    && candidates(view, command).next().is_some()
            }
            MenuCommand::Update => {
                view.refs.iter().any(|branch| branch.current)
                    && view.default_branch.as_ref().is_some_and(|name| {
                        view.refs
                            .iter()
                            .any(|branch| &branch.name == name && !branch.current)
                    })
            }
            MenuCommand::Compare => {
                view.head.is_some() && candidates(view, command).next().is_some()
            }
            _ => false,
        }
    }

    pub(super) fn show_stashes(&mut self, view: &NavigationView) -> NavigationIntent {
        self.stash_waiting = true;
        intent(view, Command::ListStash)
    }

    pub(super) fn open_menu(&mut self, view: &NavigationView, command: MenuCommand) {
        self.choose_query.clear();
        let form = match command {
            MenuCommand::NewBranch => Form::Create(BranchBase::Head),
            MenuCommand::Stash => Form::Stash,
            MenuCommand::Rename => {
                let Some(branch) = view.refs.iter().find(|branch| branch.current) else {
                    return;
                };
                Form::Rename(branch.clone())
            }
            MenuCommand::Update => {
                let Some(branch) = view
                    .refs
                    .iter()
                    .find(|branch| view.default_branch.as_ref() == Some(&branch.name))
                else {
                    return;
                };
                Form::Merge(branch.clone())
            }
            _ => Form::Choose(command),
        };
        self.open(view, form);
    }

    pub(super) fn branch_picker(
        &mut self,
        ui: &mut Ui,
        view: &NavigationView,
        writable: bool,
        l: Language,
        p: Palette,
    ) -> Option<NavigationIntent> {
        ui.set_width((ui.ctx().content_rect().width() - 24.0).clamp(175.0, 410.0));
        let enabled = writable && !view.busy && view.merge == Merge::None;
        ui.horizontal(|ui| {
            let search_width = ui.available_width() * 0.62;
            ui.vertical(|ui| {
                ui.set_width(search_width);
                c::search(
                    ui,
                    Id::unique("branch-filter"),
                    l,
                    p,
                    &mut self.branch_query,
                );
            });
            if ui
                .add_enabled(
                    enabled && view.head.is_some(),
                    Button::new(l.local("New branch", "Nova branch")).truncate(),
                )
                .clicked()
            {
                self.open(view, Form::Create(BranchBase::Head));
                ui.close();
            }
        });
        ui.separator();
        self.branch_rows(ui, view, l, p);
        ui.separator();
        let entry = view
            .refs
            .iter()
            .find(|item| self.branch.as_ref() == Some(&item.name))
            .or_else(|| view.refs.iter().find(|item| item.current));
        if let Some(entry) = entry
            && let Some(upstream) = &entry.upstream
        {
            ui.label(format!("Upstream · {upstream}"));
            if let Some((ahead, behind)) = entry.divergence {
                ui.label(format!("↑ {ahead}    ↓ {behind}"));
            }
        }
        let mut command = None;
        ui.add_enabled_ui(enabled, |ui| {
            if let Some(entry) = entry {
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .button(l.local("Create from selected", "Criar a partir da seleção"))
                        .clicked()
                    {
                        self.open(view, Form::Create(BranchBase::Ref(entry.clone())));
                        ui.close();
                    }
                    if !entry.remote {
                        command = self.local_actions(ui, view, entry, l);
                    }
                });
            }
            if Self::menu_available(Some(view), MenuCommand::Merge)
                && ui
                    .button(l.local("Choose a branch to merge…", "Escolher branch para merge…"))
                    .clicked()
            {
                self.open_menu(view, MenuCommand::Merge);
                ui.close();
            }
        });
        command
    }

    fn branch_rows(&mut self, ui: &mut Ui, view: &NavigationView, l: Language, p: Palette) {
        let query = self.branch_query.trim().to_lowercase();
        ScrollArea::vertical()
            .max_height((ui.ctx().content_rect().height() * 0.42).clamp(56.0, 340.0))
            .show(ui, |ui| {
                let mut any = false;
                for (group, label) in [
                    ((false, true), l.local("Default branch", "Branch padrão")),
                    ((false, false), l.local("Local branches", "Branches locais")),
                    (
                        (true, false),
                        l.local("Remote-tracking references", "Referências remotas locais"),
                    ),
                ] {
                    let entries = view.refs.iter().filter(|item| {
                        let is_default =
                            !item.remote && view.default_branch.as_ref() == Some(&item.name);
                        (item.remote, is_default) == group
                            && (query.is_empty() || item.label.to_lowercase().contains(&query))
                    });
                    for (index, entry) in entries.enumerate() {
                        any = true;
                        if index == 0 {
                            ui.strong(label);
                        }
                        let selected = self
                            .branch
                            .as_ref()
                            .map_or(entry.current, |name| name == &entry.name);
                        let response =
                            c::branch_choice(ui, &entry.label, entry.current, selected, p);
                        if response.clicked() {
                            self.branch = Some(entry.name.clone());
                        }
                    }
                }
                if !any {
                    ui.label(l.local("No matching branches", "Nenhuma branch encontrada"));
                }
            });
    }

    fn local_actions(
        &mut self,
        ui: &mut Ui,
        view: &NavigationView,
        reference: &Reference,
        l: Language,
    ) -> Option<NavigationIntent> {
        let available = !reference.occupied || reference.current;
        if ui
            .add_enabled(
                !reference.current && available,
                Button::new(l.local("Switch branch", "Alternar branch")),
            )
            .clicked()
        {
            ui.close();
            return Some(intent(view, Command::Switch(reference.clone())));
        }
        if ui
            .add_enabled(
                available,
                Button::new(l.local("Rename branch", "Renomear branch")),
            )
            .clicked()
        {
            self.open(view, Form::Rename(reference.clone()));
            ui.close();
        }
        if ui
            .add_enabled(
                !reference.occupied && !reference.current,
                Button::new(l.local("Delete branch", "Excluir branch")),
            )
            .clicked()
        {
            self.open(view, Form::Delete(reference.clone()));
            ui.close();
        }
        if ui
            .add_enabled(
                !reference.current && view.refs.iter().any(|branch| branch.current),
                Button::new(l.local("Merge into current branch", "Integrar na branch atual")),
            )
            .clicked()
        {
            self.open(view, Form::Merge(reference.clone()));
            ui.close();
        }
        None
    }

    pub(super) fn status(&mut self, ui: &mut Ui, view: &NavigationView, l: Language, p: Palette) {
        if let Merge::Active {
            source,
            unresolved,
            can_abort,
        } = &view.merge
        {
            ui.horizontal_wrapped(|ui| {
                if *unresolved == 0 {
                    ui.colored_label(
                        p.success,
                        l.local("Merge ready to finish", "Merge pronto para conclusão"),
                    );
                } else {
                    ui.colored_label(
                        p.warning,
                        format!(
                            "{} · {unresolved}",
                            l.local(
                                "Merge in progress, conflicts pending",
                                "Merge em andamento, conflitos pendentes"
                            )
                        ),
                    );
                }
                if ui
                    .add_enabled(
                        !view.busy && *unresolved == 0,
                        Button::new(l.local("Finish merge", "Concluir merge")),
                    )
                    .clicked()
                {
                    self.open(view, Form::Finish(source.clone()));
                }
                if ui
                    .add_enabled(
                        !view.busy && *can_abort,
                        Button::new(l.local("Abort merge", "Abortar merge")),
                    )
                    .clicked()
                {
                    self.open(view, Form::Abort);
                }
            });
        }
        if let Some(notice) = view.notice {
            ui.label(notice_text(notice, l));
        }
    }

    pub(super) fn history(
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
                c::panel(p.background, 8)
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
        if std::mem::take(&mut self.focus_search) {
            ui.memory_mut(|memory| memory.request_focus(Id::unique("history-search")));
        }
        let changed = c::search(ui, Id::unique("history-search"), l, p, &mut self.query).changed();
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
                1..=3 => 38.0,
                _ => 72.0,
            };
        }
        ui.colored_label(
            p.muted,
            l.local("Search loaded commits", "Pesquisar commits carregados"),
        );
        let mut all_refs = view.all_refs;
        c::tabs(
            ui,
            &mut all_refs,
            &[
                (false, "HEAD"),
                (true, l.local("All branches", "Todas as branches")),
            ],
            p,
        );
        if all_refs != view.all_refs && !view.busy {
            return Some(Command::UseAllRefs(all_refs));
        }
        if view.pagination == Pagination::Limit {
            ui.colored_label(
                p.muted,
                l.local(
                    "History limit reached for this view",
                    "Limite de histórico atingido nesta visão",
                ),
            );
        }
        let mut action = None;
        if ui
            .add_enabled(
                matches!(view.pagination, Pagination::Initial | Pagination::More) && !view.busy,
                Button::new(l.local("Load more commits", "Carregar mais commits")),
            )
            .clicked()
        {
            action = Some(Command::More);
        }
        if self.visible.is_empty() && view.pagination != Pagination::Initial {
            ui.label(l.local(
                "No matching loaded commits",
                "Nenhum commit carregado corresponde à busca",
            ));
        }
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
        action
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
        let row_gap = ui.spacing().item_spacing.y;
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(
                vec2(self.graph_width, HISTORY_ROW_HEIGHT),
                egui::Sense::hover(),
            );
            let previous = if self.query.trim().is_empty() && index > 0 {
                view.history.get(index - 1)
            } else {
                None
            };
            draw_graph(
                ui,
                rect,
                entry,
                previous,
                self.query.trim().is_empty(),
                row_gap,
                p,
            );
            let commit = &entry.revision;
            ui.vertical(|ui| {
                let head = view.head.as_ref() == Some(&commit.id);
                let summary = if head {
                    format!("HEAD · {}", commit.subject)
                } else if commit.parents.len() > 1 {
                    format!("{} · {}", l.local("Merge", "Merge"), commit.subject)
                } else {
                    commit.subject.clone()
                };
                let response = c::workspace_row(
                    ui,
                    &summary,
                    None,
                    self.selected.as_ref() == Some(&commit.id),
                    p,
                );
                ui.add(
                    egui::Label::new(format!(
                        "{} · {} · {}",
                        &commit.id[..12],
                        commit.author.name,
                        commit.author.time
                    ))
                    .truncate(),
                );
                let captions = view
                    .refs
                    .iter()
                    .filter(|reference| reference.commit == commit.id)
                    .map(|reference| reference.label.as_str())
                    .collect::<Vec<_>>();
                if !captions.is_empty() {
                    ui.add(
                        egui::Label::new(
                            RichText::new(captions.join(" · ")).color(p.accent).small(),
                        )
                        .truncate(),
                    );
                }
                response.clicked()
            })
            .inner
        })
        .inner
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
        ui.add(egui::Label::new(RichText::new(&detail.revision.subject).heading()).truncate());
        ui.horizontal_wrapped(|ui| {
            ui.label(&detail.revision.author.name);
            ui.colored_label(p.muted, &detail.revision.author.time);
            ui.monospace(&detail.revision.id[..12]);
            if c::icon_button(ui, c::Icon::Copy, l.local("Copy hash", "Copiar hash"), p).clicked() {
                ui.ctx().copy_text(detail.revision.id.clone());
            }
        });
        let metadata = ui
            .collapsing(l.local("Commit details", "Detalhes do commit"), |ui| {
                ScrollArea::vertical()
                    .max_height(130.0)
                    .show(ui, |ui| {
                        let action = commit_metadata(ui, detail, l);
                        self.branch_from_commit(ui, view, detail, writable, l);
                        action
                    })
                    .inner
            })
            .body_returned
            .flatten();
        ui.separator();
        let files = Panel::left("history-files")
            .resizable(true)
            .default_size(223.0)
            .size_range(143.0..=(ui.available_width() * 0.46))
            .frame(c::panel(p.surface, 8))
            .show(ui, |ui| {
                commit_files(ui, detail, view, l, ui.available_height(), p)
            })
            .inner;
        c::panel(p.background, 8).show(ui, |ui| {
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
        ui.add(
            egui::Label::new(RichText::new(patch.key.path.to_string_lossy()).strong()).truncate(),
        );
        if patch.binary {
            ui.colored_label(p.warning, l.text(Text::Binary));
        }
        self.diff.show(
            ui,
            Id::unique(("history-patch", &patch.key)),
            Diff::Real(patch),
            &mut self.mode,
            l,
            p,
        );
    }

    fn open(&mut self, view: &NavigationView, form: Form) {
        self.input = match &form {
            Form::Finish(_) => "Merge branches".into(),
            Form::Rename(branch) => branch.label.clone(),
            _ => String::new(),
        };
        self.body.clear();
        self.author.clone_from(&view.author);
        self.signing = Signing::None;
        self.key.clear();
        self.form = Some((view.version, form));
    }

    pub(super) fn dialog(
        &mut self,
        ui: &Ui,
        view: &NavigationView,
        checkout: &Checkout,
        l: Language,
        p: Palette,
    ) -> Option<NavigationIntent> {
        if self.stash_waiting && self.form.is_none() {
            return self.stash_dialog(ui, view, l, p);
        }
        if self.compare_waiting && self.form.is_none() {
            self.comparison_dialog(ui, view, l, p);
            return None;
        }
        let (version, form) = self.form.take()?;
        if version != view.version {
            return None;
        }
        if let Form::Choose(command) = form {
            return self.choose_dialog(ui, view, l, p, command);
        }
        let heading = match &form {
            Form::Create(_) => l.local("Create branch", "Criar branch"),
            Form::Rename(_) => l.local("Rename branch", "Renomear branch"),
            Form::Delete(_) => l.local("Delete integrated branch", "Excluir branch integrada"),
            Form::Merge(_) => l.local("Review merge", "Revisar merge"),
            Form::Squash(_) => l.local("Confirm squash merge", "Confirmar merge com squash"),
            Form::Rebase(_) => l.local("Confirm rebase", "Confirmar rebase"),
            Form::Stash => l.local("Stash working changes", "Guardar alterações locais"),
            Form::Finish(_) => l.local("Create merge commit", "Criar commit de merge"),
            Form::Abort => l.local("Confirm merge abort", "Confirmar aborto do merge"),
            Form::Choose(_) => unreachable!(),
        };
        let dialog = c::modal(ui, "navigation-operation", p, heading, |ui| {
            ScrollArea::vertical()
                .max_height((ui.ctx().content_rect().height() - 124.0).max(76.0))
                .show(ui, |ui| {
                    self.form_content(ui, &form, checkout, l, p);
                });
            ui.horizontal_wrapped(|ui| {
                let confirm = ui
                    .add_enabled_ui(!view.busy, |ui| c::primary(ui, heading, p))
                    .inner
                    .clicked();
                let cancel = ui.button(l.text(Text::Cancel)).clicked();
                (confirm, cancel)
            })
            .inner
        });
        if dialog.inner.0 {
            let command = match form {
                Form::Create(base) => Command::Create(self.input.trim().to_owned(), base),
                Form::Rename(branch) => Command::Rename(branch, self.input.trim().to_owned()),
                Form::Delete(branch) => Command::Delete(branch),
                Form::Merge(branch) => Command::Merge(branch, self.author.clone()),
                Form::Squash(branch) => Command::Squash(branch, self.author.clone()),
                Form::Rebase(branch) => Command::Rebase(branch),
                Form::Stash => Command::Stash,
                Form::Finish(source) => Command::Finish(
                    source,
                    CommitDraft {
                        title: self.input.clone(),
                        description: self.body.clone(),
                        author: self.author.clone(),
                        signing: match self.signing {
                            Signing::None => CommitSigning::None,
                            Signing::Gpg => CommitSigning::OpenPgp(self.key.trim().to_owned()),
                            Signing::Ssh => CommitSigning::Ssh(PathBuf::from(self.key.trim())),
                        },
                        amend: false,
                    },
                ),
                Form::Abort => Command::Abort,
                Form::Choose(_) => unreachable!(),
            };
            return Some(intent(view, command));
        }
        if !dialog.inner.1 && !dialog.should_close() {
            self.form = Some((version, form));
        }
        None
    }

    fn stash_dialog(
        &mut self,
        ui: &Ui,
        view: &NavigationView,
        l: Language,
        p: Palette,
    ) -> Option<NavigationIntent> {
        let dialog = c::modal(
            ui,
            "local-stashes",
            p,
            l.local("Stashed changes", "Alterações guardadas"),
            |ui| {
                let mut selected = None;
                if let Some(entries) = &view.stashes {
                    if entries.is_empty() {
                        ui.label(l.local("No stashed changes", "Nenhuma alteração guardada"));
                    } else {
                        ScrollArea::vertical()
                            .max_height((ui.ctx().content_rect().height() - 140.0).max(44.0))
                            .show_rows(ui, 28.0, entries.len(), |ui, rows| {
                                for index in rows {
                                    let entry = &entries[index];
                                    if ui
                                        .add(
                                            Button::new(format!(
                                                "stash@{{{index}}} · {}",
                                                entry.subject
                                            ))
                                            .truncate(),
                                        )
                                        .clicked()
                                    {
                                        selected = Some(entry.id.clone());
                                    }
                                }
                            });
                    }
                } else if view.busy {
                    c::loading(
                        ui,
                        l.local("Reading local stashes…", "Lendo alterações guardadas…"),
                        None,
                    );
                } else {
                    ui.label(view.notice.map_or(
                        l.local("Stashes unavailable", "Alterações guardadas indisponíveis"),
                        |notice| notice_text(notice, l),
                    ));
                }
                (ui.button(l.text(Text::Close)).clicked(), selected)
            },
        );
        self.stash_waiting = !dialog.inner.0 && dialog.inner.1.is_none() && !dialog.should_close();
        let selected = dialog.inner.1?;
        self.tab = Tab::History;
        self.pane = Pane::Details;
        self.anchor = view
            .root
            .as_ref()
            .map(|root| (view.identity, root.clone(), view.all_refs));
        self.selected = Some(selected.clone());
        Some(intent(view, Command::Details(selected, None)))
    }

    fn comparison_dialog(&mut self, ui: &Ui, view: &NavigationView, l: Language, p: Palette) {
        let dialog = c::modal(
            ui,
            "branch-comparison",
            p,
            l.local("Branch comparison", "Comparação de branches"),
            |ui| {
                if let Some(result) = &view.comparison {
                    ui.strong(&result.branch.label);
                    ui.label(format!(
                        "HEAD {} ↔ {}",
                        &result.head[..result.head.len().min(12)],
                        &result.branch.commit[..result.branch.commit.len().min(12)]
                    ));
                    ui.label(format!(
                        "{} {}",
                        l.local("Only on current branch:", "Apenas na branch atual:"),
                        result.ahead
                    ));
                    ui.label(format!(
                        "{} {}",
                        l.local("Only on selected branch:", "Apenas na branch selecionada:"),
                        result.behind
                    ));
                } else if view.busy {
                    c::loading(
                        ui,
                        l.local("Comparing local branches…", "Comparando branches locais…"),
                        None,
                    );
                } else {
                    ui.label(view.notice.map_or(
                        l.local("Comparison no longer available", "Comparação indisponível"),
                        |notice| notice_text(notice, l),
                    ));
                }
                ui.button(l.text(Text::Close)).clicked()
            },
        );
        self.compare_waiting = !dialog.inner && !dialog.should_close();
    }

    fn choose_dialog(
        &mut self,
        ui: &Ui,
        view: &NavigationView,
        l: Language,
        p: Palette,
        command: MenuCommand,
    ) -> Option<NavigationIntent> {
        let dialog = c::modal(ui, "choose-branch", p, command.label(l), |ui| {
            c::search(
                ui,
                Id::unique("choose-local-branch"),
                l,
                p,
                &mut self.choose_query,
            );
            let query = self.choose_query.to_lowercase();
            ScrollArea::vertical()
                .max_height((ui.ctx().content_rect().height() - 150.0).clamp(54.0, 310.0))
                .show(ui, |ui| {
                    candidates(view, command)
                        .filter(|reference| reference.label.to_lowercase().contains(&query))
                        .find(|reference| ui.button(&reference.label).clicked())
                        .cloned()
                })
                .inner
        });
        if let Some(branch) = dialog.inner {
            match command {
                MenuCommand::Compare => {
                    self.compare_waiting = true;
                    return Some(intent(view, Command::Compare(branch)));
                }
                MenuCommand::Delete => self.open(view, Form::Delete(branch)),
                MenuCommand::Squash => self.open(view, Form::Squash(branch)),
                MenuCommand::Rebase => self.open(view, Form::Rebase(branch)),
                _ => self.open(view, Form::Merge(branch)),
            }
        } else if !dialog.should_close() {
            self.form = Some((view.version, Form::Choose(command)));
        }
        None
    }

    fn form_content(
        &mut self,
        ui: &mut Ui,
        form: &Form,
        checkout: &Checkout,
        l: Language,
        p: Palette,
    ) {
        match form {
            Form::Create(_) | Form::Rename(_) => field(
                ui,
                l.local("Branch name", "Nome da branch"),
                &mut self.input,
            ),
            Form::Delete(branch) => {
                ui.label(format!(
                    "{} {}",
                    l.local("Delete the integrated branch", "Excluir a branch integrada"),
                    branch.label
                ));
            }
            Form::Merge(branch) => {
                let target = match checkout {
                    Checkout::Branch(name) | Checkout::Unborn(name) | Checkout::Detached(name) => {
                        name
                    }
                };
                ui.label(format!("{} → {target}", branch.label));
                ui.label(l.local("Git requires a clean index and worktree. A three-way merge pauses for a commit.", "O Git exige o index e a árvore de trabalho limpos. O merge de três vias pausa para o commit."));
                author(ui, &mut self.author, l);
            }
            Form::Finish(_) => {
                field(ui, l.local("Title", "Título"), &mut self.input);
                let label = ui.label(l.local("Description", "Descrição"));
                ui.add(
                    TextEdit::multiline(&mut self.body)
                        .desired_width(f32::INFINITY)
                        .desired_rows(2),
                )
                .labelled_by(label.id);
                author(ui, &mut self.author, l);
                c::tabs(
                    ui,
                    &mut self.signing,
                    &[
                        (Signing::None, l.local("Unsigned", "Sem assinatura")),
                        (Signing::Gpg, "GPG"),
                        (Signing::Ssh, "SSH"),
                    ],
                    p,
                );
                if self.signing != Signing::None {
                    let key = if self.signing == Signing::Gpg {
                        l.local("Key fingerprint", "Impressão digital da chave")
                    } else {
                        l.local("Absolute key path", "Caminho absoluto da chave")
                    };
                    field(ui, key, &mut self.key);
                }
            }
            Form::Abort => {
                ui.label(l.local("Restore the pre-merge state. Aborting is blocked if the merge content has changed.", "Restaurar o estado anterior ao merge. O aborto é bloqueado se o conteúdo do merge tiver mudado."));
            }
            Form::Squash(branch) => {
                ui.strong(&branch.label);
                ui.label(l.local(
                    "Squash the selected branch into the current working tree. Review the staged changes before creating a regular commit.",
                    "Combine as alterações da branch selecionada na árvore atual. Revise o index antes de criar um commit normal.",
                ));
            }
            Form::Rebase(branch) => {
                ui.strong(&branch.label);
                ui.label(l.local(
                    "Reapply the current branch on the selected branch. This rewrites commit IDs and requires a clean worktree.",
                    "Reaplique a branch atual sobre a selecionada. Isso reescreve os hashes e exige uma árvore de trabalho limpa.",
                ));
            }
            Form::Stash => {
                ui.label(l.local(
                    "Save staged, unstaged and untracked changes in a local stash. The saved changes are removed from the working tree. Ignored files stay in place.",
                    "Guarde em um stash local as alterações no index, fora do index e os arquivos não rastreados. As alterações guardadas saem da árvore de trabalho. Arquivos ignorados ficam no lugar.",
                ));
            }
            Form::Choose(_) => unreachable!(),
        }
    }
}

fn candidates(view: &NavigationView, command: MenuCommand) -> impl Iterator<Item = &Reference> {
    view.refs.iter().filter(move |branch| {
        !branch.current
            && match command {
                MenuCommand::Compare => true,
                MenuCommand::Delete => !branch.remote && !branch.occupied,
                _ => !branch.remote,
            }
    })
}

fn intent(view: &NavigationView, command: Command) -> NavigationIntent {
    NavigationIntent {
        version: view.version,
        command,
    }
}

fn field(ui: &mut Ui, name: &str, value: &mut String) {
    let label = ui.label(name);
    ui.add(TextEdit::singleline(value).desired_width(f32::INFINITY))
        .labelled_by(label.id);
}

fn author(ui: &mut Ui, value: &mut CommitAuthor, l: Language) {
    field(ui, l.local("Author name", "Nome do autor"), &mut value.name);
    field(
        ui,
        l.local("Author email", "E-mail do autor"),
        &mut value.email,
    );
}

fn commit_files(
    ui: &mut Ui,
    detail: &Details,
    view: &NavigationView,
    l: Language,
    max_height: f32,
    p: Palette,
) -> Option<Command> {
    ui.strong(format!(
        "{} · {}",
        l.local("Changed files", "Arquivos alterados"),
        detail.files.len()
    ));
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
    row: &HistoryRow,
    previous: Option<&HistoryRow>,
    show_edges: bool,
    gap: f32,
    p: Palette,
) {
    let canvas = ui.painter().with_clip_rect(rect.expand2(vec2(0.0, gap)));
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
                [point(to, rect.top() - gap), point(to, center)],
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

fn notice_text(notice: NavigationNotice, l: Language) -> &'static str {
    use NavigationNotice as N;
    match notice {
        N::BranchUpdated => l.local("Branch updated.", "Branch atualizada."),
        N::Integrated => l.local("Branch already integrated.", "Branch já integrada."),
        N::FastForward => l.local("Fast-forward completed.", "Fast-forward concluído."),
        N::SquashStaged => l.local(
            "Squashed changes staged for review",
            "Alterações combinadas no index para revisão",
        ),
        N::SquashConflicts => l.local(
            "Resolve squash conflicts in Changes before committing",
            "Resolva os conflitos do squash em Alterações antes do commit",
        ),
        N::Stashed => l.local(
            "Working changes saved in a local stash",
            "Alterações guardadas em um stash local",
        ),
        N::MergePending => l.local(
            "Merge applied. Review the state before finishing.",
            "Merge aplicado. Revise o estado antes de concluir.",
        ),
        N::MergeCommitted => l.local("Merge commit created.", "Commit de merge criado."),
        N::MergeAborted => l.local("Merge aborted.", "Merge abortado."),
        N::Stale => l.local(
            "Repository changed. Refresh and review the current state.",
            "O repositório mudou. Atualize e revise o estado atual.",
        ),
        N::InvalidRef => l.local("Invalid branch or commit.", "Branch ou commit inválido."),
        N::Occupied => l.local(
            "The branch is checked out in a worktree.",
            "A branch está aberta em uma árvore de trabalho.",
        ),
        N::Unmerged => l.local(
            "The branch has commits outside the current history.",
            "A branch contém commits fora do histórico atual.",
        ),
        N::SwitchBlocked => l.local(
            "Git refused to switch branches to protect local data.",
            "O Git recusou a troca de branch para proteger dados locais.",
        ),
        N::Dirty => l.local(
            "Merge requires a clean index and worktree.",
            "O merge exige o index e a árvore de trabalho limpos.",
        ),
        N::AuthorRequired => l.local(
            "Enter a valid author name and email.",
            "Informe nome e e-mail de autor válidos.",
        ),
        N::SigningFailed => l.local(
            "The requested signer failed.",
            "O assinador selecionado falhou.",
        ),
        N::Incompatible => l.local(
            "Another Git operation or repository feature blocks this action.",
            "Outra operação Git ou recurso do repositório impede esta ação.",
        ),
        N::Cancelled => l.local(
            "Operation cancelled. Review the refreshed state.",
            "Operação cancelada. Revise o estado atualizado.",
        ),
        N::Failed => l.local(
            "Git refused the operation. Review the branch name and repository state.",
            "O Git recusou a operação. Revise o nome da branch e o estado do repositório.",
        ),
    }
}
