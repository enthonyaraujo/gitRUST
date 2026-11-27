use crate::demo::REPOSITORY;

use super::{
    BRANCHES, Desktop, DiagnosticView, Icon, Id, Language, Palette, Screen, Text, Ui,
    anchored_popup, c, commands, remotes,
};

impl Desktop {
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
            if self.screen == Screen::Workspace
                && self.managed.as_ref().is_some_and(|managed| {
                    let github = &managed.view.github;
                    github.workspace_repo.as_ref().is_some_and(|key| {
                        github
                            .repositories
                            .iter()
                            .any(|repository| repository.private && &repository.key == key)
                    })
                })
            {
                Icon::Lock
            } else {
                Icon::Folder
            },
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

    pub(super) fn toolbar(&mut self, ui: &mut Ui, p: Palette, l: Language) {
        let initial_popup = self.initial_popup.take();
        let active = self.screen == Screen::Workspace
            && self
                .managed
                .as_ref()
                .is_some_and(|managed| managed.view.active.is_some());
        c::toolbar(ui, p, |ui| {
            let width = ui.available_width();
            let repo_width = self
                .managed
                .as_ref()
                .filter(|_| active)
                .and_then(|manager| manager.sidebar_width)
                .unwrap_or(250.0)
                .min(width * 0.46);
            let block_width = ((width - repo_width) * 0.5).min(240.0);
            self.repository_button(ui, p, l, initial_popup, repo_width);
            self.branch_button(ui, p, l, block_width, active, initial_popup);
            if let Some(managed) = self.managed.as_mut().filter(|_| active) {
                let response = managed
                    .fetch_button(ui, block_width, l, p)
                    .unwrap_or_else(|| {
                        let (label, detail) = if managed.view.remotes.is_some() {
                            (
                                l.local("Publish repository", "Publicar repositório"),
                                "GitHub",
                            )
                        } else {
                            (l.text(Text::Remote), l.local("Loading…", "Carregando…"))
                        };
                        let response = c::context_button(
                            ui,
                            block_width,
                            Icon::Sync,
                            label,
                            detail,
                            managed.can_publish(),
                            p,
                        );
                        if response.clicked() {
                            self.screen = Screen::Github;
                            managed.github.publish_workspace(&managed.view.github);
                        }
                        response
                    });
                // Only the repository settings command opens this menu. A primary click fetches.
                egui::Popup::menu(&response)
                    .id(Id::unique(remotes::REMOTE_POPUP))
                    .open_memory(
                        (self.open_popup == Some(remotes::REMOTE_POPUP))
                            .then_some(egui::SetOpenCommand::Bool(true)),
                    )
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| managed.remote_popup(ui, l, p));
            }
            if matches!(self.screen, Screen::Demo | Screen::Gallery) && ui.available_width() > 130.0
            {
                ui.add_space(12.0);
                c::badge(ui, l.text(Text::Demo), p.accent, p.selected);
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
        } else if let Some(managed) = self.managed.as_mut().filter(|_| active)
            && popup
                .show(|ui| managed.branch_popup(ui, l, p))
                .is_some_and(|result| result.inner)
        {
            self.screen = Screen::Github;
        }
    }
}
