use std::{
    collections::{BTreeSet, HashSet},
    path::PathBuf,
    sync::Arc,
};

use egui::{Id, Panel, RichText, ScrollArea, TextEdit, Ui};
use gitrust_core::{
    changes::{
        ChangeKey, ChangeSide, ChangedPath, ChangesCommand, ChangesIntent, ChangesNotice,
        ChangesView, CommitDraft, CommitSigning, DiscardReview, PatchSelection,
    },
    workspaces::{Checkout, Workspace},
};

use crate::{
    components as c,
    diff::{Diff, DiffMode, DiffView},
    i18n::{Language, Text},
    theme::Palette,
};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Pane {
    #[default]
    Files,
    Diff,
    Commit,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Signing {
    #[default]
    None,
    OpenPgp,
    Ssh,
}

#[derive(Debug, Default)]
#[allow(clippy::struct_excessive_bools)] // Independent filter, dialog and submission states.
pub(super) struct ChangesUi {
    identity: Option<u64>,
    rows: Option<Arc<[ChangedPath]>>,
    signed: Signing,
    key: String,
    pane: Pane,
    query: String,
    query_dirty: bool,
    filtered: Vec<usize>,
    counts: [usize; 4],
    file_count: usize,
    focus: Option<Id>,
    reveal_filter: bool,
    selected: HashSet<ChangeKey>,
    current: Option<ChangeKey>,
    diff_mode: DiffMode,
    diff: DiffView,
    partial: BTreeSet<PatchSelection>,
    draft: CommitDraft,
    prefill: Option<Arc<CommitDraft>>,
    seen_version: Option<u64>,
    author_dialog: bool,
    amend_confirmation: Option<u64>,
    submitted: bool,
}

impl ChangesUi {
    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        workspace: &Workspace,
        view: &ChangesView,
        enabled: bool,
        layout: &mut super::repositories::WorkspaceLayout,
        clean: impl FnOnce(&mut Ui),
    ) -> Option<ChangesIntent> {
        let (l, p) = (layout.language, layout.palette);
        let action = self.synchronize(view);
        let enabled = enabled && !view.busy;
        let compact = super::repositories::single_pane(ui);
        let mut command = None;
        if compact {
            layout.tabs(ui, l, p);
            c::tabs(
                ui,
                &mut self.pane,
                &[
                    (Pane::Files, l.local("Files", "Arquivos")),
                    (Pane::Diff, l.local("Diff", "Diff")),
                    (Pane::Commit, l.local("Commit", "Commit")),
                ],
                p,
            );
            match self.pane {
                Pane::Files | Pane::Diff
                    if view.rows.is_empty() && (self.pane == Pane::Diff || !self.reveal_filter) =>
                {
                    if self.pane == Pane::Files {
                        self.files(ui, view, l, p);
                    }
                    c::panel(p.background, 8).show(ui, clean);
                }
                Pane::Files => {
                    command = ui
                        .add_enabled_ui(enabled, |ui| self.files(ui, view, l, p))
                        .inner;
                }
                Pane::Diff => {
                    command = ui
                        .add_enabled_ui(enabled, |ui| self.preview(ui, view, l, p))
                        .inner;
                }
                Pane::Commit => {
                    command = self.composer(ui, workspace, l, p, enabled);
                }
            }
        } else {
            layout.sidebar_width = Some(c::repository_sidebar(ui, p, layout.delta, |ui| {
                layout.tabs(ui, l, p);
                let target = (ui.available_height() * 0.49).clamp(124.0, 202.0);
                let max = (ui.available_height() - 47.0).clamp(105.0, 290.0);
                Panel::bottom("changes-composer")
                    .default_size(target.min(max))
                    .resizable(true)
                    .size_range(98.0..=max)
                    .frame(c::panel(p.surface, 4))
                    .show(ui, |ui| {
                        command = self.composer(ui, workspace, l, p, enabled);
                    });
                command = ui
                    .add_enabled_ui(enabled, |ui| self.files(ui, view, l, p))
                    .inner
                    .or(command.take());
            }));
            c::panel(p.background, 8).show(ui, |ui| {
                if view.rows.is_empty() {
                    clean(ui);
                } else {
                    command = ui
                        .add_enabled_ui(enabled, |ui| self.preview(ui, view, l, p))
                        .inner
                        .or(command.take());
                }
            });
        }
        if let Some(review) = &view.discard {
            command = discard_dialog(ui, review, enabled, l, p).or(command);
        }
        if self.author_dialog {
            self.show_author(ui, l, p);
        }
        if self.amend_confirmation == Some(view.version) {
            command = self.confirm_amend(ui, enabled, l, p).or(command);
        }
        let command = command.or(action.filter(|_| enabled))?;
        if matches!(command, ChangesCommand::Commit(_)) {
            self.submitted = true;
        }
        Some(ChangesIntent {
            version: view.version,
            command,
        })
    }

    pub(super) fn focus(&mut self, summary: bool) {
        self.pane = if summary { Pane::Commit } else { Pane::Files };
        self.reveal_filter |= !summary;
        self.focus = Some(Id::unique(if summary {
            "commit-summary"
        } else {
            "changes-search"
        }));
    }

    fn synchronize(&mut self, view: &ChangesView) -> Option<ChangesCommand> {
        let switched = self.identity != Some(view.identity);
        if switched {
            let mode = self.diff_mode;
            *self = Self {
                identity: Some(view.identity),
                diff_mode: mode,
                reveal_filter: self.reveal_filter && self.focus.is_some(),
                focus: self.focus.take(),
                ..Self::default()
            };
        }
        if self
            .rows
            .as_ref()
            .is_none_or(|old| !Arc::ptr_eq(old, &view.rows))
        {
            self.rows = Some(view.rows.clone());
            self.file_count = view
                .rows
                .iter()
                .map(|row| &row.key.path)
                .collect::<HashSet<_>>()
                .len();
            self.counts = [0; 4];
            for row in view.rows.iter() {
                self.counts[side_index(row.key.side)] += 1;
            }
            self.query_dirty = true;
            self.selected
                .retain(|key| view.rows.iter().any(|row| row.key == *key));
            self.current = self
                .current
                .take()
                .filter(|key| view.rows.iter().any(|row| row.key == *key));
        }
        if let Some(draft) = &view.draft
            && self
                .prefill
                .as_ref()
                .is_none_or(|old| !Arc::ptr_eq(old, draft))
        {
            self.prefill = Some(draft.clone());
            if draft.amend {
                self.draft.title.clone_from(&draft.title);
                self.draft.description.clone_from(&draft.description);
                self.draft.amend = true;
            }
            if self.draft.author.name.is_empty() {
                self.draft.author.name.clone_from(&draft.author.name);
            }
            if self.draft.author.email.is_empty() {
                self.draft.author.email.clone_from(&draft.author.email);
            }
        }
        if !view.busy && self.seen_version != Some(view.version) {
            self.seen_version = Some(view.version);
            self.amend_confirmation = None;
            self.partial.clear();
            if self.submitted && view.notice == Some(ChangesNotice::Committed) {
                self.submitted = false;
                self.draft.title.clear();
                self.draft.description.clear();
                self.draft.amend = false;
                self.signed = Signing::None;
                self.key.clear();
            }
            return if switched {
                Some(ChangesCommand::LoadAuthor)
            } else {
                self.current.clone().map(ChangesCommand::Preview)
            };
        }
        None
    }

    fn files(
        &mut self,
        ui: &mut Ui,
        view: &ChangesView,
        l: Language,
        p: Palette,
    ) -> Option<ChangesCommand> {
        let file_count = self.file_count;
        let count = if file_count == 1 {
            l.local("1 changed file", "1 arquivo alterado").to_owned()
        } else {
            format!(
                "{file_count} {}",
                l.local("changed files", "arquivos alterados")
            )
        };
        if view.rows.is_empty() && !self.reveal_filter {
            c::panel(p.background, 4).show(ui, |ui| {
                ui.vertical_centered_justified(|ui| {
                    ui.colored_label(p.muted, count);
                });
            });
            return None;
        }
        let compact = ui.available_height() < 175.0 || ui.available_width() < 245.0;
        let search = Id::unique("changes-search");
        if self.focus == Some(search) {
            ui.memory_mut(|memory| memory.request_focus(search));
            self.focus = None;
        }
        if c::search(ui, search, l, p, &mut self.query).changed() {
            self.query_dirty = true;
        }
        if std::mem::take(&mut self.query_dirty) {
            self.filter(&view.rows);
        }
        let mut action = None;
        ui.horizontal_wrapped(|ui| {
            self.select_filtered(ui, view, &count, l);
            if compact && !view.rows.is_empty() {
                action = ui
                    .menu_button(l.local("Index actions", "Ações do index"), |ui| {
                        let action = self.file_actions(ui, view, l);
                        if action.is_some() {
                            ui.close();
                        }
                        action
                    })
                    .inner
                    .flatten();
            }
        });
        if !compact {
            self.count_badges(ui, l, p);
            action = self.file_actions(ui, view, l);
        }
        ui.separator();
        if self.filtered.is_empty() {
            ui.label(
                RichText::new(if view.rows.is_empty() {
                    l.local("No local changes", "Sem alterações locais")
                } else {
                    l.local("No matching changes", "Nenhuma alteração encontrada")
                })
                .color(p.muted),
            );
        }
        ScrollArea::vertical()
            .id_salt("changed-paths")
            .auto_shrink([false, false])
            .show_rows(ui, 24.0, self.filtered.len(), |ui, visible| {
                for index in visible {
                    let row = &view.rows[self.filtered[index]];
                    ui.push_id(&row.key, |ui| {
                        if self.file_row(ui, row, l, p) {
                            action = Some(ChangesCommand::Preview(row.key.clone()));
                        }
                    });
                }
            });
        action
    }

    fn select_filtered(&mut self, ui: &mut Ui, view: &ChangesView, label: &str, l: Language) {
        let mut all = !self.filtered.is_empty()
            && self
                .filtered
                .iter()
                .all(|&index| self.selected.contains(&view.rows[index].key));
        if ui
            .add_enabled(
                !self.filtered.is_empty(),
                egui::Checkbox::new(&mut all, label),
            )
            .on_hover_text(l.local("Select filtered files", "Selecionar arquivos filtrados"))
            .changed()
        {
            for &index in &self.filtered {
                let key = &view.rows[index].key;
                if all {
                    self.selected.insert(key.clone());
                } else {
                    self.selected.remove(key);
                }
            }
        }
    }

    fn count_badges(&self, ui: &mut Ui, l: Language, p: Palette) {
        ui.horizontal_wrapped(|ui| {
            for (side, count) in [
                (ChangeSide::Staged, self.counts[0]),
                (ChangeSide::Unstaged, self.counts[1]),
                (ChangeSide::Untracked, self.counts[2]),
                (ChangeSide::Conflict, self.counts[3]),
            ] {
                if count != 0 {
                    c::badge(
                        ui,
                        &format!("{} {count}", side_name(side, l)),
                        p.text,
                        p.elevated,
                    );
                }
            }
        });
    }

    fn filter(&mut self, rows: &[ChangedPath]) {
        let query = self.query.trim().to_lowercase();
        self.filtered =
            rows.iter()
                .enumerate()
                .filter(|(_, row)| {
                    query.is_empty()
                        || row
                            .key
                            .path
                            .to_string_lossy()
                            .to_lowercase()
                            .contains(&query)
                        || row.from.as_ref().is_some_and(|old| {
                            old.to_string_lossy().to_lowercase().contains(&query)
                        })
                })
                .map(|(index, _)| index)
                .collect();
    }

    fn file_row(&mut self, ui: &mut Ui, row: &ChangedPath, l: Language, p: Palette) -> bool {
        let path = row.key.path.to_string_lossy();
        ui.horizontal(|ui| {
            ui.set_min_height(24.0);
            let mut selected = self.selected.contains(&row.key);
            let response = ui.checkbox(&mut selected, "");
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::Role::CheckBox,
                    ui.is_enabled(),
                    selected,
                    format!("{} {path}", l.local("Select", "Selecionar")),
                )
            });
            if response.changed() {
                if selected {
                    self.selected.insert(row.key.clone());
                } else {
                    self.selected.remove(&row.key);
                }
            }
            ui.label(
                RichText::new(row.status.to_string())
                    .monospace()
                    .color(p.accent),
            );
            let layer = side_name(row.key.side, l);
            let caption = format!("{layer} · {path}");
            let response = ui
                .add(
                    egui::Button::selectable(self.current.as_ref() == Some(&row.key), &caption)
                        .truncate(),
                )
                .on_hover_text(if let Some(old) = &row.from {
                    format!("{layer} · {} → {path}", old.to_string_lossy())
                } else {
                    caption
                });
            if response.clicked() {
                self.current = Some(row.key.clone());
                self.partial.clear();
                self.pane = Pane::Diff;
                return true;
            }
            false
        })
        .inner
    }

    fn file_actions(
        &mut self,
        ui: &mut Ui,
        view: &ChangesView,
        l: Language,
    ) -> Option<ChangesCommand> {
        let eligible_stage = |key: &&ChangeKey| {
            matches!(
                key.side,
                ChangeSide::Unstaged | ChangeSide::Untracked | ChangeSide::Conflict
            )
        };
        let eligible_unstage = |key: &&ChangeKey| key.side == ChangeSide::Staged;
        let selected = || self.selected.iter();
        ui.horizontal_wrapped(|ui| {
            for (label, enabled, stage, all) in [
                (
                    l.local("Stage", "Preparar"),
                    selected().any(|key| eligible_stage(&key)),
                    true,
                    false,
                ),
                (
                    l.local("Unstage", "Retirar"),
                    selected().any(|key| eligible_unstage(&key)),
                    false,
                    false,
                ),
                (
                    l.local("Stage all", "Preparar todos"),
                    view.rows.iter().any(|row| {
                        eligible_stage(&&row.key) && row.key.side != ChangeSide::Conflict
                    }),
                    true,
                    true,
                ),
                (
                    l.local("Unstage all", "Retirar todos"),
                    self.counts[0] != 0,
                    false,
                    true,
                ),
            ] {
                if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                    let keys = view
                        .rows
                        .iter()
                        .map(|row| &row.key)
                        .filter(|key| {
                            (all || self.selected.contains(key))
                                && (if stage {
                                    eligible_stage(key)
                                } else {
                                    eligible_unstage(key)
                                })
                                && (!all || key.side != ChangeSide::Conflict)
                        })
                        .cloned()
                        .collect();
                    return Some(if stage {
                        ChangesCommand::Stage(keys)
                    } else {
                        ChangesCommand::Unstage(keys)
                    });
                }
            }
            if ui
                .add_enabled(
                    selected().any(|key| key.side != ChangeSide::Conflict),
                    egui::Button::new(l.local("Review discard", "Revisar descarte")),
                )
                .clicked()
            {
                return Some(ChangesCommand::ReviewDiscard(
                    view.rows
                        .iter()
                        .map(|row| &row.key)
                        .filter(|key| {
                            key.side != ChangeSide::Conflict && self.selected.contains(key)
                        })
                        .cloned()
                        .collect(),
                ));
            }
            None
        })
        .inner
    }

    fn preview(
        &mut self,
        ui: &mut Ui,
        view: &ChangesView,
        l: Language,
        p: Palette,
    ) -> Option<ChangesCommand> {
        let Some(patch) = &view.preview else {
            ui.label(
                RichText::new(l.local(
                    "Select a file to inspect its changes.",
                    "Selecione um arquivo para inspecionar as alterações.",
                ))
                .color(p.muted),
            );
            return None;
        };
        ui.label(RichText::new(patch.key.path.to_string_lossy()).strong());
        let side = patch.key.side;
        ui.colored_label(p.muted, side_description(side, l));
        if patch.binary {
            ui.colored_label(p.warning, l.text(Text::Binary));
        }
        let action = if patch.partial {
            let label = if side == ChangeSide::Staged {
                l.local("Unstage selection", "Retirar seleção")
            } else {
                l.local("Stage selection", "Preparar seleção")
            };
            ui.add_enabled(!self.partial.is_empty(), egui::Button::new(label))
                .clicked()
                .then(|| {
                    ChangesCommand::Partial(
                        patch.key.clone(),
                        self.partial.iter().copied().collect(),
                    )
                })
        } else {
            ui.colored_label(
                p.muted,
                l.local(
                    "Partial staging is unavailable for this content, mode or path change.",
                    "A seleção parcial está indisponível para este conteúdo ou alteração de modo ou caminho.",
                ),
            );
            None
        };
        self.diff.show_with_selection(
            ui,
            Id::unique(("real-change", &patch.key)),
            Diff::Real(patch),
            &mut self.diff_mode,
            l,
            p,
            Some(&mut self.partial),
        );
        action
    }

    fn composer(
        &mut self,
        ui: &mut Ui,
        workspace: &Workspace,
        l: Language,
        p: Palette,
        enabled: bool,
    ) -> Option<ChangesCommand> {
        let branch = match &workspace.checkout {
            Checkout::Branch(branch) | Checkout::Unborn(branch) => branch.as_str(),
            Checkout::Detached(_) => l.local("Detached HEAD", "HEAD destacado"),
        };
        let staged = workspace.changes.map_or(0, |counts| counts.staged);
        let valid = enabled
            && !self.draft.title.trim().is_empty()
            && (staged > 0 || self.draft.amend)
            && !matches!(workspace.checkout, Checkout::Detached(_))
            && workspace
                .changes
                .is_none_or(|counts| counts.conflicted == 0);
        let label = if self.draft.amend {
            l.local("Review amend", "Revisar emenda").to_owned()
        } else {
            format!("{} {branch}", l.local("Commit to", "Commit em"))
        };
        let commit = Panel::bottom("commit-submission")
            .exact_size(31.0)
            .frame(c::panel(p.surface, 1))
            .show(ui, |ui| {
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                    ui.add_enabled_ui(valid, |ui| c::primary(ui, &label, p))
                        .inner
                        .on_disabled_hover_text(l.local(
                            "Enter a summary and stage the files to commit.",
                            "Preencha o resumo e prepare os arquivos para o commit.",
                        ))
                        .clicked()
                })
                .inner
            })
            .inner;
        let form_action = ScrollArea::vertical()
            .id_salt("commit-fields")
            .show(ui, |ui| {
                ui.add_enabled_ui(enabled, |ui| self.composer_fields(ui, workspace, l, p))
                    .inner
            })
            .inner;
        if commit {
            if self.draft.amend {
                self.amend_confirmation = self.seen_version;
            } else {
                return Some(ChangesCommand::Commit(self.make_draft()));
            }
        }
        form_action
    }

    fn composer_fields(
        &mut self,
        ui: &mut Ui,
        workspace: &Workspace,
        l: Language,
        p: Palette,
    ) -> Option<ChangesCommand> {
        let id = Id::unique("commit-summary");
        if self.focus == Some(id) {
            self.focus = None;
            ui.memory_mut(|memory| memory.request_focus(id));
        }
        c::text_field(
            ui,
            id,
            l.local("Summary (required)", "Resumo (obrigatório)"),
            &mut self.draft.title,
        );
        c::text_area(
            ui,
            Id::unique("commit-description"),
            l.local("Description", "Descrição"),
            &mut self.draft.description,
        );
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(l.local("Author and signing", "Autoria e assinatura"))
                .clicked()
            {
                self.author_dialog = true;
            }
            if matches!(workspace.checkout, Checkout::Branch(_)) {
                if self.draft.amend {
                    if ui
                        .button(l.local("Leave amend", "Sair da emenda"))
                        .clicked()
                    {
                        self.draft.amend = false;
                    }
                } else if ui
                    .button(l.local("Load last commit", "Carregar último commit"))
                    .clicked()
                {
                    return Some(ChangesCommand::LoadAmend);
                }
            }
            let signer = match self.signed {
                Signing::None => None,
                Signing::OpenPgp => Some("GPG"),
                Signing::Ssh => Some("SSH"),
            };
            if let Some(signer) = signer {
                ui.colored_label(
                    p.accent,
                    format!("{} {signer}", l.local("Signing with", "Assinatura com")),
                );
            }
            None
        })
        .inner
    }

    fn make_draft(&self) -> CommitDraft {
        let mut draft = self.draft.clone();
        draft.signing = match self.signed {
            Signing::None => CommitSigning::None,
            Signing::OpenPgp => CommitSigning::OpenPgp(self.key.trim().to_owned()),
            Signing::Ssh => CommitSigning::Ssh(PathBuf::from(self.key.trim())),
        };
        draft
    }

    fn show_author(&mut self, ui: &Ui, l: Language, p: Palette) {
        let modal = c::modal(
            ui,
            "commit-author",
            p,
            l.local("Author and signing", "Autoria e assinatura"),
            |ui| {
                text_field(
                    ui,
                    l.local("Name", "Nome"),
                    &mut self.draft.author.name,
                    false,
                );
                text_field(
                    ui,
                    l.local("Email", "E-mail"),
                    &mut self.draft.author.email,
                    false,
                );
                ui.label(l.local("Identity applies only to this commit. Amend preserves the original author.", "A identidade vale apenas para este commit. A emenda preserva a autoria original."));
                c::tabs(
                    ui,
                    &mut self.signed,
                    &[
                        (Signing::None, l.local("Unsigned", "Sem assinatura")),
                        (Signing::OpenPgp, "GPG"),
                        (Signing::Ssh, "SSH"),
                    ],
                    p,
                );
                match self.signed {
                    Signing::None => {}
                    Signing::OpenPgp => text_field(
                        ui,
                        l.local("Key fingerprint", "Impressão digital da chave"),
                        &mut self.key,
                        false,
                    ),
                    Signing::Ssh => text_field(
                        ui,
                        l.local("Absolute key path", "Caminho absoluto da chave"),
                        &mut self.key,
                        false,
                    ),
                }
                c::primary(ui, l.local("Done", "Concluir"), p).clicked()
            },
        );
        if modal.inner || modal.should_close() {
            self.author_dialog = false;
        }
    }

    fn confirm_amend(
        &mut self,
        ui: &Ui,
        enabled: bool,
        l: Language,
        p: Palette,
    ) -> Option<ChangesCommand> {
        let modal = c::modal(
            ui,
            "confirm-rewrite",
            p,
            l.local("Rewrite last commit?", "Reescrever último commit?"),
            |ui| {
                ui.label(l.local(
                    "This replaces the current branch tip with a new commit ID.",
                    "Isso substitui a ponta da branch atual por um novo ID de commit.",
                ));
                ui.horizontal_wrapped(|ui| {
                    let amend = ui
                        .add_enabled(
                            enabled,
                            egui::Button::new(l.local("Rewrite commit", "Reescrever commit")),
                        )
                        .clicked();
                    let cancel = ui.button(l.text(Text::Cancel)).clicked();
                    (amend, cancel)
                })
                .inner
            },
        );
        if modal.inner.0 || modal.inner.1 || modal.should_close() {
            self.amend_confirmation = None;
        }
        modal
            .inner
            .0
            .then(|| ChangesCommand::Commit(self.make_draft()))
    }
}

