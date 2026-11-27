use egui::{Context, Response, Ui};
use gitrust_core::{remotes::RemoteNotice, workspaces::RepositoryAction};

use super::ManagerUi;
use crate::{
    components::{self as c, Icon},
    i18n::{Language, Text},
    theme::Palette,
};

#[derive(Debug, Default)]
pub(super) enum FetchState {
    #[default]
    Idle,
    Fetching {
        started: f64,
    },
    Error,
}

impl ManagerUi {
    pub(super) fn start_fetch(&mut self, ctx: &Context) -> bool {
        let Some(intent) = self.fetch_intent() else {
            return false;
        };
        self.fetch = FetchState::Fetching {
            started: ctx.input(|input| input.time),
        };
        self.send(RepositoryAction::Remote(intent));
        ctx.request_repaint();
        true
    }

    pub(super) fn settle_fetch(&mut self, ctx: &Context) {
        if let FetchState::Fetching { started } = self.fetch
            && ctx.input(|input| input.time) - started >= 0.4
            && self.idle()
            && !self.view.github.busy
        {
            self.fetch = match self.view.remotes.as_ref().and_then(|view| view.notice) {
                None | Some(RemoteNotice::Fetched) => FetchState::Idle,
                Some(_) => FetchState::Error,
            };
        }
    }

    pub(super) fn fetch_button(
        &mut self,
        ui: &mut Ui,
        width: f32,
        l: Language,
        p: Palette,
    ) -> Option<Response> {
        let remote = self.fetch_target()?;
        let (icon, title, detail) = match self.fetch {
            FetchState::Fetching { started } => {
                ui.ctx().request_repaint();
                let elapsed = ui.input(|input| input.time) - started;
                #[allow(clippy::cast_possible_truncation)]
                let angle = (elapsed.rem_euclid(1.0) * std::f64::consts::TAU) as f32;
                (
                    Icon::SyncRotating(angle),
                    format!("{} {}", l.local("Fetching", "Buscando"), remote.name),
                    l.local("Hang on…", "Aguarde…").to_owned(),
                )
            }
            FetchState::Idle | FetchState::Error => {
                let detail = if remote.fetch.is_none() {
                    l.local("Fetch unavailable", "Busca indisponível")
                        .to_owned()
                } else {
                    self.view
                        .remotes
                        .as_ref()
                        .and_then(|view| {
                            view.last_fetch
                                .iter()
                                .find(|(name, _)| name == &remote.name)
                        })
                        .map_or_else(
                            || {
                                l.local("Last fetch: never", "Última busca: nunca")
                                    .to_owned()
                            },
                            |(_, time)| super::remotes::last_fetch_text(ui.ctx(), *time, l),
                        )
                };
                (
                    Icon::Sync,
                    format!("{} {}", l.text(Text::Fetch), remote.name),
                    detail,
                )
            }
        };
        let response = c::context_button(ui, width, icon, &title, &detail, self.can_fetch(), p);
        if response.clicked() {
            self.start_fetch(ui.ctx());
        }
        Some(response)
    }
}
