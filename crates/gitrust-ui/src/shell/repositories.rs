mod landing;

use std::path::{Component, Path, PathBuf};

use egui::{Button, Id, RichText, ScrollArea, ThemePreference, Ui};
use gitrust_core::github::RepositoryKey;
use gitrust_core::remotes::{Auth, Credential, Remote, RemoteAction, RemoteIntent};
use gitrust_core::workspaces::{
    Activity, Appearance, Checkout, DesktopTool, Locale, Preferences, ProvisionKind,
    RepositoryAction, RepositoryView, TransferPhase, WorkspaceIssue,
};

use super::{
    Desktop, UiOptions, commands::Command, navigation::Tab, remotes::Dialog as RemoteDialog,
};
use crate::{
    components as c,
    i18n::{Language, Text},
    theme::{Palette, weight},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FormKind {
    Open,
    Create,
    Clone,
}

struct Form {
    kind: FormKind,
    path: String,
    chosen: Option<PathBuf>,
    name: String,
    suggested: String,
    branch: String,
    url: String,
    auth: super::remotes::AuthFields,
}

impl std::fmt::Debug for Form {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Form")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

impl Form {
    fn new(kind: FormKind) -> Self {
        Self {
            kind,
            path: String::new(),
            chosen: None,
            name: String::new(),
            suggested: String::new(),
            branch: "main".to_owned(),
            url: String::new(),
            auth: super::remotes::AuthFields::default(),
        }
    }

    fn action(&self) -> Option<RepositoryAction> {
        if self.path.is_empty() {
            return None;
        }
        let path = self
            .chosen
            .clone()
            .unwrap_or_else(|| PathBuf::from(&self.path));
        match self.kind {
            FormKind::Open => Some(RepositoryAction::Open(path)),
            FormKind::Create | FormKind::Clone => {
                let name = Path::new(&self.name);
                if !matches!(name.components().next(), Some(Component::Normal(_)))
                    || name.components().count() != 1
                    || name.file_name().and_then(|value| value.to_str()) != Some(self.name.as_str())
                {
                    return None;
                }
                let destination = path.join(name);
                if self.kind == FormKind::Create {
                    (!self.branch.is_empty()).then(|| RepositoryAction::Create {
                        destination,
                        branch: self.branch.clone(),
                    })
                } else {
                    (!self.url.is_empty()).then(|| RepositoryAction::Clone {
                        destination,
                        url: self.url.clone(),
                    })
                }
            }
        }
    }

    fn suggest(&mut self) {
        if !self.name.is_empty() && self.name != self.suggested {
            return;
        }
        let url = self.url.trim_end_matches('/');
        let name = url.rsplit(['/', ':']).next().unwrap_or("");
        let name = name.strip_suffix(".git").unwrap_or(name);
        if name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        {
            self.name = name.to_owned();
            self.suggested.clone_from(&self.name);
        }
    }
}

pub(super) struct WorkspaceLayout {
    pub tab: Tab,
    pub has_changes: bool,
    pub readable: bool,
    pub sidebar_width: Option<f32>,
    pub delta: f32,
    pub language: Language,
    pub palette: Palette,
}

impl WorkspaceLayout {
    pub(super) fn tabs(&mut self, ui: &mut Ui, l: Language, p: Palette) {
        let tabs = [
            (Tab::Changes, l.text(Text::Changes)),
            (Tab::History, l.text(Text::History)),
        ];
        c::full_width_tabs(
            ui,
            &mut self.tab,
            if self.has_changes { &tabs } else { &tabs[1..] },
            p,
        );
        ui.separator();
    }
}

pub(super) fn single_pane(ui: &Ui) -> bool {
    ui.available_width() < 500.0 || ui.available_height() < 223.0
}

#[derive(Debug, Default)]
pub(super) struct ManagerUi {
    pub(super) view: RepositoryView,
    pub(super) sidebar_width: Option<f32>,
    pub(super) fetch: super::fetch::FetchState,
    query: String,
    changes: super::changes::ChangesUi,
    pub(super) navigation: super::navigation::NavigationUi,
    pub(super) remotes: super::remotes::RemoteUi,
    pub(super) github: super::github::GithubUi,
    form: Option<Form>,
    forget: Option<PathBuf>,
    action: Option<RepositoryAction>,
}

impl ManagerUi {
    #[cfg(test)]
    pub(super) fn is_clone_form(&self) -> bool {
        self.form
            .as_ref()
            .is_some_and(|form| form.kind == FormKind::Clone)
    }

    pub(super) fn open_form(&mut self, kind: FormKind) {
        self.form = Some(Form::new(kind));
    }

    pub(super) fn send(&mut self, message: RepositoryAction) {
        self.action = Some(message);
    }

    pub(super) fn usable(&self) -> bool {
        self.view.git_available
            && self.view.remotes.as_ref().is_none_or(|view| !view.busy)
            && matches!(self.view.activity, Activity::Idle | Activity::Inspecting)
            && self
                .view
                .changes
                .as_ref()
                .is_none_or(|changes| !changes.busy)
    }

    pub(super) fn idle(&self) -> bool {
        self.usable() && self.view.activity == Activity::Idle
    }

    pub(super) fn title(&self, l: Language) -> String {
        self.view.active.as_ref().map_or_else(
            || l.text(Text::NoRepositoryShort).to_owned(),
            |workspace| filename(&workspace.path),
        )
    }

    pub(super) fn popup(&mut self, ui: &mut Ui, p: Palette, l: Language) {
        ui.set_width((ui.ctx().content_rect().width() - 24.0).clamp(160.0, 310.0));
        c::search(ui, Id::unique("repository-search"), l, p, &mut self.query);
        ui.separator();
        let search = self.query.trim().to_lowercase();
        self.recents(
            ui,
            &search,
            p,
            l,
            (ui.ctx().content_rect().height() * 0.42).clamp(50.0, 220.0),
        );
        ui.separator();
        self.form_buttons(ui, l);
    }

    pub(super) fn workspace(
        &mut self,
        ui: &mut Ui,
        p: Palette,
        l: Language,
        delta: f32,
    ) -> Option<Command> {
        let mut command = None;
        if self.real_changes(ui, p, l, delta, &mut command) {
            return command;
        }
        ScrollArea::vertical()
            .id_salt("repository-workspace")
            .show(ui, |ui| {
                if let Some(issue) = self.view.issue {
                    c::notice(
                        ui,
                        l.local("Repository management", "Gerenciamento de repositórios"),
                        issue_text(l, issue),
                        if issue == WorkspaceIssue::Cancelled {
                            p.muted
                        } else {
                            p.error
                        },
                        p,
                    );
                }
                if self.view.storage_issue {
                    ui.colored_label(
                        p.warning,
                        l.local(
                            "Local preferences or recents could not be loaded or saved.",
                            "Não foi possível ler ou salvar preferências ou recentes.",
                        ),
                    );
                }
                if Self::activity(ui, &self.view, l) {
                    self.send(RepositoryAction::Cancel);
                }
                if let Some(workspace) = self.view.active.clone() {
                    if self.view.activity != Activity::Idle {
                        ui.separator();
                    }
                    self.summary(ui, p, l, &workspace);
                } else {
                    self.empty_landing(ui, p, l);
                }
            });
        None
    }

    pub(super) fn remote_dialog(&self, action: Command) -> Option<RemoteDialog> {
        let view = self.view.remotes.as_ref()?;
        let active = self.view.active.as_ref()?;
        if !self.usable() || self.view.activity != Activity::Idle {
            return None;
        }
        let remote = self.remotes.selected(view)?;
        let tracked = view
            .upstream
            .as_ref()
            .is_some_and(|(name, _)| name == &remote.name);
        match action {
            Command::Pull
                if active.changes.is_some()
                    && !view.rebase.in_progress()
                    && tracked
                    && remote.fetch.is_some() =>
            {
                Some(RemoteDialog::Pull(remote.clone()))
            }
            Command::Push
                if active.changes.is_some()
                    && !view.rebase.in_progress()
                    && view.current.is_some()
                    && remote.push.is_some() =>
            {
                Some(RemoteDialog::Push(remote.clone()))
            }
            _ => None,
        }
    }

    pub(super) fn fetch_target(&self) -> Option<&Remote> {
        let view = self.view.remotes.as_ref()?;
        view.entries
            .iter()
            .find(|remote| remote.name == "origin")
            .or_else(|| self.remotes.selected(view))
    }

    pub(super) fn can_fetch(&self) -> bool {
        !matches!(self.fetch, super::fetch::FetchState::Fetching { .. })
            && self.idle()
            && self.view.active.is_some()
            && !self.view.github.busy
            && self
                .fetch_target()
                .is_some_and(|remote| remote.fetch.is_some())
    }

    pub(super) fn fetch_intent(&self) -> Option<RemoteIntent> {
        if !self.can_fetch() {
            return None;
        }
        let view = self.view.remotes.as_ref()?;
        let remote = self.fetch_target()?;
        let endpoint = remote.fetch.as_deref()?;
        let github = &self.view.github;
        let linked = github.workspace_account.is_some()
            && RepositoryKey::from_https(endpoint)
                .as_ref()
                .is_some_and(|repo| Some(repo) == github.workspace_repo.as_ref());
        Some(RemoteIntent {
            version: view.version,
            action: RemoteAction::Fetch(
                vec![remote.name.clone()],
                Auth {
                    credential: if linked {
                        Credential::LinkedAccount
                    } else {
                        Credential::Anonymous
                    },
                    ..Auth::default()
                },
            ),
        })
    }

    pub(super) fn can_publish(&self) -> bool {
        self.view.remotes.is_some()
            && self.view.github.workspace.is_some()
            && !self.view.github.busy
            && self
                .view
                .active
                .as_ref()
                .is_some_and(|active| active.changes.is_some())
    }

    pub(super) fn focus(&mut self, action: Command) {
        if (action, self.navigation.tab) == (Command::Find, Tab::History) {
            self.navigation.focus_search();
        } else {
            self.navigation.tab = Tab::Changes;
            self.changes.focus(action == Command::Summary);
        }
    }

    pub(super) fn confirm_forget(&mut self) {
        self.forget = self.view.active.as_ref().map(|active| active.path.clone());
    }

    pub(super) fn discard_all(&mut self) {
        if let Some(view) = &self.view.changes {
            use gitrust_core::changes::{ChangeSide, ChangesCommand, ChangesIntent};
            let keys: Vec<_> = view
                .rows
                .iter()
                .filter(|row| row.key.side != ChangeSide::Conflict)
                .map(|row| row.key.clone())
                .collect();
            if !keys.is_empty() {
                self.navigation.tab = Tab::Changes;
                self.send(RepositoryAction::Changes(ChangesIntent {
                    version: view.version,
                    command: ChangesCommand::ReviewDiscard(keys),
                }));
            }
        }
    }

    pub(super) fn open_desktop_target(&mut self, action: Command) {
        if let Some(workspace) = &self.view.active {
            let path = workspace.path.clone();
            let action = match action {
                Command::Terminal => RepositoryAction::OpenDesktopTool {
                    tool: DesktopTool::Terminal,
                    path,
                },
                Command::Editor => RepositoryAction::OpenDesktopTool {
                    tool: DesktopTool::Editor,
                    path,
                },
                _ => RepositoryAction::Reveal(path),
            };
            self.send(action);
        }
    }

    pub(super) fn remote_popup(&mut self, ui: &mut Ui, l: Language, p: Palette) {
        if let Some(view) = &self.view.remotes {
            self.remotes
                .popup(ui, view, self.view.changes.is_some(), l, p);
        } else {
            ui.label(l.local("Loading remotes…", "Carregando remotes…"));
        }
    }

    pub(super) fn branch_popup(&mut self, ui: &mut Ui, l: Language, p: Palette) -> bool {
        if let Some(view) = self.view.navigation.as_ref() {
            if let Some(action) = self.navigation.branch_picker(
                ui,
                view,
                self.view.changes.is_some() && self.idle(),
                l,
                p,
            ) {
                self.action = Some(RepositoryAction::Navigate(action));
            }
        } else {
            ui.label(l.local("Loading local branches…", "Carregando branches locais…"));
        }
        let github = &self.view.github;
        if github.workspace_repo.is_some()
            && ui
                .add_enabled(
                    github.active.is_some() && !github.busy,
                    Button::new(l.local("View pull requests", "Ver pull requests")),
                )
                .clicked()
        {
            self.github.workspace_pulls(github);
            ui.close();
            return true;
        }
        false
    }

    fn real_changes(
        &mut self,
        ui: &mut Ui,
        p: Palette,
        l: Language,
        delta: f32,
        command: &mut Option<Command>,
    ) -> bool {
        if self.view.active.is_none() {
            return false;
        }
        let changes = self.view.changes.as_ref();
        let navigation = self.view.navigation.as_ref();
        if changes.is_none() && navigation.is_none() {
            return false;
        }
        if changes.is_none() && self.navigation.tab == Tab::Changes {
            self.navigation.tab = Tab::History;
        }
        if changes.is_some_and(|changes| changes.busy) || navigation.is_some_and(|view| view.busy) {
            ui.horizontal_wrapped(|ui| {
                c::loading(ui, l.local("Working…", "Processando…"), None);
                if ui.button(l.text(Text::Cancel)).clicked() {
                    self.action = Some(RepositoryAction::Cancel);
                }
            });
        } else if self.navigation.tab == Tab::Changes
            && let Some((success, message)) =
                changes.and_then(|changes| super::changes::notice(changes, l))
        {
            ui.colored_label(if success { p.success } else { p.warning }, message);
        }
        self.workspace_operations(ui, p, l);
        let Some(workspace) = self.view.active.as_ref() else {
            return false;
        };
        let changes = self.view.changes.as_ref();
        let navigation = self.view.navigation.as_ref();
        let priority = self.action.take();
        let mut layout = WorkspaceLayout {
            tab: self.navigation.tab,
            has_changes: changes.is_some(),
            readable: self.idle() && priority.is_none(),
            sidebar_width: None,
            delta,
            language: l,
            palette: p,
        };
        match self.navigation.tab {
            Tab::Overview => {
                self.action = priority;
                return false;
            }
            Tab::History => {
                if let Some(view) = navigation {
                    if let Some(action) =
                        self.navigation
                            .history(ui, view, changes.is_some(), l, p, &mut layout)
                    {
                        self.action = Some(RepositoryAction::Navigate(action));
                    }
                } else {
                    layout.tabs(ui, l, p);
                    c::loading(
                        ui,
                        l.local("Loading history…", "Carregando histórico…"),
                        None,
                    );
                }
            }
            Tab::Changes => {
                if let Some(issue) = self.view.issue {
                    ui.colored_label(p.error, issue_text(l, issue));
                }
                if let Some(changes) = changes
                    && let Some(intent) = self.changes.show(
                        ui,
                        workspace,
                        changes,
                        self.view.activity == Activity::Idle
                            && navigation.is_none_or(|view| !view.busy)
                            && self.view.remotes.as_ref().is_none_or(|view| !view.busy),
                        &mut layout,
                        |ui| {
                            *command = landing::clean_workspace(ui, p, l, &self.view);
                        },
                    )
                {
                    self.action = Some(RepositoryAction::Changes(intent));
                }
            }
        }
        if priority.is_some() {
            *command = None;
            self.action = priority;
        }
        if self.navigation.tab != layout.tab {
            self.navigation.tab = layout.tab;
            ui.ctx().request_repaint();
        }
        self.sync_sidebar_width(ui, layout.sidebar_width);
        true
    }

    fn sync_sidebar_width(&mut self, ui: &Ui, width: Option<f32>) {
        if self.sidebar_width.zip(width).map_or(
            self.sidebar_width.is_some() != width.is_some(),
            |(previous, current)| (previous - current).abs() > 0.5,
        ) {
            self.sidebar_width = width;
            ui.ctx().request_repaint();
        }
    }

    fn workspace_operations(&mut self, ui: &mut Ui, p: Palette, l: Language) {
        let Some(workspace) = &self.view.active else {
            return;
        };
        let navigation = self.view.navigation.as_ref();
        if self.navigation.tab != Tab::Overview
            && self.view.destination.is_some()
            && Self::activity(ui, &self.view, l)
        {
            self.action = Some(RepositoryAction::Cancel);
        }
        if let Some(view) = navigation {
            self.navigation.status(ui, view, l, p);
            if let Some(action) = self.navigation.dialog(ui, view, &workspace.checkout, l, p) {
                self.action = Some(RepositoryAction::Navigate(action));
            }
        }
        if let Some(view) = &self.view.remotes {
            let toolbar_shows_progress =
                view.busy && matches!(self.fetch, super::fetch::FetchState::Fetching { .. });
            let status = if toolbar_shows_progress {
                None
            } else {
                self.remotes.status(ui, view, self.view.progress, l, p)
            };
            if let Some(action) = status.or_else(|| {
                self.remotes
                    .dialog(ui, view, self.view.github.workspace_account.is_some(), l, p)
            }) {
                self.action = Some(action);
            }
        }
    }

    fn summary(
        &mut self,
        ui: &mut Ui,
        p: Palette,
        l: Language,
        workspace: &gitrust_core::workspaces::Workspace,
    ) {
        ui.add_space(8.0);
        ui.add(egui::Label::new(RichText::new(filename(&workspace.path)).heading()).wrap());
        ui.add(
            egui::Label::new(RichText::new(workspace.path.to_string_lossy()).color(p.muted)).wrap(),
        );
        let checkout = match &workspace.checkout {
            Checkout::Branch(name) => format!("{} {name}", l.text(Text::Branch)),
            Checkout::Unborn(name) => {
                format!("{name} · {}", l.local("No commits yet", "Sem commits"))
            }
            Checkout::Detached(id) => {
                format!("{} · {id}", l.local("Detached HEAD", "HEAD destacado"))
            }
        };
        ui.add(egui::Label::new(checkout).wrap());
        if self.view.activity == Activity::Inspecting {
            ui.label(l.local(
                "Showing the last completed inspection.",
                "Exibindo a última inspeção concluída.",
            ));
        }
        ui.add_space(10.0);
        if let Some(changes) = workspace.changes {
            ui.label(l.local("Working tree", "Árvore de trabalho"));
            ui.horizontal_wrapped(|ui| {
                for (label, count) in [
                    (l.local("Staged", "No index"), changes.staged),
                    (l.local("Modified", "Modificados"), changes.unstaged),
                    (l.local("Untracked", "Não rastreados"), changes.untracked),
                    (l.local("Conflicted", "Conflitos"), changes.conflicted),
                ] {
                    c::badge(ui, &format!("{label} {count}"), p.text, p.elevated);
                }
            });
        } else {
            ui.label(l.local("Bare repository", "Repositório bare"));
        }
        ui.add_space(12.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    self.view.activity == Activity::Idle,
                    Button::new(l.local("Refresh", "Atualizar")),
                )
                .clicked()
            {
                self.send(RepositoryAction::Refresh);
            }
            if ui
                .button(l.local("Show in file manager", "Mostrar no gerenciador de arquivos"))
                .clicked()
            {
                self.send(RepositoryAction::Reveal(workspace.path.clone()));
            }
        });
        if !self.view.watching {
            ui.label(
                RichText::new(l.local(
                    "Updates on window focus or manual refresh.",
                    "Atualiza ao recuperar o foco da janela ou manualmente.",
                ))
                .small()
                .color(p.muted),
            );
        }
    }

    fn recents(&mut self, ui: &mut Ui, search: &str, p: Palette, l: Language, height: f32) {
        let active = self.view.active.as_ref().map(|workspace| &workspace.path);
        let enabled = self.usable();
        let mut clicked = None;
        let remove = l.local("Remove", "Remover");
        let font = egui::TextStyle::Body.resolve(ui.style());
        let remove_width = ui
            .fonts_mut(|fonts| fonts.layout_no_wrap(remove.into(), font, p.text).size().x)
            + 2.0 * ui.spacing().button_padding.x;
        ScrollArea::vertical().max_height(height).show(ui, |ui| {
            let mut matches = 0;
            for path in &self.view.recent.paths {
                let name = filename(path);
                let full = path.to_string_lossy();
                if !search.is_empty()
                    && !name.to_lowercase().contains(search)
                    && !full.to_lowercase().contains(search)
                {
                    continue;
                }
                matches += 1;
                ui.horizontal(|ui| {
                    let width = (ui.available_width() - remove_width - ui.spacing().item_spacing.x)
                        .max(0.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(width, 0.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_width(width);
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                            let selected = active == Some(path);
                            let label = if selected {
                                format!("{name} · {}", l.local("Active", "Ativo"))
                            } else if self.view.unavailable.contains(path) {
                                format!("{name} · {}", l.local("Unavailable", "Indisponível"))
                            } else {
                                name
                            };
                            ui.add_enabled_ui(enabled, |ui| {
                                if c::menu_choice(ui, &label, selected, p)
                                    .on_hover_text(full.as_ref())
                                    .clicked()
                                {
                                    clicked = Some(RepositoryAction::Open(path.clone()));
                                    ui.close_kind(egui::UiKind::Menu);
                                }
                            });
                            ui.add(
                                egui::Label::new(
                                    RichText::new(full.as_ref()).small().color(p.muted),
                                )
                                .truncate(),
                            );
                        },
                    );
                    if ui
                        .small_button(remove)
                        .on_hover_text(
                            l.local("Remove only the saved entry", "Remover apenas da lista"),
                        )
                        .clicked()
                    {
                        clicked = Some(RepositoryAction::Forget(path.clone()));
                    }
                });
            }
            if matches == 0 {
                ui.label(l.local("No matching repositories", "Nenhum repositório encontrado"));
            }
        });
        if let Some(action) = clicked {
            self.send(action);
        }
    }

    fn form_buttons(&mut self, ui: &mut Ui, l: Language) {
        ui.add_enabled_ui(self.usable(), |ui| {
            ui.horizontal_wrapped(|ui| {
                let label = l.local("Add local repository", "Adicionar repositório local");
                for (response, kind) in [
                    (ui.button(label), FormKind::Open),
                    (ui.button(l.local("Create", "Criar")), FormKind::Create),
                    (ui.button(l.local("Clone", "Clonar")), FormKind::Clone),
                ] {
                    if response.clicked() {
                        self.open_form(kind);
                        ui.close_kind(egui::UiKind::Menu);
                    }
                }
            });
        });
    }

    fn activity(ui: &mut Ui, view: &RepositoryView, l: Language) -> bool {
        let message = match view.activity {
            Activity::Starting => l.local("Starting Git services", "Iniciando serviços do Git"),
            Activity::Identifying => l.local("Identifying repository", "Identificando repositório"),
            Activity::Inspecting => l.local("Inspecting repository", "Inspecionando repositório"),
            Activity::Creating => l.local("Creating repository", "Criando repositório"),
            Activity::Cloning => l.local("Cloning repository", "Clonando repositório"),
            Activity::Cancelling => l.local("Cancelling and cleaning up", "Cancelando e limpando"),
            Activity::Idle | Activity::Trust => return false,
        };
        if view.activity == Activity::Inspecting {
            return ui
                .vertical_centered(|ui| {
                    ui.spinner();
                    ui.label(message);
                    ui.button(l.text(Text::Cancel)).clicked()
                })
                .inner;
        }
        let progress = view.progress.map(|progress| {
            let phase = match progress.phase {
                TransferPhase::Counting => l.local("Counting objects", "Contando objetos"),
                TransferPhase::Compressing => l.local("Compressing objects", "Comprimindo objetos"),
                TransferPhase::Receiving => l.local("Receiving objects", "Recebendo objetos"),
                TransferPhase::Resolving => l.local("Resolving deltas", "Resolvendo deltas"),
            };
            (
                format!("{phase} · {}%", progress.percent),
                f32::from(progress.percent) / 100.0,
            )
        });
        c::loading(
            ui,
            progress.as_ref().map_or(message, |(label, _)| label),
            progress.as_ref().map(|(_, value)| *value),
        );
        if let Some(destination) = &view.destination {
            let label = l.local("Destination", "Destino");
            ui.add(egui::Label::new(format!("{label} · {}", destination.display())).wrap());
        }
        matches!(
            view.activity,
            Activity::Identifying | Activity::Creating | Activity::Cloning
        ) && ui.button(l.text(Text::Cancel)).clicked()
    }

    pub(super) fn dialogs(&mut self, ui: &Ui, p: Palette, l: Language) {
        if self.forget_dialog(ui, p, l) {
            return;
        }
        if let Some(candidate) = self.view.candidate.clone() {
            let dialog = c::modal(
                ui,
                "workspace-trust",
                p,
                l.local("Trust local metadata?", "Confiar nos metadados locais?"),
                |ui| {
                    let height = (ui.ctx().content_rect().height() - 96.0).max(65.0);
                    ScrollArea::vertical().max_height(height).show(ui, |ui| {
                        if let Some(kind) = self.view.provisioned {
                            ui.label(match kind {
                                ProvisionKind::Create => l.local("Repository created.", "Repositório criado."),
                                ProvisionKind::Clone => l.local("Repository cloned.", "Repositório clonado."),
                            });
                        }
                        ui.add(egui::Label::new(candidate.to_string_lossy()).wrap());
                        ui.label(l.local("Trust this repository only if you control its local Git metadata. Approval lasts for this session.",
                    "Confie apenas se você controla os metadados Git locais. A autorização vale nesta sessão."));
                        ui.horizontal_wrapped(|ui| {
                        let cancel = ui.button(l.local("Decline", "Recusar"));
                        if ui.memory(|memory| memory.focused().is_none()) {
                            cancel.request_focus();
                        }
                        if cancel.clicked() {
                            return Some(false);
                        }
                        if c::primary(ui, l.local("Trust and open", "Confiar e abrir"), p).clicked()
                        {
                            return Some(true);
                        }
                        None
                        }).inner
                    }).inner
                },
            );
            if let Some(accepted) = dialog
                .inner
                .or_else(|| dialog.should_close().then_some(false))
            {
                self.form = None;
                self.send(RepositoryAction::Authorize {
                    path: candidate,
                    accepted,
                });
            }
        } else if let Some(mut form) = self.form.take() {
            let title = match form.kind {
                FormKind::Open => l.local("Add local repository", "Adicionar repositório local"),
                FormKind::Create => l.local("Create repository", "Criar repositório"),
                FormKind::Clone => l.local("Clone repository", "Clonar repositório"),
            };
            let mut close = false;
            let height = (ui.ctx().content_rect().height() - 96.0).max(65.0);
            let dialog = c::modal(ui, "workspace-form", p, title, |ui| {
                ScrollArea::vertical().max_height(height).show(ui, |ui| {
                    self.form_fields(ui, &mut form, p, l, &mut close);
                });
            });
            if !close && !dialog.should_close() {
                self.form = Some(form);
            }
        }
    }

    fn forget_dialog(&mut self, ui: &Ui, p: Palette, l: Language) -> bool {
        if let Some(path) = self.forget.take()
            && self
                .view
                .active
                .as_ref()
                .is_some_and(|active| active.path == path)
        {
            let dialog = c::modal(
                ui,
                "forget-workspace",
                p,
                l.local("Remove repository?", "Remover repositório?"),
                |ui| {
                    ui.label(filename(&path));
                    ui.label(l.local(
                    "Remove this entry from gitRUST. Its files and Git history stay on disk.",
                    "Remova esta entrada do gitRUST. Os arquivos e o histórico Git ficam no disco.",
                ));
                    ui.horizontal_wrapped(|ui| {
                        let cancel = ui.button(l.text(Text::Cancel));
                        if ui.memory(|memory| memory.focused().is_none()) {
                            cancel.request_focus();
                        }
                        (
                            ui.button(l.local("Remove repository", "Remover repositório"))
                                .clicked(),
                            cancel.clicked(),
                        )
                    })
                    .inner
                },
            );
            if dialog.inner.0 {
                self.send(RepositoryAction::Forget(path));
            } else if !dialog.inner.1 && !dialog.should_close() {
                self.forget = Some(path);
            }
            return true;
        }
        false
    }

    fn form_fields(
        &mut self,
        ui: &mut Ui,
        form: &mut Form,
        p: Palette,
        l: Language,
        close: &mut bool,
    ) {
        if form.kind == FormKind::Clone {
            if field(
                ui,
                "source",
                l.local("HTTPS or SSH URL", "URL HTTPS ou SSH"),
                &mut form.url,
            )
            .changed()
            {
                form.suggest();
            }
        } else if form.kind == FormKind::Create {
            field(
                ui,
                "branch",
                l.local("Initial branch", "Branch inicial"),
                &mut form.branch,
            );
        }
        let path_label = if form.kind == FormKind::Open {
            l.local("Repository or subdirectory", "Repositório ou subdiretório")
        } else {
            l.local("Destination parent directory", "Diretório pai do destino")
        };
        if field(ui, "destination", path_label, &mut form.path).changed() {
            form.chosen = None;
        }
        if ui
            .button(l.local("Browse folders…", "Procurar pastas…"))
            .clicked()
        {
            self.send(if form.kind == FormKind::Open {
                RepositoryAction::Browse
            } else {
                RepositoryAction::BrowseDestination
            });
        }
        if form.kind != FormKind::Open {
            field(
                ui,
                "project",
                l.local("New directory name", "Nome do novo diretório"),
                &mut form.name,
            );
            if let Some(
                RepositoryAction::Create { destination, .. }
                | RepositoryAction::Clone { destination, .. },
            ) = form.action()
            {
                ui.add(
                    egui::Label::new(
                        RichText::new(destination.to_string_lossy())
                            .small()
                            .color(p.muted),
                    )
                    .wrap(),
                );
            }
        }
        if form.kind == FormKind::Clone {
            form.auth
                .show(ui, super::remotes::ssh_url(&form.url), false, l);
        }
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button(l.text(Text::Cancel)).clicked() {
                *close = true;
            }
            let action = form.action();
            let label = match form.kind {
                FormKind::Open => l.local("Identify", "Identificar"),
                FormKind::Create => l.local("Confirm creation", "Confirmar criação"),
                FormKind::Clone => l.local("Confirm clone", "Confirmar clonagem"),
            };
            if ui
                .add_enabled_ui(action.is_some(), |ui| c::primary(ui, label, p))
                .inner
                .clicked()
                && let Some(action) = action
            {
                self.send(
                    if let RepositoryAction::Clone { destination, url } = action {
                        let auth = form.auth.take(super::remotes::ssh_url(&url));
                        RepositoryAction::CloneRemote {
                            destination,
                            url,
                            auth,
                        }
                    } else {
                        action
                    },
                );
                *close = true;
            }
        });
    }
}

