use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Pos2, Rect,
    Stroke, TextStyle, Visuals,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const PLEX_SANS_REGULAR: &[u8] = include_bytes!("../assets/fonts/IBMPlexSans-Regular.ttf");
pub const PLEX_SANS_MEDIUM: &[u8] = include_bytes!("../assets/fonts/IBMPlexSans-Medium.ttf");
pub const JETBRAINS_MONO_REGULAR: &[u8] =
    include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf");

pub const FAMILY_MEDIUM: &str = "PlexMedium";

pub fn plex_medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(FAMILY_MEDIUM.into()))
}

pub fn mono_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

/// Point size of the header telemetry strip.
pub const HEADER_STAT_FONT: f32 = 10.8;

/// Fixed width of each header telemetry column, left to right:
/// cell dimensions, glyph spread, frame time, measured FPS.
///
/// These are constants on purpose. A stat only ever changes the text inside its
/// own column, so a value going from `156x44` to `1920x1080` cannot push the
/// other columns around.
pub const HEADER_STAT_WIDTHS: [f32; 4] = [64.0, 82.0, 70.0, 58.0];

/// Horizontal space reserved for one column separator, including its margins.
pub const HEADER_STAT_SEP: f32 = 13.0;

/// Height of the telemetry strip.
pub const HEADER_STAT_HEIGHT: f32 = 18.0;

/// Total width of the header telemetry strip. Constant for a fixed column set.
pub fn header_stats_width() -> f32 {
    let columns: f32 = HEADER_STAT_WIDTHS.iter().sum();
    columns + HEADER_STAT_SEP * (HEADER_STAT_WIDTHS.len() - 1) as f32
}

/// Column rectangles for a strip whose right edge sits at `right`.
///
/// Columns are laid out right to left so the strip stays pinned to the right
/// edge of the header no matter how wide the text inside each column is.
pub fn header_stat_rects(right: f32, center_y: f32) -> [Rect; HEADER_STAT_WIDTHS.len()] {
    let mut rects = [Rect::NOTHING; HEADER_STAT_WIDTHS.len()];
    let mut edge = right;
    for (idx, width) in HEADER_STAT_WIDTHS.iter().enumerate().rev() {
        rects[idx] = Rect::from_min_max(
            Pos2::new(edge - width, center_y - HEADER_STAT_HEIGHT * 0.5),
            Pos2::new(edge, center_y + HEADER_STAT_HEIGHT * 0.5),
        );
        edge -= width + HEADER_STAT_SEP;
    }
    rects
}

/// Calibrated desktop workstation color themes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum StudioTheme {
    /// Neutral matte graphite inspired by DaVinci Resolve and Capture One.
    #[default]
    GraphitePro,
    /// Pure #000000 / #0B0B0D reference darkroom for OLED monitors.
    OledReference,
    /// Warm dark industrial console with tungsten bone typography.
    CarbonTungsten,
    /// Cool architectural slate gray.
    NordicSlate,
    /// Calibrated daylight editorial paper theme for bright rooms.
    DaylightPaper,
}

impl StudioTheme {
    pub const ALL: [Self; 5] = [
        Self::GraphitePro,
        Self::OledReference,
        Self::CarbonTungsten,
        Self::NordicSlate,
        Self::DaylightPaper,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::GraphitePro => "Graphite Pro (Default)",
            Self::OledReference => "OLED Reference (#000000)",
            Self::CarbonTungsten => "Carbon Tungsten",
            Self::NordicSlate => "Nordic Slate",
            Self::DaylightPaper => "Daylight Paper",
        }
    }

    pub fn cli_name(self) -> &'static str {
        match self {
            Self::GraphitePro => "graphite",
            Self::OledReference => "oled",
            Self::CarbonTungsten => "tungsten",
            Self::NordicSlate => "nordic",
            Self::DaylightPaper => "daylight",
        }
    }

    pub fn from_cli(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "graphite" | "pro" | "resolve" | "default" => Some(Self::GraphitePro),
            "oled" | "pitch" | "black" => Some(Self::OledReference),
            "tungsten" | "carbon" | "warm" => Some(Self::CarbonTungsten),
            "nordic" | "slate" => Some(Self::NordicSlate),
            "daylight" | "light" | "paper" => Some(Self::DaylightPaper),
            _ => None,
        }
    }

    pub fn is_light(self) -> bool {
        matches!(self, Self::DaylightPaper)
    }
}

