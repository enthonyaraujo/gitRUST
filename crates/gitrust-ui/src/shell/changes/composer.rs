use egui::{
    Align, Align2, Button, FontId, Frame, Id, Layout, Panel, ScrollArea, Sense, Stroke, StrokeKind,
    TextEdit, TextFormat, Ui, vec2,
};
use gitrust_core::{
    changes::ChangesCommand,
    workspaces::{Checkout, Workspace},
};

use super::{ChangesUi, text_field};
use crate::{
    components as c,
    i18n::{Language, Text},
    theme::{Palette, size, weight},
};

#[derive(Default)]
pub(super) struct Coauthor {
    name: String,
    email: String,
}

impl std::fmt::Debug for Coauthor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Coauthor").finish_non_exhaustive()
    }
}

impl Coauthor {
    fn trailer(&self) -> Option<String> {
        let name = self.name.trim();
        let email = self.email.trim();
        let invalid = |c: char| c.is_control() || matches!(c, '<' | '>');
        let (user, host) = email.split_once('@')?;
        (!name.is_empty()
            && !name.contains(invalid)
            && !user.is_empty()
            && !host.is_empty()
            && !host.contains('@')
            && !email.contains(invalid)
            && !email.contains(char::is_whitespace))
        .then(|| format!("Co-authored-by: {name} <{email}>"))
    }
}

/// Append a standard trailer without changing the user's existing message.
fn append_trailer(description: &mut String, trailer: &str) {
    let message = description.trim_end_matches('\n');
    if !message.is_empty() {
        let same_block = message
            .lines()
            .next_back()
            .is_some_and(|last| last.starts_with("Co-authored-by: "));
        let required = if same_block { 1 } else { 2 };
        let present = description.len() - message.len();
        for _ in present..required {
            description.push('\n');
        }
    }
    description.push_str(trailer);
}

impl ChangesUi {
    pub(super) fn composer(
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
        let valid = enabled
            && !self.draft.title.trim().is_empty()
            && (self.counts[0] > 0 || self.draft.amend)
            && !matches!(workspace.checkout, Checkout::Detached(_))
            && workspace
                .changes
                .is_none_or(|counts| counts.conflicted == 0);
        let commit = Panel::bottom("commit-submission")
            .exact_size(33.0)
            .frame(c::panel(p.surface, 1))
            .show(ui, |ui| {
                commit_button(ui, branch, self.draft.amend, valid, l, p)
            })
            .inner;
        ScrollArea::vertical()
            .id_salt("commit-fields")
            .show(ui, |ui| {
                ui.add_enabled_ui(enabled, |ui| self.composer_fields(ui, l, p));
            });
        if commit && self.draft.amend {
            self.amend_confirmation = self.seen_version;
            return None;
        }
        commit.then(|| ChangesCommand::Commit(self.make_draft()))
    }

    fn composer_fields(&mut self, ui: &mut Ui, l: Language, p: Palette) {
        let id = Id::unique("commit-summary");
        if self.focus == Some(id) {
            self.focus = None;
            ui.memory_mut(|memory| memory.request_focus(id));
        }
        ui.horizontal(|ui| {
            let author_label = l.local("Author and signing", "Autoria e assinatura");
            if person_button(ui, Some(&self.draft.author.name), author_label, p).clicked() {
                self.author_dialog = true;
            }
            c::text_field(
                ui,
                id,
                l.local("Summary (required)", "Resumo (obrigatório)"),
                &mut self.draft.title,
            );
        });
        let description = l.local("Description", "Descrição");
        let height = (ui.available_height() - 40.0).clamp(34.0, 132.0);
        Frame::new()
            .fill(p.background)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(size::RADIUS)
            .inner_margin(4)
            .show(ui, |ui| {
                let response = ui
                    .add_sized(
                        [ui.available_width(), height],
                        TextEdit::multiline(&mut self.draft.description)
                            .id(Id::unique("commit-description"))
                            .desired_rows(2)
                            .frame(Frame::NONE)
                            .hint_text(description),
                    )
                    .accessible_name(description);
                if response.has_focus() {
                    ui.painter().line_segment(
                        [response.rect.left_top(), response.rect.left_bottom()],
                        Stroke::new(2.0, p.focus),
                    );
                }
                if person_button(ui, None, l.local("Add co-author", "Adicionar coautor"), p)
                    .clicked()
                {
                    self.coauthor = Some(Coauthor::default());
                }
            });
    }