fn field(ui: &mut Ui, id: &'static str, label: &str, value: &mut String) -> egui::Response {
    let label_id = ui.label(label).id;
    c::text_field(ui, Id::unique(("workspace-form", id)), label, value).labelled_by(label_id)
}

fn filename(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

fn issue_text(l: Language, issue: WorkspaceIssue) -> &'static str {
    match issue {
        WorkspaceIssue::GitUnavailable => l.local("Git 2.45 or newer is required to manage repositories.", "O gerenciamento de repositórios requer Git 2.45 ou superior."),
        WorkspaceIssue::Unavailable => l.local("The directory is missing. Reopen it at its current location.", "O diretório não existe. Abra-o no local atual."),
        WorkspaceIssue::NotRepository => l.local("The directory is outside a Git repository or its metadata is incomplete.", "O diretório está fora de um repositório Git ou os metadados estão incompletos."),
        WorkspaceIssue::PermissionDenied => l.local("The system denied access to this directory.", "O sistema negou acesso a este diretório."),
        WorkspaceIssue::Security => l.local("The repository was declined or its metadata is outside the supported trust policy.", "O repositório foi recusado ou seus metadados não atendem à política de confiança."),
        WorkspaceIssue::InvalidInput => l.local("Check the directory name, initial branch or HTTPS/SSH URL.", "Confira o nome do diretório, a branch inicial ou a URL HTTPS/SSH."),
        WorkspaceIssue::DestinationExists => l.local("The destination already exists. Choose a new directory name.", "O destino já existe. Escolha um nome de diretório novo."),
        WorkspaceIssue::Cancelled => l.local("The operation was cancelled.", "A operação foi cancelada."),
        WorkspaceIssue::OperationFailed => l.local("Git could not complete this operation. Check the destination or connection and try again.", "O Git não concluiu a operação. Confira o destino ou a conexão e tente novamente."),
        WorkspaceIssue::Transport(notice) => super::remotes::notice_text(notice, l),
        WorkspaceIssue::DesktopToolFailed => l.local("Could not open the desktop application. Check that it is installed.", "Não foi possível abrir o aplicativo. Confira se ele está instalado."),
        WorkspaceIssue::FolderActionFailed => l.local("The system could not open the folder.", "O sistema não conseguiu abrir a pasta."),
    }
}

