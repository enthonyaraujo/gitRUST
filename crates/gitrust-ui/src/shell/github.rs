use std::{fmt, path::PathBuf, sync::Arc};

use egui::{Button, ColorImage, ComboBox, RichText, ScrollArea, TextEdit, TextureHandle, Ui, Vec2};
use gitrust_core::{
    github::{
        Account, AccountId, Action, AuthMethod, Avatar, BrowserState, Connection, CreatePull,
        CreateRepository, Notice, Publish, PullFilter, Repository, RepositoryKey, View,
    },
    workspaces::RepositoryAction,
};

use crate::{
    components as c,
    i18n::{Language, Text},
    theme::Palette,
};

#[derive(Clone, Copy, Default, Eq, PartialEq)]
enum Tab {
    #[default]
    Accounts,
    Repositories,
    Pulls,
}

enum Form {
    Clone(Repository),
    Publish {
        local: bool,
        resume: Option<RepositoryKey>,
    },
    Fork(RepositoryKey),
    Pull,
    Sync(RepositoryKey, String),
    Checkout(RepositoryKey, u64),
    Disconnect(AccountId),
}

#[derive(Default)]
pub(super) struct GithubUi {
    tab: Tab,
    form: Option<Form>,
    client: String,
    configure_client: bool,
    account: Option<AccountId>,
    repository: Option<RepositoryKey>,
    pull: Option<u64>,
    pull_repo: Option<RepositoryKey>,
    installation: Option<u64>,
    filter: String,
    public_query: String,
    visible: Visibility,
    sort: Sort,
    pulls: PullFilter,
    page: u32,
    pull_page: u32,
    owner: String,
    name: String,
    description: String,
    base: String,
    head: String,
    source_owner: String,
    source_repo: String,
    remote: String,
    chosen: Option<PathBuf>,
    path: String,
    flag: bool,
    image: Option<(Arc<Avatar>, TextureHandle)>,
    action: Option<RepositoryAction>,
}

impl fmt::Debug for GithubUi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GitHub view")
    }
}

#[derive(Clone, Copy, Default, Eq, PartialEq)]
enum Visibility {
    #[default]
    All,
    Public,
    Private,
}

#[derive(Clone, Copy, Default, Eq, PartialEq)]
enum Sort {
    #[default]
    Updated,
    Name,
}

impl GithubUi {
    pub(super) fn accounts(&mut self) {
        self.tab = Tab::Accounts;
    }

    pub(super) fn destination(&mut self, path: &std::path::Path) -> bool {
        if matches!(self.form, Some(Form::Clone(_))) {
            self.path = path.to_string_lossy().into_owned();
            self.chosen = Some(path.to_owned());
            true
        } else {
            false
        }
    }

    pub(super) fn send(&mut self, action: Action) {
        self.action = Some(RepositoryAction::Github(action));
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        view: &View,
        l: Language,
        p: Palette,
    ) -> Option<RepositoryAction> {
        if self.account != view.active {
            self.account = view.active;
            self.repository = None;
            self.pull = None;
            self.pull_repo = None;
            self.installation = None;
            self.page = 1;
            self.pull_page = 1;
            if self.form.is_some() && self.account.is_some() && self.tab == Tab::Pulls {
                self.pull_repo.clone_from(&view.workspace_repo);
            } else if !matches!(self.form, Some(Form::Publish { local: true, .. })) {
                self.form = None;
            }
        }
        ui.heading("GitHub");
        c::tabs(
            ui,
            &mut self.tab,
            &[
                (Tab::Accounts, l.local("Accounts", "Contas")),
                (Tab::Repositories, l.local("Repositories", "Repositórios")),
                (Tab::Pulls, l.local("Pull requests", "Pull requests")),
            ],
            p,
        );
        self.authorization_status(ui, view, l, p);
        ui.add_enabled_ui(!view.busy, |ui| match self.tab {
            Tab::Accounts => self.account_panel(ui, view, l, p),
            Tab::Repositories => self.repository_panel(ui, view, l),
            Tab::Pulls => self.pull_panel(ui, view, l),
        });
        if self.form.is_some() {
            self.dialog(ui, view, l, p);
        }
        self.take_action()
    }

    pub(super) fn take_action(&mut self) -> Option<RepositoryAction> {
        self.action.take()
    }

