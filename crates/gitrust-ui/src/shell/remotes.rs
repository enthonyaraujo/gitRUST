use std::{collections::BTreeSet, fmt, path::PathBuf};

use egui::{Button, ComboBox, RichText, ScrollArea, TextEdit, Ui};
use gitrust_core::{
    remotes::{
        Auth, Credential, Integration, RebaseState, Remote, RemoteAction as Action, RemoteIntent,
        RemoteNotice, RemotesView,
    },
    workspaces::{RepositoryAction, TransferPhase, TransferProgress},
};

use crate::{
    components as c,
    i18n::{Language, Text},
    theme::Palette,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum CredentialChoice {
    #[default]
    Anonymous,
    Account,
    Memory,
    SecretService,
    LinkedAccount,
}

#[derive(Default)]
pub(super) struct AuthFields {
    credential: CredentialChoice,
    user: String,
    password: String,
    ca: String,
    known_hosts: String,
    identity: String,
}

impl fmt::Debug for AuthFields {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AuthFields(<private>)")
    }
}

impl AuthFields {
    pub(super) fn show(&mut self, ui: &mut Ui, ssh: bool, linked: bool, l: Language) {
        if ssh {
            ui.label(l.local(
                "OpenSSH verifies the host. Unlock protected keys in your SSH agent.",
                "O OpenSSH verifica o host. Desbloqueie chaves protegidas no agente SSH.",
            ));
            ui.collapsing(
                l.local(
                    "SSH trust and identity files",
                    "Arquivos de confiança e identidade SSH",
                ),
                |ui| {
                    field(
                        ui,
                        l.local("Known hosts file", "Arquivo known_hosts"),
                        &mut self.known_hosts,
                        false,
                    );
                    field(
                        ui,
                        l.local("SSH identity file", "Arquivo de identidade SSH"),
                        &mut self.identity,
                        false,
                    );
                },
            );
            return;
        }
        let label = l.local("HTTPS authentication", "Autenticação HTTPS");
        let previous = self.credential;
        ComboBox::from_label(label)
            .selected_text(credential_label(self.credential, l))
            .show_ui(ui, |ui| {
                for choice in [
                    CredentialChoice::Anonymous,
                    CredentialChoice::Account,
                    CredentialChoice::Memory,
                    CredentialChoice::SecretService,
                    CredentialChoice::LinkedAccount,
                ]
                .into_iter()
                .filter(|choice| linked || *choice != CredentialChoice::LinkedAccount)
                {
                    if ui
                        .selectable_value(&mut self.credential, choice, credential_label(choice, l))
                        .clicked()
                    {
                        ui.close();
                    }
                }
            });
        if self.credential == CredentialChoice::Account {
            field(ui, l.local("Username", "Usuário"), &mut self.user, false);
            field(
                ui,
                l.local("Password or token", "Senha ou token"),
                &mut self.password,
                true,
            );
            if previous != self.credential {
                ui.scroll_to_cursor(Some(egui::Align::Center));
            }
        }
        ui.collapsing(l.local("TLS trust", "Confiança TLS"), |ui| {
            field(
                ui,
                l.local(
                    "Trusted CA bundle file",
                    "Arquivo de autoridades certificadoras",
                ),
                &mut self.ca,
                false,
            );
        });
    }

    pub(super) fn take(&mut self, ssh: bool) -> Auth {
        let path = |value: &str| (!value.is_empty()).then(|| PathBuf::from(value));
        if ssh {
            self.password.clear();
            return Auth {
                known_hosts: path(&self.known_hosts),
                identity: path(&self.identity),
                ..Auth::default()
            };
        }
        let password = std::mem::take(&mut self.password);
        Auth {
            credential: match self.credential {
                CredentialChoice::Anonymous => Credential::Anonymous,
                CredentialChoice::Account => Credential::Password {
                    user: self.user.clone(),
                    secret: password,
                },
                CredentialChoice::Memory => Credential::MemoryCache,
                CredentialChoice::SecretService => Credential::SecretService,
                CredentialChoice::LinkedAccount => Credential::LinkedAccount,
            },
            ca: path(&self.ca),
            ..Auth::default()
        }
    }
}

