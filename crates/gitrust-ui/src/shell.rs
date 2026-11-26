mod changes;
mod commands;
mod dialogs;
mod github;
mod navigation;
mod remotes;
mod repositories;
mod welcome;

use repositories::{FormKind, ManagerUi};

pub(crate) use dialogs::preferences;

use egui::{
    Align, Button, CentralPanel, Id, Key, Label, Layout, Modifiers, RichText, ScrollArea,
    ThemePreference, Ui,
};
use gitrust_core::{AppEvent, AppState};

use crate::components::{self as c, Icon, ListSelection, Row};
use crate::demo::{BRANCHES, COMMITS, FILES, Fixtures, REPOSITORY};
use crate::diff::{DiffMode, DiffView};
use crate::gallery::Gallery;
use crate::i18n::{Language, Text};
use crate::theme::{self, FileStatus, Palette, code, size, weight};

/// Initial diagnostics screen. Only exposed through explicit binary flags.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DiagnosticView {
    /// Start in the component gallery.
    #[default]
    Gallery,
    /// Open the synthetic working tree in unified mode.
    Unified,
    /// Open the synthetic working tree in split mode.
    Split,
    /// Open the synthetic commit history.
    History,
    /// Open application settings with diagnostic navigation enabled.
    Settings,
    /// Show an actual keyboard focus target in the synthetic working tree.
    Keyboard,
    /// Open the compact draft composer.
    Compose,
    /// Open the compact draft composer with its summary focused.
    ComposeSummary,
    /// Open the compact draft composer with its description focused.
    ComposeDescription,
    /// Show the repository context menu.
    Repository,
    /// Show the branch context menu.
    Branch,
    /// Open a split diff with long, tabbed Unicode lines.
    LongDiff,
    /// Open the binary file fixture.
    Binary,
    /// Open a gallery diff with 100,000 synthetic rows.
    StressDiff,
    /// Open a gallery list with 10,000 synthetic rows.
    StressList,
}

/// Initial window preferences. Changes are kept in memory for this session.
#[derive(Clone, Copy, Debug)]
pub struct UiOptions {
    /// Appearance preference, including the window system's reported color scheme.
    pub theme: ThemePreference,
    /// Interface language.
    pub language: Language,
    /// User scale, multiplying the monitor scale.
    pub zoom: f32,
    /// Optional opt-in diagnostics screen.
    pub diagnostics: Option<DiagnosticView>,
}

impl Default for UiOptions {
    fn default() -> Self {
        Self {
            theme: ThemePreference::System,
            language: Language::English,
            zoom: 1.0,
            diagnostics: None,
        }
    }
}

/// Stack-allocated batch of lifecycle events from a single UI pass.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameEvents {
    resize: Option<AppEvent>,
    action: Option<AppEvent>,
    show_logs: bool,
}

impl FrameEvents {
    /// Whether the user asked to open the runtime logs folder.
    #[must_use]
    pub fn show_logs(&self) -> bool {
        self.show_logs
    }