    pub(super) fn authorization_status(
        &mut self,
        ui: &mut Ui,
        view: &View,
        l: Language,
        p: Palette,
    ) {
        use BrowserState as B;
        if let Some(message) = view.notice {
            ui.label(notice(message, l));
        }
        if view.busy
            && matches!(
                view.browser_state,
                B::Starting
                    | B::AwaitingBrowserAuthorization
                    | B::CallbackReceived
                    | B::ExchangingCode
                    | B::FetchingProfile
            )
        {
            let dialog = c::modal(
                ui,
                "github-browser-authorization",
                p,
                l.local("GitHub authorization", "Autorização do GitHub"),
                |ui| {
                    ui.set_max_width(410.0_f32.min(ui.ctx().content_rect().width() - 48.0));
                    ui.label(match view.browser_state {
                    B::Starting => l.local("Preparing to open the browser…", "Preparando a abertura do navegador…"),
                    B::AwaitingBrowserAuthorization => l.local(
                        "Complete the gitRUST authorization in your browser. You will return to this window after approving access.",
                        "Conclua a autorização do gitRUST no navegador. Você retornará a esta janela após aprovar o acesso.",
                    ),
                    _ => l.local("Completing GitHub sign-in…", "Concluindo a conexão com o GitHub…"),
                });
                    if let Some(message) = view.notice {
                        ui.label(notice(message, l));
                    }
                    ui.horizontal_wrapped(|ui| {
                        if view.browser_state == B::AwaitingBrowserAuthorization
                            && c::primary(ui, l.local("Reopen browser", "Reabrir navegador"), p)
                                .clicked()
                        {
                            self.send(Action::ReopenBrowser);
                        }
                        ui.button(l.text(Text::Cancel)).clicked()
                    })
                    .inner
                },
            );
            if dialog.inner || dialog.should_close() {
                self.send(Action::Cancel);
            }
            return;
        }
        if view.busy {
            ui.horizontal_wrapped(|ui| {
                ui.spinner();
                ui.label(l.local("Waiting for GitHub", "Aguardando o GitHub"));
                if ui.button(l.text(Text::Cancel)).clicked() {
                    self.send(Action::Cancel);
                }
            });
        }
        if let Some(prompt) = &view.prompt {
            ui.group(|ui| {
                ui.label("github.com/login/device");
                ui.label(l.local(
                    "Enter this code in your browser",
                    "Digite este código no navegador",
                ));
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(&prompt.code).monospace().strong());
                    if ui.button(l.local("Copy code", "Copiar código")).clicked() {
                        ui.ctx().copy_text(prompt.code.clone());
                    }
                    if c::primary(
                        ui,
                        l.local("Open GitHub authorization", "Abrir autorização no GitHub"),
                        p,
                    )
                    .clicked()
                    {
                        self.send(Action::Open("https://github.com/login/device".into()));
                    }
                });
            });
        }
    }

    pub(super) fn sign_in_controls(&mut self, ui: &mut Ui, view: &View, l: Language, p: Palette) {
        let label = if view.accounts.is_empty() {
            l.local("Sign in with GitHub", "Entrar com GitHub")
        } else {
            l.local("Add a GitHub account", "Adicionar conta do GitHub")
        };
        if c::primary(ui, label, p).clicked() {
            self.send(Action::BrowserSignIn { reconnect: None });
        }
        if matches!(
            view.notice,
            Some(Notice::OAuthConfiguration | Notice::OAuthHandler)
        ) && ui
            .link(l.local("OAuth App setup", "Configurar o OAuth App"))
            .clicked()
        {
            self.send(Action::Open(
                "https://github.com/enthonyaraujo/gitRUST/blob/main/docs/GITHUB_OAUTH_SETUP.md"
                    .into(),
            ));
        }
    }

    fn device_sign_in(&mut self, ui: &mut Ui, view: &View, l: Language, p: Palette) {
        let configured = !view.client_id.is_empty();
        let label = if view.accounts.is_empty() {
            l.local("Sign in to GitHub.com", "Entrar no GitHub.com")
        } else {
            l.local("Add a GitHub account", "Adicionar conta do GitHub")
        };
        ui.add_enabled_ui(!view.busy, |ui| {
            let mut start = false;
            if !self.configure_client || configured {
                if c::primary(ui, label, p).clicked() {
                    if configured {
                        start = true;
                    } else {
                        self.configure_client = true;
                    }
                }
            } else {
                ui.label(l.local(
                    "This build needs the public Client ID of the gitRUST GitHub App.",
                    "Esta versão precisa do Client ID público do GitHub App do gitRUST.",
                ));
                let field = ui.label(l.local("GitHub App Client ID", "Client ID do GitHub App"));
                let input = ui.add(TextEdit::singleline(&mut self.client)
                    .id_salt("github-client")
                    .desired_width(310.0_f32.min(ui.available_width())))
                    .labelled_by(field.id);
                if !input.has_focus() && ui.memory(|memory| memory.focused().is_none()) {
                    input.request_focus();
                }
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled_ui(!self.client.trim().is_empty(), |ui| c::primary(ui, label, p)).inner.clicked() {
                        start = true;
                    }
                    if ui.link(l.local("GitHub App setup", "Configurar o GitHub App")).clicked() {
                        self.send(Action::Open("https://github.com/enthonyaraujo/gitRUST/blob/main/docs/GITHUB_APP_SETUP.md".into()));
                    }
                });
            }
            if start {
                self.send(Action::SignIn {
                    client_id: if configured { view.client_id.clone() } else { self.client.trim().into() },
                    reconnect: None,
                });
            }
        });
    }

    fn account_panel(&mut self, ui: &mut Ui, view: &View, l: Language, p: Palette) {
        ScrollArea::vertical()
            .id_salt("github-accounts")
            .show(ui, |ui| {
                self.sign_in_controls(ui, view, l, p);
                egui::CollapsingHeader::new(l.local(
                    "GitHub App (Device Flow)",
                    "GitHub App (código de dispositivo)",
                ))
                .show(ui, |ui| self.device_sign_in(ui, view, l, p));
                ui.horizontal_wrapped(|ui| {
                    if (!view.client_id.is_empty() || !view.browser_client_id.is_empty())
                        && ui
                            .button(l.local("Load saved accounts", "Carregar contas salvas"))
                            .clicked()
                    {
                        self.send(Action::Restore {
                            device: String::new(),
                            browser: String::new(),
                        });
                    }
                    if ui
                        .button(l.local(
                            "Manage authorization on GitHub",
                            "Gerenciar autorizações no GitHub",
                        ))
                        .clicked()
                    {
                        self.send(Action::Open(
                            if Self::is_browser(view) {
                                "https://github.com/settings/applications"
                            } else {
                                "https://github.com/settings/installations"
                            }
                            .into(),
                        ));
                    }
                });
                for account in view.accounts.iter() {
                    self.account_line(ui, view, account, l);
                }
                if let Some(id) = view.active {
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .button(
                                l.local("Refresh account and access", "Atualizar conta e acesso"),
                            )
                            .clicked()
                        {
                            self.send(Action::Refresh);
                        }
                        if let Some(path) = &view.workspace {
                            let bound = view.workspace_account == Some(id);
                            let label = if bound {
                                l.local("Unlink current workspace", "Desvincular workspace atual")
                            } else {
                                l.local(
                                    "Link current workspace to this account",
                                    "Vincular workspace atual a esta conta",
                                )
                            };
                            if ui.button(label).clicked() {
                                self.send(Action::Bind(path.clone(), (!bound).then_some(id)));
                            }
                        }
                    });
                }
                if !view.organizations.is_empty() {
                    ui.separator();
                    ui.strong(l.local("Authorized installations", "Instalações autorizadas"));
                    for org in view.organizations.iter() {
                        if ui.link(&org.login).clicked() {
                            self.send(Action::Open(format!("https://github.com/{}", org.login)));
                        }
                    }
                }
            });
    }

    fn account_line(&mut self, ui: &mut Ui, view: &View, account: &Account, l: Language) {
        let profile = &account.profile;
        let active = view.active == Some(account.id);
        ui.horizontal_wrapped(|ui| {
            if let Some(avatar) = view.avatar.as_ref().filter(|_| active) {
                if self
                    .image
                    .as_ref()
                    .is_none_or(|(old, _)| !Arc::ptr_eq(old, avatar))
                {
                    let image = ColorImage::from_rgba_unmultiplied(
                        [avatar.width, avatar.height],
                        &avatar.rgba,
                    );
                    self.image = Some((
                        avatar.clone(),
                        ui.ctx()
                            .load_texture("github-avatar", image, egui::TextureOptions::LINEAR),
                    ));
                }
                if let Some((_, image)) = &self.image {
                    ui.add(
                        egui::Image::new((image.id(), Vec2::splat(32.0)))
                            .alt_text(l.local("GitHub avatar", "Avatar do GitHub")),
                    );
                }
            }
            if ui
                .selectable_label(active, format!("@{}", profile.login))
                .clicked()
            {
                self.send(Action::Select(account.id));
            }
            if let Some(name) = &profile.name {
                ui.label(name);
            }
            ui.label(match account.connection {
                Connection::Temporary => l.local("Temporary session", "Sessão temporária"),
                Connection::Stored => {
                    l.local("Saved in credential vault", "Salva no cofre de credenciais")
                }
                Connection::Reconnect => l.local("Reconnect required", "Reconexão necessária"),
            });
            ui.label(match account.id.method {
                AuthMethod::Device => "GitHub App",
                AuthMethod::Browser => "OAuth App",
            });
            if ui.button(l.local("Reconnect", "Reconectar")).clicked() {
                self.send(match account.id.method {
                    AuthMethod::Device => Action::SignIn {
                        client_id: view.client_id.clone(),
                        reconnect: Some(account.id),
                    },
                    AuthMethod::Browser => Action::BrowserSignIn {
                        reconnect: Some(account.id),
                    },
                });
            }
            if ui.button(l.local("Disconnect", "Desconectar")).clicked() {
                self.form = Some(Form::Disconnect(account.id));
            }
        });
    }

    fn is_browser(view: &View) -> bool {
        view.active
            .is_some_and(|id| id.method == AuthMethod::Browser)
    }

    fn active_login(view: &View) -> &str {
        view.accounts
            .iter()
            .find(|account| Some(account.id) == view.active)
            .map_or("", |account| &account.profile.login)
    }

    #[allow(clippy::too_many_lines)]
    fn repository_panel(&mut self, ui: &mut Ui, view: &View, l: Language) {
        if view.active.is_none() {
            ui.label(l.local(
                "Connect an account to list authorized repositories.",
                "Conecte uma conta para listar os repositórios autorizados.",
            ));
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(Self::active_login(view));
            if ui
                .button(l.local("Refresh repositories", "Atualizar repositórios"))
                .clicked()
            {
                self.page = 1;
                if self.installation.is_some() {
                    self.send(Action::Repositories {
                        installation: self.installation,
                        page: 1,
                    });
                } else {
                    self.send(Action::Refresh);
                }
            }
            if ui
                .button(l.local("Create repository", "Criar repositório"))
                .clicked()
            {
                self.open_publish(view, false, None);
            }
            if view.workspace_branch.is_some()
                && ui
                    .button(l.local("Publish repository", "Publicar repositório"))
                    .clicked()
            {
                self.open_publish(view, true, None);
            }
        });
        ui.horizontal_wrapped(|ui| {
            if !Self::is_browser(view) {
                let mut selected = self.installation;
                let summary = view
                    .organizations
                    .iter()
                    .find(|org| Some(org.installation) == selected)
                    .map_or(l.local("All available", "Todos disponíveis"), |org| {
                        &org.login
                    });
                ComboBox::from_label(l.local("Organization", "Organização"))
                    .selected_text(summary)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut selected,
                            None,
                            l.local("All available", "Todos disponíveis"),
                        );
                        for org in view.organizations.iter() {
                            ui.selectable_value(&mut selected, Some(org.installation), &org.login);
                        }
                    });
                if selected != self.installation {
                    self.installation = selected;
                    self.page = 1;
                    self.send(Action::Repositories {
                        installation: selected,
                        page: 1,
                    });
                }
            }
            ComboBox::from_label(l.local("Visibility", "Visibilidade"))
                .selected_text(visibility(self.visible, l))
                .show_ui(ui, |ui| {
                    for item in [Visibility::All, Visibility::Public, Visibility::Private] {
                        ui.selectable_value(&mut self.visible, item, visibility(item, l));
                    }
                });
            ComboBox::from_label(l.local("Sort page", "Ordenar página"))
                .selected_text(if self.sort == Sort::Name {
                    l.local("Name", "Nome")
                } else {
                    l.local("Updated", "Atualização")
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.sort, Sort::Name, l.local("Name", "Nome"));
                    ui.selectable_value(
                        &mut self.sort,
                        Sort::Updated,
                        l.local("Updated", "Atualização"),
                    );
                });
        });
        ui.horizontal_wrapped(|ui| {
            let label = ui.label(l.local("Repository owner/name", "Repositório proprietário/nome"));
            ui.add(TextEdit::singleline(&mut self.public_query).desired_width(215.0))
                .labelled_by(label.id);
            if ui
                .button(l.local("Find specific repository", "Buscar repositório específico"))
                .clicked()
                && let Some((owner, name)) = self.public_query.trim().split_once('/')
                && let Some(repo) = RepositoryKey::new(owner, name)
            {
                self.repository = Some(repo.clone());
                self.page = 1;
                self.send(Action::Repository(repo));
            }
        });
        field(
            ui,
            l.local("Search current page", "Buscar na página atual"),
            &mut self.filter,
        );
        let filter = self.filter.to_lowercase();
        let mut entries: Vec<_> = view
            .repositories
            .iter()
            .filter(|repo| {
                (self.visible == Visibility::All
                    || repo.private == (self.visible == Visibility::Private))
                    && repo.key.slug().to_lowercase().contains(&filter)
            })
            .collect();
        if self.sort == Sort::Name {
            entries.sort_by_key(|repo| &repo.key);
        } else {
            entries.sort_by(|left, right| right.updated.cmp(&left.updated));
        }
        ScrollArea::vertical()
            .id_salt("github-repositories")
            .max_height((ui.available_height() * 0.42).max(72.0))
            .show_rows(ui, 25.0, entries.len(), |ui, range| {
                for index in range {
                    let repo = entries[index];
                    let label = format!(
                        "{} · {}{}",
                        repo.key.slug(),
                        visibility(
                            if repo.private {
                                Visibility::Private
                            } else {
                                Visibility::Public
                            },
                            l
                        ),
                        if view.local_repositories.contains(&repo.key) {
                            l.local(" · Opened locally", " · Aberto localmente")
                        } else {
                            ""
                        }
                    );
                    if ui
                        .selectable_label(self.repository.as_ref() == Some(&repo.key), label)
                        .clicked()
                    {
                        self.repository = Some(repo.key.clone());
                    }
                }
            });
        ui.horizontal_wrapped(|ui| {
            let prev = self.page.saturating_sub(1);
            if ui
                .add_enabled(
                    prev != 0,
                    Button::new(l.local("Previous page", "Página anterior")),
                )
                .clicked()
            {
                self.page = prev;
                self.send(Action::Repositories {
                    installation: self.installation,
                    page: prev,
                });
            }
            if let Some(next) = view.repository_page
                && ui.button(l.local("Next page", "Próxima página")).clicked()
            {
                self.page = next;
                self.send(Action::Repositories {
                    installation: self.installation,
                    page: next,
                });
            }
            if ui
                .button(l.local("Grant repository access", "Conceder acesso a repositórios"))
                .clicked()
            {
                self.send(Action::Open(
                    "https://github.com/settings/installations".into(),
                ));
            }
        });
        if view.repositories.is_empty() {
            ui.label(l.local(
                "No results loaded. Refresh or check installation access.",
                "Nenhum resultado carregado. Atualize ou confira o acesso da instalação.",
            ));
        }
        if let Some(repo) = view
            .repositories
            .iter()
            .find(|repo| Some(&repo.key) == self.repository.as_ref())
        {
            self.repository_details(ui, view, repo, l);
        }
        if let Some(repo) = &view.result {
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                if ui.link(repo.slug()).clicked() {
                    self.send(Action::Open(repo.browser()));
                }
                if ui
                    .button(l.local("Check availability", "Conferir disponibilidade"))
                    .clicked()
                {
                    self.repository = Some(repo.clone());
                    self.send(Action::Repository(repo.clone()));
                }
                if view.workspace_branch.is_some()
                    && ui
                        .button(l.local("Resume publication", "Retomar publicação"))
                        .clicked()
                {
                    self.open_publish(view, true, Some(repo.clone()));
                }
            });
        }
    }

    fn repository_details(&mut self, ui: &mut Ui, view: &View, repo: &Repository, l: Language) {
        ui.separator();
        if let Some(description) = &repo.description {
            ui.label(description);
        }
        ui.label(format!(
            "{}  ·  {}  ·  {}",
            repo.default_branch,
            repo.language.as_deref().unwrap_or("—"),
            repo.updated
        ));
        ui.horizontal_wrapped(|ui| {
            if repo.archived {
                ui.label(l.local("Archived", "Arquivado"));
            }
            if repo.fork {
                ui.label("Fork");
            }
            ui.label(if repo.can_push {
                l.local("Write access", "Acesso de escrita")
            } else {
                l.local("Read access", "Acesso de leitura")
            });
            if let Some(parent) = &repo.parent
                && ui.link(parent.slug()).clicked()
            {
                self.send(Action::Open(parent.browser()));
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(l.local(
                    "Clone selected repository",
                    "Clonar repositório selecionado",
                ))
                .clicked()
            {
                repo.key.name().clone_into(&mut self.name);
                self.path.clear();
                self.chosen = None;
                self.flag = false;
                self.form = Some(Form::Clone(repo.clone()));
            }
            if ui
                .button(l.local("View on GitHub", "Ver no GitHub"))
                .clicked()
            {
                self.send(Action::Open(repo.key.browser()));
            }
            if ui
                .button(l.local("Copy repository URL", "Copiar URL do repositório"))
                .clicked()
            {
                ui.ctx().copy_text(repo.key.browser());
            }
            if ui.button(l.local("Create fork", "Criar fork")).clicked() {
                Self::active_login(view).clone_into(&mut self.owner);
                repo.key.name().clone_into(&mut self.name);
                self.form = Some(Form::Fork(repo.key.clone()));
            }
            if repo.can_push
                && view.workspace_branch.is_some()
                && ui
                    .button(l.local("Publish to this repository", "Publicar neste repositório"))
                    .clicked()
            {
                self.open_publish(view, true, Some(repo.key.clone()));
            }
            if repo.fork
                && repo.can_push
                && ui.button(l.local("Sync fork", "Atualizar fork")).clicked()
            {
                self.form = Some(Form::Sync(repo.key.clone(), repo.default_branch.clone()));
            }
            if ui
                .button(l.local("View pull requests", "Ver pull requests"))
                .clicked()
            {
                self.pull_repo = Some(repo.key.clone());
                self.tab = Tab::Pulls;
                self.refresh_pulls(repo.key.clone(), 1);
            }
        });
    }

    fn refresh_pulls(&mut self, repo: RepositoryKey, page: u32) {
        self.pull = None;
        self.pull_page = page;
        self.send(Action::Pulls {
            repository: repo,
            page,
            state: self.pulls,
        });
    }

    #[allow(clippy::too_many_lines)]
    fn pull_panel(&mut self, ui: &mut Ui, view: &View, l: Language) {
        let Some(repo) = self
            .pull_repo
            .clone()
            .or_else(|| view.workspace_repo.clone())
        else {
            ui.label(l.local(
                "Select a GitHub repository to inspect pull requests.",
                "Selecione um repositório GitHub para consultar pull requests.",
            ));
            return;
        };
        ui.horizontal_wrapped(|ui| {
            ui.strong(repo.slug());
            if ui
                .button(l.local("Refresh pull requests", "Atualizar pull requests"))
                .clicked()
            {
                self.refresh_pulls(repo.clone(), 1);
            }
            if let Some(workspace) = &view.workspace_repo
                && workspace != &repo
                && ui
                    .button(l.local(
                        "Use local workspace repository",
                        "Usar repositório do workspace local",
                    ))
                    .clicked()
            {
                self.pull_repo = Some(workspace.clone());
                self.refresh_pulls(workspace.clone(), 1);
            }
            ComboBox::from_label(l.local("State", "Estado"))
                .selected_text(pull_filter(self.pulls, l))
                .show_ui(ui, |ui| {
                    for filter in [PullFilter::Open, PullFilter::Closed, PullFilter::All] {
                        if ui
                            .selectable_value(&mut self.pulls, filter, pull_filter(filter, l))
                            .clicked()
                        {
                            self.refresh_pulls(repo.clone(), 1);
                        }
                    }
                });
        });
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(l.local("Create pull request", "Criar pull request"))
                .clicked()
            {
                self.open_pull(view, &repo);
            }
            if ui
                .button(l.local("Open GitHub Actions", "Abrir GitHub Actions"))
                .clicked()
            {
                self.send(Action::Open(format!("{}/actions", repo.browser())));
            }
            if view.workspace_repo.as_ref() == Some(&repo)
                && let Some(branch) = &view.workspace_branch
                && ui
                    .button(l.local("Open branch on GitHub", "Abrir branch no GitHub"))
                    .clicked()
            {
                let branch = gitrust_core::github::encode_reference(branch);
                self.send(Action::Open(format!("{}/tree/{branch}", repo.browser())));
            }
            if view.workspace_repo.as_ref() == Some(&repo)
                && let Some(revision) = &view.workspace_revision
            {
                if ui
                    .button(l.local("Open commit on GitHub", "Abrir commit no GitHub"))
                    .clicked()
                {
                    self.send(Action::Open(format!(
                        "{}/commit/{revision}",
                        repo.browser()
                    )));
                }
                if ui
                    .button(l.local("Copy commit URL", "Copiar URL do commit"))
                    .clicked()
                {
                    ui.ctx()
                        .copy_text(format!("{}/commit/{revision}", repo.browser()));
                }
                if ui
                    .button(l.local("Load workflows", "Carregar workflows"))
                    .clicked()
                {
                    self.send(Action::Workflows {
                        repository: repo.clone(),
                        revision: revision.clone(),
                    });
                }
                if ui
                    .button(l.local("Check current commit", "Consultar checks do commit"))
                    .clicked()
                {
                    self.send(Action::Checks {
                        repository: repo.clone(),
                        revision: revision.clone(),
                    });
                }
            }
        });
        field(
            ui,
            l.local("Search current page", "Buscar na página atual"),
            &mut self.filter,
        );
        let query = self.filter.to_lowercase();
        let entries: Vec<_> = view
            .pulls
            .iter()
            .filter(|pull| {
                view.pull_repository.as_ref() == Some(&repo)
                    && format!("{} {} {}", pull.number, pull.title, pull.author)
                        .to_lowercase()
                        .contains(&query)
            })
            .collect();
        ScrollArea::vertical()
            .id_salt("github-pull-list")
            .max_height((ui.available_height() * 0.39).max(66.0))
            .show_rows(ui, 25.0, entries.len(), |ui, range| {
                for index in range {
                    let pull = entries[index];
                    let current = view.workspace_branch.as_deref() == Some(&pull.head)
                        && pull.head_repository.as_ref() == view.workspace_repo.as_ref();
                    if ui
                        .selectable_label(
                            self.pull == Some(pull.number),
                            format!(
                                "#{} {}{}",
                                pull.number,
                                pull.title,
                                if current {
                                    l.local(" · Current branch", " · Branch atual")
                                } else {
                                    ""
                                }
                            ),
                        )
                        .clicked()
                    {
                        self.pull = Some(pull.number);
                    }
                }
            });
        ui.horizontal_wrapped(|ui| {
            let previous = self.pull_page.saturating_sub(1);
            if ui
                .add_enabled(
                    previous > 0,
                    Button::new(l.local("Previous page", "Página anterior")),
                )
                .clicked()
            {
                self.refresh_pulls(repo.clone(), previous);
            }
            if let Some(page) = view
                .pull_page
                .filter(|_| view.pull_repository.as_ref() == Some(&repo))
                && ui.button(l.local("Next page", "Próxima página")).clicked()
            {
                self.refresh_pulls(repo.clone(), page);
            }
            if let Some((destination, number)) = &view.result_pull
                && ui
                    .button(format!(
                        "{} #{number}",
                        l.local("Open new pull request", "Abrir novo pull request")
                    ))
                    .clicked()
            {
                self.send(Action::Open(format!(
                    "{}/pull/{number}",
                    destination.browser()
                )));
            }
        });
        if let Some(pull) = entries
            .into_iter()
            .find(|pull| Some(pull.number) == self.pull)
        {
            ui.label(format!(
                "{} · {} · {} → {}{}",
                pull.author,
                pull.state,
                pull.head,
                pull.base,
                if pull.draft { " · Draft" } else { "" }
            ));
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button(l.local("Open pull request", "Abrir pull request"))
                    .clicked()
                {
                    self.send(Action::Open(format!(
                        "{}/pull/{}",
                        repo.browser(),
                        pull.number
                    )));
                }
                if ui
                    .button(l.local("Load checks", "Carregar checks"))
                    .clicked()
                {
                    let source = pull.head_repository.as_ref().unwrap_or(&repo);
                    self.send(Action::Checks {
                        repository: source.clone(),
                        revision: pull.revision.clone(),
                    });
                }
                if ui
                    .add_enabled(
                        view.workspace_repo.as_ref() == Some(&repo),
                        Button::new(
                            l.local("Check out pull request", "Abrir branch do pull request"),
                        ),
                    )
                    .clicked()
                {
                    self.name = format!("review/pr-{}", pull.number);
                    self.form = Some(Form::Checkout(repo.clone(), pull.number));
                }
            });
        }
        if let Some(checks) = &view.checks {
            if let Some((repository, revision)) = &view.check_scope {
                ui.label(format!(
                    "{} · {}",
                    repository.slug(),
                    revision.get(..12).unwrap_or(revision)
                ));
            }
            if view.workflows {
                ui.strong(l.local("Workflow runs", "Execuções de workflows"));
            }
            if checks.is_empty() {
                ui.label(l.local("No checks reported.", "Nenhum check informado."));
            }
            ScrollArea::vertical()
                .id_salt("github-checks")
                .show(ui, |ui| {
                    for check in checks.iter() {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(format!(
                                "{} · {}",
                                check.name,
                                check.conclusion.as_deref().unwrap_or(&check.state)
                            ));
                            if let Some(url) = &check.details
                                && ui.link(l.local("Details", "Detalhes")).clicked()
                            {
                                self.send(Action::Open(url.clone()));
                            }
                        });
                    }
                });
        }
    }

    pub(super) fn publish_workspace(&mut self, view: &View) {
        self.tab = Tab::Repositories;
        self.open_publish(view, true, None);
    }

    pub(super) fn create_workspace_pull(&mut self, view: &View) {
        if let Some(repo) = &view.workspace_repo {
            self.tab = Tab::Pulls;
            self.pull_repo = Some(repo.clone());
            self.open_pull(view, repo);
        }
    }

    pub(super) fn workspace_pulls(&mut self, view: &View) {
        if let Some(repo) = view.workspace_repo.clone() {
            self.tab = Tab::Pulls;
            self.pull_repo = Some(repo.clone());
            self.refresh_pulls(repo, 1);
        }
    }

    fn open_pull(&mut self, view: &View, repo: &RepositoryKey) {
        let source = view.workspace_repo.as_ref().unwrap_or(repo);
        self.source_owner = source.owner().into();
        self.source_repo = source.name().into();
        self.head = view.workspace_branch.clone().unwrap_or_default();
        self.base = view
            .repositories
            .iter()
            .find(|entry| &entry.key == repo)
            .map_or(String::new(), |entry| entry.default_branch.clone());
        self.owner = repo.owner().into();
        self.remote = repo.name().into();
        self.name.clear();
        self.description.clear();
        self.flag = true;
        self.form = Some(Form::Pull);
    }

    fn open_publish(&mut self, view: &View, local: bool, resume: Option<RepositoryKey>) {
        self.owner = resume.as_ref().map_or_else(
            || Self::active_login(view).into(),
            |repo| repo.owner().into(),
        );
        self.name = resume.as_ref().map_or_else(
            || {
                view.workspace
                    .as_ref()
                    .and_then(|path| path.file_name())
                    .map_or(String::new(), |name| name.to_string_lossy().into_owned())
            },
            |repo| repo.name().into(),
        );
        self.description.clear();
        self.remote = "origin".into();
        self.flag = true;
        self.form = Some(Form::Publish { local, resume });
    }

    fn dialog(&mut self, ui: &mut Ui, view: &View, l: Language, p: Palette) {
        let Some(form) = self.form.take() else {
            return;
        };
        let title = match &form {
            Form::Clone(_) => l.local("Clone from GitHub", "Clonar do GitHub"),
            Form::Publish { local: true, .. } => {
                l.local("Publish repository", "Publicar repositório")
            }
            Form::Publish { local: false, .. } => l.local("Create repository", "Criar repositório"),
            Form::Fork(_) => l.local("Create fork", "Criar fork"),
            Form::Pull => l.local("Create pull request", "Criar pull request"),
            Form::Checkout(..) => l.local("Check out pull request", "Abrir branch do pull request"),
            Form::Disconnect(_) => l.local("Disconnect account", "Desconectar conta"),
            Form::Sync(..) => l.local("Sync fork", "Atualizar fork"),
        };
        let dialog = c::modal(ui, "github-review", p, title, |ui| {
            ScrollArea::vertical()
                .max_height((ui.ctx().content_rect().height() - 125.0).max(50.0))
                .show(ui, |ui| {
                    self.form_fields(ui, view, &form, l);
                });
            ui.horizontal_wrapped(|ui| {
                let confirm = c::primary(ui, title, p).clicked();
                let cancel = ui.button(l.text(Text::Cancel)).clicked();
                (confirm, cancel)
            })
            .inner
        });
        if dialog.inner.0 {
            if let Some(action) = self.form_action(view, &form) {
                self.send(action);
            } else {
                self.form = Some(form);
            }
        } else if !dialog.inner.1 && !dialog.should_close() {
            self.form = Some(form);
        }
    }

    #[allow(clippy::too_many_lines)]
    fn form_fields(&mut self, ui: &mut Ui, view: &View, form: &Form, l: Language) {
        match form {
            Form::Clone(repo) => {
                ui.strong(repo.key.slug());
                if field(
                    ui,
                    l.local("Destination parent directory", "Diretório pai do destino"),
                    &mut self.path,
                ) {
                    self.chosen = None;
                }
                if ui
                    .button(l.local("Browse folders…", "Procurar pastas…"))
                    .clicked()
                {
                    self.action = Some(RepositoryAction::BrowseDestination);
                }
                field(
                    ui,
                    l.local("New directory name", "Nome do novo diretório"),
                    &mut self.name,
                );
                ui.checkbox(
                    &mut self.flag,
                    l.local("Use SSH instead of HTTPS", "Usar SSH em vez de HTTPS"),
                );
                if let Some(parent) = &repo.parent {
                    ui.label(format!("upstream · {}", parent.slug()));
                }
            }
            Form::Publish { local, resume } => {
                if resume.is_none() {
                    self.owner(ui, view, l);
                    field(
                        ui,
                        l.local("Repository name", "Nome do repositório"),
                        &mut self.name,
                    );
                    field(
                        ui,
                        l.local("Description", "Descrição"),
                        &mut self.description,
                    );
                    ui.checkbox(
                        &mut self.flag,
                        l.local("Private repository", "Repositório privado"),
                    );
                } else {
                    ui.label(format!("{}/{}", self.owner, self.name));
                }
                if *local {
                    ui.label(view.workspace_branch.as_deref().unwrap_or("—"));
                    field(
                        ui,
                        l.local("Local remote name", "Nome do remote local"),
                        &mut self.remote,
                    );
                }
            }
            Form::Fork(repo) => {
                ui.strong(repo.slug());
                self.owner(ui, view, l);
                field(ui, l.local("Fork name", "Nome do fork"), &mut self.name);
            }
            Form::Pull => {
                field(
                    ui,
                    l.local("Destination owner", "Proprietário de destino"),
                    &mut self.owner,
                );
                field(
                    ui,
                    l.local("Destination repository", "Repositório de destino"),
                    &mut self.remote,
                );
                field(ui, l.local("Base branch", "Branch base"), &mut self.base);
                field(
                    ui,
                    l.local("Source owner", "Proprietário de origem"),
                    &mut self.source_owner,
                );
                field(
                    ui,
                    l.local("Source repository", "Repositório de origem"),
                    &mut self.source_repo,
                );
                field(
                    ui,
                    l.local("Head branch", "Branch de origem"),
                    &mut self.head,
                );
                field(ui, l.local("Title", "Título"), &mut self.name);
                let text = ui.label(l.local("Description", "Descrição"));
                ui.add(TextEdit::multiline(&mut self.description).desired_rows(3))
                    .labelled_by(text.id);
                ui.checkbox(
                    &mut self.flag,
                    l.local("Draft pull request", "Pull request em rascunho"),
                );
            }
            Form::Checkout(repo, number) => {
                ui.strong(format!("{} #{number}", repo.slug()));
                field(
                    ui,
                    l.local("New local branch", "Nova branch local"),
                    &mut self.name,
                );
            }
            Form::Disconnect(account) => {
                if let Some(account) = view.accounts.iter().find(|item| item.id == *account) {
                    ui.strong(&account.profile.login);
                }
                ui.label(l.local(
                    "Remove the session from this device. Manage remote authorization on GitHub.",
                    "Remova a sessão deste dispositivo. Gerencie a autorização remota no GitHub.",
                ));
            }
            Form::Sync(repo, branch) => {
                ui.label(format!("{} · {branch}", repo.slug()));
            }
        }
    }

    fn owner(&mut self, ui: &mut Ui, view: &View, l: Language) {
        if Self::is_browser(view) {
            field(
                ui,
                l.local("Account or organization", "Conta ou organização"),
                &mut self.owner,
            );
            return;
        }
        let login = Self::active_login(view);
        ComboBox::from_label(l.local("Owner", "Proprietário"))
            .selected_text(&self.owner)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.owner, login.into(), login);
                for org in view.organizations.iter().filter(|org| org.login != login) {
                    ui.selectable_value(&mut self.owner, org.login.clone(), &org.login);
                }
            });
    }

    fn form_action(&self, view: &View, form: &Form) -> Option<Action> {
        let name = self.name.trim();
        Some(match form {
            Form::Clone(repo) => {
                let component = std::path::Path::new(name);
                if name.is_empty()
                    || name.contains(['/', '\\'])
                    || !matches!(
                        component.components().next(),
                        Some(std::path::Component::Normal(_))
                    )
                {
                    return None;
                }
                let parent = self
                    .chosen
                    .clone()
                    .unwrap_or_else(|| PathBuf::from(self.path.trim()));
                if !parent.is_absolute() {
                    return None;
                }
                Action::Clone {
                    repository: repo.key.clone(),
                    parent: repo.parent.clone(),
                    destination: parent.join(component),
                    ssh: self.flag,
                }
            }
            Form::Publish { local, resume } => {
                let plan = if *local {
                    Some(Publish {
                        workspace: view.workspace.clone()?,
                        branch: view.workspace_branch.clone()?,
                        remote: self.remote.trim().into(),
                    })
                } else {
                    None
                };
                if let Some(repo) = resume {
                    Action::ResumePublish {
                        repository: repo.clone(),
                        publish: plan?,
                    }
                } else {
                    RepositoryKey::new(&self.owner, name)?;
                    Action::Create(CreateRepository {
                        owner: self.owner.clone(),
                        name: name.into(),
                        description: self.description.clone(),
                        private: self.flag,
                        local: plan,
                    })
                }
            }
            Form::Fork(repo) => {
                RepositoryKey::new(&self.owner, name)?;
                Action::Fork {
                    repository: repo.clone(),
                    organization: (self.owner != Self::active_login(view))
                        .then(|| self.owner.clone()),
                    name: name.into(),
                }
            }
            Form::Pull => Action::CreatePull(CreatePull {
                destination: RepositoryKey::new(self.owner.trim(), self.remote.trim())?,
                source: RepositoryKey::new(self.source_owner.trim(), self.source_repo.trim())?,
                base: self.base.trim().into(),
                head: self.head.trim().into(),
                title: name.into(),
                body: self.description.clone(),
                draft: self.flag,
            }),
            Form::Checkout(repo, number) => Action::CheckoutPull {
                repository: repo.clone(),
                number: *number,
                local: name.into(),
            },
            Form::Disconnect(id) => Action::Disconnect(*id),
            Form::Sync(repo, branch) => Action::SyncFork {
                repository: repo.clone(),
                branch: branch.clone(),
            },
        })
    }
}