fn credential_label(choice: CredentialChoice, l: Language) -> &'static str {
    match choice {
        CredentialChoice::LinkedAccount => {
            l.local("Linked GitHub account", "Conta GitHub vinculada")
        }
        CredentialChoice::Anonymous => l.local("Anonymous", "Anônima"),
        CredentialChoice::Account => l.local(
            "Password or token for this operation",
            "Senha ou token para esta operação",
        ),
        CredentialChoice::Memory => l.local("Git memory cache", "Cache Git em memória"),
        CredentialChoice::SecretService => {
            l.local("System Secret Service", "Serviço de segredos do sistema")
        }
    }
}

pub(super) fn ssh_url(value: &str) -> bool {
    value.starts_with("ssh://") || (!value.contains("://") && value.contains('@'))
}

#[derive(Debug)]
pub(super) enum Dialog {
    Add,
    Edit(Remote),
    Rename(Remote),
    Remove(Remote),
    Upstream(Remote),
    Fetch(Vec<Remote>),
    Pull(Remote),
    Push(Remote),
    Abort,
}

#[derive(Default)]
pub(super) struct RemoteUi {
    selected: Option<String>,
    batch: BTreeSet<String>,
    dialog: Option<(u64, Dialog)>,
    name: String,
    fetch: String,
    push: String,
    branch: String,
    track: bool,
    strategy: Integration,
    auth: AuthFields,
}

impl fmt::Debug for RemoteUi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RemoteUi(<private remote form>)")
    }
}

pub(super) const REMOTE_POPUP: &str = "desktop-remote-settings";

