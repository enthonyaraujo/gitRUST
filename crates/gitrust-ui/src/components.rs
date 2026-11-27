use egui::{
    Align, Button, Color32, FontId, Frame, Id, Key, Label, Layout, Margin, Modifiers, Panel, Rect,
    Response, RichText, Role, ScrollArea, Sense, Stroke, StrokeKind, TextEdit, TextStyle, Ui, Vec2,
    Widget, WidgetInfo, pos2, vec2,
};

use crate::i18n::{Language, Text};
use crate::theme::{FileStatus, Palette, size, weight};

pub(crate) fn primary(ui: &mut Ui, label: &str, p: Palette) -> Response {
    let enabled = ui.is_enabled();
    let r = ui.add(
        Button::new(RichText::new(label).family(weight()).color(if enabled {
            p.on_accent
        } else {
            p.disabled
        }))
        .fill(if enabled { p.accent } else { p.elevated })
        .stroke(Stroke::new(1.0, if enabled { p.accent } else { p.divider }))
        .min_size(vec2(0.0, size::CONTROL)),
    );
    let edge = if r.has_focus() {
        p.focus
    } else if r.is_pointer_button_down_on() {
        p.on_accent
    } else if r.hovered() {
        p.text
    } else {
        return r;
    };
    ui.painter().rect_stroke(
        r.rect.expand(2.0),
        size::RADIUS,
        Stroke::new(1.0, edge),
        egui::StrokeKind::Inside,
    );
    r
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Icon {
    Repo,
    Back,
    Branch,
    Remote,
    Chevron,
    Settings,
    Search,
    Close,
    Files,
    History,
    Copy,
    Mark,
}

impl Icon {
    pub(crate) fn show(self, ui: &mut Ui, dimension: f32, tint: Color32) {
        let (r, _) = ui.allocate_exact_size(Vec2::splat(dimension), Sense::hover());
        self.paint(&ui.painter_at(r), r, tint);
    }

    fn paint(self, pen: &egui::Painter, r: Rect, tint: Color32) {
        let dimension = r.width();
        let point = |x, y| pos2(r.left() + dimension * x, r.top() + dimension * y);
        let stroke = Stroke::new((dimension / 12.0).max(1.15), tint);
        let line = |a, b, c, d| pen.line_segment([point(a, b), point(c, d)], stroke);
        let dot = |x, y, radius| pen.circle_stroke(point(x, y), dimension * radius, stroke);
        let rect = |x, y, a, b| {
            pen.rect_stroke(
                egui::Rect::from_min_max(point(x, y), point(a, b)),
                1.0,
                stroke,
                egui::StrokeKind::Inside,
            );
        };
        match self {
            Self::Back => {
                line(0.24, 0.5, 0.82, 0.5);
                line(0.24, 0.5, 0.49, 0.25);
                line(0.24, 0.5, 0.49, 0.75);
            }
            Self::Repo => {
                rect(0.12, 0.22, 0.88, 0.8);
                line(0.3, 0.22, 0.3, 0.8);
                line(0.44, 0.42, 0.72, 0.42);
            }
            Self::Branch | Self::Mark => {
                if matches!(self, Self::Mark) {
                    rect(0.025, 0.025, 0.975, 0.975);
                }
                dot(0.31, 0.26, 0.09);
                dot(0.31, 0.75, 0.09);
                dot(0.71, 0.32, 0.09);
                line(0.31, 0.35, 0.31, 0.66);
                line(0.71, 0.41, 0.71, 0.51);
                line(0.71, 0.51, 0.31, 0.61);
            }
            Self::Settings => {
                for (y, x) in [(0.24, 0.36), (0.5, 0.68), (0.76, 0.47)] {
                    line(0.15, y, x - 0.09, y);
                    line(x + 0.09, y, 0.85, y);
                    dot(x, y, 0.085);
                }
            }
            Self::Chevron => {
                line(0.25, 0.40, 0.50, 0.64);
                line(0.50, 0.64, 0.75, 0.40);
            }
            Self::Remote => {
                line(0.18, 0.35, 0.82, 0.35);
                line(0.63, 0.16, 0.82, 0.35);
                line(0.63, 0.54, 0.82, 0.35);
                line(0.82, 0.69, 0.18, 0.69);
                line(0.37, 0.50, 0.18, 0.69);
                line(0.37, 0.88, 0.18, 0.69);
            }
            Self::Search => {
                dot(0.43, 0.43, 0.25);
                line(0.62, 0.62, 0.85, 0.85);
            }
            Self::Close => {
                line(0.25, 0.25, 0.75, 0.75);
                line(0.25, 0.75, 0.75, 0.25);
            }
            Self::Files => {
                rect(0.25, 0.12, 0.75, 0.88);
                line(0.38, 0.4, 0.62, 0.4);
                line(0.38, 0.59, 0.62, 0.59);
            }
            Self::History => {
                dot(0.5, 0.5, 0.36);
                line(0.5, 0.27, 0.5, 0.5);
                line(0.5, 0.5, 0.67, 0.59);
            }
            Self::Copy => {
                rect(0.35, 0.35, 0.87, 0.87);
                line(0.13, 0.13, 0.65, 0.13);
                line(0.13, 0.13, 0.13, 0.65);
                line(0.65, 0.13, 0.65, 0.25);
                line(0.13, 0.65, 0.25, 0.65);
            }
        }
    }
}

pub(crate) fn icon_button(ui: &mut Ui, icon: Icon, label: &str, p: Palette) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size::CONTROL), Sense::click());
    let style = ui.style().interact(&response);
    if response.hovered() || response.has_focus() || response.is_pointer_button_down_on() {
        ui.painter().rect(
            rect,
            size::RADIUS,
            style.bg_fill,
            Stroke::new(
                1.0,
                if response.has_focus() {
                    p.focus
                } else {
                    p.divider
                },
            ),
            egui::StrokeKind::Inside,
        );
    }
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::centered_and_justified(egui::Direction::TopDown)),
        |ui| {
            icon.show(ui, size::ICON, p.text);
        },
    );
    response.widget_info(|| WidgetInfo::labeled(Role::Button, ui.is_enabled(), label));
    response.on_hover_text(label)
}