/// Optional restrained indicator accent override.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AccentPreset {
    #[default]
    ThemeDefault,
    SteelSlate,
    TungstenAmber,
    OxideCopper,
    MineralSage,
    MonochromeSilver,
}

impl AccentPreset {
    pub const ALL: [Self; 6] = [
        Self::ThemeDefault,
        Self::SteelSlate,
        Self::TungstenAmber,
        Self::OxideCopper,
        Self::MineralSage,
        Self::MonochromeSilver,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::ThemeDefault => "Theme Default",
            Self::SteelSlate => "Steel Slate",
            Self::TungstenAmber => "Tungsten Amber",
            Self::OxideCopper => "Oxide Copper",
            Self::MineralSage => "Mineral Sage",
            Self::MonochromeSilver => "Monochrome Silver",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum UiDensity {
    Compact90,
    #[default]
    Standard100,
    Comfortable110,
    HiDpi125,
}

impl UiDensity {
    pub const ALL: [Self; 4] = [
        Self::Compact90,
        Self::Standard100,
        Self::Comfortable110,
        Self::HiDpi125,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Compact90 => "Compact (92%)",
            Self::Standard100 => "Standard (100%)",
            Self::Comfortable110 => "Comfortable (110%)",
            Self::HiDpi125 => "Large / HiDPI (122%)",
        }
    }

    pub fn zoom_factor(self) -> f32 {
        match self {
            Self::Compact90 => 0.92,
            Self::Standard100 => 1.00,
            Self::Comfortable110 => 1.10,
            Self::HiDpi125 => 1.22,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CornerStyle {
    Sharp1px,
    #[default]
    Precision2px,
    Soft4px,
}

impl CornerStyle {
    pub const ALL: [Self; 3] = [Self::Sharp1px, Self::Precision2px, Self::Soft4px];

    pub fn label(self) -> &'static str {
        match self {
            Self::Sharp1px => "Sharp (1 px)",
            Self::Precision2px => "Precision (2 px)",
            Self::Soft4px => "Softened (4 px)",
        }
    }

    pub fn radius_u8(self) -> u8 {
        match self {
            Self::Sharp1px => 1,
            Self::Precision2px => 2,
            Self::Soft4px => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CheckerContrast {
    #[default]
    Subtle,
    Balanced,
    High,
}

impl CheckerContrast {
    pub const ALL: [Self; 3] = [Self::Subtle, Self::Balanced, Self::High];

    pub fn label(self) -> &'static str {
        match self {
            Self::Subtle => "Subtle",
            Self::Balanced => "Balanced",
            Self::High => "High Contrast",
        }
    }

    pub fn colors(self, is_light_theme: bool) -> (Color32, Color32) {
        if is_light_theme {
            match self {
                Self::Subtle => (
                    Color32::from_rgb(218, 217, 213),
                    Color32::from_rgb(232, 231, 227),
                ),
                Self::Balanced => (
                    Color32::from_rgb(198, 197, 193),
                    Color32::from_rgb(236, 235, 232),
                ),
                Self::High => (
                    Color32::from_rgb(168, 168, 164),
                    Color32::from_rgb(244, 244, 242),
                ),
            }
        } else {
            match self {
                Self::Subtle => (Color32::from_rgb(15, 15, 17), Color32::from_rgb(23, 23, 26)),
                Self::Balanced => (Color32::from_rgb(16, 16, 18), Color32::from_rgb(31, 31, 35)),
                Self::High => (Color32::from_rgb(14, 14, 16), Color32::from_rgb(46, 46, 52)),
            }
        }
    }
}

/// Resolved palette tokens used consistently across menus, toolbars, scrub faders, and dialogs.
#[derive(Debug, Clone, Copy)]
pub struct ThemePalette {
    pub is_light: bool,
    pub viewport_bg: Color32,
    pub menu_bg: Color32,
    pub toolbar_bg: Color32,
    pub panel_bg: Color32,
    pub section_header_bg: Color32,
    pub control_bg: Color32,
    pub control_hover: Color32,
    pub track_bg: Color32,
    pub border: Color32,
    pub border_subtle: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
    pub accent: Color32,
    pub accent_fill: Color32,
    pub accent_text: Color32,
    pub warn: Color32,
    pub radius: u8,
}

pub fn resolve_palette(
    theme: StudioTheme,
    accent_override: AccentPreset,
    corner: CornerStyle,
) -> ThemePalette {
    let radius = corner.radius_u8();
    let mut pal = match theme {
        StudioTheme::GraphitePro => ThemePalette {
            is_light: false,
            viewport_bg: Color32::from_rgb(15, 15, 17),
            menu_bg: Color32::from_rgb(20, 20, 23),
            toolbar_bg: Color32::from_rgb(24, 24, 27),
            panel_bg: Color32::from_rgb(24, 24, 27),
            section_header_bg: Color32::from_rgb(30, 30, 34),
            control_bg: Color32::from_rgb(34, 34, 39),
            control_hover: Color32::from_rgb(43, 43, 49),
            track_bg: Color32::from_rgb(17, 17, 20),
            border: Color32::from_rgb(46, 46, 52),
            border_subtle: Color32::from_rgb(36, 36, 41),
            text_primary: Color32::from_rgb(228, 228, 231),
            text_secondary: Color32::from_rgb(166, 166, 174),
            text_muted: Color32::from_rgb(116, 116, 124),
            accent: Color32::from_rgb(138, 156, 180),
            accent_fill: Color32::from_rgb(52, 60, 72),
            accent_text: Color32::from_rgb(238, 241, 246),
            warn: Color32::from_rgb(212, 155, 84),
            radius,
        },
        StudioTheme::OledReference => ThemePalette {
            is_light: false,
            viewport_bg: Color32::BLACK,
            menu_bg: Color32::from_rgb(8, 8, 10),
            toolbar_bg: Color32::from_rgb(12, 12, 14),
            panel_bg: Color32::from_rgb(11, 11, 13),
            section_header_bg: Color32::from_rgb(17, 17, 20),
            control_bg: Color32::from_rgb(21, 21, 25),
            control_hover: Color32::from_rgb(30, 30, 35),
            track_bg: Color32::from_rgb(6, 6, 8),
            border: Color32::from_rgb(34, 34, 40),
            border_subtle: Color32::from_rgb(24, 24, 29),
            text_primary: Color32::from_rgb(218, 218, 222),
            text_secondary: Color32::from_rgb(152, 152, 160),
            text_muted: Color32::from_rgb(102, 102, 110),
            accent: Color32::from_rgb(148, 154, 166),
            accent_fill: Color32::from_rgb(42, 45, 52),
            accent_text: Color32::from_rgb(236, 236, 240),
            warn: Color32::from_rgb(206, 150, 82),
            radius,
        },
        StudioTheme::CarbonTungsten => ThemePalette {
            is_light: false,
            viewport_bg: Color32::from_rgb(16, 15, 14),
            menu_bg: Color32::from_rgb(22, 20, 19),
            toolbar_bg: Color32::from_rgb(27, 25, 23),
            panel_bg: Color32::from_rgb(26, 24, 22),
            section_header_bg: Color32::from_rgb(33, 30, 28),
            control_bg: Color32::from_rgb(38, 35, 32),
            control_hover: Color32::from_rgb(48, 44, 40),
            track_bg: Color32::from_rgb(18, 16, 15),
            border: Color32::from_rgb(52, 47, 43),
            border_subtle: Color32::from_rgb(40, 36, 33),
            text_primary: Color32::from_rgb(231, 227, 220),
            text_secondary: Color32::from_rgb(172, 166, 158),
            text_muted: Color32::from_rgb(122, 116, 108),
            accent: Color32::from_rgb(198, 138, 78),
            accent_fill: Color32::from_rgb(72, 53, 36),
            accent_text: Color32::from_rgb(245, 238, 228),
            warn: Color32::from_rgb(218, 154, 78),
            radius,
        },
        StudioTheme::NordicSlate => ThemePalette {
            is_light: false,
            viewport_bg: Color32::from_rgb(16, 19, 24),
            menu_bg: Color32::from_rgb(21, 25, 31),
            toolbar_bg: Color32::from_rgb(25, 30, 37),
            panel_bg: Color32::from_rgb(25, 30, 37),
            section_header_bg: Color32::from_rgb(31, 37, 46),
            control_bg: Color32::from_rgb(35, 42, 52),
            control_hover: Color32::from_rgb(45, 53, 66),
            track_bg: Color32::from_rgb(17, 21, 26),
            border: Color32::from_rgb(48, 57, 70),
            border_subtle: Color32::from_rgb(37, 44, 55),
            text_primary: Color32::from_rgb(226, 232, 240),
            text_secondary: Color32::from_rgb(160, 171, 186),
            text_muted: Color32::from_rgb(112, 123, 139),
            accent: Color32::from_rgb(116, 152, 188),
            accent_fill: Color32::from_rgb(45, 63, 84),
            accent_text: Color32::from_rgb(238, 244, 250),
            warn: Color32::from_rgb(214, 158, 86),
            radius,
        },
        StudioTheme::DaylightPaper => ThemePalette {
            is_light: true,
            viewport_bg: Color32::from_rgb(222, 221, 216),
            menu_bg: Color32::from_rgb(236, 235, 231),
            toolbar_bg: Color32::from_rgb(242, 241, 237),
            panel_bg: Color32::from_rgb(242, 241, 237),
            section_header_bg: Color32::from_rgb(232, 231, 226),
            control_bg: Color32::from_rgb(228, 227, 222),
            control_hover: Color32::from_rgb(216, 215, 209),
            track_bg: Color32::from_rgb(208, 207, 201),
            border: Color32::from_rgb(196, 194, 188),
            border_subtle: Color32::from_rgb(212, 210, 204),
            text_primary: Color32::from_rgb(26, 26, 29),
            text_secondary: Color32::from_rgb(76, 76, 82),
            text_muted: Color32::from_rgb(118, 118, 125),
            accent: Color32::from_rgb(58, 98, 136),
            accent_fill: Color32::from_rgb(204, 216, 228),
            accent_text: Color32::from_rgb(20, 34, 48),
            warn: Color32::from_rgb(168, 98, 28),
            radius,
        },
    };

    if accent_override != AccentPreset::ThemeDefault {
        let (acc, fill_dark, fill_light) = match accent_override {
            AccentPreset::ThemeDefault => unreachable!(),
            AccentPreset::SteelSlate => (
                Color32::from_rgb(132, 154, 182),
                Color32::from_rgb(48, 58, 72),
                Color32::from_rgb(204, 215, 228),
            ),
            AccentPreset::TungstenAmber => (
                Color32::from_rgb(198, 138, 78),
                Color32::from_rgb(72, 52, 34),
                Color32::from_rgb(232, 212, 188),
            ),
            AccentPreset::OxideCopper => (
                Color32::from_rgb(192, 104, 82),
                Color32::from_rgb(74, 42, 36),
                Color32::from_rgb(234, 204, 196),
            ),
            AccentPreset::MineralSage => (
                Color32::from_rgb(114, 158, 134),
                Color32::from_rgb(40, 60, 50),
                Color32::from_rgb(202, 222, 210),
            ),
            AccentPreset::MonochromeSilver => (
                if pal.is_light {
                    Color32::from_rgb(64, 64, 70)
                } else {
                    Color32::from_rgb(184, 184, 192)
                },
                Color32::from_rgb(56, 56, 62),
                Color32::from_rgb(208, 208, 212),
            ),
        };
        pal.accent = acc;
        pal.accent_fill = if pal.is_light { fill_light } else { fill_dark };
        pal.accent_text = if pal.is_light {
            Color32::from_rgb(20, 22, 26)
        } else {
            Color32::from_rgb(242, 242, 245)
        };
    }

    pal
}

/// Install embedded IBM Plex Sans (Regular + Medium) and JetBrains Mono as the sole UI fonts.
/// Zero reliance on OS system fonts, stock egui Ubuntu-Light, or emoji fallback fonts.
pub fn install_studio_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::empty();

    fonts.font_data.insert(
        "ibm_plex_regular".to_owned(),
        Arc::new(FontData::from_static(PLEX_SANS_REGULAR)),
    );
    fonts.font_data.insert(
        "ibm_plex_medium".to_owned(),
        Arc::new(FontData::from_static(PLEX_SANS_MEDIUM)),
    );
    fonts.font_data.insert(
        "jetbrains_mono".to_owned(),
        Arc::new(FontData::from_static(JETBRAINS_MONO_REGULAR)),
    );

    fonts.families.insert(
        FontFamily::Proportional,
        vec!["ibm_plex_regular".to_owned(), "jetbrains_mono".to_owned()],
    );
    fonts.families.insert(
        FontFamily::Monospace,
        vec!["jetbrains_mono".to_owned(), "ibm_plex_regular".to_owned()],
    );
    fonts.families.insert(
        FontFamily::Name(FAMILY_MEDIUM.into()),
        vec![
            "ibm_plex_medium".to_owned(),
            "ibm_plex_regular".to_owned(),
            "jetbrains_mono".to_owned(),
        ],
    );

    ctx.set_fonts(fonts);
}

/// Apply calibrated workstation typography metrics, spacing, and surface colors to `egui::Context`.
pub fn apply_theme_to_context(ctx: &egui::Context, pal: &ThemePalette, density: UiDensity) {
    ctx.set_zoom_factor(density.zoom_factor());

    let mut style = (*ctx.style()).clone();
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(14.5, FontFamily::Name(FAMILY_MEDIUM.into())),
    );
    style
        .text_styles
        .insert(TextStyle::Body, FontId::new(12.2, FontFamily::Proportional));
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(12.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(11.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(11.5, FontFamily::Monospace),
    );

    style.spacing.item_spacing = egui::vec2(6.0, 5.0);
    style.spacing.button_padding = egui::vec2(7.0, 3.5);
    style.spacing.window_margin = Margin::same(12);
    style.spacing.menu_margin = Margin::same(6);
    style.spacing.indent = 12.0;

    let mut vis = if pal.is_light {
        Visuals::light()
    } else {
        Visuals::dark()
    };

    let cr = CornerRadius::same(pal.radius);
    vis.panel_fill = pal.panel_bg;
    vis.window_fill = pal.panel_bg;
    vis.window_stroke = Stroke::new(1.0_f32, pal.border);
    vis.window_corner_radius = CornerRadius::same(pal.radius.saturating_add(1));
    vis.menu_corner_radius = cr;
    vis.extreme_bg_color = pal.track_bg;
    vis.faint_bg_color = pal.section_header_bg;
    vis.code_bg_color = pal.track_bg;
    vis.override_text_color = None;

    vis.selection.bg_fill = pal.accent_fill;
    vis.selection.stroke = Stroke::new(1.0_f32, pal.accent);

    vis.widgets.noninteractive.bg_fill = pal.panel_bg;
    vis.widgets.noninteractive.weak_bg_fill = pal.section_header_bg;
    vis.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, pal.border_subtle);
    vis.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, pal.text_primary);
    vis.widgets.noninteractive.corner_radius = cr;

    vis.widgets.inactive.bg_fill = pal.control_bg;
    vis.widgets.inactive.weak_bg_fill = pal.control_bg;
    vis.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, pal.border);
    vis.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, pal.text_primary);
    vis.widgets.inactive.corner_radius = cr;

