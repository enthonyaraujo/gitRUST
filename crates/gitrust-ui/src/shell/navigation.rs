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

use crate::{
    components as c,
    diff::{Diff, DiffMode, DiffView},
    i18n::{Language, Text},
    theme::Palette,
};

const HISTORY_ROW_HEIGHT: f32 = 74.0;

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
    Finish(String),
    Abort,
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
    selected: Option<String>,
    diff: DiffView,
    mode: DiffMode,
    root: Option<String>,
}

impl NavigationUi {
    pub(super) fn branch_picker(
        &mut self,
        ui: &mut Ui,
        view: &NavigationView,
        writable: bool,
        l: Language,
        p: Palette,
    ) -> Option<NavigationIntent> {
        ui.set_min_width(280.0);
        c::search(
            ui,
            Id::unique("branch-filter"),
            l,
            p,
            &mut self.branch_query,
        );
        let query = self.branch_query.to_lowercase();
        ScrollArea::vertical().max_height(230.0).show(ui, |ui| {
            for (remote, label) in [
                (false, l.local("Local branches", "Branches locais")),
                (
                    true,
                    l.local("Remote-tracking references", "Referências remotas locais"),
                ),
            ] {
                ui.strong(label);
                for entry in view.refs.iter().filter(|item| {
                    item.remote == remote && item.label.to_lowercase().contains(&query)
                }) {
                    let caption = if entry.current {
                        format!("{} · HEAD", entry.label)
                    } else {
                        entry.label.clone()
                    };
                    if c::menu_choice(ui, &caption, self.branch.as_ref() == Some(&entry.name), p)
                        .clicked()
                    {
                        self.branch = Some(entry.name.clone());
                    }
                }
            }
        });
        let entry = view
            .refs
            .iter()
            .find(|item| self.branch.as_ref() == Some(&item.name));
        if let Some(entry) = entry
            && let Some(upstream) = &entry.upstream
        {
            ui.label(format!("Upstream · {upstream}"));
            if let Some((ahead, behind)) = entry.divergence {
                ui.label(format!("↑ {ahead}    ↓ {behind}"));
            }
        }
        let enabled = writable && !view.busy && view.merge == Merge::None;
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
            if ui
                .add_enabled(
                    view.root.is_some(),
                    Button::new(l.local("New branch from HEAD", "Nova branch a partir de HEAD")),
                )
                .clicked()
            {
                self.open(view, Form::Create(BranchBase::Head));
                ui.close();
            }
        });
        command
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
    ) -> Option<NavigationIntent> {
        if self.root != view.root {
            self.root.clone_from(&view.root);
            self.selected = None;
            self.pane = Pane::Revisions;
        }
        if view.root.is_none() {
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
        let mut command =
            (view.pagination == Pagination::Initial && !view.busy).then_some(Command::More);
        let compact = ui.available_width() < 690.0 || ui.available_height() < 290.0;
        if compact {
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
            command = chosen.or(command);
        } else {
            Panel::left("real-history-commits")
                .default_size(390.0)
                .size_range(235.0..=(ui.available_width() - 240.0))
                .resizable(true)
                .frame(c::panel(p.surface, 8))
                .show(ui, |ui| {
                    command = self.revisions(ui, view, l, p).or(command.take());
                });
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
            } else {
                self.pane = Pane::Details;
                command = self.details(ui, view, writable, l).or(command);
            }
        }
        command
            .filter(|_| !view.busy)
            .map(|command| intent(view, command))
    }

    fn revisions(
        &mut self,
        ui: &mut Ui,
        view: &NavigationView,
        l: Language,
        p: Palette,
    ) -> Option<Command> {
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
                        if self.revision_row(ui, view, entry, self.visible[index], l, p) {
                            action = Some(Command::Details(entry.revision.id.clone(), None));
                            self.selected = Some(entry.revision.id.clone());
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
            let (rect, _) =
                ui.allocate_exact_size(vec2(72.0, HISTORY_ROW_HEIGHT), egui::Sense::hover());
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
                let response = ui.add(
                    Button::selectable(self.selected.as_ref() == Some(&commit.id), summary)
                        .truncate(),
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
                if ui
                    .add_enabled(
                        writable && !view.busy && view.merge == Merge::None,
                        Button::new(
                            l.local("New branch from commit", "Nova branch a partir do commit"),
                        ),
                    )
                    .clicked()
                {
                    self.open(
                        view,
                        Form::Create(BranchBase::Commit(detail.revision.id.clone())),
                    );
                }
                ui.separator();
                ui.strong(l.local("Changed files", "Arquivos alterados"));
                ScrollArea::vertical()
                    .id_salt("commit-paths")
                    .max_height(195.0)
                    .show_rows(ui, 24.0, detail.files.len(), |ui, visible| {
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
                            if ui
                                .add(
                                    Button::new(format!("{title}  {stats}"))
                                        .truncate()
                                        .min_size(vec2(0.0, 24.0)),
                                )
                                .clicked()
                            {
                                action = Some(Command::Patch(
                                    detail.revision.id.clone(),
                                    detail.parent.clone(),
                                    file.path.clone(),
                                ));
                                self.pane = Pane::Diff;
                            }
                        }
                    });
                if view.busy {
                    c::loading(ui, l.local("Loading…", "Carregando…"), None);
                }
                action
            })
            .inner
    }

    fn patch(&mut self, ui: &mut Ui, view: &NavigationView, l: Language, p: Palette) {
        let Some(patch) = &view.patch else {
            ui.label(l.local(
                "Select a changed file in commit details",
                "Selecione um arquivo alterado nos detalhes do commit",
            ));
            return;
        };
        ui.strong(patch.key.path.to_string_lossy());
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
        let (version, form) = self.form.take()?;
        if version != view.version {
            return None;
        }
        let heading = match &form {
            Form::Create(_) => l.local("Create branch", "Criar branch"),
            Form::Rename(_) => l.local("Rename branch", "Renomear branch"),
            Form::Delete(_) => l.local("Delete integrated branch", "Excluir branch integrada"),
            Form::Merge(_) => l.local("Review merge", "Revisar merge"),
            Form::Finish(_) => l.local("Create merge commit", "Criar commit de merge"),
            Form::Abort => l.local("Confirm merge abort", "Confirmar aborto do merge"),
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
            };
            return Some(intent(view, command));
        }
        if !dialog.inner.1 && !dialog.should_close() {
            self.form = Some((version, form));
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
        }
    }
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
