use egui::{Panel, Rect, RichText, ScrollArea, Sense, Stroke, Ui, pos2, vec2};
use gitrust_core::{github::Action, workspaces::RepositoryAction};

use super::{Desktop, Screen};
use crate::{
    components as c,
    i18n::Language,
    theme::{Palette, size, weight},
};

impl Desktop {
    pub(super) fn welcome(&mut self, ui: &mut Ui, l: Language, p: Palette) {
        let Some(managed) = &mut self.managed else {
            return;
        };
        let available = ui.available_size();
        if available.x >= 760.0 && available.y >= 315.0 {
            let width = (available.x * 0.36).clamp(272.0, 570.0);
            Panel::right("welcome-graphic")
                .exact_size(width)
                .frame(c::panel(p.surface, 0))
                .show(ui, |ui| draw_branches(ui, p));
        }
        let padding = if available.x >= 760.0 { 40 } else { size::LG };
        let skip = c::panel(p.background, padding)
            .show(ui, |ui| {
                ScrollArea::vertical()
                    .id_salt("github-welcome")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_max_width(560.0);
                        let space = if managed.view.github.busy { 8.0 } else { 40.0 };
                        ui.add_space((available.y * 0.09).min(space));
                        ui.horizontal(|ui| {
                            c::Icon::Mark.show(ui, 25.0, p.accent);
                            ui.label(RichText::new("gitRUST").family(weight()).size(18.0));
                        });
                        ui.add_space(16.0);
                        ui.heading(
                            RichText::new(l.local("Welcome to gitRUST", "Bem-vindo ao gitRUST"))
                                .size(32.0),
                        );
                        ui.label(
                            RichText::new(l.local(
                                "Connect to GitHub to clone, publish and review your projects.",
                                "Conecte sua conta do GitHub para clonar, publicar e revisar seus projetos.",
                            ))
                            .size(15.0)
                            .color(p.muted),
                        );
                        ui.add_space(18.0);
                        let view = &managed.view.github;
                        managed.github.authorization_status(ui, view, l, p);
                        if !view.busy {
                            managed.github.sign_in_controls(ui, view, l, p);
                        }
                        ui.add_space(12.0);
                        if !view.busy
                            && ui.link(l.local("Create a GitHub account", "Criar conta no GitHub")).clicked()
                        {
                            managed.github.send(Action::Open("https://github.com/signup".into()));
                        }
                        ui.button(l.local("Continue without an account", "Continuar sem conta")).clicked()
                    }).inner
            }).inner;
        if skip {
            managed.send(RepositoryAction::DismissWelcome);
            self.screen = Screen::Workspace;
        } else if let Some(action) = managed.github.take_action() {
            managed.send(action);
        }
    }
}

fn draw_branches(ui: &mut Ui, p: Palette) {
    let (bounds, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
    let canvas = Rect::from_center_size(
        bounds.center(),
        vec2(bounds.width() * 0.76, (bounds.height() * 0.68).min(480.0)),
    );
    let painter = ui.painter_at(bounds);
    let point = |x, y| {
        pos2(
            canvas.left() + canvas.width() * x,
            canvas.top() + canvas.height() * y,
        )
    };
    for column in 0_u8..=10 {
        for row in 0_u8..=18 {
            painter.circle_filled(
                point(f32::from(column) * 0.10, f32::from(row) / 18.0),
                1.2,
                p.border,
            );
        }
    }
    for (x, y, w, h, tint) in [
        (0.14, 0.13, 0.54, 0.15, p.divider),
        (0.32, 0.37, 0.56, 0.19, p.border),
        (0.08, 0.69, 0.63, 0.19, p.divider),
    ] {
        painter.rect_filled(
            Rect::from_min_max(point(x, y), point(x + w, y + h)),
            6.0,
            p.elevated,
        );
        painter.line_segment(
            [
                point(x + 0.06, y + h * 0.5),
                point(x + w * 0.54, y + h * 0.5),
            ],
            Stroke::new(2.0, tint),
        );
    }
    let main = [
        point(0.26, 0.20),
        point(0.26, 0.46),
        point(0.26, 0.78),
        point(0.26, 0.95),
    ];
    let topic = [main[0], point(0.72, 0.44), point(0.72, 0.57), main[2]];
    painter.add(egui::Shape::line(main.to_vec(), Stroke::new(2.5, p.accent)));
    painter.add(egui::Shape::line(topic.to_vec(), Stroke::new(2.0, p.muted)));
    for node in main {
        painter.circle(node, 6.0, p.surface, Stroke::new(2.5, p.accent));
    }
    for node in &topic[1..3] {
        painter.circle(*node, 5.0, p.surface, Stroke::new(2.0, p.muted));
    }
}