fn field(ui: &mut Ui, label: &str, value: &mut String) -> bool {
    let label = ui.label(label);
    ui.add(TextEdit::singleline(value).desired_width(f32::INFINITY))
        .labelled_by(label.id)
        .changed()
}

fn visibility(value: Visibility, l: Language) -> &'static str {
    match value {
        Visibility::All => l.local("All", "Todas"),
        Visibility::Public => l.local("Public", "Público"),
        Visibility::Private => l.local("Private", "Privado"),
    }
}

fn pull_filter(value: PullFilter, l: Language) -> &'static str {
    match value {
        PullFilter::Open => l.local("Open", "Abertos"),
        PullFilter::Closed => l.local("Closed", "Fechados"),
        PullFilter::All => l.local("All", "Todos"),
    }
}

#[allow(clippy::too_many_lines)]
fn notice(notice: Notice, l: Language) -> String {
    use Notice as N;
    if let N::RateLimited(seconds) = notice {
        return format!(
            "{} · {seconds}s",
            l.local("GitHub rate limit", "Limite de consultas do GitHub")
        );
    }
    match notice {
        N::InvalidClient => l.local(
            "Check the gitRUST GitHub App Client ID and Device Flow setting.",
            "Confira o Client ID do GitHub App e a opção Device Flow.",
        ),
        N::OAuthConfiguration => l.local(
            "Browser sign-in is unavailable until the maintainer configures the gitRUST OAuth App.",
            "O login pelo navegador requer que o mantenedor configure o OAuth App do gitRUST.",
        ),
        N::OAuthHandler => l.local(
            "Could not open browser authorization or register the gitRUST callback. Check the desktop session and try again.",
            "Não foi possível abrir a autorização ou registrar o retorno ao gitRUST. Confira a sessão gráfica e tente novamente.",
        ),
        N::SignedIn => l.local("GitHub account connected.", "Conta do GitHub vinculada."),
        N::InsufficientScope => l.local(
            "GitHub did not grant profile and repository access. Authorize those permissions and try again.",
            "O GitHub não concedeu acesso ao perfil e aos repositórios. Autorize essas permissões e tente novamente.",
        ),
        N::WrongAccount => l.local(
            "GitHub authorized a different account. Select the account you are reconnecting.",
            "O GitHub autorizou outra conta. Selecione a conta que você quer reconectar.",
        ),
        N::SsoRequired => l.local(
            "This organization requires SSO. Complete SSO on GitHub, then reconnect and retry.",
            "Esta organização exige SSO. Conclua o SSO no GitHub, reconecte a conta e tente novamente.",
        ),
        N::InvalidInput => l.local(
            "Check the supplied repository, branch or account.",
            "Confira o repositório, a branch ou a conta informada.",
        ),
        N::Offline => l.local(
            "GitHub connection failed.",
            "Falha na conexão com o GitHub.",
        ),
        N::Timeout => l.local(
            "GitHub request timed out.",
            "A consulta ao GitHub excedeu o tempo limite.",
        ),
        N::Cancelled => l.local("Cancelled.", "Cancelado."),
        N::Expired => l.local(
            "The device code expired. Sign in again.",
            "O código de dispositivo expirou. Entre novamente.",
        ),
        N::Denied => l.local(
            "GitHub authorization was declined.",
            "A autorização no GitHub foi recusada.",
        ),
        N::Reconnect => l.local("Reconnect this account.", "Reconecte esta conta."),
        N::Forbidden => l.local(
            "GitHub denied access. Check your account permissions and the organization’s app policy.",
            "O GitHub negou o acesso. Confira as permissões da conta e a política de aplicativos da organização.",
        ),
        N::Unavailable | N::RateLimited(_) => {
            l.local("GitHub is unavailable.", "GitHub indisponível.")
        }
        N::InvalidResponse => l.local(
            "GitHub returned an invalid response.",
            "O GitHub retornou uma resposta inválida.",
        ),
        N::Redirect => l.local(
            "GitHub returned an unexpected redirect.",
            "O GitHub retornou um redirecionamento inesperado.",
        ),
        N::TemporarySession => l.local(
            "The credential vault is unavailable. This session is in memory.",
            "O cofre de credenciais está indisponível. Esta sessão fica em memória.",
        ),
        N::StorageUnavailable => l.local(
            "The credential store could not be accessed.",
            "Não foi possível acessar o armazenamento de credenciais.",
        ),
        N::NoAccess => l.local(
            "GitHub could not expose this resource. Check repository access and app authorization.",
            "O GitHub não disponibilizou este recurso. Confira o acesso ao repositório e a autorização do aplicativo.",
        ),
        N::Created => l.local(
            "The remote request completed.",
            "A operação remota foi concluída.",
        ),
        N::AwaitingInstallation => l.local(
            "Grant access to the created repository, then resume publication.",
            "Conceda acesso ao repositório criado e retome a publicação.",
        ),
        N::ForkPending => l.local(
            "GitHub accepted the fork. Check its availability before cloning.",
            "O GitHub aceitou o fork. Confira a disponibilidade antes de clonar.",
        ),
        N::LocalGitFailed => l.local(
            "Local Git action failed. Review the workspace and retry if appropriate.",
            "A ação do Git local falhou. Confira o workspace antes de tentar novamente.",
        ),
        N::LocalGitComplete => l.local("Git operation completed.", "A operação Git foi concluída."),
        N::UnknownMutation => l.local(
            "GitHub outcome is uncertain. Check the destination before retrying.",
            "O resultado no GitHub é incerto. Confira o destino antes de tentar novamente.",
        ),
        N::SignedOut => l.local(
            "Session removed from this device.",
            "Sessão removida deste dispositivo.",
        ),
    }
    .into()
}