impl Desktop {
    /// Enable the real repository workspace for interactive launches.
    pub fn enable_repository_management(&mut self) {
        self.managed = Some(ManagerUi::default());
    }

    /// Replace the repository presentation snapshot.
    pub fn set_repository_view(&mut self, view: RepositoryView) {
        if self.screen == super::Screen::Github && view.activity == Activity::Cloning {
            self.screen = super::Screen::Workspace;
        }
        if let Some(managed) = &mut self.managed {
            if self.screen == super::Screen::Workspace
                && view.offer_welcome
                && managed.view.activity == Activity::Starting
            {
                self.screen = super::Screen::Welcome;
            }
            if self.screen == super::Screen::Welcome && view.github.active.is_some() {
                managed.github.accounts();
                self.screen = super::Screen::Github;
            }
            if managed.view.navigation.as_ref().map(|nav| nav.identity)
                != view.navigation.as_ref().map(|nav| nav.identity)
            {
                managed.navigation = super::navigation::NavigationUi::default();
            }
            if managed
                .view
                .active
                .as_ref()
                .map(|workspace| &workspace.path)
                != view.active.as_ref().map(|workspace| &workspace.path)
            {
                managed.fetch = super::fetch::FetchState::Idle;
            }
            managed.view = view;
        }
    }