    /// Iterates in deterministic window-event order.
    pub fn into_iter_events(self) -> impl Iterator<Item = AppEvent> {
        self.resize.into_iter().chain(self.action)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Screen {
    #[default]
    Workspace,
    Welcome,
    Demo,
    Gallery,
    Github,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Tab {
    #[default]
    Changes,
    History,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Dialog {
    #[default]
    None,
    Settings,
    About,
    Shortcuts,
    Compose,
    Discard {
        return_to_compose: bool,
    },
}

#[derive(Debug, Default)]
struct Demo {
    fixtures: Fixtures,
    files: ListSelection,
    history: ListSelection,
    branch: usize,
    query: String,
    matches: Vec<usize>,
    diff: DiffView,
    summary: String,
    description: String,
}

impl Demo {
    fn count(&self, tab: Tab) -> usize {
        match (self.branch, tab) {
            (0, Tab::Changes) => FILES.len(),
            (0 | 1, Tab::History) => COMMITS.len(),
            _ => 0,
        }
    }

    fn item(tab: Tab, index: usize) -> Row<'static> {
        match tab {
            Tab::Changes => FILES[index].row(),
            Tab::History => Row {
                name: COMMITS[index].subject,
                detail: COMMITS[index].short,
                status: None,
            },
        }
    }

    fn filter(&mut self, tab: Tab) {
        self.matches.clear();
        let query = self.query.trim().to_lowercase();
        for index in 0..self.count(tab) {
            let row = Self::item(tab, index);
            if query.is_empty()
                || row.name.to_lowercase().contains(&query)
                || row.detail.to_lowercase().contains(&query)
            {
                self.matches.push(index);
            }
        }
        match tab {
            Tab::Changes => self.files.retain(&self.matches),
            Tab::History => self.history.retain(&self.matches),
        }
    }
}

/// The application shell and ephemeral presentation state.
#[derive(Debug)]
pub struct Desktop {
    options: UiOptions,
    screen: Screen,
    dialog: Dialog,
    tab: Tab,
    mode: DiffMode,
    compact_detail: bool,
    demo: Option<Demo>,
    gallery: Gallery,
    focus_search: bool,
    initial_popup: Option<DiagnosticView>,
    open_popup: Option<&'static str>,
    sidebar_delta: f32,
    text_focus: Option<Id>,
    initial_compose_focus: Option<Id>,
    managed: Option<ManagerUi>,
}

impl Desktop {
    /// Install visual styles and local fonts, then initialize the requested screen.
    #[must_use]
    pub fn new(ctx: &egui::Context, options: UiOptions) -> Self {
        theme::install(ctx);
        ctx.options_mut(|options| options.zoom_with_keyboard = false);
        ctx.set_theme(options.theme);
        ctx.set_zoom_factor(options.zoom);
        let mut view = Self {
            options,
            screen: Screen::Workspace,
            dialog: Dialog::None,
            tab: Tab::Changes,
            mode: DiffMode::Unified,
            compact_detail: false,
            demo: None,
            gallery: Gallery::default(),
            focus_search: false,
            initial_popup: None,
            open_popup: None,
            sidebar_delta: 0.0,
            text_focus: None,
            initial_compose_focus: None,
            managed: None,
        };
        match options.diagnostics {
            Some(DiagnosticView::Gallery) => view.screen = Screen::Gallery,
            Some(DiagnosticView::Settings) => view.dialog = Dialog::Settings,
            Some(DiagnosticView::StressDiff | DiagnosticView::StressList) => {
                view.screen = Screen::Gallery;
                view.gallery
                    .open_stress(options.diagnostics == Some(DiagnosticView::StressDiff));
            }
            Some(diagnostic) => {
                view.screen = Screen::Demo;
                view.demo = Some(Demo::default());
                view.mode =
                    if matches!(diagnostic, DiagnosticView::Split | DiagnosticView::LongDiff) {
                        DiffMode::Split
                    } else {
                        DiffMode::Unified
                    };
                view.tab = if diagnostic == DiagnosticView::History {
                    Tab::History
                } else {
                    Tab::Changes
                };
                view.reset_demo_list(true);
                view.compact_detail = diagnostic != DiagnosticView::Keyboard;
                view.focus_search = diagnostic == DiagnosticView::Keyboard;
                match diagnostic {
                    DiagnosticView::Compose
                    | DiagnosticView::ComposeSummary
                    | DiagnosticView::ComposeDescription => {
                        view.compact_detail = false;
                        view.dialog = Dialog::Compose;
                        view.initial_compose_focus = match diagnostic {
                            DiagnosticView::ComposeSummary => {
                                Some(Id::unique(("modal-draft", "summary")))
                            }
                            DiagnosticView::ComposeDescription => {
                                Some(Id::unique(("modal-draft", "description")))
                            }
                            _ => None,
                        };
                    }
                    DiagnosticView::Repository | DiagnosticView::Branch => {
                        view.initial_popup = Some(diagnostic);
                    }
                    DiagnosticView::LongDiff | DiagnosticView::Binary => {
                        if let Some(demo) = &mut view.demo {
                            if diagnostic == DiagnosticView::LongDiff {
                                demo.diff.focus_on_open();
                            }
                            demo.files.index = Some(if diagnostic == DiagnosticView::LongDiff {
                                6
                            } else {
                                3
                            });
                        }
                    }
                    _ => {}
                }
            }
            None => {}
        }
        view
    }

    fn reset_demo_list(&mut self, select_first: bool) {
        if let Some(demo) = &mut self.demo {
            demo.query.clear();
            demo.filter(self.tab);
            if select_first && !demo.matches.is_empty() {
                match self.tab {
                    Tab::Changes => demo.files.index = Some(0),
                    Tab::History => demo.history.index = Some(0),
                }
            }
        }
    }

    fn open_demo(&mut self) {
        if self.demo.is_none() {
            self.demo = Some(Demo::default());
            self.reset_demo_list(true);
        }
        self.screen = Screen::Demo;
    }

    /// Render one frame and return any viewport or close events.
    pub fn show(&mut self, ui: &mut Ui, state: &AppState) -> FrameEvents {
        let rect = ui.ctx().content_rect();
        let should_report_size = (rect.width() - state.viewport_size[0]).abs() > 0.5
            || (rect.height() - state.viewport_size[1]).abs() > 0.5;
        let mut events = FrameEvents {
            resize: should_report_size.then_some(AppEvent::ViewportResized {
                width: rect.width(),
                height: rect.height(),
            }),
            action: None,
            show_logs: false,
        };
        let p = Palette::for_theme(ui.ctx().theme());
        let l = self.options.language;
        let compact = rect.width() < size::COMPACT;
        self.accelerators(ui, &mut events);
        if self.dialog == Dialog::None && ui.memory(|memory| memory.top_modal_layer().is_none()) {
            if let Some(tab) = tab_shortcut(ui) {
                self.show_tab(tab);
            }
            if compact && shortcut(ui, Modifiers::NONE, Key::Escape) {
                self.compact_detail = false;
            }
        }
        self.menu_bar(ui, p, l, &mut events);
        if self.screen != Screen::Welcome
            && (self.screen != Screen::Demo || rect.height() >= size::SHORT)
        {
            self.toolbar(ui, p, l);
        }
        if self.screen != Screen::Welcome {
            c::status_bar(ui, p, |ui| {
                let status = if let Some(managed) = &self.managed {
                    if !managed.view.git_available
                        && managed.view.activity == gitrust_core::workspaces::Activity::Idle
                    {
                        l.text(Text::NoGit)
                    } else if managed.view.active.is_some() {
                        l.local("Local workspace", "Workspace local")
                    } else {
                        l.text(Text::NoRepositoryShort)
                    }
                } else if self.screen == Screen::Workspace {
                    l.text(Text::NoGit)
                } else {
                    l.text(Text::DemoHint)
                };
                c::status_indicator(ui, status, p.muted);
                if !compact {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(l.text(Text::Telemetry))
                                .small()
                                .color(p.muted),
                        );
                    });
                }
            });
        }
        self.workspace(ui, p, l, compact);
        self.dialogs(ui, state, p, l);
        if let Some(managed) = &mut self.managed {
            managed.dialogs(ui, p, l);
        }
        events
    }

