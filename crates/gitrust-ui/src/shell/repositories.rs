use std::path::{Component, Path, PathBuf};

use egui::{Button, Id, RichText, ScrollArea, ThemePreference, Ui};
use gitrust_core::workspaces::{
    Activity, Appearance, Checkout, Locale, Preferences, ProvisionKind, RepositoryAction,
    RepositoryView, TransferPhase, WorkspaceIssue,
};

use super::{Desktop, UiOptions};
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
        let name = url
            .strip_prefix("https://")
            .and_then(|value| value.rsplit_once('/'))
            .map_or("", |(_, name)| name);
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

#[derive(Debug, Default)]
pub(super) struct ManagerUi {
    pub(super) view: RepositoryView,
    query: String,
    changes: super::changes::ChangesUi,
    overview: bool,
    form: Option<Form>,
    action: Option<RepositoryAction>,
}

impl ManagerUi {
    pub(super) fn open_form(&mut self, kind: FormKind) {
        self.form = Some(Form::new(kind));
    }

    pub(super) fn send(&mut self, message: RepositoryAction) {
        self.action = Some(message);
    }

    pub(super) fn usable(&self) -> bool {
        self.view.git_available
            && matches!(self.view.activity, Activity::Idle | Activity::Inspecting)
            && self
                .view
                .changes
                .as_ref()
                .is_none_or(|changes| !changes.busy)
    }

    pub(super) fn title(&self, l: Language) -> String {
        self.view.active.as_ref().map_or_else(
            || l.text(Text::NoRepositoryShort).to_owned(),
            |workspace| filename(&workspace.path),
        )
    }

    pub(super) fn popup(&mut self, ui: &mut Ui, p: Palette, l: Language) {
        ui.set_min_width(240.0);
        c::search(ui, Id::unique("repository-search"), l, p, &mut self.query);
        ui.separator();
        self.recents(ui, p, l, 190.0);
        ui.separator();
        self.form_buttons(ui, p, l, false);
    }