impl RemoteUi {
    pub(super) fn selected<'a>(&self, view: &'a RemotesView) -> Option<&'a Remote> {
        self.selected
            .as_ref()
            .and_then(|name| view.entries.iter().find(|remote| remote.name == *name))
            .or_else(|| Self::default_remote(view))
    }

    fn default_remote(view: &RemotesView) -> Option<&Remote> {
        view.upstream
            .as_ref()
            .and_then(|(name, _)| view.entries.iter().find(|remote| remote.name == *name))
            .or_else(|| view.entries.iter().find(|remote| remote.name == "origin"))
            .or_else(|| view.entries.first())
    }

    pub(super) fn popup(
        &mut self,
        ui: &mut Ui,
        view: &RemotesView,
        writable: bool,
        l: Language,
        p: Palette,
    ) {
        ui.set_min_width(295.0);
        ui.strong(l.local("Configured remotes", "Remotes configurados"));
        if let Some(remote) = Self::default_remote(view) {
            ui.label(format!(
                "{} · {}",
                l.local("Default remote", "Remote padrão"),
                remote.name
            ));
        }
        ScrollArea::vertical().max_height(168.0).show(ui, |ui| {
            for remote in view.entries.iter() {
                ui.horizontal(|ui| {
                    let mut checked = self.batch.contains(&remote.name);
                    if ui
                        .checkbox(&mut checked, "")
                        .accessible_name(format!(
                            "{} {}",
                            l.local("Select fetch source", "Selecionar origem de fetch"),
                            remote.name
                        ))
                        .changed()
                    {
                        if checked {
                            self.batch.insert(remote.name.clone());
                        } else {
                            self.batch.remove(&remote.name);
                        }
                    }
                    if c::menu_choice(ui, &remote.name, self.selected(view) == Some(remote), p)
                        .clicked()
                    {
                        self.selected = Some(remote.name.clone());
                    }
                });
            }
        });
        let selected = self.selected(view).cloned();
        if let Some(remote) = &selected {
            endpoint_line(
                ui,
                l.local("Fetch URL", "URL de fetch"),
                remote.fetch.as_deref(),
                l,
            );
            endpoint_line(
                ui,
                l.local("Push URL", "URL de push"),
                remote.push.as_deref(),
                l,
            );
            if let Some((_, time)) = view
                .last_fetch
                .iter()
                .find(|(name, _)| name == &remote.name)
            {
                ui.label(format!(
                    "{} · {}",
                    l.local(
                        "Last successful fetch (UTC)",
                        "Último fetch bem-sucedido (UTC)"
                    ),
                    utc(*time)
                ));
            }
        }
        Self::tracking(ui, view, l);
        ui.add_enabled_ui(!view.busy, |ui| {
            if let Some(remote) = selected {
                self.actions(ui, view, &remote, writable, l);
            }
            let batch = view
                .entries
                .iter()
                .filter(|remote| remote.fetch.is_some() && self.batch.contains(&remote.name))
                .cloned()
                .collect::<Vec<_>>();
            if ui
                .add_enabled(
                    !batch.is_empty(),
                    Button::new(l.local("Fetch checked remotes", "Buscar remotes marcados")),
                )
                .clicked()
            {
                self.open(view, Dialog::Fetch(batch));
                ui.close();
            }
            if ui
                .add_enabled(
                    !view.rebase.in_progress(),
                    Button::new(l.local("Add remote", "Adicionar remote")),
                )
                .clicked()
            {
                self.open(view, Dialog::Add);
                ui.close();
            }
        });
    }

    fn tracking(ui: &mut Ui, view: &RemotesView, l: Language) {
        if let Some((name, branch)) = &view.upstream {
            ui.label(format!("Upstream · {name}/{branch}"));
            if let Some((ahead, behind)) = view.ahead_behind {
                ui.label(format!(
                    "{} · {}",
                    l.local("Local tracking data", "Referências locais"),
                    divergence(ahead, behind, l)
                ));
            }
        } else {
            ui.label(l.local("No upstream", "Sem upstream"));
        }
    }

    fn actions(
        &mut self,
        ui: &mut Ui,
        view: &RemotesView,
        remote: &Remote,
        writable: bool,
        l: Language,
    ) {
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    remote.fetch.is_some(),
                    Button::new(format!("Fetch {}", remote.name)),
                )
                .clicked()
            {
                self.open(view, Dialog::Fetch(vec![remote.clone()]));
                ui.close();
            }
            let tracked = view
                .upstream
                .as_ref()
                .is_some_and(|(upstream, _)| upstream == &remote.name);
            if ui
                .add_enabled(
                    writable && !view.rebase.in_progress() && tracked && remote.fetch.is_some(),
                    Button::new(l.text(Text::Pull)),
                )
                .clicked()
            {
                self.open(view, Dialog::Pull(remote.clone()));
                ui.close();
            }
            let label = if tracked {
                l.text(Text::Push)
            } else {
                l.local("Publish branch", "Publicar branch")
            };
            if ui
                .add_enabled(
                    writable
                        && !view.rebase.in_progress()
                        && view.current.is_some()
                        && remote.push.is_some(),
                    Button::new(label),
                )
                .clicked()
            {
                self.open(view, Dialog::Push(remote.clone()));
                ui.close();
            }
        });
        ui.horizontal_wrapped(|ui| {
            for (label, mode) in [
                (
                    l.local("Edit remote", "Editar remote"),
                    Dialog::Edit(remote.clone()),
                ),
                (
                    l.local("Rename remote", "Renomear remote"),
                    Dialog::Rename(remote.clone()),
                ),
                (
                    l.local("Remove remote", "Remover remote"),
                    Dialog::Remove(remote.clone()),
                ),
            ] {
                if ui
                    .add_enabled(!view.rebase.in_progress(), Button::new(label))
                    .clicked()
                {
                    self.open(view, mode);
                    ui.close();
                }
            }
            if ui
                .add_enabled(
                    writable && !view.rebase.in_progress() && view.current.is_some(),
                    Button::new(l.local("Set upstream", "Configurar upstream")),
                )
                .clicked()
            {
                self.open(view, Dialog::Upstream(remote.clone()));
                ui.close();
            }
        });
    }

    pub(super) fn status(
        &mut self,
        ui: &mut Ui,
        view: &RemotesView,
        progress: Option<TransferProgress>,
        l: Language,
        p: Palette,
    ) -> Option<RepositoryAction> {
        if view.busy {
            let text = progress.map_or(
                l.local(
                    "Remote operation in progress…",
                    "Operação remota em andamento…",
                ),
                |progress| match progress.phase {
                    TransferPhase::Counting => l.local("Counting objects", "Contando objetos"),
                    TransferPhase::Compressing => {
                        l.local("Compressing objects", "Compactando objetos")
                    }
                    TransferPhase::Receiving => {
                        l.local("Transferring objects", "Transferindo objetos")
                    }
                    TransferPhase::Resolving => l.local("Resolving deltas", "Resolvendo deltas"),
                },
            );
            c::loading(
                ui,
                text,
                progress.map(|progress| f32::from(progress.percent) / 100.0),
            );
            return ui
                .button(l.text(Text::Cancel))
                .clicked()
                .then_some(RepositoryAction::Cancel);
        }
        if let Some(notice) = view
            .notice
            .filter(|notice| *notice != RemoteNotice::Fetched)
        {
            ui.colored_label(p.muted, notice_text(notice, l));
        }
        if view.rebase.in_progress() {
            ui.colored_label(
                p.warning,
                if view.rebase == RebaseState::Recoverable {
                    l.local(
                        "Rebase in progress. Resolve and stage conflicts in Changes.",
                        "Rebase em andamento. Resolva e prepare os conflitos em Alterações.",
                    )
                } else {
                    l.local(
                        "Rebase in progress. Review its state with the Git CLI.",
                        "Rebase em andamento. Revise o estado pela CLI do Git.",
                    )
                },
            );
            return ui
                .horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(
                            view.rebase == RebaseState::Recoverable,
                            Button::new(l.local("Abort rebase", "Abortar rebase")),
                        )
                        .clicked()
                    {
                        self.open(view, Dialog::Abort);
                    }
                    ui.add_enabled(
                        view.rebase == RebaseState::Recoverable,
                        Button::new(l.local("Continue rebase", "Continuar rebase")),
                    )
                    .clicked()
                    .then_some(send(view, Action::ContinueRebase))
                })
                .inner;
        }
        None
    }

    pub(super) fn open(&mut self, view: &RemotesView, dialog: Dialog) {
        self.name.clear();
        self.fetch.clear();
        self.push.clear();
        if let Dialog::Edit(remote) = &dialog {
            self.fetch = remote.fetch.clone().unwrap_or_default();
            if remote.push != remote.fetch {
                self.push = remote.push.clone().unwrap_or_default();
            }
        }
        self.branch = view
            .upstream
            .as_ref()
            .map(|(_, branch)| branch)
            .or(view.current.as_ref())
            .cloned()
            .unwrap_or_default();
        self.strategy = view.preference;
        self.auth = AuthFields::default();
        self.track = false;
        self.dialog = Some((view.version, dialog));
    }

    pub(super) fn dialog(
        &mut self,
        ui: &Ui,
        view: &RemotesView,
        linked: bool,
        l: Language,
        p: Palette,
    ) -> Option<RepositoryAction> {
        let (version, dialog) = self.dialog.take()?;
        if version != view.version {
            return None;
        }
        let label = dialog_title(&dialog, l);
        let modal = c::modal(ui, "remote-confirmation", p, label, |ui| {
            ScrollArea::vertical()
                .max_height((ui.ctx().content_rect().height() - 130.0).max(70.0))
                .show(ui, |ui| {
                    self.form(ui, view, &dialog, linked, l, p);
                });
            ui.horizontal_wrapped(|ui| {
                let submit = ui
                    .add_enabled_ui(!view.busy, |ui| c::primary(ui, label, p))
                    .inner
                    .clicked();
                let cancel = ui.button(l.text(Text::Cancel)).clicked();
                (submit, cancel)
            })
            .inner
        });
        if modal.inner.0 {
            return Some(send(view, self.action(dialog)));
        }
        if !modal.inner.1 && !modal.should_close() {
            self.dialog = Some((version, dialog));
        } else {
            self.auth = AuthFields::default();
        }
        None
    }

    fn form(
        &mut self,
        ui: &mut Ui,
        view: &RemotesView,
        dialog: &Dialog,
        linked: bool,
        l: Language,
        p: Palette,
    ) {
        match dialog {
            Dialog::Add | Dialog::Edit(_) | Dialog::Rename(_) => {
                if !matches!(dialog, Dialog::Edit(_)) {
                    field(
                        ui,
                        l.local("Remote name", "Nome do remote"),
                        &mut self.name,
                        false,
                    );
                }
                if !matches!(dialog, Dialog::Rename(_)) {
                    field(
                        ui,
                        l.local("Fetch URL", "URL de fetch"),
                        &mut self.fetch,
                        false,
                    );
                    field(
                        ui,
                        l.local("Push URL (optional)", "URL de push (opcional)"),
                        &mut self.push,
                        false,
                    );
                }
            }
            Dialog::Remove(remote) => {
                ui.label(format!(
                    "{} {}?",
                    l.local("Remove remote", "Remover remote"),
                    remote.name
                ));
            }
            Dialog::Upstream(remote) => {
                ui.label(&remote.name);
                field(
                    ui,
                    l.local("Remote branch", "Branch remota"),
                    &mut self.branch,
                    false,
                );
            }
            Dialog::Fetch(remotes) => {
                for remote in remotes {
                    endpoint_line(ui, &remote.name, remote.fetch.as_deref(), l);
                }
                let endpoints = || remotes.iter().filter_map(|remote| remote.fetch.as_deref());
                if endpoints().any(ssh_url) {
                    self.auth.show(ui, true, false, l);
                }
                if endpoints().any(|url| !ssh_url(url)) {
                    self.auth.show(
                        ui,
                        false,
                        linked && remotes.len() == 1 && endpoints().any(github_https),
                        l,
                    );
                }
            }
            Dialog::Pull(remote) => {
                endpoint_line(ui, &remote.name, remote.fetch.as_deref(), l);
                if let Some(current) = &view.current {
                    ui.label(format!(
                        "{} · {current}",
                        l.local("Local branch", "Branch local")
                    ));
                }
                Self::tracking(ui, view, l);
                if view.worktree_dirty {
                    ui.colored_label(p.warning, notice_text(RemoteNotice::Dirty, l));
                } else {
                    ui.label(l.local("Clean worktree", "Árvore de trabalho limpa"));
                }
                self.integration_options(ui, l, p);
                self.auth.show(
                    ui,
                    remote.fetch.as_deref().is_some_and(ssh_url),
                    linked && remote.fetch.as_deref().is_some_and(github_https),
                    l,
                );
            }
            Dialog::Push(remote) => self.push_fields(ui, view, remote, linked, l),
            Dialog::Abort => {
                ui.label(l.local("Restore the pre-rebase state. New work blocks abort.", "Restaurar o estado anterior ao rebase. Alterações posteriores impedem o aborto."));
            }
        }
    }

    fn push_fields(
        &mut self,
        ui: &mut Ui,
        view: &RemotesView,
        remote: &Remote,
        linked: bool,
        l: Language,
    ) {
        endpoint_line(ui, &remote.name, remote.push.as_deref(), l);
        if let Some(current) = &view.current {
            ui.label(format!(
                "{} · {current}",
                l.local("Local branch", "Branch local")
            ));
        }
        Self::tracking(ui, view, l);
        field(
            ui,
            l.local("Remote branch", "Branch remota"),
            &mut self.branch,
            false,
        );
        ui.checkbox(
            &mut self.track,
            l.local(
                "Set as upstream after push",
                "Configurar como upstream após push",
            ),
        );
        self.auth.show(
            ui,
            remote.push.as_deref().is_some_and(ssh_url),
            linked && remote.push.as_deref().is_some_and(github_https),
            l,
        );
    }

    fn integration_options(&mut self, ui: &mut Ui, l: Language, p: Palette) {
        c::tabs(
            ui,
            &mut self.strategy,
            &[
                (Integration::FastForward, "Fast-forward"),
                (Integration::Merge, "Merge"),
                (Integration::Rebase, "Rebase"),
            ],
            p,
        );
        if self.strategy == Integration::Rebase {
            ui.colored_label(
                p.warning,
                l.local(
                    "Rebase rewrites local commits and requires a clean worktree.",
                    "O rebase reescreve commits locais e exige árvore de trabalho limpa.",
                ),
            );
        }
    }

    fn action(&mut self, dialog: Dialog) -> Action {
        let push = (!self.push.trim().is_empty()).then(|| self.push.trim().to_owned());
        match dialog {
            Dialog::Add => Action::Add(
                self.name.trim().to_owned(),
                self.fetch.trim().to_owned(),
                push,
            ),
            Dialog::Edit(remote) => Action::Edit(remote.name, self.fetch.trim().to_owned(), push),
            Dialog::Rename(remote) => Action::Rename(remote.name, self.name.trim().to_owned()),
            Dialog::Remove(remote) => Action::Remove(remote.name),
            Dialog::Upstream(remote) => {
                Action::Upstream(remote.name, self.branch.trim().to_owned())
            }
            Dialog::Fetch(remotes) => {
                let https = remotes
                    .iter()
                    .filter_map(|remote| remote.fetch.as_deref())
                    .any(|url| !ssh_url(url));
                let mut auth = if https {
                    self.auth.take(false)
                } else {
                    Auth::default()
                };
                let ssh = self.auth.take(true);
                auth.known_hosts = ssh.known_hosts;
                auth.identity = ssh.identity;
                Action::Fetch(
                    remotes.into_iter().map(|remote| remote.name).collect(),
                    auth,
                )
            }
            Dialog::Pull(remote) => Action::Pull(
                remote.name,
                self.strategy,
                self.auth.take(remote.fetch.as_deref().is_some_and(ssh_url)),
            ),
            Dialog::Push(remote) => Action::Push {
                remote: remote.name,
                branch: self.branch.trim().to_owned(),
                track: self.track,
                auth: self.auth.take(remote.push.as_deref().is_some_and(ssh_url)),
            },
            Dialog::Abort => Action::AbortRebase,
        }
    }
}