pub(crate) fn badge(ui: &mut Ui, label: &str, foreground: Color32, background: Color32) {
    Frame::new()
        .fill(background)
        .corner_radius(size::RADIUS)
        .inner_margin(Margin::symmetric(size::SM, 2))
        .show(ui, |ui| {
            ui.label(
                RichText::new(label)
                    .small()
                    .family(weight())
                    .color(foreground),
            );
        });
}

pub(crate) fn status_indicator(ui: &mut Ui, label: &str, color: Color32) -> Response {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(8.0, 10.0), Sense::hover());
        ui.painter().circle_filled(r.center(), 3.0, color);
        ui.label(RichText::new(label).small().color(color))
    })
    .inner
}

pub(crate) fn text_field(ui: &mut Ui, id: Id, label: &str, value: &mut String) -> Response {
    ui.add_sized(
        [ui.available_width(), size::CONTROL],
        TextEdit::singleline(value)
            .id(id)
            .hint_text(label.to_owned()),
    )
    .accessible_name(label)
    .on_hover_text(label)
}

pub(crate) fn text_area(ui: &mut Ui, id: Id, label: &str, value: &mut String) -> Response {
    ui.add(
        TextEdit::multiline(value)
            .id(id)
            .desired_width(f32::INFINITY)
            .desired_rows(2)
            .hint_text(label.to_owned()),
    )
    .accessible_name(label)
    .on_hover_text(label)
}