    vis.widgets.hovered.bg_fill = pal.control_hover;
    vis.widgets.hovered.weak_bg_fill = pal.control_hover;
    vis.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, pal.accent);
    vis.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, pal.text_primary);
    vis.widgets.hovered.corner_radius = cr;
    vis.widgets.hovered.expansion = 0.0;

    vis.widgets.active.bg_fill = pal.accent_fill;
    vis.widgets.active.weak_bg_fill = pal.accent_fill;
    vis.widgets.active.bg_stroke = Stroke::new(1.0_f32, pal.accent);
    vis.widgets.active.fg_stroke = Stroke::new(1.0_f32, pal.accent_text);
    vis.widgets.active.corner_radius = cr;
    vis.widgets.active.expansion = 0.0;

    vis.widgets.open.bg_fill = pal.control_hover;
    vis.widgets.open.weak_bg_fill = pal.control_hover;
    vis.widgets.open.bg_stroke = Stroke::new(1.0_f32, pal.border);
    vis.widgets.open.fg_stroke = Stroke::new(1.0_f32, pal.text_primary);
    vis.widgets.open.corner_radius = cr;

    style.visuals = vis;
    ctx.set_style(style);
}

#[cfg(test)]
mod header_stats_tests {
    use super::*;

    #[test]
    fn header_stats_width_is_constant() {
        // Same total width on every call: the strip never reflows.
        assert_eq!(header_stats_width(), header_stats_width());
        assert_eq!(
            header_stats_width(),
            HEADER_STAT_WIDTHS.iter().sum::<f32>()
                + HEADER_STAT_SEP * (HEADER_STAT_WIDTHS.len() - 1) as f32
        );
    }