fn github_https(endpoint: &str) -> bool {
    gitrust_core::github::RepositoryKey::from_https(endpoint).is_some()
}

fn send(view: &RemotesView, action: Action) -> RepositoryAction {
    RepositoryAction::Remote(RemoteIntent {
        version: view.version,
        action,
    })
}

fn field(ui: &mut Ui, label: &str, text: &mut String, secret: bool) -> egui::Response {
    let label = ui.label(label);
    ui.add(
        TextEdit::singleline(text)
            .desired_width(f32::INFINITY)
            .password(secret),
    )
    .labelled_by(label.id)
}

fn endpoint_line(ui: &mut Ui, label: &str, endpoint: Option<&str>, l: Language) {
    ui.label(
        RichText::new(format!(
            "{label} · {}",
            endpoint.unwrap_or(l.text(Text::Unavailable))
        ))
        .small(),
    );
}

fn divergence(ahead: u64, behind: u64, l: Language) -> String {
    let value = match (ahead > 0, behind > 0) {
        (true, true) => l.local("Diverged", "Divergente"),
        (true, false) => l.local("Ahead", "À frente"),
        (false, true) => l.local("Behind", "Atrás"),
        (false, false) => l.local("Matches cached upstream", "Igual ao upstream local"),
    };
    format!("{value} · ↑ {ahead} ↓ {behind}")
}

