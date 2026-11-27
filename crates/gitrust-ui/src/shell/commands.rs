use egui::{Button, Id, Key, KeyboardShortcut, Modifiers, Ui, ViewportCommand};
use gitrust_core::{
    AppEvent,
    github::{Action as GithubAction, encode_reference},
    navigation::Merge as MergeState,
    workspaces::RepositoryAction,
};

use super::{Desktop, Dialog, FormKind, FrameEvents, Screen, Tab};
use crate::{
    i18n::{Language, Text},
    theme::Palette,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Command {
    NewRepository,
    Open,
    CloneRepository,
    Settings,
    Quit,
    Undo,
    Redo,
    Cut,
    CopyText,
    Paste,
    SelectAll,
    Find,
    Changes,
    History,
    Repositories,
    Branches,
    Summary,
    ShowStash,
    Fullscreen,
    ZoomReset,
    ZoomIn,
    ZoomOut,
    Expand,
    Contract,
    Diagnostics,
    GithubWorkspace,
    Overview,
    Push,
    Pull,
    Fetch,
    Forget,
    Github,
    Terminal,
    FileManager,
    Editor,
    Issue,
    RepositorySettings,
    NewBranch,
    Rename,
    Delete,
    Discard,
    Stash,
    Update,
    Compare,
    Merge,
    Squash,
    Rebase,
    GithubCompare,
    GithubBranch,
    PullPreview,
    CreatePull,
    ReportIssue,
    Guides,
    Shortcuts,
    Logs,
    About,
}

use Command::{
    About, Branches, Changes, CloneRepository, Compare, Contract, CopyText, CreatePull, Cut,
    Delete, Diagnostics, Discard, Editor, Expand, Fetch, FileManager, Find, Forget, Fullscreen,
    Github, GithubBranch, GithubCompare, GithubWorkspace, Guides, History, Issue, Logs, Merge,
    NewBranch, NewRepository, Open, Overview, Paste, Pull, PullPreview, Push, Quit, Rebase, Redo,
    Rename, ReportIssue, Repositories, RepositorySettings, SelectAll, Settings, Shortcuts,
    ShowStash, Squash, Stash, Summary, Terminal, Undo, Update, ZoomIn, ZoomOut, ZoomReset,
};

const MENUS: &[(Text, &[&[Command]])] = &[
    (
        Text::File,
        &[
            &[NewRepository],
            &[Open, CloneRepository],
            &[Settings],
            &[Quit],
        ],
    ),
    (
        Text::Edit,
        &[&[Undo, Redo], &[Cut, CopyText, Paste, SelectAll], &[Find]],
    ),
    (
        Text::View,
        &[
            &[Changes, History, Repositories, Branches],
            &[Summary, ShowStash, Fullscreen],
            &[ZoomReset, ZoomIn, ZoomOut, Expand, Contract],
            &[Diagnostics],
            &[GithubWorkspace, Overview],
        ],
    ),
    (
        Text::Repository,
        &[
            &[Push, Pull, Fetch, Forget],
            &[Github, Terminal, FileManager, Editor],
            &[Issue],
            &[RepositorySettings],
        ],
    ),
    (
        Text::Branch,
        &[
            &[NewBranch, Rename, Delete],
            &[Discard, Stash],
            &[Update, Compare, Merge, Squash, Rebase],
            &[GithubCompare, GithubBranch, PullPreview, CreatePull],
        ],
    ),
    (
        Text::Help,
        &[&[ReportIssue, Guides, Shortcuts, Logs], &[About]],
    ),
];

const CMD: Modifiers = Modifiers::COMMAND;
const SHIFT: Modifiers = CMD.plus(Modifiers::SHIFT);
const ALT: Modifiers = CMD.plus(Modifiers::ALT);
const NONE: Modifiers = Modifiers::NONE;

pub(super) const REPOSITORY_POPUP: &str = "desktop-repositories";
pub(super) const BRANCH_POPUP: &str = "desktop-branches";
const PROJECT_URL: &str = "https://github.com/enthonyaraujo/gitRUST";

impl Command {
    pub(super) fn label(self, l: Language) -> &'static str {
        let (en, pt) = match self {
            NewRepository => ("New repository…", "Novo repositório…"),
            Open => ("Add local repository…", "Adicionar repositório local…"),
            CloneRepository => ("Clone repository…", "Clonar repositório…"),
            Settings => ("Options…", "Opções…"),
            Quit => ("Exit", "Sair"),
            Undo => ("Undo", "Desfazer"),
            Redo => ("Redo", "Refazer"),
            Cut => ("Cut", "Recortar"),
            CopyText => ("Copy", "Copiar"),
            Paste => ("Paste", "Colar"),
            SelectAll => ("Select all", "Selecionar tudo"),
            Find => ("Find", "Buscar"),
            Changes => ("Changes", "Alterações"),
            History => ("History", "Histórico"),
            Repositories => ("Repository list", "Lista de repositórios"),
            Branches => ("Branches list", "Lista de branches"),
            Summary => ("Go to summary", "Ir para o resumo"),
            ShowStash => ("Show stashed changes", "Exibir alterações guardadas"),
            Fullscreen => ("Toggle full screen", "Alternar tela cheia"),
            ZoomReset => ("Reset zoom", "Restaurar zoom"),
            ZoomIn => ("Zoom in", "Aumentar zoom"),
            ZoomOut => ("Zoom out", "Diminuir zoom"),
            Expand => ("Expand sidebar", "Expandir painel lateral"),
            Contract => ("Contract sidebar", "Recolher painel lateral"),
            Diagnostics => ("Toggle diagnostics", "Alternar diagnósticos"),
            GithubWorkspace => ("GitHub workspace", "Área do GitHub"),
            Overview => ("Repository overview", "Visão geral do repositório"),
            Push => ("Push", "Enviar"),
            Pull => ("Pull", "Puxar"),
            Fetch => ("Fetch", "Buscar do remoto"),
            Forget => ("Remove repository…", "Remover repositório…"),
            Github => ("View on GitHub", "Ver no GitHub"),
            Terminal => ("Open in terminal", "Abrir no terminal"),
            FileManager => ("Show in file manager", "Mostrar no gerenciador de arquivos"),
            Editor => ("Open in Visual Studio Code", "Abrir no Visual Studio Code"),
            Issue => ("Create issue on GitHub", "Criar issue no GitHub"),
            RepositorySettings => ("Repository settings…", "Configurações do repositório…"),
            NewBranch => ("New branch…", "Nova branch…"),
            Rename => ("Rename…", "Renomear…"),
            Delete => ("Delete…", "Excluir…"),
            Discard => ("Discard all changes…", "Descartar todas as alterações…"),
            Stash => ("Stash all changes", "Guardar todas as alterações"),
            Update => (
                "Update from default branch…",
                "Atualizar a partir da branch padrão…",
            ),
            Compare => ("Compare to branch…", "Comparar com uma branch…"),
            Merge => ("Merge into current branch…", "Integrar à branch atual…"),
            Squash => (
                "Squash and merge into current branch…",
                "Integrar à branch atual com squash…",
            ),
            Rebase => (
                "Rebase current branch…",
                "Reaplicar commits da branch atual…",
            ),
            GithubCompare => ("Compare on GitHub", "Comparar no GitHub"),
            GithubBranch => ("View branch on GitHub", "Ver branch no GitHub"),
            PullPreview => ("Preview pull request", "Prévia do pull request"),
            CreatePull => ("Create pull request", "Criar pull request"),
            ReportIssue => ("Report issue…", "Relatar problema…"),
            Guides => ("Show user guides", "Exibir guias"),
            Shortcuts => ("Show keyboard shortcuts", "Exibir atalhos de teclado"),
            Logs => (
                "Show logs in file manager",
                "Mostrar logs no gerenciador de arquivos",
            ),
            About => ("About gitRUST", "Sobre o gitRUST"),
        };
        l.local(en, pt)
    }

    pub(super) fn shortcut(self) -> Option<KeyboardShortcut> {
        let (mods, key) = match self {
            NewRepository => (CMD, Key::N),
            Open => (CMD, Key::O),
            CloneRepository => (SHIFT, Key::O),
            Settings => (CMD, Key::Comma),
            Quit => (CMD, Key::Q),
            Undo => (CMD, Key::Z),
            Redo => (SHIFT, Key::Z),
            Cut => (CMD, Key::X),
            CopyText => (CMD, Key::C),
            Paste => (CMD, Key::V),
            SelectAll => (CMD, Key::A),
            Find => (CMD, Key::F),
            Changes => (CMD, Key::Num1),
            History => (CMD, Key::Num2),
            Repositories => (CMD, Key::T),
            Branches => (CMD, Key::B),
            Summary => (CMD, Key::G),
            ShowStash => (CMD, Key::H),
            Fullscreen => (NONE, Key::F11),
            ZoomReset => (CMD, Key::Num0),
            ZoomIn => (CMD, Key::Equals),
            ZoomOut => (CMD, Key::Minus),
            Expand => (CMD, Key::Num9),
            Contract => (CMD, Key::Num8),
            Diagnostics => (SHIFT, Key::I),
            Push => (CMD, Key::P),
            Pull => (SHIFT, Key::P),
            Fetch => (SHIFT, Key::T),
            Forget => (CMD, Key::Backspace),
            Github => (SHIFT, Key::G),
            Terminal => (CMD, Key::Backtick),
            FileManager => (SHIFT, Key::F),
            Editor => (SHIFT, Key::A),
            Issue => (CMD, Key::I),
            NewBranch => (SHIFT, Key::N),
            Rename => (SHIFT, Key::R),
            Delete => (SHIFT, Key::D),
            Discard => (SHIFT, Key::Backspace),
            Stash => (SHIFT, Key::S),
            Update => (SHIFT, Key::U),
            Compare => (SHIFT, Key::B),
            Merge => (SHIFT, Key::M),
            Squash => (SHIFT, Key::H),
            Rebase => (SHIFT, Key::E),
            GithubCompare => (SHIFT, Key::C),
            GithubBranch => (ALT, Key::B),
            PullPreview => (ALT, Key::P),
            CreatePull => (CMD, Key::R),
            RepositorySettings | ReportIssue | Guides | Shortcuts | Logs | About
            | GithubWorkspace | Overview => return None,
        };
        Some(KeyboardShortcut::new(mods, key))
    }

    fn edit(self) -> bool {
        matches!(self, Undo | Redo | Cut | CopyText | Paste | SelectAll)
    }

    fn available(self, shell: &Desktop) -> bool {
        let managed = shell.managed.as_ref();
        let workspace = managed.and_then(|managed| managed.view.active.as_ref());
        let changes = managed.and_then(|managed| managed.view.changes.as_ref());
        let navigation = managed.and_then(|managed| managed.view.navigation.as_ref());
        let writable = managed.is_some_and(super::ManagerUi::idle)
            && changes.is_some()
            && navigation.is_some_and(|view| !view.busy && view.merge == MergeState::None);
        let github = managed.map(|managed| &managed.view.github);
        match self {
            Undo | Redo | Cut | CopyText | Paste | SelectAll => shell.text_focus.is_some(),
            NewRepository | Open | CloneRepository => managed.is_some_and(super::ManagerUi::usable),
            Changes | Summary => shell.screen == Screen::Demo || changes.is_some(),
            Find | Expand | Contract | History => {
                shell.screen == Screen::Demo || navigation.is_some() || changes.is_some()
            }
            Repositories => managed.is_some() || shell.options.diagnostics.is_some(),
            Branches => workspace.is_some() || shell.screen == Screen::Demo,
            ZoomIn => shell.options.zoom < 2.0,
            ZoomOut => shell.options.zoom > 1.0,
            Fetch => managed.is_some_and(super::ManagerUi::can_fetch),
            Push | Pull => managed.is_some_and(|managed| managed.remote_dialog(self).is_some()),
            Forget => workspace.is_some() && managed.is_some_and(super::ManagerUi::usable),
            Terminal | Editor => workspace.is_some() && cfg!(target_os = "linux"),
            FileManager | RepositorySettings | Overview => workspace.is_some(),
            Github | Issue => github.is_some_and(|view| view.workspace_repo.is_some()),
            GithubCompare | GithubBranch | PullPreview => github.is_some_and(|view| {
                view.workspace_repo.is_some() && view.workspace_branch.is_some()
            }),
            CreatePull => github.is_some_and(|view| {
                !view.busy && view.workspace_repo.is_some() && view.workspace_branch.is_some()
            }),
            NewBranch | Rename | Delete | Update | Compare | Merge | Squash | Rebase => {
                (writable || self == Compare)
                    && (self != Rebase || cfg!(target_os = "linux"))
                    && super::navigation::NavigationUi::menu_available(navigation, self)
            }
            Discard => writable && changes.is_some_and(super::changes::can_save_or_discard),
            GithubWorkspace | ReportIssue | Guides | Logs => managed.is_some(),
            ShowStash => {
                managed.is_some_and(super::ManagerUi::idle)
                    && navigation.is_some_and(|view| !view.busy)
            }
            Stash => {
                writable
                    && navigation.is_some_and(|view| view.head.is_some())
                    && changes.is_some_and(super::changes::can_save_or_discard)
            }
            _ => true,
        }
    }
}