fn text_field(ui: &mut Ui, label: &str, value: &mut String, multiline: bool) {
    let label = ui.label(label);
    let editor = if multiline {
        TextEdit::multiline(value).desired_rows(2)
    } else {
        TextEdit::singleline(value)
    };
    ui.add(editor.desired_width(f32::INFINITY))
        .labelled_by(label.id);
}

fn discard_dialog(
    ui: &Ui,
    review: &DiscardReview,
    enabled: bool,
    l: Language,
    p: Palette,
) -> Option<ChangesCommand> {
    let contains_untracked = review
        .files
        .iter()
        .any(|file| file.key.side == ChangeSide::Untracked);
    let modal = c::modal(
        ui,
        "discard-real-changes",
        p,
        l.local("Review permanent discard", "Revisar descarte permanente"),
        |ui| {
            ui.colored_label(
                p.error,
                l.local(
                    "Git cannot recover discarded working-tree content.",
                    "O Git não consegue recuperar o conteúdo descartado da árvore de trabalho.",
                ),
            );
            ScrollArea::vertical()
                .max_height((ui.ctx().content_rect().height() - 185.0).clamp(55.0, 270.0))
                .show(ui, |ui| {
                    for file in &review.files {
                        ui.collapsing(file.key.path.to_string_lossy(), |ui| {
                            if file.binary {
                                ui.label(l.text(Text::Binary));
                            }
                            ui.add(
                                egui::Label::new(RichText::new(&file.text).monospace())
                                    .wrap()
                                    .selectable(true),
                            );
                        });
                        let version = match file.key.side {
                            ChangeSide::Staged => l.local(
                                "Restore index and working tree from HEAD.",
                                "Restaura o index e a árvore de trabalho a partir do HEAD.",
                            ),
                            ChangeSide::Unstaged => l.local(
                                "Restore working tree from the index.",
                                "Restaura a árvore de trabalho a partir do index.",
                            ),
                            ChangeSide::Untracked => l.local(
                                "Permanently delete the untracked file.",
                                "Exclui permanentemente o arquivo não rastreado.",
                            ),
                            ChangeSide::Conflict => "",
                        };
                        ui.colored_label(p.muted, version);
                    }
                });
            let label = if contains_untracked {
                l.local(
                    "Discard and delete untracked files",
                    "Descartar e excluir arquivos não rastreados",
                )
            } else {
                l.local("Discard permanently", "Descartar permanentemente")
            };
            ui.horizontal_wrapped(|ui| {
                (
                    ui.add_enabled(enabled, egui::Button::new(label)).clicked(),
                    ui.button(l.text(Text::Cancel)).clicked(),
                )
            })
            .inner
        },
    );
    if modal.inner.0 {
        Some(ChangesCommand::Discard(review.id))
    } else if modal.inner.1 || modal.should_close() {
        Some(ChangesCommand::DismissDiscard)
    } else {
        None
    }
}