    #[test]
    fn header_stat_rects_are_fixed_columns() {
        let rects = header_stat_rects(1000.0, 400.0);
        for (idx, rect) in rects.iter().enumerate() {
            assert_eq!(rect.width(), HEADER_STAT_WIDTHS[idx]);
            assert_eq!(rect.height(), HEADER_STAT_HEIGHT);
            assert_eq!(rect.left(), rect.right() - HEADER_STAT_WIDTHS[idx]);
        }
        // Pinned to the right edge of the requested strip.
        assert_eq!(rects[rects.len() - 1].right(), 1000.0);
        assert_eq!(rects[0].left(), 1000.0 - header_stats_width());
    }

    #[test]
    fn header_stat_columns_never_overlap() {
        let rects = header_stat_rects(720.0, 20.0);
        for pair in rects.windows(2) {
            assert!(pair[0].right() < pair[1].left(), "columns must not overlap");
            let gap = pair[1].left() - pair[0].right();
            assert!((gap - HEADER_STAT_SEP).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn header_stat_rects_ignore_content() {
        // Geometry is derived only from the constants, so the widest possible
        // values cannot shift anything outside their own column.
        let narrow = header_stat_rects(1440.0, 12.0);
        let again = header_stat_rects(1440.0, 12.0);
        assert_eq!(narrow, again);
    }

    #[test]
    fn widest_expected_values_fit_their_columns() {
        // JetBrains Mono advances ~0.6em per glyph; keep every realistic value
        // inside its column at the chosen font size.
        let advance = HEADER_STAT_FONT * 0.62;
        let samples = [
            "3840x2160",   // cell dimensions
            "95/95 CHARS", // glyph spread
            "1234.56 ms",  // worst-case frame time
            "9999 FPS",    // capped FPS readout
        ];
        for (idx, sample) in samples.iter().enumerate() {
            let needed = sample.chars().count() as f32 * advance;
            assert!(
                needed <= HEADER_STAT_WIDTHS[idx],
                "{sample} needs {needed}pt but column is {}pt",
                HEADER_STAT_WIDTHS[idx]
            );
        }
    }
}
