use std::sync::Arc;

use egui::{
    Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, Style,
    TextStyle, Theme, Visuals,
};

const fn rgb(value: u32) -> Color32 {
    let [_, r, g, b] = value.to_be_bytes();
    Color32::from_rgb(r, g, b)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Palette {
    pub(crate) background: Color32,
    pub(crate) surface: Color32,
    pub(crate) elevated: Color32,
    pub(crate) selected: Color32,
    pub(crate) border: Color32,
    pub(crate) divider: Color32,
    pub(crate) text: Color32,
    pub(crate) muted: Color32,
    pub(crate) disabled: Color32,
    pub(crate) accent: Color32,
    pub(crate) on_accent: Color32,
    pub(crate) focus: Color32,
    pub(crate) hover: Color32,
    pub(crate) active: Color32,
    pub(crate) selection: Color32,
    pub(crate) success: Color32,
    pub(crate) warning: Color32,
    pub(crate) error: Color32,
    pub(crate) info: Color32,
    pub(crate) added_line: Color32,
    pub(crate) removed_line: Color32,
    pub(crate) added_edit: Color32,
    pub(crate) removed_edit: Color32,
}

/// Semantic statuses shared by the file list and component gallery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Untracked,
    Conflicted,
    Staged,
    Unstaged,
    Ignored,
}

impl FileStatus {
    pub(crate) const ALL: [Self; 9] = [
        Self::Added,
        Self::Modified,
        Self::Deleted,
        Self::Renamed,
        Self::Untracked,
        Self::Conflicted,
        Self::Staged,
        Self::Unstaged,
        Self::Ignored,
    ];

    pub(crate) const fn mark(self) -> &'static str {
        match self {
            Self::Added => "A",
            Self::Modified => "M",
            Self::Deleted => "D",
            Self::Renamed => "R",
            Self::Untracked => "?",
            Self::Conflicted => "!",
            Self::Staged => "+",
            Self::Unstaged => "−",
            Self::Ignored => "I",
        }
    }
}

impl Palette {
    pub(crate) const fn for_theme(theme: Theme) -> Self {
        match theme {
            Theme::Dark => Self {
                background: rgb(0x0018_1a1e),
                surface: rgb(0x001e_2126),
                elevated: rgb(0x0027_2b31),
                selected: rgb(0x0034_2c29),
                border: rgb(0x0073_7986),
                divider: rgb(0x0035_3942),
                text: rgb(0x00f0_eeeb),
                muted: rgb(0x00b4_b5be),
                disabled: rgb(0x008e_929e),
                accent: rgb(0x00f3_ad86),
                on_accent: rgb(0x0027_1b16),
                focus: rgb(0x009c_c7ff),
                hover: rgb(0x0030_343b),
                active: rgb(0x003a_3e46),
                selection: rgb(0x0032_455b),
                success: rgb(0x009d_dcb3),
                warning: rgb(0x00f0_c682),
                error: rgb(0x00ff_adab),
                info: rgb(0x00a6_cafd),
                added_line: rgb(0x0020_2f29),
                removed_line: rgb(0x0035_272a),
                added_edit: rgb(0x002c_4839),
                removed_edit: rgb(0x0060_3b40),
            },
            Theme::Light => Self {
                background: rgb(0x00fb_faf8),
                surface: rgb(0x00f2_f1ef),
                elevated: rgb(0x00e9_e7e4),
                selected: rgb(0x00f2_dfd4),
                border: rgb(0x0076_727a),
                divider: rgb(0x00d6_d3d0),
                text: rgb(0x0025_252a),
                muted: rgb(0x0054_515c),
                disabled: rgb(0x0062_5f68),
                accent: rgb(0x0089_3d21),
                on_accent: Color32::WHITE,
                focus: rgb(0x0024_5da5),
                hover: rgb(0x00e9_e6e2),
                active: rgb(0x00de_d9d5),
                selection: rgb(0x00bd_d8f7),
                success: rgb(0x0024_5c3b),
                warning: rgb(0x0072_5011),
                error: rgb(0x008c_2f36),
                info: rgb(0x0025_5788),
                added_line: rgb(0x00e3_f1e7),
                removed_line: rgb(0x00f7_e5e4),
                added_edit: rgb(0x00c2_dec9),
                removed_edit: rgb(0x00ec_c2c4),
            },
        }
    }

    pub(crate) const fn status(self, status: FileStatus) -> Color32 {
        match status {
            FileStatus::Added | FileStatus::Staged => self.success,
            FileStatus::Modified | FileStatus::Unstaged => self.warning,
            FileStatus::Deleted | FileStatus::Conflicted => self.error,
            FileStatus::Renamed | FileStatus::Untracked => self.info,
            FileStatus::Ignored => self.muted,
        }
    }
}

pub(crate) mod size {
    pub(crate) const XS: i8 = 4;
    pub(crate) const SM: i8 = 8;
    pub(crate) const MD: i8 = 12;
    pub(crate) const LG: i8 = 16;
    pub(crate) const XL: i8 = 24;
    pub(crate) const RADIUS: u8 = 5;
    pub(crate) const STROKE: f32 = 1.0;
    pub(crate) const CONTROL: f32 = 28.0;
    pub(crate) const HISTORY_BAR: f32 = 34.0;
    pub(crate) const ROW: f32 = 34.0;
    pub(crate) const ICON: f32 = 16.0;
    pub(crate) const SIDEBAR: f32 = 252.0;
    pub(crate) const COMPACT: f32 = 590.0;
    pub(crate) const SHORT: f32 = 280.0;
}