pub(super) fn last_fetch_text(ctx: &egui::Context, seconds: u64, l: Language) -> String {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    let age = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .saturating_sub(seconds);
    ctx.request_repaint_after(Duration::from_secs(30));
    let suffix = match age {
        0..60 => l.local("just now", "agora mesmo").to_owned(),
        60..120 => l.local("1 minute ago", "há 1 minuto").to_owned(),
        120..3600 => match l {
            Language::English => format!("{} minutes ago", age / 60),
            Language::Portuguese => format!("há {} minutos", age / 60),
        },
        3600..86400 => match l {
            Language::English => format!("{} h ago", age / 3600),
            Language::Portuguese => format!("há {} h", age / 3600),
        },
        _ => format!("{} UTC", utc(seconds)),
    };
    format!("{} {suffix}", l.local("Last fetch:", "Última busca:"))
}

fn utc(seconds: u64) -> String {
    let day = seconds / 86400;
    let shifted = day + 719_468;
    let era = shifted / 146_097;
    let cycle = shifted % 146_097;
    let year = (cycle - cycle / 1460 + cycle / 36_524 - cycle / 146_096) / 365;
    let ordinal = cycle - (365 * year + year / 4 - year / 100);
    let month = (5 * ordinal + 2) / 153;
    let date = ordinal - (153 * month + 2) / 5 + 1;
    let month = if month < 10 { month + 3 } else { month - 9 };
    let year = year + era * 400 + u64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{date:02} {:02}:{:02}",
        (seconds % 86400) / 3600,
        (seconds % 3600) / 60
    )
}