    pub(super) fn workspace(&mut self, ui: &mut Ui, p: Palette, l: Language) {
        if self.real_changes(ui, p, l) {
            return;
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
                self.activity(ui, l);
                if let Some(workspace) = self.view.active.clone() {
                    if self.view.activity != Activity::Idle {
                        ui.separator();
                    }
                    self.summary(ui, p, l, &workspace);
                } else {
                    ui.add_space((ui.available_height() * 0.10).clamp(4.0, 26.0));
                    ui.vertical_centered(|ui| {
                        ui.set_max_width(415.0);
                        c::empty(
                            ui,
                            c::Icon::Repo,
                            l.text(Text::NoRepository),
                            l.local(
                                "Open an existing workspace or start a new repository.",
                                "Abra um workspace existente ou comece um repositório.",
                            ),
                            p,
                        );
                        ui.add_space(12.0);
                        if self.view.activity != Activity::Starting && !self.view.git_available {
                            if ui
                                .button(l.local("Retry Git discovery", "Buscar Git novamente"))
                                .clicked()
                            {
                                self.send(RepositoryAction::RetryGit);
                            }
                        } else {
                            self.form_buttons(ui, p, l, true);
                        }
                    });
                    if !self.view.recent.paths.is_empty() {
                        ui.add_space(14.0);
                        ui.label(
                            RichText::new(l.local("Recent repositories", "Repositórios recentes"))
                                .family(weight()),
                        );
                        self.recents(ui, p, l, 156.0);
                    }
                }
            });
    }

    fn real_changes(&mut self, ui: &mut Ui, p: Palette, l: Language) -> bool {
        if let Some(changes) = self.view.changes.as_ref()
            && let Some(workspace) = self.view.active.as_ref()
        {
            ui.horizontal_wrapped(|ui| {
                c::tabs(
                    ui,
                    &mut self.overview,
                    &[
                        (false, l.text(Text::Changes)),
                        (true, l.local("Overview", "Visão geral")),
                    ],
                    p,
                );
                if changes.busy {
                    c::loading(ui, l.local("Working…", "Processando…"), None);
                    if ui.button(l.text(Text::Cancel)).clicked() {
                        self.action = Some(RepositoryAction::Cancel);
                    }
                } else if let Some((success, message)) = super::changes::notice(changes, l) {
                    ui.colored_label(if success { p.success } else { p.warning }, message);
                }
            });
            if !self.overview {
                if let Some(issue) = self.view.issue {
                    ui.colored_label(p.error, issue_text(l, issue));
                }
                if let Some(intent) = self.changes.show(
                    ui,
                    workspace,
                    changes,
                    self.view.activity == Activity::Idle,
                    l,
                    p,
                ) {
                    self.action = Some(RepositoryAction::Changes(intent));
                }
                return true;
            }
        }
        false
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

    fn recents(&mut self, ui: &mut Ui, p: Palette, l: Language, height: f32) {
        let search = self.query.trim().to_lowercase();
        let active = self.view.active.as_ref().map(|workspace| &workspace.path);
        let enabled = self.usable();
        let mut clicked = None;
        ScrollArea::vertical().max_height(height).show(ui, |ui| {
            let mut matches = 0;
            for path in &self.view.recent.paths {
                let name = filename(path);
                let full = path.to_string_lossy();
                if !search.is_empty()
                    && !name.to_lowercase().contains(&search)
                    && !full.to_lowercase().contains(&search)
                {
                    continue;
                }
                matches += 1;
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
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
                            egui::Label::new(RichText::new(full.as_ref()).small().color(p.muted))
                                .truncate(),
                        );
                    });
                    if ui
                        .small_button(l.local("Remove", "Remover"))
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

    fn form_buttons(&mut self, ui: &mut Ui, p: Palette, l: Language, primary_open: bool) {
        ui.add_enabled_ui(self.usable(), |ui| {
            ui.horizontal_wrapped(|ui| {
                let label = l.local("Add local repository", "Adicionar repositório local");
                let open = if primary_open {
                    c::primary(ui, label, p)
                } else {
                    ui.button(label)
                };
                for (response, kind) in [
                    (open, FormKind::Open),
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

    fn activity(&mut self, ui: &mut Ui, l: Language) {
        let message = match self.view.activity {
            Activity::Starting => l.local("Starting Git services", "Iniciando serviços do Git"),
            Activity::Identifying => l.local("Identifying repository", "Identificando repositório"),
            Activity::Inspecting => l.local("Inspecting repository", "Inspecionando repositório"),
            Activity::Creating => l.local("Creating repository", "Criando repositório"),
            Activity::Cloning => l.local("Cloning repository", "Clonando repositório"),
            Activity::Cancelling => l.local("Cancelling and cleaning up", "Cancelando e limpando"),
            Activity::Idle | Activity::Trust => return,
        };
        let progress = self.view.progress.map(|progress| {
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
        if let Some(destination) = &self.view.destination {
            let label = l.local("Destination", "Destino");
            ui.add(egui::Label::new(format!("{label} · {}", destination.display())).wrap());
        }
        if matches!(
            self.view.activity,
            Activity::Identifying | Activity::Inspecting | Activity::Creating | Activity::Cloning
        ) && ui.button(l.text(Text::Cancel)).clicked()
        {
            self.send(RepositoryAction::Cancel);
        }
    }

    pub(super) fn dialogs(&mut self, ui: &Ui, p: Palette, l: Language) {
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
                FormKind::Clone => l.local("Clone public repository", "Clonar repositório público"),
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
                l.local("Public HTTPS URL", "URL HTTPS pública"),
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
                self.send(action);
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
        WorkspaceIssue::InvalidInput => l.local("Check the directory name, initial branch or public HTTPS URL.", "Confira o nome do diretório, a branch inicial ou a URL HTTPS pública."),
        WorkspaceIssue::DestinationExists => l.local("The destination already exists. Choose a new directory name.", "O destino já existe. Escolha um nome de diretório novo."),
        WorkspaceIssue::Cancelled => l.local("The operation was cancelled.", "A operação foi cancelada."),
        WorkspaceIssue::OperationFailed => l.local("Git could not complete this operation. Check the destination or connection and try again.", "O Git não concluiu a operação. Confira o destino ou a conexão e tente novamente."),
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
        if let Some(managed) = &mut self.managed {
            managed.view = view;
        }
    }

    /// Fill the destination from the native picker without losing path bytes.
    pub fn set_repository_destination(&mut self, path: PathBuf) {
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
