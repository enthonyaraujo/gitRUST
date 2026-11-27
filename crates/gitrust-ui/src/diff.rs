use std::collections::BTreeSet;

use gitrust_core::changes::{ChangePreview, PatchRow, PatchSelection};

use std::borrow::Cow;
use std::ops::Range;

use egui::{
    Align, Color32, Id, Key, Label, Layout, Modifiers, Rect, RichText, Role, ScrollArea, Sense, Ui,
    WidgetInfo, text::LayoutJob, vec2,
};

use crate::components::{self, Icon};
use crate::i18n::{Language, Text};
use crate::theme::{Palette, code, size};

const LINE_HEIGHT: f32 = 21.0;
const GUTTER: f32 = 54.0;
const PADDING: f32 = 8.0;
const TAB_WIDTH: usize = 4;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum DiffMode {
    #[default]
    Unified,
    Split,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Line<'a> {
    Hunk(&'a str),
    Context(usize, usize, &'a str),
    Change {
        old: Option<(usize, &'a str)>,
        new: Option<(usize, &'a str)>,
    },
}

#[derive(Clone, Copy, Debug)]
struct Cell<'a> {
    number: Option<usize>,
    text: &'a str,
    mark: &'static str,
    tone: Tone,
    edit: Option<(usize, usize)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tone {
    Hunk,
    Plain,
    Removed,
    Added,
    Missing,
}

impl Tone {
    const fn colors(self, p: Palette) -> (Color32, Color32, Color32) {
        match self {
            Self::Hunk => (p.elevated, p.info, p.elevated),
            Self::Plain => (p.background, p.text, p.background),
            Self::Removed => (p.removed_line, p.error, p.removed_edit),
            Self::Added => (p.added_line, p.success, p.added_edit),
            Self::Missing => (p.surface, p.muted, p.surface),
        }
    }
}

impl<'a> Cell<'a> {
    const fn new(number: Option<usize>, text: &'a str, mark: &'static str, tone: Tone) -> Self {
        Self {
            number,
            text,
            mark,
            tone,
            edit: None,
        }
    }
}

impl<'a> Line<'a> {
    fn cells(self) -> [Cell<'a>; 2] {
        let missing = Cell::new(None, "", "", Tone::Missing);
        match self {
            Self::Hunk(text) => [Cell::new(None, text, "", Tone::Hunk), missing],
            Self::Context(old, new, text) => [
                Cell::new(Some(old), text, "", Tone::Plain),
                Cell::new(Some(new), text, "", Tone::Plain),
            ],
            Self::Change { old, new } => {
                let mut before = old.map_or(missing, |(n, text)| {
                    Cell::new(Some(n), text, "−", Tone::Removed)
                });
                let mut after = new.map_or(missing, |(n, text)| {
                    Cell::new(Some(n), text, "+", Tone::Added)
                });
                if let (Some((_, a)), Some((_, b))) = (old, new) {
                    let (old_span, new_span) = edit_spans(a, b);
                    before.edit = Some(old_span);
                    after.edit = Some(new_span);
                }
                [before, after]
            }
        }
    }
}

fn edit_spans(old: &str, new: &str) -> ((usize, usize), (usize, usize)) {
    let prefix = old
        .chars()
        .zip(new.chars())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum::<usize>();
    let suffix = old[prefix..]
        .chars()
        .rev()
        .zip(new[prefix..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum::<usize>();
    ((prefix, old.len() - suffix), (prefix, new.len() - suffix))
}

#[derive(Debug)]
pub(crate) struct TextDiff {
    lines: Vec<Line<'static>>,
    unified: Vec<(usize, usize)>,
    longest: usize,
}

impl TextDiff {
    pub(crate) fn new(lines: Vec<Line<'static>>) -> Self {
        let mut unified = Vec::with_capacity(lines.len());
        let mut longest = 0;
        for (index, line) in lines.iter().enumerate() {
            let [a, b] = line.cells();
            longest = longest
                .max(display_columns(a.text))
                .max(display_columns(b.text));
            if a.tone != Tone::Missing {
                unified.push((index, 0));
            }
            if b.tone == Tone::Added {
                unified.push((index, 1));
            }
        }
        Self {
            lines,
            unified,
            longest,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Diff<'a> {
    Text(&'a TextDiff),
    Real(&'a ChangePreview),
    Stress(usize),
    Binary,
    Empty,
}

impl<'a> Diff<'a> {
    fn count(self, mode: DiffMode) -> usize {
        match self {
            Self::Text(data) => match mode {
                DiffMode::Unified => data.unified.len(),
                DiffMode::Split => data.lines.len(),
            },
            Self::Real(preview) => patch_rows(preview, mode).len(),
            Self::Stress(len) => len,
            Self::Binary | Self::Empty => 0,
        }
    }

    fn row(self, mode: DiffMode, index: usize) -> [Cell<'a>; 2] {
        if let Self::Real(preview) = self {
            return patch_cells(preview, patch_rows(preview, mode)[index], mode);
        }
        if let Self::Text(data) = self {
            return match mode {
                DiffMode::Unified => {
                    let (row, side) = data.unified[index];
                    if side == 0 {
                        data.lines[row].cells()
                    } else {
                        swap_if(data.lines[row].cells(), true)
                    }
                }
                DiffMode::Split => data.lines[index].cells(),
            };
        }
        let n = index + 1;
        if index.is_multiple_of(20) {
            return Line::Hunk("@@ generated section @@").cells();
        }
        if index.is_multiple_of(3) {
            return swap_if(
                Line::Change {
                    old: None,
                    new: Some((n, "    cache.store(key, value);")),
                }
                .cells(),
                mode == DiffMode::Unified,
            );
        }
        Line::Context(n, n, "    let value = cache.lookup(key);").cells()
    }

    const fn longest(self) -> usize {
        match self {
            Self::Text(data) => data.longest,
            Self::Real(preview) => preview.columns,
            Self::Stress(_) => 38,
            Self::Binary | Self::Empty => 0,
        }
    }
}

fn patch_rows(preview: &ChangePreview, mode: DiffMode) -> &[PatchRow] {
    match mode {
        DiffMode::Unified => &preview.unified,
        DiffMode::Split => &preview.split,
    }
}

fn patch_cells<'a>(preview: &'a ChangePreview, row: PatchRow, mode: DiffMode) -> [Cell<'a>; 2] {
    match row {
        PatchRow::Metadata(index) => Line::Hunk(&preview.metadata[index]).cells(),
        PatchRow::Hunk(index) => Line::Hunk(&preview.hunks[index].header).cells(),
        PatchRow::NoNewline(sides) => sides.map(|show| {
            if show {
                Cell::new(None, "\\ No newline at end of file", "", Tone::Plain)
            } else {
                Cell::new(None, "", "", Tone::Missing)
            }
        }),
        PatchRow::Lines(hunk, [first, second]) => {
            let read = |index: Option<usize>| index.map(|index| &preview.hunks[hunk].lines[index]);
            let a = read(first);
            let b = read(second);
            if let Some(line) = a
                && let (Some(old), Some(new)) = (line.old, line.new)
            {
                return Line::Context(old as usize, new as usize, &line.text).cells();
            }
            let removed = |line: Option<&'a gitrust_core::changes::PatchLine>| {
                line.and_then(|line| line.old.map(|number| (number as usize, line.text.as_str())))
            };
            let added = |line: Option<&'a gitrust_core::changes::PatchLine>| {
                line.and_then(|line| line.new.map(|number| (number as usize, line.text.as_str())))
            };
            let swapped = mode == DiffMode::Unified && a.is_some_and(|line| line.old.is_none());
            if swapped {
                swap_if(
                    Line::Change {
                        old: removed(b),
                        new: added(a),
                    }
                    .cells(),
                    true,
                )
            } else {
                Line::Change {
                    old: removed(a),
                    new: added(b),
                }
                .cells()
            }
        }
    }
}

fn selection_at(source: Diff<'_>, mode: DiffMode, index: usize) -> [Option<PatchSelection>; 2] {
    let Diff::Real(preview) = source else {
        return [None; 2];
    };
    if !preview.partial {
        return [None; 2];
    }
    match patch_rows(preview, mode)[index] {
        PatchRow::Hunk(hunk) => [Some(PatchSelection { hunk, line: None }), None],
        PatchRow::Lines(hunk, lines) => std::array::from_fn(|side| {
            lines[side]
                .filter(|&position| {
                    let line = &preview.hunks[hunk].lines[position];
                    (mode == DiffMode::Split || side == 0)
                        && (line.old.is_none() || line.new.is_none())
                })
                .map(|line| PatchSelection {
                    hunk,
                    line: Some(line),
                })
        }),
        _ => [None; 2],
    }
}

fn swap_if(mut cells: [Cell<'_>; 2], swap: bool) -> [Cell<'_>; 2] {
    if swap {
        cells.swap(0, 1);
    }
    cells
}

#[derive(Debug, Default)]
pub(crate) struct DiffView {
    #[cfg(test)]
    visible: Range<usize>,
    copied: bool,
    focus_on_open: bool,
    active: Option<(Id, DiffMode)>,
}

impl DiffView {
    pub(crate) fn focus_on_open(&mut self) {
        self.focus_on_open = true;
    }

    pub(crate) fn show(
        &mut self,
        ui: &mut Ui,
        id: Id,
        source: Diff<'_>,
        mode: &mut DiffMode,
        l: Language,
        p: Palette,
    ) {
        self.show_with_selection(ui, id, source, mode, l, p, None);
    }

    pub(crate) fn show_commit(
        &mut self,
        ui: &mut Ui,
        id: Id,
        patch: &ChangePreview,
        mode: &mut DiffMode,
        l: Language,
        p: Palette,
    ) {
        let filename = patch.key.path.to_string_lossy();
        self.show_diff(
            ui,
            id,
            Diff::Real(patch),
            mode,
            l,
            p,
            None,
            Some(filename.as_ref()),
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn show_with_selection(
        &mut self,
        ui: &mut Ui,
        id: Id,
        source: Diff<'_>,
        mode: &mut DiffMode,
        l: Language,
        p: Palette,
        selection: Option<&mut BTreeSet<PatchSelection>>,
    ) {
        self.show_diff(ui, id, source, mode, l, p, selection, None);
    }

    #[allow(clippy::too_many_arguments)]
    fn show_diff(
        &mut self,
        ui: &mut Ui,
        id: Id,
        source: Diff<'_>,
        mode: &mut DiffMode,
        l: Language,
        p: Palette,
        selection: Option<&mut BTreeSet<PatchSelection>>,
        commit_path: Option<&str>,
    ) {
        if self.active != Some((id, *mode)) {
            self.copied = false;
        }
        let mut copy = commit_path.is_some_and(|path| {
            commit_diff_header(ui, path, mode, l, p, self.copied, source.count(*mode) > 0)
        });
        if source.count(*mode) == 0 {
            self.active = None;
            self.copied = false;
            let key = if matches!(source, Diff::Binary) {
                Text::Binary
            } else {
                Text::EmptyDiff
            };
            ui.label(RichText::new(l.text(key)).color(p.muted));
            return;
        }
        if commit_path.is_none() {
            copy = diff_toolbar(ui, mode, l, p, self.copied);
        }
        self.active = Some((id, *mode));
        if *mode == DiffMode::Split {
            split_header(ui, l, p, (ui.available_width() / 2.0).floor());
        }
        self.viewport(ui, id.with(*mode as u8), source, *mode, l, p, selection);
        if copy {
            ui.ctx().copy_text(copy_diff(source));
            self.copied = true;
        }
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn viewport(
        &mut self,
        ui: &mut Ui,
        id: Id,
        source: Diff<'_>,
        mode: DiffMode,
        l: Language,
        p: Palette,
        mut selection: Option<&mut BTreeSet<PatchSelection>>,
    ) {
        let count = source.count(mode);
        let split = mode == DiffMode::Split;
        let font_width = ui.fonts_mut(|fonts| fonts.glyph_width(&code(), 'M'));
        #[allow(clippy::cast_precision_loss)]
        let text_width = font_width * source.longest() as f32 + PADDING * 3.0;
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let keyboard = id.with("keyboard");
            let mut scroll = ScrollArea::both()
                .id_salt(id)
                .content_margin(0)
                .auto_shrink([false; 2]);
            if ui.memory(|m| m.has_focus(keyboard)) {
                let prior = egui::scroll_area::State::load(
                    ui.ctx(),
                    ui.make_persistent_id(egui::IdSalt::new(id)),
                )
                .map_or(egui::Vec2::ZERO, |state| state.clamped_offset());
                if let Some(offset) = keyboard_scroll(ui, prior) {
                    scroll = scroll.scroll_offset(offset);
                }
            }
            let area = scroll.show_viewport(ui, |ui, viewport| {
                let pane = (viewport.width() / 2.0).floor().max(1.0);
                let split_overflow = (GUTTER + text_width - pane).max(0.0);
                let content_width = if split {
                    pane * 2.0 + split_overflow
                } else {
                    viewport.width().max(GUTTER * 2.0 + text_width)
                };
                #[allow(clippy::cast_precision_loss)]
                ui.set_min_size(vec2(content_width, LINE_HEIGHT * count as f32));
                let rows = rows_in_view(viewport, count);
                #[cfg(test)]
                self.visible.clone_from(&rows);
                let origin = ui.max_rect().min;
                for index in rows {
                    let cells = source.row(mode, index);
                    #[allow(clippy::cast_precision_loss)]
                    let y = origin.y + index as f32 * LINE_HEIGHT;
                    let offset = viewport.min.x.max(0.0);
                    let (x, width) = if split {
                        (origin.x + viewport.min.x, pane * 2.0)
                    } else {
                        (origin.x, content_width)
                    };
                    let row = Rect::from_min_size(egui::pos2(x, y), vec2(width, LINE_HEIGHT));
                    let mut row_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .id_salt(index)
                            .max_rect(row)
                            .layout(Layout::left_to_right(Align::Center)),
                    );
                    row_ui.spacing_mut().item_spacing.x = 0.0;
                    render_row(&mut row_ui, cells, mode, width, offset, l, p);
                    if let Some(selection) = selection.as_deref_mut() {
                        for (side, item) in
                            selection_at(source, mode, index).into_iter().enumerate()
                        {
                            if let Some(item) = item {
                                let left = if side == 0 {
                                    row.left()
                                } else {
                                    row.center().x
                                };
                                patch_checkbox(
                                    &mut row_ui,
                                    selection,
                                    item,
                                    egui::pos2(left + 3.0, y),
                                    l,
                                    p,
                                );
                            }
                        }
                    }
                }
            });
            let focused = ui.interact(area.inner_rect, keyboard, Sense::focusable_noninteractive());
            focused.widget_info(|| {
                WidgetInfo::labeled(Role::ScrollView, true, l.text(Text::DiffViewport))
            });
            let focus_shortcut = ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::F6));
            if std::mem::take(&mut self.focus_on_open) || focus_shortcut {
                focused.request_focus();
            }
            if focused.has_focus() {
                ui.memory_mut(|m| {
                    m.set_focus_lock_filter(
                        keyboard,
                        egui::EventFilter {
                            horizontal_arrows: true,
                            vertical_arrows: true,
                            ..Default::default()
                        },
                    );
                });
                ui.painter().rect_stroke(
                    area.inner_rect,
                    0,
                    egui::Stroke::new(1.0, p.focus),
                    egui::StrokeKind::Inside,
                );
            }
        });
    }
}

fn patch_checkbox(
    ui: &mut Ui,
    selection: &mut BTreeSet<PatchSelection>,
    item: PatchSelection,
    position: egui::Pos2,
    l: Language,
    p: Palette,
) {
    let selected = selection.contains(&item);
    let label = if item.line.is_some() {
        l.local("Select line", "Selecionar linha")
    } else {
        l.local("Select hunk", "Selecionar trecho")
    };
    let rect = Rect::from_min_size(position, vec2(17.0, LINE_HEIGHT));
    let response = ui.interact(rect, ui.scope_id().with(item), Sense::click());
    response.widget_info(|| WidgetInfo::selected(Role::CheckBox, ui.is_enabled(), selected, label));
    let checkbox = Rect::from_center_size(rect.center(), vec2(12.0, 12.0));
    ui.painter().rect(
        checkbox,
        2,
        if selected { p.accent } else { p.surface },
        egui::Stroke::new(
            1.0,
            if response.has_focus() || response.hovered() {
                p.focus
            } else {
                p.muted
            },
        ),
        egui::StrokeKind::Inside,
    );
    if selected {
        let mid = checkbox.center();
        ui.painter().add(egui::Shape::line(
            vec![
                mid + vec2(-3.0, 0.0),
                mid + vec2(-1.0, 2.0),
                mid + vec2(3.0, -2.0),
            ],
            egui::Stroke::new(1.3, p.background),
        ));
    }
    if response.on_hover_text(label).clicked() && !selection.remove(&item) {
        selection.insert(item);
    }
}

fn keyboard_scroll(ui: &Ui, offset: egui::Vec2) -> Option<egui::Vec2> {
    let mut movement = egui::Vec2::ZERO;
    for (key, delta) in [
        (Key::ArrowLeft, vec2(-48.0, 0.0)),
        (Key::ArrowRight, vec2(48.0, 0.0)),
        (Key::ArrowUp, vec2(0.0, -LINE_HEIGHT)),
        (Key::ArrowDown, vec2(0.0, LINE_HEIGHT)),
        (Key::PageUp, vec2(0.0, -LINE_HEIGHT * 8.0)),
        (Key::PageDown, vec2(0.0, LINE_HEIGHT * 8.0)),
    ] {
        if ui.input_mut(|input| input.consume_key(Modifiers::NONE, key)) {
            movement += delta;
        }
    }
    (movement != egui::Vec2::ZERO).then_some((offset + movement).max(egui::Vec2::ZERO))
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn rows_in_view(viewport: Rect, count: usize) -> Range<usize> {
    let first = (viewport.min.y / LINE_HEIGHT).floor() as usize;
    let last = (viewport.max.y / LINE_HEIGHT).ceil() as usize;
    first.min(count)..last.min(count)
}

fn render_row(
    ui: &mut Ui,
    [a, b]: [Cell<'_>; 2],
    mode: DiffMode,
    width: f32,
    offset: f32,
    l: Language,
    p: Palette,
) {
    if mode == DiffMode::Split && a.tone != Tone::Hunk {
        render_cell(ui, a, [a.number, None], width / 2.0, false, offset, l, p);
        render_cell(ui, b, [None, b.number], width / 2.0, false, offset, l, p);
    } else {
        let numbers = match a.tone {
            Tone::Plain => [a.number, b.number],
            Tone::Added => [None, a.number],
            _ => [a.number, None],
        };
        render_cell(ui, a, numbers, width, mode == DiffMode::Unified, 0.0, l, p);
    }
}

fn commit_diff_header(
    ui: &mut Ui,
    path: &str,
    mode: &mut DiffMode,
    l: Language,
    p: Palette,
    copied: bool,
    has_rows: bool,
) -> bool {
    components::panel(p.elevated, 0)
        .inner_margin(egui::Margin::symmetric(8, 0))
        .show(ui, |ui| {
            let ((), copy) = egui::Sides::new()
                .shrink_left()
                .truncate()
                .height(size::HISTORY_BAR)
                .show(
                    ui,
                    |ui| {
                        ui.add(Label::new(path).truncate()).on_hover_text(path);
                    },
                    |ui| {
                        ui.add_enabled_ui(has_rows, |ui| {
                            let copy = components::icon_button(
                                ui,
                                Icon::Copy,
                                l.text(Text::CopyFullDiff),
                                p,
                            )
                            .clicked();
                            let options = components::icon_button(
                                ui,
                                Icon::Settings,
                                l.local("Diff options", "Opções do diff"),
                                p,
                            );
                            egui::Popup::menu(&options).show(|ui| {
                                for (value, key) in [
                                    (DiffMode::Unified, Text::Unified),
                                    (DiffMode::Split, Text::Split),
                                ] {
                                    if ui.selectable_value(mode, value, l.text(key)).clicked() {
                                        ui.close();
                                    }
                                }
                            });
                            if copied {
                                ui.label(
                                    RichText::new(l.text(Text::Copied)).small().color(p.success),
                                );
                            }
                            copy
                        })
                        .inner
                    },
                );
            copy
        })
        .inner
}

fn diff_toolbar(ui: &mut Ui, mode: &mut DiffMode, l: Language, p: Palette, copied: bool) -> bool {
    ui.horizontal_wrapped(|ui| {
        components::tabs(
            ui,
            mode,
            &[
                (DiffMode::Unified, l.text(Text::Unified)),
                (DiffMode::Split, l.text(Text::Split)),
            ],
            p,
        );
        let copy = components::icon_button(ui, Icon::Copy, l.text(Text::CopyFullDiff), p).clicked();
        if copied {
            ui.label(RichText::new(l.text(Text::Copied)).small().color(p.success));
        }
        copy
    })
    .inner
}

fn split_header(ui: &mut Ui, l: Language, p: Palette, pane: f32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for key in [Text::Old, Text::New] {
            let (rect, _) = ui.allocate_exact_size(vec2(pane, LINE_HEIGHT), egui::Sense::hover());
            let mut label_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(rect.shrink2(vec2(PADDING, 0.0)))
                    .layout(Layout::left_to_right(Align::Center)),
            );
            label_ui.label(RichText::new(l.text(key)).small().color(p.muted));
        }
    });
}

fn copy_diff(source: Diff<'_>) -> String {
    if let Diff::Real(preview) = source {
        return preview.text.clone();
    }
    let mut text = String::new();
    for index in 0..source.count(DiffMode::Unified) {
        let cell = source.row(DiffMode::Unified, index)[0];
        match cell.tone {
            Tone::Plain => text.push(' '),
            Tone::Removed => text.push('-'),
            Tone::Added => text.push('+'),
            Tone::Hunk | Tone::Missing => {}
        }
        text.push_str(cell.text);
        text.push('\n');
    }
    text
}

fn display_columns(text: &str) -> usize {
    text.chars().fold(0, |column, ch| {
        column
            + match ch {
                '\t' => TAB_WIDTH - column % TAB_WIDTH,
                ch if ch.is_ascii() => 1,
                _ => 2,
            }
    })
}

fn expand_tabs<'a>(text: &'a str, column: Option<&mut usize>) -> Cow<'a, str> {
    let Some(column) = column else {
        return Cow::Borrowed(text);
    };
    if !text.contains('\t') {
        *column += text.chars().count();
        return Cow::Borrowed(text);
    }
    let mut expanded = String::with_capacity(text.len() + TAB_WIDTH);
    for ch in text.chars() {
        if ch == '\t' {
            let spaces = TAB_WIDTH - *column % TAB_WIDTH;
            expanded.extend(std::iter::repeat_n(' ', spaces));
            *column += spaces;
        } else {
            expanded.push(ch);
            *column += 1;
        }
    }
    Cow::Owned(expanded)
}

#[allow(clippy::too_many_arguments)]
fn render_cell(
    ui: &mut Ui,
    cell: Cell<'_>,
    numbers: [Option<usize>; 2],
    width: f32,
    unified: bool,
    offset: f32,
    l: Language,
    p: Palette,
) {
    let (background, tone, edit) = cell.tone.colors(p);
    let (rect, _) = ui.allocate_exact_size(vec2(width, LINE_HEIGHT), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, background);
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    let ui = &mut child;
    ui.set_clip_rect(ui.clip_rect().intersect(rect));
    ui.spacing_mut().item_spacing.x = 0.0;
    if cell.tone != Tone::Hunk {
        number(ui, if unified { numbers[0] } else { cell.number }, p);
        if unified {
            number(ui, numbers[1], p);
        }
    }
    let text_left = ui.next_widget_position().x
        + if cell.tone == Tone::Hunk {
            PADDING + 17.0
        } else {
            PADDING
        };
    let mut text_rect = egui::Rect::from_min_max(egui::pos2(text_left, rect.top()), rect.max);
    let clip = ui.clip_rect().intersect(text_rect);
    text_rect.min.x -= offset;
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(text_rect)
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            ui.set_clip_rect(clip);
            let mut job = LayoutJob::default();
            if cell.tone != Tone::Hunk {
                let mark = match cell.mark {
                    "" => "  ",
                    "−" => "− ",
                    _ => "+ ",
                };
                job.append(mark, 0.0, egui::TextFormat::simple(code(), tone));
            }
            let mut column = 0;
            let has_tabs = cell.text.contains('\t');
            let mut append = |text, background| {
                let text = expand_tabs(text, has_tabs.then_some(&mut column));
                job.append(
                    &text,
                    0.0,
                    egui::TextFormat {
                        font_id: code(),
                        color: tone,
                        background,
                        ..Default::default()
                    },
                );
            };
            if let Some((start, end)) = cell.edit {
                append(&cell.text[..start], Color32::TRANSPARENT);
                append(&cell.text[start..end], edit);
                append(&cell.text[end..], Color32::TRANSPARENT);
            } else {
                append(cell.text, Color32::TRANSPARENT);
            }
            if cell.tone != Tone::Missing {
                let label = ui.add(Label::new(job).extend().selectable(true));
                if cell.number.is_some() {
                    label.widget_info(|| {
                        WidgetInfo::labeled(Role::Label, true, accessible_line(cell, numbers, l))
                    });
                }
            }
        },
    );
}

fn accessible_line(cell: Cell<'_>, numbers: [Option<usize>; 2], l: Language) -> String {
    let [old, new] = numbers;
    let mut label = String::new();
    for (number, key) in [(old, Text::OldLine), (new, Text::NewLine)] {
        if let Some(number) = number {
            use std::fmt::Write as _;
            let _ = write!(label, "{} {number}. ", l.text(key));
        }
    }
    label.push_str(cell.text);
    label
}

fn number(ui: &mut Ui, number: Option<usize>, p: Palette) {
    let (rect, _) = ui.allocate_exact_size(vec2(GUTTER, LINE_HEIGHT), egui::Sense::hover());
    ui.painter().vline(
        rect.right(),
        rect.y_range(),
        egui::Stroke::new(1.0, p.divider),
    );
    if let Some(number) = number {
        ui.painter().text(
            rect.right_center() - vec2(PADDING, 0.0),
            egui::Align2::RIGHT_CENTER,
            number,
            code(),
            p.muted,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unified_counts_changes_and_split_aligns_context() {
        let data = TextDiff::new(vec![
            Line::Hunk("@@"),
            Line::Context(1, 2, "context"),
            Line::Change {
                old: Some((2, "old")),
                new: Some((3, "new")),
            },
            Line::Change {
                old: None,
                new: Some((4, "addition")),
            },
        ]);
        let diff = Diff::Text(&data);
        assert_eq!(diff.count(DiffMode::Unified), 5);
        assert_eq!(diff.count(DiffMode::Split), 4);
        assert_eq!(diff.row(DiffMode::Unified, 4)[0].tone, Tone::Added);
        assert_eq!(diff.row(DiffMode::Split, 3)[0].tone, Tone::Missing);
        assert_eq!(diff.row(DiffMode::Split, 1)[1].number, Some(2));
        assert_eq!(edit_spans("café aqui", "caso aqui"), ((2, 5), (2, 4)));
    }

    #[test]
    fn diff_renders_a_bounded_range_with_one_hundred_thousand_lines() {
        let ctx = egui::Context::default();
        let mut view = DiffView::default();
        for mut mode in [DiffMode::Unified, DiffMode::Split] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        vec2(640.0, 400.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    view.show(
                        ui,
                        Id::unique("stress"),
                        Diff::Stress(100_000),
                        &mut mode,
                        Language::English,
                        Palette::for_theme(egui::Theme::Dark),
                    );
                    assert!((8..=22).contains(&view.visible.len()), "{:?}", view.visible);
                },
            );
            output.textures_delta.clear();
        }
    }
}

#[cfg(test)]
mod viewport_tests {
    use super::*;

    #[test]
    fn hunk_glyphs_fit_vertically_inside_the_first_row() {
        let ctx = egui::Context::default();
        let mut view = DiffView::default();
        let mut mode = DiffMode::Unified;
        let data = TextDiff::new(vec![Line::Hunk("@@ Sample @@")]);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            view.show(
                ui,
                Id::unique("hunk-glyphs"),
                Diff::Text(&data),
                &mut mode,
                Language::English,
                Palette::for_theme(egui::Theme::Dark),
            );
        });
        output.textures_delta.clear();
        let found = output.shapes.iter().any(|shape| {
            if let egui::Shape::Text(text) = &shape.shape
                && text.galley.text() == "@@ Sample @@"
            {
                let bounds = text.visual_bounding_rect();
                return shape.clip_rect.top() <= bounds.top()
                    && shape.clip_rect.bottom() >= bounds.bottom();
            }
            false
        });
        assert!(found, "the first hunk's glyphs should fit in its clip row");
    }
}