pub(super) fn can_save_or_discard(view: &ChangesView) -> bool {
    !view.rows.is_empty()
        && view
            .rows
            .iter()
            .all(|row| row.key.side != ChangeSide::Conflict)
}

pub(super) fn notice(view: &ChangesView, l: Language) -> Option<(bool, &str)> {
    Some(match view.notice? {
        ChangesNotice::Updated => (true, l.local("Changes updated.", "Alterações atualizadas.")),
        ChangesNotice::Committed => (true, l.local("Commit created.", "Commit criado.")),
        ChangesNotice::Stale => (false, l.local("Repository changed. Review the refreshed changes and retry.", "O repositório mudou. Revise as alterações atualizadas e tente novamente.")),
        ChangesNotice::AuthorRequired => (false, l.local("Enter a valid name and email under Author and signing.", "Informe nome e e-mail válidos em Autoria e assinatura.")),
        ChangesNotice::SigningFailed => (false, l.local("Signing failed or was required. Verify the authorized signer and key.", "A assinatura falhou ou era obrigatória. Verifique o assinador autorizado e a chave.")),
        ChangesNotice::Unsupported => (false, l.local("Git rejected this operation for the selected state or message. Review the selection.", "O Git rejeitou a operação para o estado ou mensagem selecionados. Revise a seleção.")),
        ChangesNotice::Failed => (false, l.local("Operation interrupted or failed. Check the current files and index before retrying.", "Operação interrompida ou malsucedida. Confira os arquivos e o index atuais antes de tentar novamente.")),
    })
}

const fn side_index(side: ChangeSide) -> usize {
    match side {
        ChangeSide::Staged => 0,
        ChangeSide::Unstaged => 1,
        ChangeSide::Untracked => 2,
        ChangeSide::Conflict => 3,
    }
}

fn side_name(side: ChangeSide, l: Language) -> &'static str {
    match side {
        ChangeSide::Staged => l.local("Staged", "No index"),
        ChangeSide::Unstaged => l.local("Unstaged", "Fora do index"),
        ChangeSide::Untracked => l.local("Untracked", "Não rastreado"),
        ChangeSide::Conflict => l.local("Conflict", "Conflito"),
    }
}

fn side_description(side: ChangeSide, l: Language) -> &'static str {
    match side {
        ChangeSide::Staged => l.local("Staged · HEAD → index", "No index · HEAD → index"),
        ChangeSide::Unstaged => l.local(
            "Unstaged · index → working tree",
            "Fora do index · index → árvore de trabalho",
        ),
        ChangeSide::Untracked => l.local(
            "Untracked · new working-tree file",
            "Não rastreado · arquivo novo na árvore de trabalho",
        ),
        ChangeSide::Conflict => l.local("Conflict · combined diff", "Conflito · diff combinado"),
    }
}