impl Desktop {
    pub(super) fn menu_bar(
        &mut self,
        ui: &mut Ui,
        p: Palette,
        l: Language,
        events: &mut FrameEvents,
    ) {
        egui::Panel::top("desktop-menu")
            .frame(
                crate::components::panel(p.background, 0)
                    .inner_margin(egui::Margin::symmetric(6, 2)),
            )
            .show(ui, |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    let mut menus = |ui: &mut Ui| {
                        for &(name, groups) in MENUS {
                            ui.menu_button(l.text(name), |ui| {
                                self.menu_commands(ui, groups, l, events);
                            });
                        }
                    };
                    if ui.available_width() < 355.0 {
                        ui.menu_button(l.text(Text::Menu), menus);
                    } else {
                        menus(ui);
                    }
                });
            });
    }

    fn menu_commands(
        &mut self,
        ui: &mut Ui,
        groups: &[&[Command]],
        l: Language,
        events: &mut FrameEvents,
    ) {
        let viewport = ui.ctx().content_rect().size();
        let width: f32 = if groups.iter().any(|group| group.contains(&Squash)) {
            366.0
        } else if groups.iter().any(|group| group.contains(&Editor)) {
            316.0
        } else {
            263.0
        };
        ui.set_width(width.min((viewport.x - 24.0).max(135.0)));
        ui.spacing_mut().interact_size.y = 24.0;
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.spacing_mut().button_padding.y = 3.0;
        egui::ScrollArea::vertical()
            .max_height((viewport.y - 68.0).max(64.0))
            .show(ui, |ui| {
                let demo_gallery = self.options.diagnostics.is_some()
                    && self.screen != Screen::Gallery
                    && groups.iter().any(|group| group.contains(&Diagnostics));
                if demo_gallery {
                    if ui.button(l.text(Text::Gallery)).clicked() {
                        ui.close();
                        self.run_command(ui, events, Diagnostics);
                    }
                    ui.separator();
                }
                for (index, group) in groups.iter().enumerate() {
                    if index != 0 {
                        ui.separator();
                    }
                    for &command in *group {
                        if demo_gallery && command == Diagnostics {
                            continue;
                        }
                        let mut button = Button::new(command.label(l)).wrap();
                        if let Some(shortcut) = command.shortcut() {
                            button = button.shortcut_text(ui.ctx().format_shortcut(&shortcut));
                        }
                        let response = ui.add_enabled(command.available(self), button);
                        if response.clicked() {
                            ui.close();
                            self.run_command(ui, events, command);
                        }
                    }
                }
            });
    }

    pub(super) fn accelerators(&mut self, ui: &mut Ui, events: &mut FrameEvents) {
        let focused = ui.memory(egui::Memory::focused);
        if let Some(id) = focused
            && egui::TextEdit::load_state(ui.ctx(), id).is_some()
        {
            self.text_focus = Some(id);
        }
        if self.dialog != Dialog::None
            || ui.memory(|memory| memory.top_modal_layer().is_some())
            || ui.input(|input| input.events.is_empty())
        {
            return;
        }
        for &(_, groups) in MENUS {
            for &command in groups.iter().flat_map(|group| group.iter()) {
                let handled_by_editor = command.edit()
                    || command == Forget && focused.is_some() && focused == self.text_focus;
                if !handled_by_editor
                    && command
                        .shortcut()
                        .is_some_and(|shortcut| consume(ui, shortcut))
                {
                    if command.available(self) {
                        self.run_command(ui, events, command);
                    }
                    return;
                }
            }
        }
    }

    pub(super) fn run_command(&mut self, ui: &mut Ui, events: &mut FrameEvents, command: Command) {
        match command {
            Undo | Redo | Cut | CopyText | Paste | SelectAll => self.edit_command(ui, command),
            NewRepository | Open | CloneRepository => {
                if let Some(managed) = &mut self.managed {
                    managed.open_form(match command {
                        NewRepository => FormKind::Create,
                        Open => FormKind::Open,
                        _ => FormKind::Clone,
                    });
                }
                self.screen = Screen::Workspace;
            }
            Settings => self.dialog = Dialog::Settings,
            Quit => events.action = Some(AppEvent::RequestClose),
            Changes | History => self.show_tab(if command == Changes {
                Tab::Changes
            } else {
                Tab::History
            }),
            Summary | Find => {
                if self.screen == Screen::Demo {
                    self.compact_detail = false;
                    if command == Summary {
                        self.switch_tab(Tab::Changes);
                        self.dialog = Dialog::Compose;
                        self.initial_compose_focus = Some(Id::unique(("modal-draft", "summary")));
                    } else {
                        self.focus_search = true;
                    }
                } else {
                    if command == Summary || self.screen != Screen::Workspace {
                        self.show_tab(Tab::Changes);
                    }
                    if let Some(managed) = &mut self.managed {
                        managed.focus(command);
                    }
                }
            }
            Repositories | Branches | RepositorySettings => {
                if self.screen != Screen::Demo {
                    self.screen = Screen::Workspace;
                }
                let id = match command {
                    Repositories => REPOSITORY_POPUP,
                    Branches => BRANCH_POPUP,
                    _ => super::remotes::REMOTE_POPUP,
                };
                self.open_popup = Some(id);
            }
            Fullscreen => ui.ctx().send_viewport_cmd(ViewportCommand::Fullscreen(
                !ui.input(|input| input.viewport().fullscreen.unwrap_or(false)),
            )),
            ZoomReset | ZoomIn | ZoomOut => {
                const SCALES: [f32; 4] = [1.0, 1.25, 1.5, 2.0];
                self.options.zoom = match command {
                    ZoomReset => 1.0,
                    ZoomIn => SCALES
                        .into_iter()
                        .find(|zoom| *zoom > self.options.zoom)
                        .unwrap_or(2.0),
                    _ => SCALES
                        .into_iter()
                        .rev()
                        .find(|zoom| *zoom < self.options.zoom)
                        .unwrap_or(1.0),
                };
                ui.ctx().set_zoom_factor(self.options.zoom);
            }
            Expand | Contract => {
                self.sidebar_delta = if command == Expand { 30.0 } else { -30.0 };
            }
            Diagnostics => {
                self.screen = if self.screen == Screen::Gallery {
                    Screen::Workspace
                } else {
                    Screen::Gallery
                };
            }
            GithubWorkspace => self.screen = Screen::Github,
            Overview => {
                self.screen = Screen::Workspace;
                if let Some(managed) = &mut self.managed {
                    managed.navigation.tab = super::navigation::Tab::Overview;
                }
            }
            Shortcuts => self.dialog = Dialog::Shortcuts,
            Logs => events.show_logs = true,
            About => self.dialog = Dialog::About,
            _ => self.run_repository_command(command),
        }
    }

    fn run_repository_command(&mut self, command: Command) {
        if let Some(managed) = &mut self.managed {
            let github = &managed.view.github;
            match command {
                Fetch => {
                    if let Some(intent) = managed.fetch_intent() {
                        self.screen = Screen::Workspace;
                        managed.send(RepositoryAction::Remote(intent));
                    }
                }
                Push | Pull => {
                    if let Some(dialog) = managed.remote_dialog(command)
                        && let Some(view) = &managed.view.remotes
                    {
                        self.screen = Screen::Workspace;
                        managed.remotes.open(view, dialog);
                    }
                }
                Forget => {
                    self.screen = Screen::Workspace;
                    managed.confirm_forget();
                }
                Terminal | Editor | FileManager => managed.open_desktop_target(command),
                ShowStash => {
                    if let Some(view) = &managed.view.navigation {
                        self.screen = Screen::Workspace;
                        let intent = managed.navigation.show_stashes(view);
                        managed.send(RepositoryAction::Navigate(intent));
                    }
                }
                Discard => {
                    self.screen = Screen::Workspace;
                    managed.discard_all();
                }
                NewBranch | Rename | Delete | Update | Compare | Merge | Squash | Rebase
                | Stash => {
                    if let Some(view) = &managed.view.navigation {
                        self.screen = Screen::Workspace;
                        managed.navigation.open_menu(view, command);
                    }
                }
                CreatePull => {
                    self.screen = Screen::Github;
                    managed.github.create_workspace_pull(github);
                }
                Github | Issue | GithubBranch | GithubCompare | PullPreview => {
                    if let Some(repo) = &github.workspace_repo {
                        let root = repo.browser();
                        let url = match command {
                            Github => root,
                            Issue => format!("{root}/issues/new"),
                            GithubBranch | GithubCompare | PullPreview => {
                                let Some(branch) = &github.workspace_branch else {
                                    return;
                                };
                                let branch = encode_reference(branch);
                                let route = if command == GithubBranch {
                                    "tree"
                                } else {
                                    "compare"
                                };
                                format!("{root}/{route}/{branch}")
                            }
                            _ => unreachable!(),
                        };
                        managed.send(RepositoryAction::Github(GithubAction::Open(url)));
                    }
                }
                ReportIssue | Guides => {
                    let route = if command == ReportIssue {
                        "issues/new"
                    } else {
                        "tree/main/docs"
                    };
                    managed.send(RepositoryAction::Github(GithubAction::Open(format!(
                        "{PROJECT_URL}/{route}"
                    ))));
                }
                _ => {}
            }
        }
    }

    pub(super) fn show_tab(&mut self, tab: Tab) {
        if self.screen == Screen::Demo {
            self.switch_tab(tab);
        } else if let Some(managed) = &mut self.managed {
            self.screen = Screen::Workspace;
            managed.navigation.tab = match tab {
                Tab::Changes => super::navigation::Tab::Changes,
                Tab::History => super::navigation::Tab::History,
            };
        }
    }

    fn edit_command(&self, ui: &Ui, command: Command) {
        let Some(id) = self.text_focus.filter(|id| {
            ui.ctx()
                .read_response(*id)
                .is_some_and(|response| response.enabled())
        }) else {
            return;
        };
        ui.memory_mut(|memory| memory.request_focus(id));
        let viewport = match command {
            Cut => Some(ViewportCommand::RequestCut),
            CopyText => Some(ViewportCommand::RequestCopy),
            Paste => Some(ViewportCommand::RequestPaste),
            _ => None,
        };
        if let Some(viewport) = viewport {
            ui.ctx().send_viewport_cmd(viewport);
        } else if let Some(shortcut) = command.shortcut() {
            ui.input_mut(|input| {
                input.events.push(egui::Event::Key {
                    key: shortcut.logical_key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: shortcut.modifiers,
                });
            });
        }
    }

    pub(super) fn shortcuts(ui: &mut Ui, l: Language) {
        for &(name, groups) in MENUS {
            ui.strong(l.text(name));
            for command in groups.iter().flat_map(|group| group.iter()) {
                if let Some(shortcut) = command.shortcut() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(ui.ctx().format_shortcut(&shortcut));
                        ui.label(command.label(l));
                    });
                }
            }
            ui.separator();
        }
    }
}

fn consume(ui: &Ui, shortcut: KeyboardShortcut) -> bool {
    ui.input_mut(|input| {
        let mut found = false;
        let mut discard_text = false;
        input.events.retain(|event| {
            if std::mem::take(&mut discard_text) && matches!(event, egui::Event::Text(_)) {
                return false;
            }
            if let egui::Event::Key {
                key,
                modifiers,
                pressed: true,
                repeat: false,
                ..
            } = event
                && *key == shortcut.logical_key
                && modifiers.matches_exact(shortcut.modifiers)
            {
                found = true;
                discard_text = true;
                false
            } else {
                true
            }
        });
        found
    })
}