pub(crate) fn search(
    ui: &mut Ui,
    id: Id,
    language: Language,
    p: Palette,
    value: &mut String,
) -> Response {
    ui.horizontal(|ui| {
        Icon::Search.show(ui, size::ICON, p.muted);
        let clear = !value.is_empty();
        let width = ui.available_width()
            - if clear {
                size::CONTROL + ui.spacing().item_spacing.x
            } else {
                0.0
            };
        let label = language.text(Text::Find);
        let mut response = ui
            .add_sized(
                [width.max(32.0), size::CONTROL],
                TextEdit::singleline(value).id(id).hint_text(label),
            )
            .accessible_name(label);
        if clear && icon_button(ui, Icon::Close, language.text(Text::Filters), p).clicked() {
            value.clear();
            response.mark_changed();
            response.request_focus();
        }
        response
    })
    .inner
}

pub(crate) fn dropdown<T: Copy + PartialEq>(
    ui: &mut Ui,
    id: &'static str,
    value: &mut T,
    options: &[(T, &str)],
) -> Response {
    let selected = options
        .iter()
        .find(|(candidate, _)| candidate == value)
        .map_or("", |(_, label)| *label);
    egui::ComboBox::from_id_salt(id)
        .width(ui.available_width().clamp(80.0, 160.0))
        .selected_text(selected)
        .show_ui(ui, |ui| {
            for &(choice, label) in options {
                let p = Palette::for_theme(ui.ctx().theme());
                if menu_choice(ui, label, *value == choice, p).clicked() {
                    *value = choice;
                    ui.close();
                }
            }
        })
        .response
}

pub(crate) fn tabs<T: Copy + PartialEq>(
    ui: &mut Ui,
    current: &mut T,
    options: &[(T, &str)],
    p: Palette,
) {
    ui.horizontal_wrapped(|ui| {
        for &(value, label) in options {
            if tab(ui, label, *current == value, 0.0, p) {
                *current = value;
            }
        }
    });
}

pub(crate) fn full_width_tabs<T: Copy + PartialEq>(
    ui: &mut Ui,
    current: &mut T,
    options: &[(T, &str)],
    p: Palette,
) {
    ui.horizontal(|ui| {
        let gap = ui.spacing().item_spacing.x;
        let count = f32::from(u16::try_from(options.len()).unwrap_or(u16::MAX).max(1));
        let width = (ui.available_width() + gap) / count - gap;
        for &(value, label) in options {
            if tab(ui, label, *current == value, width, p) {
                *current = value;
            }
        }
    });
}

fn tab(ui: &mut Ui, label: &str, selected: bool, width: f32, p: Palette) -> bool {
    let text =
        RichText::new(label)
            .family(weight())
            .color(if selected { p.accent } else { p.muted });
    let response = flat_button(ui, Button::new(text).truncate(), selected, width, p);
    response.widget_info(|| WidgetInfo::labeled(Role::Tab, ui.is_enabled(), label));
    ui.ctx()
        .accesskit_node_builder(response.id, |node| node.set_selected(selected));
    if selected {
        let bottom = response.rect.shrink2(vec2(5.0, 0.0));
        ui.painter().line_segment(
            [bottom.left_bottom(), bottom.right_bottom()],
            Stroke::new(2.0, p.accent),
        );
    }
    response.clicked()
}

fn flat_button(
    ui: &mut Ui,
    button: Button<'_>,
    selected: bool,
    min_width: f32,
    p: Palette,
) -> Response {
    let underlay = ui.painter().add(egui::Shape::Noop);
    let response = ui.add(button.frame(false).min_size(vec2(min_width, size::CONTROL)));
    let fill = if response.is_pointer_button_down_on() {
        p.active
    } else if response.hovered() {
        p.hover
    } else if selected {
        p.selected
    } else {
        Color32::TRANSPARENT
    };
    let outline = if response.has_focus() {
        Stroke::new(1.0, p.focus)
    } else {
        Stroke::NONE
    };
    if fill != Color32::TRANSPARENT || outline != Stroke::NONE {
        ui.painter().set(
            underlay,
            egui::epaint::RectShape::new(
                response.rect,
                size::RADIUS,
                fill,
                outline,
                egui::StrokeKind::Inside,
            ),
        );
    }
    response
}