#[cfg(test)]
mod refinement_tests {
    use super::*;

    fn sized_input(events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(640.0, 360.0))),
            events,
            ..Default::default()
        }
    }

    #[test]
    fn full_copy_uses_unified_body_and_retains_tabs_and_unicode() {
        let source = TextDiff::new(vec![
            Line::Hunk("@@ -3,2 +3,2 @@"),
            Line::Context(3, 3, "context"),
            Line::Change {
                old: Some((4, "\tgravação antiga")),
                new: Some((4, "\tgravação nova")),
            },
        ]);
        assert_eq!(
            copy_diff(Diff::Text(&source)),
            "@@ -3,2 +3,2 @@\n context\n-\tgravação antiga\n+\tgravação nova\n"
        );
        let mut column = 3;
        assert_eq!(expand_tabs("\tá\t!", Some(&mut column)), " á   !");
        assert_eq!(column, 9);
        assert_eq!(
            rows_in_view(
                Rect::from_min_max(egui::pos2(0.0, 21.0), egui::pos2(100.0, 42.0)),
                100_000
            ),
            1..2
        );
    }

    #[test]
    fn copy_control_exports_lines_beyond_the_split_viewport() {
        use egui::accesskit::{Action, ActionRequest, TreeId};

        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut view = DiffView::default();
        let mut mode = DiffMode::Split;
        let mut lines: Vec<_> = (1..=99)
            .map(|n| Line::Context(n, n, "visible context"))
            .collect();
        lines.push(Line::Context(100, 100, "\tgravação final  "));
        let source = TextDiff::new(lines);
        let mut draw = |events| {
            let mut output = ctx.run_ui(sized_input(events), |ui| {
                view.show(
                    ui,
                    Id::unique("verified-full-copy"),
                    Diff::Text(&source),
                    &mut mode,
                    Language::English,
                    Palette::for_theme(egui::Theme::Dark),
                );
            });
            output.textures_delta.clear();
            output.platform_output
        };

        let first = draw(Vec::new());
        let tree = first.accesskit_update.expect("accessible copy button");
        let (target_node, _) = tree
            .nodes
            .into_iter()
            .find(|(_, node)| node.label() == Some("Copy entire diff"))
            .expect("copy action has an accessible name");
        let output = draw(vec![egui::Event::AccessKitActionRequest(ActionRequest {
            action: Action::Click,
            target_tree: TreeId::ROOT,
            target_node,
            data: None,
        })]);
        let expected = " visible context\n".repeat(99) + " \tgravação final  \n";
        assert_eq!(output.commands, [egui::OutputCommand::CopyText(expected)]);
    }

    #[test]
    fn split_scroll_moves_both_text_runs_and_keeps_gutters_fixed() {
        let ctx = egui::Context::default();
        let mut view = DiffView::default();
        view.focus_on_open();
        let mut mode = DiffMode::Split;
        let source = crate::demo::Fixtures::default();
        let mut draw = |events| {
            let mut output = ctx.run_ui(sized_input(events), |ui| {
                view.show(
                    ui,
                    Id::unique("split-keyboard"),
                    source.file(6),
                    &mut mode,
                    Language::English,
                    Palette::for_theme(egui::Theme::Dark),
                );
            });
            output.textures_delta.clear();
            output
        };
        let _ = draw(Vec::new());
        let before = draw(Vec::new());
        let key = egui::Event::Key {
            key: Key::ArrowRight,
            physical_key: Some(Key::ArrowRight),
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let after = draw(vec![key.clone()]);
        let _ = draw(Vec::new());
        let after_keys = draw(vec![key]);
        let positions = |output: egui::FullOutput, prefix: &str| {
            output
                .shapes
                .into_iter()
                .filter_map(|shape| {
                    if let egui::Shape::Text(text) = shape.shape
                        && text.galley.text().starts_with(prefix)
                    {
                        return Some(text.pos.x);
                    }
                    None
                })
                .collect::<Vec<_>>()
        };
        let _ = draw(vec![egui::Event::PointerMoved(egui::pos2(160.0, 110.0))]);
        let _ = draw(vec![egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(-96.0, 0.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
            modifiers: Modifiers::NONE,
        }]);
        let after_wheel = draw(Vec::new());
        let before = [
            positions(before.clone(), "− fallback"),
            positions(before.clone(), "+ fallback"),
            positions(before.clone(), "@@"),
            positions(before, "2"),
        ];
        let after = [
            positions(after.clone(), "− fallback"),
            positions(after.clone(), "+ fallback"),
            positions(after.clone(), "@@"),
            positions(after, "2"),
        ];
        for index in 0..4 {
            let expected = if index < 2 { 48.0 } else { 0.0 };
            assert_ne!(before[index].len(), 0);
            assert_eq!(before[index].len(), after[index].len());
            for (a, b) in before[index].iter().zip(&after[index]) {
                assert!((a - b - expected).abs() < 1.0, "{a}, {b}, {expected}");
            }
        }
        let mut wheel_movement = [0.0; 2];
        for (index, prefix) in ["− fallback", "+ fallback"].into_iter().enumerate() {
            let after_key = positions(after_keys.clone(), prefix)[0];
            assert!((before[index][0] - after_key - 96.0).abs() < 1.0);
            wheel_movement[index] = after_key - positions(after_wheel.clone(), prefix)[0];
        }
        assert!(wheel_movement[0] > 0.0);
        assert!((wheel_movement[0] - wheel_movement[1]).abs() < 1.0);
        let old_hunk = positions(after_keys, "@@")[0];
        let new_hunk = positions(after_wheel, "@@")[0];
        assert!((old_hunk - new_hunk).abs() < 1.0);
    }

    #[test]
    fn mounted_diff_exposes_localized_line_numbers_to_accesskit() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut view = DiffView::default();
        let mut mode = DiffMode::Unified;
        let source = TextDiff::new(vec![Line::Context(7, 8, "sample")]);
        let mut found = false;
        for _ in 0..3 {
            let mut output = ctx.run_ui(sized_input(Vec::new()), |ui| {
                view.show(
                    ui,
                    Id::unique("diff-accessible"),
                    Diff::Text(&source),
                    &mut mode,
                    Language::Portuguese,
                    Palette::for_theme(egui::Theme::Dark),
                );
            });
            output.textures_delta.clear();
            if let Some(update) = output.platform_output.accesskit_update {
                found |= update.nodes.iter().any(|(_, node)| {
                    node.value() == Some("Antes, linha 7. Depois, linha 8. sample")
                });
            }
        }
        assert!(found);
    }
}