fn dialog_title(dialog: &Dialog, l: Language) -> &'static str {
    match dialog {
        Dialog::Add => l.local("Add remote", "Adicionar remote"),
        Dialog::Edit(_) => l.local("Save remote", "Salvar remote"),
        Dialog::Rename(_) => l.local("Rename remote", "Renomear remote"),
        Dialog::Remove(_) => l.local("Confirm remote removal", "Confirmar remoção do remote"),
        Dialog::Upstream(_) => l.local("Set upstream", "Configurar upstream"),
        Dialog::Fetch(_) => l.local("Confirm fetch", "Confirmar fetch"),
        Dialog::Pull(_) => l.local("Confirm pull", "Confirmar pull"),
        Dialog::Push(_) => l.local("Confirm push", "Confirmar push"),
        Dialog::Abort => l.local("Confirm rebase abort", "Confirmar aborto do rebase"),
    }
}

pub(super) fn notice_text(notice: RemoteNotice, l: Language) -> &'static str {
    use RemoteNotice as N;
    match notice {
        N::Updated => l.local(
            "Remote configuration updated.",
            "Configuração remota atualizada.",
        ),
        N::Fetched => l.local("Fetch completed.", "Fetch concluído."),
        N::Pushed => l.local("Push completed.", "Push concluído."),
        N::PushedWithoutTracking => l.local(
            "Push completed. Upstream could not be configured.",
            "Push concluído. Não foi possível configurar o upstream.",
        ),
        N::UpToDate => l.local(
            "Already up to date for this operation.",
            "Já atualizado para esta operação.",
        ),
        N::Integrated => l.local("Pull integrated.", "Pull integrado."),
        N::MergePending => l.local(
            "Pull paused for merge completion.",
            "Pull pausado para concluir o merge.",
        ),
        N::RebasePending => l.local("Rebase paused.", "Rebase pausado."),
        N::RebaseAborted => l.local("Rebase aborted.", "Rebase abortado."),
        N::NoUpstream => l.local(
            "Set an upstream before pulling.",
            "Configure um upstream antes do pull.",
        ),
        N::Diverged => l.local(
            "Histories diverged. Review merge or rebase.",
            "Os históricos divergiram. Revise merge ou rebase.",
        ),
        N::Dirty => l.local(
            "The operation requires a clean worktree or staged resolutions.",
            "A operação exige uma árvore limpa ou resoluções no index.",
        ),
        N::SignedRebase => l.local(
            "Rebase would replace signed commits. Choose merge to preserve them.",
            "O rebase substituiria commits assinados. Escolha merge para preservá-los.",
        ),
        N::Authentication => l.local(
            "Authentication required. Check credentials or permissions.",
            "Autenticação necessária. Confira credenciais ou permissões.",
        ),
        N::Agent => l.local(
            "SSH key rejected. Check the SSH agent or identity.",
            "Chave SSH rejeitada. Confira o agente SSH ou a identidade.",
        ),
        N::HostKey => l.local(
            "Unknown SSH host key. Verify its fingerprint before adding it to known_hosts.",
            "Chave SSH desconhecida. Confira a impressão digital antes de adicioná-la ao known_hosts.",
        ),
        N::ChangedHostKey => l.local(
            "The SSH host key changed. Verify the new fingerprint with the server administrator.",
            "A chave do host SSH mudou. Confirme a nova impressão digital com o administrador do servidor.",
        ),
        N::Certificate => l.local(
            "TLS certificate verification failed.",
            "A verificação do certificado TLS falhou.",
        ),
        N::Offline => l.local(
            "Could not connect to the remote.",
            "Não foi possível conectar ao remote.",
        ),
        N::NonFastForward => l.local(
            "Push rejected because the remote branch moved.",
            "Push rejeitado porque a branch remota avançou.",
        ),
        N::Rejected => l.local(
            "The server rejected the push. Review access and branch rules.",
            "O servidor rejeitou o push. Confira acesso e regras da branch.",
        ),
        N::Invalid => l.local(
            "Check the remote name, URL or branch.",
            "Confira o nome do remote, a URL ou a branch.",
        ),
        N::Stale => l.local(
            "Repository changed. Review the refreshed state.",
            "O repositório mudou. Confira o estado atualizado.",
        ),
        N::Uncertain => l.local(
            "Push interrupted. The server outcome is unknown.",
            "Push interrompido. O resultado no servidor é desconhecido.",
        ),
        N::Cancelled => l.local("Transfer cancelled.", "Transferência cancelada."),
        N::Timeout => l.local("The operation timed out.", "A operação excedeu o tempo limite."),
        N::Failed => l.local("Remote operation failed.", "A operação remota falhou."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_credential_field_accepts_keyboard_input_and_keeps_it_out_of_debug() {
        let context = egui::Context::default();
        let mut form = AuthFields {
            credential: CredentialChoice::Account,
            user: "fixture".into(),
            ..AuthFields::default()
        };
        let mut first = context.run_ui(egui::RawInput::default(), |ui| {
            field(ui, "Password or token", &mut form.password, true).request_focus();
        });
        first.textures_delta.clear();
        let input = egui::RawInput {
            events: vec![egui::Event::Text("fixture-protected-input".into())],
            ..egui::RawInput::default()
        };
        let mut second = context.run_ui(input, |ui| {
            field(ui, "Password or token", &mut form.password, true);
        });
        second.textures_delta.clear();
        assert_eq!(form.password, "fixture-protected-input");
        assert!(!format!("{form:?}").contains("fixture-protected-input"));
        let auth = form.take(false);
        assert!(matches!(auth.credential, Credential::Password { .. }));
        assert_eq!(form.password, "");
    }
}