pub(crate) fn menu_choice(ui: &mut Ui, label: &str, selected: bool, p: Palette) -> Response {
    let text = RichText::new(label).color(if selected { p.accent } else { p.text });
    let response = flat_button(ui, Button::new(text), selected, 0.0, p);
    response.widget_info(|| WidgetInfo::selected(Role::MenuItem, ui.is_enabled(), selected, label));
    response
}

pub(crate) fn branch_choice(
    ui: &mut Ui,
    label: &str,
    current: bool,
    selected: bool,
    p: Palette,
) -> Response {
    let text = if current {
        format!("✓  {label} · HEAD")
    } else {
        label.to_owned()
    };
    let response = workspace_row(ui, &text, None, selected, p);
    response.widget_info(|| WidgetInfo::selected(Role::MenuItem, ui.is_enabled(), selected, &text));
    response
}

pub(crate) fn workspace_row(
    ui: &mut Ui,
    label: &str,
    trailing: Option<&str>,
    selected: bool,
    p: Palette,
) -> Response {
    let text = RichText::new(label).color(if selected { p.accent } else { p.text });
    let mut button = Button::new(text).truncate();
    if let Some(trailing) = trailing {
        button = button.right_text(RichText::new(trailing).color(p.muted));
    }
    let response = flat_button(ui, button, selected, ui.available_width(), p);
    response.widget_info(|| WidgetInfo::selected(Role::Button, ui.is_enabled(), selected, label));
    response
}

pub(crate) fn repository_width(width: f32) -> f32 {
    (width * 0.29).clamp(230.0, 324.0)
}