    pub(super) fn show_coauthor(&mut self, ui: &Ui, l: Language, p: Palette) {
        let Some(person) = self.coauthor.as_mut() else {
            return;
        };
        let modal = c::modal(
            ui,
            "commit-coauthor",
            p,
            l.local("Add co-author", "Adicionar coautor"),
            |ui| {
                text_field(ui, l.local("Name", "Nome"), &mut person.name, false);
                text_field(ui, l.local("Email", "E-mail"), &mut person.email, false);
                let trailer = person.trailer();
                ui.horizontal(|ui| {
                    let add = ui
                        .add_enabled_ui(trailer.is_some(), |ui| {
                            c::primary(ui, l.local("Add", "Adicionar"), p)
                        })
                        .inner
                        .clicked();
                    let cancel = ui.button(l.text(Text::Cancel)).clicked();
                    (add.then_some(trailer).flatten(), cancel)
                })
                .inner
            },
        );
        let close = modal.should_close();
        let (trailer, cancel) = modal.inner;
        if let Some(trailer) = trailer {
            append_trailer(&mut self.draft.description, &trailer);
            self.coauthor = None;
        } else if cancel || close {
            self.coauthor = None;
        }
    }
}

fn person_button(ui: &mut Ui, author: Option<&str>, label: &str, p: Palette) -> egui::Response {
    let diameter = if author.is_some() { 27.0 } else { 22.0 };
    let (rect, response) = ui.allocate_exact_size(vec2(diameter, diameter), Sense::click());
    let painter = ui.painter_at(rect);
    let center = rect.center();
    if author.is_some() || response.hovered() || response.has_focus() {
        painter.circle_filled(
            center,
            diameter / 2.0 - 1.0,
            if response.hovered() {
                p.hover
            } else {
                p.elevated
            },
        );
    }
    if response.has_focus() {
        painter.circle_stroke(center, diameter / 2.0 - 1.0, Stroke::new(1.5, p.focus));
    }
    let initials: String = author
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(|word| word.chars().next())
        .take(2)
        .flat_map(char::to_uppercase)
        .collect();
    if initials.is_empty() {
        let at = if author.is_some() {
            center
        } else {
            center - vec2(2.0, 0.0)
        };
        let stroke = Stroke::new(1.4, p.muted);
        painter.circle_stroke(at - vec2(0.0, 3.0), 3.0, stroke);
        painter.line_segment([at + vec2(-5.0, 6.0), at + vec2(-3.5, 2.0)], stroke);
        painter.line_segment([at + vec2(-3.5, 2.0), at + vec2(3.5, 2.0)], stroke);
        painter.line_segment([at + vec2(3.5, 2.0), at + vec2(5.0, 6.0)], stroke);
        if author.is_none() {
            let at = at + vec2(7.0, 0.0);
            painter.line_segment([at - vec2(2.5, 0.0), at + vec2(2.5, 0.0)], stroke);
            painter.line_segment([at - vec2(0.0, 2.5), at + vec2(0.0, 2.5)], stroke);
        }
    } else {
        painter.text(
            center,
            Align2::CENTER_CENTER,
            initials,
            FontId::new(13.0, weight()),
            p.text,
        );
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
    response.on_hover_text(label)
}

fn commit_button(
    ui: &mut Ui,
    branch: &str,
    amend: bool,
    valid: bool,
    l: Language,
    p: Palette,
) -> bool {
    let mut label = egui::text::LayoutJob::default();
    let format = TextFormat {
        font_id: FontId::proportional(14.0),
        color: p.on_accent,
        ..Default::default()
    };
    if amend {
        label.append(l.local("Review amend", "Revisar emenda"), 0.0, format);
    } else {
        label.append(l.local("Commit to ", "Commit em "), 0.0, format.clone());
        label.append(
            branch,
            0.0,
            TextFormat {
                font_id: FontId::new(14.0, weight()),
                ..format
            },
        );
    }
    let button = Button::new(label)
        .fill(p.accent)
        .stroke(Stroke::NONE)
        .min_size(vec2(ui.available_width(), 30.0))
        .truncate();
    let response = ui
        .with_layout(Layout::top_down(Align::Center), |ui| {
            ui.visuals_mut().disabled_alpha = 0.5;
            ui.add_enabled(valid, button)
        })
        .inner
        .on_disabled_hover_text(l.local(
            "Enter a summary and stage the files to commit.",
            "Preencha o resumo e prepare os arquivos para o commit.",
        ));
    if response.has_focus() || response.hovered() {
        ui.painter().rect_stroke(
            response.rect,
            size::RADIUS,
            Stroke::new(
                1.0,
                if response.has_focus() {
                    p.focus
                } else {
                    p.text
                },
            ),
            StrokeKind::Inside,
        );
    }
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coauthors_are_validated_and_form_one_trailer_block() {
        let mut author = Coauthor {
            name: "Érica".into(),
            email: "erica@example.test".into(),
        };
        let trailer = author.trailer().expect("valid trailer");
        let mut description = String::from("Fix spacing.\n");
        append_trailer(&mut description, &trailer);
        append_trailer(&mut description, "Co-authored-by: John <j@example.test>");
        assert_eq!(
            description,
            "Fix spacing.\n\nCo-authored-by: Érica <erica@example.test>\nCo-authored-by: John <j@example.test>"
        );
        author.name.push_str("\nSigned-off-by: another");
        assert!(author.trailer().is_none());
    }
}