    fn switch_tab(&mut self, tab: Tab) {
        if self.tab != tab {
            self.tab = tab;
            if let Some(demo) = &mut self.demo {
                demo.filter(tab);
            }
            self.compact_detail = false;
        }
    }

    fn switch_branch(&mut self, index: usize) {
        if let Some(demo) = &mut self.demo {
            if demo.branch == index {
                return;
            }
            demo.branch = index;
            demo.files = ListSelection::default();
            demo.history = ListSelection::default();
            demo.diff = DiffView::default();
            self.reset_demo_list(false);
            self.compact_detail = false;
        }
    }

    fn repository_button(
        &mut self,
        ui: &mut Ui,
        p: Palette,
        l: Language,
        initial_popup: Option<DiagnosticView>,
        width: f32,
    ) {
        let repository_name = self.managed.as_ref().map(|managed| managed.title(l));
        let repository = c::context_button(
            ui,
            width,
            Icon::Repo,
            l.local("Current repository", "Repositório atual"),
            if self.screen == Screen::Demo {
                REPOSITORY
            } else {
                repository_name
                    .as_deref()
                    .unwrap_or(l.text(Text::NoRepositoryShort))
            },
            self.options.diagnostics.is_some() || self.managed.is_some(),
            p,
        );
        if self.managed.is_some()
            && (repository.clicked() || self.open_popup == Some(commands::REPOSITORY_POPUP))
        {
            ui.memory_mut(|memory| memory.request_focus(Id::unique("repository-search")));
        }
        let popup = anchored_popup(
            &repository,
            commands::REPOSITORY_POPUP,
            self.open_popup == Some(commands::REPOSITORY_POPUP)
                || initial_popup == Some(DiagnosticView::Repository),
        );
        if let Some(managed) = &mut self.managed {
            popup.show(|ui| managed.popup(ui, p, l));
        } else if self.options.diagnostics.is_some() {
            popup.show(|ui| {
                for (screen, label) in [
                    (Screen::Workspace, Text::Workspace),
                    (Screen::Demo, Text::DemoRepo),
                    (Screen::Gallery, Text::Gallery),
                ] {
                    if c::menu_choice(ui, l.text(label), self.screen == screen, p).clicked() {
                        if screen == Screen::Demo {
                            self.open_demo();
                        } else {
                            self.screen = screen;
                        }
                        ui.close();
                    }
                }
            });
        }
    }