pub(crate) fn context_button(
    ui: &mut Ui,
    width: f32,
    icon: Icon,
    label: &str,
    value: &str,
    interactive: bool,
    p: Palette,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(
        vec2(width, 42.0),
        if interactive {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let painter = ui.painter_at(rect);
    if response.hovered() && interactive {
        painter.rect_filled(rect, size::RADIUS, p.hover);
    }
    if response.has_focus() {
        painter.rect_stroke(
            rect,
            size::RADIUS,
            Stroke::new(1.0, p.focus),
            StrokeKind::Inside,
        );
    }
    let left = rect.min.x + if width >= 105.0 { 29.0 } else { 6.0 };
    if width >= 105.0 {
        let origin = pos2(rect.min.x + 5.0, rect.center().y - size::ICON * 0.5);
        icon.paint(
            &painter,
            Rect::from_min_size(origin, Vec2::splat(size::ICON)),
            p.muted,
        );
    }
    let dropdown = interactive && matches!(icon, Icon::Repo | Icon::Branch);
    let right = if dropdown {
        Icon::Chevron.paint(
            &painter,
            Rect::from_center_size(
                pos2(rect.right() - 11.0, rect.center().y),
                Vec2::splat(12.0),
            ),
            p.muted,
        );
        rect.right() - 21.0
    } else {
        rect.right() - 6.0
    };
    let remote = matches!(icon, Icon::Remote);
    for (text, strong, y) in [(label, remote, 5.0), (value, !remote, 21.0)] {
        let (color, font) = if strong {
            (
                if interactive { p.text } else { p.disabled },
                FontId::new(14.0, weight()),
            )
        } else {
            (p.muted, FontId::proportional(11.5))
        };
        let mut job =
            egui::text::LayoutJob::simple(text.to_owned(), font, color, (right - left).max(10.0));
        job.wrap.max_rows = 1;
        painter.galley(pos2(left, rect.min.y + y), painter.layout_job(job), color);
    }
    response.widget_info(|| {
        WidgetInfo::labeled(Role::Button, interactive, format!("{label}, {value}"))
    });
    response.on_hover_text(format!("{label} · {value}"))
}

pub(crate) fn adjust_panel(ui: &Ui, name: &'static str, delta: f32) {
    if delta != 0.0 {
        let id = ui.scope_id().with_salt(egui::IdSalt::new(name));
        if let Some(mut state) = egui::containers::panel::PanelState::load(ui.ctx(), id) {
            state.outer_rect.max.x =
                (state.outer_rect.max.x + delta).max(state.outer_rect.min.x + 1.0);
            ui.ctx().data_mut(|data| data.insert_persisted(id, state));
        }
    }
}

pub(crate) fn repository_sidebar(
    ui: &mut Ui,
    p: Palette,
    delta: f32,
    body: impl FnOnce(&mut Ui),
) -> f32 {
    let width = ui.available_width();
    let min = if width < 590.0 {
        (width * 0.41).max(136.0)
    } else {
        230.0
    };
    let max = (width - 226.0).max(min).min(460.0);
    adjust_panel(ui, "repository-sidebar", delta);
    Panel::left("repository-sidebar")
        .resizable(true)
        .default_size(repository_width(width).clamp(min, max))
        .size_range(min..=max)
        .frame(panel(p.surface, size::SM))
        .show(ui, body)
        .response
        .rect
        .width()
}

pub(crate) fn panel(fill: Color32, padding: i8) -> Frame {
    Frame::new().fill(fill).inner_margin(Margin::same(padding))
}

pub(crate) fn toolbar(ui: &mut Ui, p: Palette, body: impl FnOnce(&mut Ui)) {
    Panel::top("workspace-toolbar")
        .frame(panel(p.surface, 0).inner_margin(Margin::symmetric(size::MD, 5)))
        .show(ui, |ui| {
            ui.horizontal_wrapped(body);
        });
}

pub(crate) fn status_bar(ui: &mut Ui, p: Palette, body: impl FnOnce(&mut Ui)) {
    Panel::bottom("workspace-status")
        .frame(panel(p.surface, 0).inner_margin(Margin::symmetric(size::MD, size::XS)))
        .show(ui, |ui| {
            ui.horizontal(body);
        });
}

pub(crate) fn resizable_panel(ui: &mut Ui, p: Palette, body: impl FnOnce(&mut Ui)) {
    let maximum = (ui.available_width() - 250.0).clamp(210.0, 390.0);
    Panel::left("workspace-list")
        .resizable(true)
        .default_size(size::SIDEBAR.min(maximum))
        .size_range(210.0..=maximum)
        .frame(panel(p.surface, size::MD))
        .show(ui, body);
}

pub(crate) fn empty(ui: &mut Ui, icon: Icon, title: &str, message: &str, p: Palette) {
    icon.show(ui, 32.0, p.accent);
    ui.add_space(f32::from(size::SM));
    ui.heading(title);
    ui.label(RichText::new(message).color(p.muted));
}

pub(crate) fn notice(ui: &mut Ui, title: &str, message: &str, tint: Color32, p: Palette) {
    panel(p.elevated, size::MD)
        .corner_radius(size::RADIUS)
        .show(ui, |ui| {
            status_indicator(ui, title, tint);
            ui.label(RichText::new(message).color(p.muted));
        });
}

pub(crate) fn loading(ui: &mut Ui, label: &str, fraction: Option<f32>) {
    ui.horizontal(|ui| {
        if let Some(fraction) = fraction {
            ui.add(
                egui::ProgressBar::new(fraction)
                    .desired_width(88.0)
                    .show_percentage(),
            );
        } else {
            ui.spinner();
        }
        ui.label(label);
    });
}

pub(crate) fn modal<R>(
    ui: &Ui,
    id: &'static str,
    p: Palette,
    title: &str,
    body: impl FnOnce(&mut Ui) -> R,
) -> egui::ModalResponse<R> {
    let padding = if ui.ctx().content_rect().height() < 260.0 {
        size::MD
    } else {
        size::LG
    };
    egui::Modal::new(Id::unique(id))
        .frame(
            panel(p.surface, padding)
                .stroke(Stroke::new(1.0, p.border))
                .corner_radius(size::RADIUS),
        )
        .show(ui.ctx(), |ui| {
            ui.set_width((ui.ctx().content_rect().width() - 72.0).clamp(200.0, 370.0));
            ui.heading(title);
            ui.add_space(f32::from(size::SM));
            body(ui)
        })
}

pub(crate) fn confirmation(ui: &Ui, p: Palette, l: Language) -> Option<bool> {
    let r = modal(ui, "discard-draft", p, l.text(Text::Destructive), |ui| {
        ui.label(l.text(Text::DiscardDetails));
        ui.add_space(f32::from(size::SM));
        ui.horizontal(|ui| {
            let cancel = ui.button(l.text(Text::Cancel));
            if !ui.memory(|m| m.has_focus(cancel.id) || m.focused().is_some()) {
                cancel.request_focus();
            }
            if cancel.clicked() {
                return Some(false);
            }
            if primary(ui, l.text(Text::Confirm), p).clicked() {
                return Some(true);
            }
            None
        })
        .inner
    });
    r.inner.or_else(|| r.should_close().then_some(false))
}

pub(crate) fn truncated(ui: &mut Ui, value: &str, width: f32, style: TextStyle) {
    let r = ui.allocate_ui_with_layout(
        vec2(width.max(1.0), 0.0),
        Layout::left_to_right(Align::Center),
        |ui| {
            Label::new(RichText::new(value).text_style(style))
                .truncate()
                .selectable(false)
                .ui(ui)
        },
    );
    r.inner.on_hover_text(value);
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Row<'a> {
    pub(crate) name: &'a str,
    pub(crate) detail: &'a str,
    pub(crate) status: Option<FileStatus>,
}

fn row(ui: &mut Ui, id: Id, row: Row<'_>, selected: bool, p: Palette, l: Language) -> Response {
    let (_, rect) = ui.allocate_space(vec2(ui.available_width().max(1.0), size::ROW));
    let response = ui.interact(rect, id, Sense::click());
    let fill = if response.is_pointer_button_down_on() {
        p.active
    } else if selected {
        p.selected
    } else if response.hovered() {
        p.hover
    } else {
        p.surface
    };
    ui.painter().rect_filled(rect, size::RADIUS, fill);
    if selected {
        ui.painter().rect_filled(
            egui::Rect::from_min_size(rect.min, vec2(2.0, rect.height())),
            0.0,
            p.accent,
        );
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            size::RADIUS,
            Stroke::new(1.0, p.focus),
            egui::StrokeKind::Inside,
        );
    }
    let inner = rect.shrink2(vec2(f32::from(size::SM), 0.0));
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            if let Some(status) = row.status {
                let (mark, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
                ui.painter().rect_stroke(
                    mark,
                    3,
                    Stroke::new(1.0, p.divider),
                    egui::StrokeKind::Inside,
                );
                ui.painter().text(
                    mark.center(),
                    egui::Align2::CENTER_CENTER,
                    status.mark(),
                    crate::theme::code(),
                    p.status(status),
                );
            }
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.add(
                    Label::new(RichText::new(row.name).family(weight()))
                        .truncate()
                        .selectable(false),
                );
                if !row.detail.is_empty() {
                    ui.add(
                        Label::new(RichText::new(row.detail).small().color(p.muted))
                            .truncate()
                            .selectable(false),
                    );
                }
            });
        },
    );
    if response.has_focus() {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                id,
                egui::EventFilter {
                    vertical_arrows: true,
                    ..Default::default()
                },
            );
        });
    }
    response.widget_info(|| {
        let status = row.status.map_or("", |s| l.text(s.label()));
        WidgetInfo::labeled(
            Role::ListBoxOption,
            ui.is_enabled(),
            format!("{} {} {}", row.name, row.detail, status),
        )
    });
    ui.ctx()
        .accesskit_node_builder(response.id, |node| node.set_selected(selected));
    response.on_hover_ui(|ui| {
        ui.label(row.name);
        if !row.detail.is_empty() {
            ui.label(row.detail);
        }
        if let Some(s) = row.status {
            ui.label(l.text(s.label()));
        }
    })
}