pub(crate) fn weight() -> FontFamily {
    FontFamily::Name(Arc::from("strong"))
}

pub(crate) fn code() -> FontId {
    FontId::monospace(13.0)
}

pub(crate) fn install(ctx: &Context) {
    let mut fonts = FontDefinitions::default();
    for (name, data, family) in [
        (
            "source-sans",
            include_bytes!("../assets/SourceSans3-Regular.ttf").as_slice(),
            FontFamily::Proportional,
        ),
        (
            "source-code",
            include_bytes!("../assets/SourceCodePro-Regular.ttf").as_slice(),
            FontFamily::Monospace,
        ),
        (
            "source-semibold",
            include_bytes!("../assets/SourceSans3-Semibold.ttf").as_slice(),
            weight(),
        ),
    ] {
        fonts
            .font_data
            .insert(name.into(), Arc::new(FontData::from_static(data)));
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, name.into());
    }
    ctx.set_fonts(fonts);
    for theme in [Theme::Dark, Theme::Light] {
        ctx.set_style_of(theme, style(theme));
    }
    ctx.set_theme(egui::ThemePreference::System);
}

fn style(theme: Theme) -> Style {
    let p = Palette::for_theme(theme);
    let mut visuals = match theme {
        Theme::Dark => Visuals::dark(),
        Theme::Light => Visuals::light(),
    };
    visuals.panel_fill = p.background;
    visuals.window_fill = p.surface;
    visuals.window_stroke = Stroke::new(size::STROKE, p.divider);
    visuals.extreme_bg_color = p.background;
    visuals.faint_bg_color = p.elevated;
    visuals.code_bg_color = p.elevated;
    visuals.hyperlink_color = p.info;
    visuals.warn_fg_color = p.warning;
    visuals.error_fg_color = p.error;
    visuals.selection.bg_fill = p.selection;
    visuals.selection.stroke = Stroke::new(1.0, p.focus);
    for (widget, fill, text, edge) in [
        (
            &mut visuals.widgets.noninteractive,
            p.surface,
            p.text,
            p.divider,
        ),
        (&mut visuals.widgets.inactive, p.elevated, p.text, p.border),
        (&mut visuals.widgets.hovered, p.hover, p.text, p.border),
        (&mut visuals.widgets.active, p.active, p.text, p.focus),
        (&mut visuals.widgets.open, p.selected, p.text, p.border),
    ] {
        widget.bg_fill = fill;
        widget.weak_bg_fill = fill;
        widget.bg_stroke = Stroke::new(size::STROKE, edge);
        widget.fg_stroke = Stroke::new(size::STROKE, text);
        widget.corner_radius = CornerRadius::same(size::RADIUS);
        widget.expansion = 0.0;
    }
    visuals.disabled_alpha = 0.68;
    visuals.button_frame = false;
    let spacing = egui::style::Spacing {
        item_spacing: egui::vec2(f32::from(size::SM), f32::from(size::XS)),
        interact_size: egui::vec2(36.0, size::CONTROL),
        icon_width: size::ICON,
        button_padding: egui::vec2(9.0, 3.0),
        ..Default::default()
    };
    Style {
        visuals,
        spacing,
        animation_time: 0.0,
        text_styles: [
            (TextStyle::Heading, FontId::new(23.0, weight())),
            (TextStyle::Body, FontId::proportional(15.0)),
            (TextStyle::Small, FontId::proportional(13.0)),
            (TextStyle::Button, FontId::proportional(14.0)),
            (TextStyle::Monospace, code()),
        ]
        .into(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(color: Color32) -> f32 {
        let linear = |channel| {
            let v = f32::from(channel) / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(color.r()) + 0.7152 * linear(color.g()) + 0.0722 * linear(color.b())
    }

    fn ratio(fg: Color32, bg: Color32) -> f32 {
        let (a, b) = (luminance(fg), luminance(bg));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn text_and_diff_roles_meet_wcag_aa_reference() {
        for theme in [Theme::Light, Theme::Dark] {
            let p = Palette::for_theme(theme);
            for bg in [
                p.background,
                p.surface,
                p.elevated,
                p.selected,
                p.hover,
                p.selection,
                p.added_line,
                p.removed_line,
            ] {
                for fg in [p.text, p.muted] {
                    assert!(ratio(fg, bg) >= 4.5, "{theme:?}: {fg:?} on {bg:?}");
                }
            }
            for (fg, bg) in [
                (p.on_accent, p.accent),
                (p.success, p.added_edit),
                (p.error, p.removed_edit),
                (p.text, p.added_edit),
                (p.text, p.removed_edit),
            ] {
                assert!(ratio(fg, bg) >= 4.5, "{theme:?}: {fg:?} on {bg:?}");
            }
            for status in FileStatus::ALL {
                assert!(ratio(p.status(status), p.selected) >= 4.5);
            }
            for bg in [
                p.background,
                p.surface,
                p.elevated,
                p.hover,
                p.active,
                p.selected,
            ] {
                assert!(ratio(p.focus, bg) >= 3.0);
            }
        }
    }
}