    fn toolbar(&mut self, ui: &mut Ui, p: Palette, l: Language) {
        let initial_popup = self.initial_popup.take();
        let active = self.screen == Screen::Workspace
            && self
                .managed
                .as_ref()
                .is_some_and(|managed| managed.view.active.is_some());
        c::toolbar(ui, p, |ui| {
            let width = ui.available_width();
            let narrow = width < 470.0;
            let gap = ui.spacing().item_spacing.x;
            let repo_width = if narrow {
                ((width - 2.0 * gap) * 0.35).max(70.0)
            } else {
                (width * 0.28).clamp(150.0, 280.0)
            };
            let branch_width = if narrow {
                ((width - 2.0 * gap) * 0.30).max(65.0)
            } else {
                (width * 0.21).clamp(130.0, 220.0)
            };
            let remote_width = if narrow {
                (width - repo_width - branch_width - 2.0 * gap).max(58.0)
            } else {
                (width * 0.23).clamp(125.0, 220.0)
            };
            self.repository_button(ui, p, l, initial_popup, repo_width);
            if active || self.screen == Screen::Demo || !narrow {
                self.branch_button(ui, p, l, branch_width, active, initial_popup);
            }
            if let Some(managed) = self.managed.as_mut().filter(|_| active) {
                use commands::Command;
                let (action, label, detail) = managed.toolbar_remote(l);
                let response = c::context_button(
                    ui,
                    (remote_width - 26.0).max(44.0),
                    Icon::Remote,
                    &label,
                    &detail,
                    action.is_some(),
                    p,
                );
                if response.clicked() {
                    match action {
                        Some(action @ (Command::Fetch | Command::Push | Command::Pull)) => {
                            if let (Some(view), Some(dialog)) =
                                (&managed.view.remotes, managed.remote_dialog(action))
                            {
                                managed.remotes.open(view, dialog);
                            }
                        }
                        Some(Command::Github) => {
                            self.screen = Screen::Github;
                            managed.github.publish_workspace(&managed.view.github);
                        }
                        _ => {}
                    }
                }
                let response = c::icon_button(
                    ui,
                    Icon::Chevron,
                    l.local("Remote settings", "Configurações de remotes"),
                    p,
                );
                anchored_popup(
                    &response,
                    remotes::REMOTE_POPUP,
                    self.open_popup == Some(remotes::REMOTE_POPUP),
                )
                .show(|ui| managed.remote_popup(ui, l, p));
            }
            if !narrow && ui.available_width() >= 100.0 {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if c::icon_button(ui, Icon::Settings, l.text(Text::Settings), p).clicked() {
                        self.dialog = Dialog::Settings;
                    }
                    if self.managed.is_some() && ui.button("GitHub").clicked() {
                        self.screen = if self.screen == Screen::Github {
                            Screen::Workspace
                        } else {
                            Screen::Github
                        };
                    }
                    if self.screen == Screen::Demo || self.screen == Screen::Gallery {
                        c::badge(ui, l.text(Text::Demo), p.accent, p.selected);
                    } else if ui.available_width() > 22.0 {
                        Icon::Mark.show(ui, 21.0, p.accent);
                    }
                });
            }
        });
        self.open_popup = None;
    }

    fn branch_button(
        &mut self,
        ui: &mut Ui,
        p: Palette,
        l: Language,
        branch_width: f32,
        active: bool,
        initial_popup: Option<DiagnosticView>,
    ) {
        let branch = self
            .demo
            .as_ref()
            .filter(|_| self.screen == Screen::Demo)
            .map(|demo| demo.branch);
        let caption = branch.map_or_else(
            || {
                self.managed
                    .as_ref()
                    .and_then(|managed| managed.view.active.as_ref())
                    .map_or(l.text(Text::NoBranch), |active| match &active.checkout {
                        gitrust_core::workspaces::Checkout::Branch(name)
                        | gitrust_core::workspaces::Checkout::Unborn(name)
                        | gitrust_core::workspaces::Checkout::Detached(name) => name.as_str(),
                    })
            },
            |index| BRANCHES[index],
        );
        let response = c::context_button(
            ui,
            branch_width,
            Icon::Branch,
            l.local("Current branch", "Branch atual"),
            caption,
            branch.is_some() || active,
            p,
        );
        if active && (response.clicked() || self.open_popup == Some(commands::BRANCH_POPUP)) {
            ui.memory_mut(|memory| memory.request_focus(Id::unique("branch-filter")));
        }
        let popup = anchored_popup(
            &response,
            commands::BRANCH_POPUP,
            self.open_popup == Some(commands::BRANCH_POPUP)
                || initial_popup == Some(DiagnosticView::Branch),
        );
        if let Some(branch) = branch {
            popup.show(|ui| {
                for (index, name) in BRANCHES.iter().enumerate() {
                    if c::menu_choice(ui, name, index == branch, p).clicked() {
                        self.switch_branch(index);
                        ui.close();
                    }
                }
            });
        } else if let Some(managed) = self.managed.as_mut().filter(|_| active) {
            popup.show(|ui| managed.branch_popup(ui, l, p));
        }
    }

    fn workspace(&mut self, ui: &mut Ui, p: Palette, l: Language, compact: bool) {
        if self.screen == Screen::Welcome {
            CentralPanel::default()
                .frame(c::panel(p.background, 0))
                .show(ui, |ui| self.welcome(ui, l, p));
            return;
        }
        if self.screen == Screen::Gallery {
            CentralPanel::default()
                .frame(c::panel(p.background, size::LG))
                .show(ui, |ui| {
                    self.gallery.show(ui, &mut self.options, p);
                });
            return;
        }
        if self.screen == Screen::Demo && !compact {
            let delta = std::mem::take(&mut self.sidebar_delta);
            c::adjust_panel(ui, "workspace-list", delta);
            c::resizable_panel(ui, p, |ui| self.sidebar(ui, p, l, false));
        }
        let padding = if ui.ctx().content_rect().height() < size::SHORT {
            size::SM
        } else {
            size::LG
        };
        let padding = if self.screen == Screen::Workspace
            && self
                .managed
                .as_ref()
                .is_some_and(|managed| managed.view.active.is_some())
        {
            0
        } else {
            padding
        };
        CentralPanel::default()
            .frame(c::panel(p.background, padding))
            .show(ui, |ui| match self.screen {
                Screen::Workspace => {
                    if let Some(managed) = &mut self.managed {
                        managed.workspace(ui, p, l, std::mem::take(&mut self.sidebar_delta));
                    } else {
                        self.empty_workspace(ui, p, l);
                    }
                }
                Screen::Github => {
                    if let Some(managed) = &mut self.managed
                        && let Some(action) = managed.github.show(ui, &managed.view.github, l, p)
                    {
                        managed.send(action);
                    }
                }
                Screen::Demo if compact && !self.compact_detail => self.sidebar(ui, p, l, true),
                Screen::Demo => self.details(ui, p, l, compact),
                Screen::Gallery | Screen::Welcome => {}
            });
    }

    fn empty_workspace(&mut self, ui: &mut Ui, p: Palette, l: Language) {
        ScrollArea::vertical().id_salt("welcome").show(ui, |ui| {
            ui.add_space((ui.available_height() * 0.19).clamp(12.0, 64.0));
            ui.vertical_centered(|ui| {
                ui.set_max_width(410.0);
                c::empty(
                    ui,
                    Icon::Repo,
                    l.text(Text::NoRepository),
                    l.text(Text::Intro),
                    p,
                );
                ui.add_space(f32::from(size::LG));
                if c::primary(ui, l.text(Text::Settings), p).clicked() {
                    self.dialog = Dialog::Settings;
                }
            });
        });
    }

    fn sidebar(&mut self, ui: &mut Ui, p: Palette, l: Language, compact: bool) {
        let mut tab = self.tab;
        c::tabs(
            ui,
            &mut tab,
            &[
                (Tab::Changes, l.text(Text::Changes)),
                (Tab::History, l.text(Text::History)),
            ],
            p,
        );
        self.switch_tab(tab);
        ui.add_space(f32::from(size::XS));
        let Some(demo) = &mut self.demo else {
            return;
        };
        let filter = c::search(ui, Id::unique("workspace-filter"), l, p, &mut demo.query);
        if self.focus_search {
            filter.request_focus();
            self.focus_search = false;
        }
        if filter.changed() {
            demo.filter(self.tab);
        }
        let count = demo.count(self.tab);
        ui.label(
            RichText::new(list_count(l, self.tab, count, demo))
                .small()
                .color(p.muted),
        );
        ui.separator();
        let list = match self.tab {
            Tab::Changes => &mut demo.files,
            Tab::History => &mut demo.history,
        };
        let has_compose = self.tab == Tab::Changes;
        let show_details = compact && !demo.matches.is_empty();
        let inline = has_compose && ui.available_height() >= 298.0;
        let footer = has_compose || show_details;
        let reserve = if inline {
            170.0 + if show_details { size::CONTROL } else { 0.0 }
        } else if footer {
            size::CONTROL + 12.0
        } else {
            0.0
        };
        let height = (ui.available_height() - reserve).max(0.0);
        if demo.matches.is_empty() {
            let key = if count > 0 {
                Text::NoMatches
            } else if has_compose {
                Text::NoChanges
            } else {
                Text::NoHistory
            };
            ui.allocate_ui(egui::vec2(ui.available_width(), height), |ui| {
                ui.label(RichText::new(l.text(key)).small().color(p.muted));
            });
        } else {
            list.show(
                ui,
                Id::unique(("demo-list", self.tab as u8, demo.branch)),
                height,
                demo.matches.len(),
                p,
                l,
                |i| demo.matches[i],
                |key| Demo::item(self.tab, key),
            );
        }
        if footer {
            ui.separator();
            if show_details || (has_compose && !inline) {
                ui.horizontal_wrapped(|ui| {
                    if show_details
                        && ui
                            .add_enabled(
                                list.index.is_some(),
                                Button::new(l.text(Text::ShowDetail)),
                            )
                            .clicked()
                    {
                        self.compact_detail = true;
                    }
                    if has_compose && !inline && ui.button(l.text(Text::Compose)).clicked() {
                        self.dialog = Dialog::Compose;
                    }
                });
            }
            if inline && dialogs::composer(ui, demo, p, l, "inline-draft") {
                self.dialog = Dialog::Discard {
                    return_to_compose: false,
                };
            }
        }
    }

    fn details(&mut self, ui: &mut Ui, p: Palette, l: Language, compact: bool) {
        let Some(demo) = &mut self.demo else {
            return;
        };
        let selection = match self.tab {
            Tab::Changes => demo.files.index,
            Tab::History => demo.history.index,
        };
        let index = selection.filter(|key| demo.matches.contains(key));
        let Some(index) = index else {
            if compact && c::icon_button(ui, Icon::Back, l.text(Text::ShowList), p).clicked() {
                self.compact_detail = false;
            }
            let (icon, title, hint) = match self.tab {
                Tab::Changes => (
                    Icon::Files,
                    Text::Changes,
                    if demo.count(self.tab) == 0 {
                        Text::NoChanges
                    } else {
                        Text::PickFile
                    },
                ),
                Tab::History => (
                    Icon::History,
                    Text::History,
                    if demo.count(self.tab) == 0 {
                        Text::NoHistory
                    } else {
                        Text::PickCommit
                    },
                ),
            };
            ui.add_space(f32::from(size::XL));
            c::empty(ui, icon, l.text(title), l.text(hint), p);
            return;
        };
        if self.tab == Tab::History {
            if compact && c::icon_button(ui, Icon::Back, l.text(Text::ShowList), p).clicked() {
                self.compact_detail = false;
            }
            commit_detail(ui, p, l, index);
            return;
        }
        let file = FILES[index];
        ui.horizontal_wrapped(|ui| {
            if compact {
                if c::icon_button(ui, Icon::Back, l.text(Text::ShowList), p).clicked() {
                    self.compact_detail = false;
                }
            } else {
                Icon::Files.show(ui, size::ICON, p.accent);
            }
            ui.add(Label::new(RichText::new(file.name).heading()).truncate())
                .on_hover_text(file.name);
            c::status_indicator(ui, l.text(file.status.label()), p.status(file.status));
        });
        if ui.available_height() > 145.0 {
            c::truncated(ui, file.path, ui.available_width(), egui::TextStyle::Small);
        }
        ui.separator();
        if file.status == FileStatus::Conflicted {
            c::notice(
                ui,
                l.text(Text::Conflict),
                l.text(Text::ConflictDetails),
                p.error,
                p,
            );
            return;
        }
        demo.diff.show(
            ui,
            Id::unique(("file-diff", index)),
            demo.fixtures.file(index),
            &mut self.mode,
            l,
            p,
        );
    }
}