#[derive(Debug, Default)]
pub(crate) struct ListSelection {
    pub(crate) index: Option<usize>,
    focused: Option<(Id, usize)>,
}

fn keyboard_target(current: usize, count: usize, key: Key) -> usize {
    match key {
        Key::ArrowUp => current.saturating_sub(1),
        Key::ArrowDown => (current + 1).min(count - 1),
        Key::Home => 0,
        Key::End => count - 1,
        _ => current,
    }
}

impl ListSelection {
    // The selected value is an item key, independent of its current position.
    pub(crate) fn retain(&mut self, keys: &[usize]) {
        self.index = self.index.filter(|key| keys.contains(key));
        self.focused = self.focused.filter(|(_, key)| keys.contains(key));
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn show<'a>(
        &mut self,
        ui: &mut Ui,
        id: Id,
        height: f32,
        count: usize,
        p: Palette,
        l: Language,
        key_at: impl Fn(usize) -> usize,
        mut item: impl FnMut(usize) -> Row<'a>,
    ) {
        if count == 0 {
            self.index = None;
            self.focused = None;
            return;
        }
        let keys = [Key::ArrowUp, Key::ArrowDown, Key::Home, Key::End];
        let target = self
            .focused
            .filter(|(widget, _)| ui.memory(|m| m.has_focus(*widget)))
            .and_then(|(_, key)| {
                let pressed = keys
                    .into_iter()
                    .find(|press| ui.input_mut(|i| i.consume_key(Modifiers::NONE, *press)))?;
                let old = (0..count).position(|index| key_at(index) == key)?;
                Some(keyboard_target(old, count, pressed))
            });
        if let Some(index) = target {
            self.index = Some(key_at(index));
        }
        let spacing = ui.spacing().item_spacing.y;
        let mut scroll = ScrollArea::vertical()
            .id_salt(id)
            .max_height(height.max(0.0))
            .auto_shrink([false; 2]);
        let previous =
            egui::scroll_area::State::load(ui.ctx(), ui.make_persistent_id(egui::IdSalt::new(id)));
        let scroll_target = target.or_else(|| {
            if previous.is_some() {
                return None;
            }
            self.index
                .and_then(|key| (0..count).position(|index| key_at(index) == key))
        });
        if let Some(index) = scroll_target {
            #[allow(clippy::cast_precision_loss)]
            let offset = index as f32 * (size::ROW + spacing);
            let previous = previous.map_or(0.0, |state| state.clamped_offset().y);
            let visible = height.max(size::ROW);
            scroll = scroll
                .vertical_scroll_offset(previous.min(offset).max(offset + size::ROW - visible));
        }
        scroll.show_rows(ui, size::ROW, count, |ui, rows| {
            for index in rows {
                let key = key_at(index);
                let widget = id.with(key);
                let response = ui
                    .push_id(key, |ui| {
                        row(ui, widget, item(key), self.index == Some(key), p, l)
                    })
                    .inner;
                if target == Some(index) || response.clicked() {
                    response.request_focus();
                }
                // A focused row from the prior frame must not override an arrow target.
                if response.clicked()
                    || (target.is_none() && response.has_focus())
                    || target == Some(index)
                {
                    self.focused = Some((widget, key));
                    self.index = Some(key);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_is_bounded() {
        assert_eq!(keyboard_target(0, 100, Key::ArrowUp), 0);
        assert_eq!(keyboard_target(99, 100, Key::ArrowDown), 99);
        assert_eq!(keyboard_target(0, 100, Key::End), 99);
        assert_eq!(keyboard_target(99, 100, Key::Home), 0);
    }

    #[test]
    fn drag_moves_the_workspace_divider_and_preserves_detail_width() {
        let ctx = egui::Context::default();
        let width = 880.0;
        let render = |events| {
            let mut detail_width = 0.0;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        vec2(width, 500.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    resizable_panel(ui, Palette::for_theme(egui::Theme::Dark), |ui| {
                        ui.set_min_width(ui.available_width());
                    });
                    detail_width = ui.available_width();
                },
            );
            output.textures_delta.clear();
            detail_width
        };
        let initial = render(Vec::new());
        let origin = pos2(width - initial, 110.0);
        let target = origin + vec2(90.0, 0.0);
        let press = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        render(vec![egui::Event::PointerMoved(origin)]);
        render(vec![press(origin, true)]);
        render(vec![egui::Event::PointerMoved(target)]);
        let after = render(vec![press(target, false)]);
        assert!(
            after < initial - 50.0,
            "divider did not move: {initial} → {after}"
        );
        assert!(
            after >= 250.0,
            "detail pane lost its minimum width: {after}"
        );
    }

    #[test]
    fn virtual_lists_render_only_viewport_rows_and_keep_ids() {
        let ctx = egui::Context::default();
        let mut expected = None;
        for count in [100, 1_000, 10_000] {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                let id = ui.make_persistent_id("virtual-test");
                assert_eq!(*expected.get_or_insert(id), id);
                ScrollArea::vertical()
                    .id_salt(id)
                    .max_height(180.0)
                    .show_rows(ui, size::ROW, count, |_, visible| {
                        assert!((4..=9).contains(&visible.len()));
                    });
            });
            output.textures_delta.clear();
        }
    }

    #[test]
    fn tabs_expose_selection_independently_of_focus() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut active = 1;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            tabs(
                ui,
                &mut active,
                &[(0, "Changes"), (1, "History")],
                Palette::for_theme(egui::Theme::Dark),
            );
        });
        output.textures_delta.clear();
        let tree = output.platform_output.accesskit_update.expect("tab tree");
        for (label, selected) in [("Changes", false), ("History", true)] {
            let (id, node) = tree
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(label))
                .expect("tab is mounted");
            assert_eq!(node.role(), Role::Tab);
            assert_eq!(node.is_selected(), Some(selected));
            assert_ne!(*id, tree.focus);
        }
    }

    #[test]
    fn flat_button_keeps_hover_fill_and_keyboard_focus_distinct() {
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            let ctx = egui::Context::default();
            let palette = Palette::for_theme(theme);
            let draw = |events| {
                let mut response = None;
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        response =
                            Some(flat_button(ui, Button::new("Changes"), true, 0.0, palette));
                    },
                );
                output.textures_delta.clear();
                let response = response.expect("rendered button");
                let background = output
                    .shapes
                    .into_iter()
                    .find_map(|clipped| match clipped.shape {
                        egui::Shape::Rect(shape) if shape.rect == response.rect => {
                            Some((shape.fill, shape.stroke))
                        }
                        _ => None,
                    })
                    .expect("button fill and outline");
                (response, background)
            };

            let (button, normal) = draw(Vec::new());
            assert_eq!(normal, (palette.selected, Stroke::NONE));
            let (_, hover) = draw(vec![egui::Event::PointerMoved(button.rect.center())]);
            assert_eq!(hover, (palette.hover, Stroke::NONE));
            let _ = draw(vec![egui::Event::PointerMoved(egui::pos2(-10.0, -10.0))]);
            ctx.memory_mut(|memory| memory.request_focus(button.id));
            let (_, focus) = draw(Vec::new());
            assert_eq!(focus, (palette.selected, Stroke::new(1.0, palette.focus)));
        }
    }

    #[test]
    fn list_keeps_focus_and_selection_when_rows_are_reordered() {
        let ctx = egui::Context::default();
        let id = Id::unique("reordered-list");
        let mut selection = ListSelection::default();
        let mut draw = |keys: [usize; 3]| {
            selection.retain(&keys);
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                selection.show(
                    ui,
                    id,
                    180.0,
                    keys.len(),
                    Palette::for_theme(egui::Theme::Dark),
                    Language::English,
                    |position| keys[position],
                    |_| Row {
                        name: "Fixture row",
                        detail: "",
                        status: None,
                    },
                );
            });
            output.textures_delta.clear();
            selection.index
        };

        let _ = draw([17, 43, 9]);
        ctx.memory_mut(|memory| memory.request_focus(id.with(43_usize)));
        assert_eq!(draw([17, 43, 9]), Some(43));
        assert_eq!(draw([43, 9, 17]), Some(43));
        assert!(ctx.memory(|memory| memory.has_focus(id.with(43_usize))));
    }
}