    /// Fill the destination from the native picker without losing path bytes.
    pub fn set_repository_destination(&mut self, path: PathBuf) {
        if let Some(managed) = &mut self.managed
            && managed.github.destination(&path)
        {
            return;
        }
        if let Some(form) = self
            .managed
            .as_mut()
            .and_then(|managed| managed.form.as_mut())
            && form.kind != FormKind::Open
        {
            form.path = path.to_string_lossy().into_owned();
            form.chosen = Some(path);
        }
    }

    /// Take the action selected during the most recent rendering pass.
    #[must_use]
    pub fn take_repository_action(&mut self) -> Option<RepositoryAction> {
        self.managed
            .as_mut()
            .and_then(|manager| manager.action.take())
    }

    /// Return the current appearance choices as persistence values.
    #[must_use]
    pub fn preferences(&self) -> Preferences {
        self.options.preferences()
    }

    /// Apply loaded preferences while keeping explicit launch overrides.
    pub fn apply_saved_preferences(
        &mut self,
        ctx: &egui::Context,
        saved: Preferences,
        overridden: [bool; 3],
    ) {
        let [theme, language, zoom] = overridden;
        if !theme {
            self.options.theme = match saved.appearance {
                Appearance::System => ThemePreference::System,
                Appearance::Dark => ThemePreference::Dark,
                Appearance::Light => ThemePreference::Light,
            };
            ctx.set_theme(self.options.theme);
        }
        if !language {
            self.options.language = match saved.locale {
                Locale::English => Language::English,
                Locale::Portuguese => Language::Portuguese,
            };
        }
        if !zoom {
            self.options.zoom = f32::from(saved.scale_percent) / 100.0;
            ctx.set_zoom_factor(self.options.zoom);
        }
    }
}

impl UiOptions {
    /// Convert session UI choices into the local storage representation.
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn preferences(self) -> Preferences {
        Preferences {
            appearance: match self.theme {
                ThemePreference::System => Appearance::System,
                ThemePreference::Light => Appearance::Light,
                ThemePreference::Dark => Appearance::Dark,
            },
            locale: match self.language {
                Language::English => Locale::English,
                Language::Portuguese => Locale::Portuguese,
            },
            scale_percent: (self.zoom * 100.0).round().clamp(100.0, 200.0) as u16,
        }
    }
}