fn list_count(l: Language, tab: Tab, count: usize, demo: &Demo) -> String {
    if !demo.query.trim().is_empty() {
        return format!("{}/{count} {}", demo.matches.len(), l.text(Text::Count));
    }
    let key = match (tab, count) {
        (Tab::Changes, 1) => Text::OneChangedFile,
        (Tab::Changes, _) => Text::ChangedFiles,
        (Tab::History, 1) => Text::OneCommit,
        (Tab::History, _) => Text::CommitCount,
    };
    format!("{count} {}", l.text(key))
}

fn anchored_popup<'a>(
    response: &'a egui::Response,
    name: &'static str,
    open: bool,
) -> egui::Popup<'a> {
    let popup = egui::Popup::menu(response)
        .id(Id::unique(name))
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    if open {
        popup.open_memory(Some(egui::SetOpenCommand::Bool(true)))
    } else {
        popup
    }
}

fn shortcut(ui: &Ui, modifiers: Modifiers, key: Key) -> bool {
    ui.input_mut(|input| input.consume_key(modifiers, key))
}

fn tab_shortcut(ui: &Ui) -> Option<Tab> {
    ui.input_mut(|input| {
        let mut selected = None;
        let mut discard_text = false;
        input.events.retain(|event| {
            // The native integration can append a text event immediately after an Alt key event.
            if std::mem::take(&mut discard_text) && matches!(event, egui::Event::Text(_)) {
                return false;
            }
            let egui::Event::Key {
                key,
                modifiers,
                pressed: true,
                ..
            } = event
            else {
                return true;
            };
            if !modifiers.matches_exact(Modifiers::ALT) {
                return true;
            }
            selected = Some(match key {
                Key::Num1 => Tab::Changes,
                Key::Num2 => Tab::History,
                _ => return true,
            });
            discard_text = true;
            false
        });
        selected
    })
}

fn commit_detail(ui: &mut Ui, p: Palette, l: Language, index: usize) {
    let commit = COMMITS[index];
    ScrollArea::vertical()
        .id_salt(("commit", index))
        .show(ui, |ui| {
            c::badge(ui, commit.short, p.accent, p.selected);
            ui.add_space(f32::from(size::XS));
            ui.heading(commit.subject);
            ui.label(RichText::new(commit.message).color(p.muted));
            ui.add_space(f32::from(size::MD));
            ui.separator();
            for (key, value) in [
                (Text::Author, commit.author),
                (Text::Date, commit.date),
                (Text::Revision, commit.hash),
            ] {
                ui.label(
                    RichText::new(l.text(key))
                        .small()
                        .family(weight())
                        .color(p.muted),
                );
                ui.label(RichText::new(value).font(code()));
                ui.add_space(f32::from(size::XS));
            }
        });
}

#[cfg(test)]
mod tests;
