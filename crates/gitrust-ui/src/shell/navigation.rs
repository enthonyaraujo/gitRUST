mod history;

use std::{path::PathBuf, sync::Arc};

use egui::{Button, Id, ScrollArea, TextEdit, Ui};
use gitrust_core::{
    changes::{CommitAuthor, CommitDraft, CommitSigning},
    navigation::{
        BranchBase, Details, HistoryRow, Merge, NavigationCommand as Command, NavigationIntent,
        NavigationNotice, NavigationView, Reference,
    },
    workspaces::Checkout,
};

use super::commands::Command as MenuCommand;

use crate::{
    components as c,
    diff::{DiffMode, DiffView},
    i18n::{Language, Text},
    theme::Palette,
};

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
    branch_context: Option<(egui::Id, egui::Pos2)>,
    choose_query: String,
    compare_waiting: bool,
    stash_waiting: bool,
    search: history::Search,
    show_details: bool,
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
        self.search = history::Search::Focus;
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
        let command = self.branch_rows(ui, view, enabled, l, p);
        ui.separator();
        if enabled
            && Self::menu_available(Some(view), MenuCommand::Merge)
            && ui
                .button(l.local("Choose a branch to merge…", "Escolher branch para merge…"))
                .clicked()
        {
            self.open_menu(view, MenuCommand::Merge);
            ui.close();
        }
        command
    }

    fn branch_rows(
        &mut self,
        ui: &mut Ui,
        view: &NavigationView,
        enabled: bool,
        l: Language,
        p: Palette,
    ) -> Option<NavigationIntent> {
        let query = self.branch_query.trim().to_lowercase();
        let mut switch = None;
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
                        let response =
                            c::branch_choice(ui, &entry.label, entry.current, entry.current, p);
                        if let Some(upstream) = &entry.upstream {
                            let tracking = entry.divergence.map_or_else(
                                || upstream.clone(),
                                |(ahead, behind)| format!("{upstream} · ↑ {ahead} ↓ {behind}"),
                            );
                            response.clone().on_hover_text(tracking);
                        }
                        if response.secondary_clicked() {
                            self.branch_context = response
                                .interact_pointer_pos()
                                .map(|pos| (response.id, pos));
                        }
                        let mut context_open =
                            self.branch_context.is_some_and(|(id, _)| id == response.id);
                        let menu = egui::Popup::menu(&response)
                            .anchor(
                                self.branch_context
                                    .map_or(response.rect.center(), |(_, pos)| pos),
                            )
                            .open_bool(&mut context_open)
                            .show(|ui| {
                                self.branch_context_actions(ui, view, entry, enabled, l);
                            });
                        if !context_open
                            && self.branch_context.is_some_and(|(id, _)| id == response.id)
                        {
                            self.branch_context = None;
                        }
                        if menu.is_some() {
                            continue;
                        }
                        if response.clicked() && enabled {
                            if entry.remote {
                                self.open(view, Form::Create(BranchBase::Ref(entry.clone())));
                                ui.close();
                            } else if !entry.current && !entry.occupied {
                                switch = Some(intent(view, Command::Switch(entry.clone())));
                                ui.close();
                            }
                        }
                    }
                }
                if !any {
                    ui.label(l.local("No matching branches", "Nenhuma branch encontrada"));
                }
            });
        switch
    }

    fn branch_context_actions(
        &mut self,
        ui: &mut Ui,
        view: &NavigationView,
        entry: &Reference,
        enabled: bool,
        l: Language,
    ) {
        if !entry.remote
            && ui
                .add_enabled(
                    enabled && (!entry.occupied || entry.current),
                    Button::new(l.local("Rename…", "Renomear…")),
                )
                .clicked()
        {
            self.open(view, Form::Rename(entry.clone()));
            ui.close();
        }
        if ui
            .button(l.local("Copy branch name", "Copiar nome da branch"))
            .clicked()
        {
            ui.ctx().copy_text(entry.label.clone());
            ui.close();
        }
        if !entry.remote
            && ui
                .add_enabled(
                    enabled && !entry.occupied && !entry.current,
                    Button::new(l.local("Delete…", "Excluir…")),
                )
                .clicked()
        {
            self.open(view, Form::Delete(entry.clone()));
            ui.close();
        }
        if ui
            .add_enabled(
                enabled,
                Button::new(l.local("Create branch from here…", "Criar branch a partir daqui…")),
            )
            .clicked()
        {
            self.open(view, Form::Create(BranchBase::Ref(entry.clone())));
            ui.close();
        }
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
