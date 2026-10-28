//! Exhaustive Workstation Theme x AccentPreset x CornerStyle palette & contrast matrix (90 `#[test]`s),
//! UI density & checkerboard tests, `AppPreferences` JSON resilience, zero-emoji codebase invariant,
//! and Windows single-GUI subsystem (`windows_subsystem = "windows"`) verification (~115 `#[test]`s).

use eframe::egui::Color32;
use lumaglyph::theme::{
    resolve_palette, AccentPreset, CheckerContrast, CornerStyle, StudioTheme, UiDensity,
};
use lumaglyph::updater::{AppPreferences, DEFAULT_GITHUB_REPO};
use std::path::Path;

fn relative_luminance(c: Color32) -> f32 {
    let lin = |u: u8| -> f32 {
        let s = (u as f32) / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

fn contrast_ratio(a: Color32, b: Color32) -> f32 {
    let la = relative_luminance(a);
    let lb = relative_luminance(b);
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

fn assert_palette_invariants(theme: StudioTheme, accent: AccentPreset, corner: CornerStyle) {
    let pal = resolve_palette(theme, accent, corner);
    assert_eq!(pal.is_light, theme.is_light());
    assert_eq!(pal.radius, corner.radius_u8());

    // Primary text vs panel background MUST maintain strong WCAG AA+ legibility (>= 7.0:1)!
    let ratio = contrast_ratio(pal.text_primary, pal.panel_bg);
    assert!(
        ratio >= 7.0,
        "Insufficient primary text contrast ({ratio:.2}:1) for {:?} / {:?}",
        theme,
        accent
    );

    // Secondary text vs panel background must remain clearly legible (>= 4.2:1)
    let sec_ratio = contrast_ratio(pal.text_secondary, pal.panel_bg);
    assert!(
        sec_ratio >= 4.2,
        "Insufficient secondary text contrast ({sec_ratio:.2}:1) for {:?}",
        theme
    );

    if theme == StudioTheme::OledReference {
        assert_eq!(pal.viewport_bg, Color32::BLACK);
    }
}

macro_rules! theme_matrix_tests {
    ($($name:ident => ($t:expr, $a:expr, $c:expr)),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                assert_palette_invariants($t, $a, $c);
            }
        )+
    };
}

// All 5 StudioThemes x 6 AccentPresets x 3 CornerStyles = 90 individual `#[test]`s!
theme_matrix_tests! {
    // GraphitePro (18)
    tm_graphite_def_s => (StudioTheme::GraphitePro, AccentPreset::ThemeDefault, CornerStyle::Sharp1px),
    tm_graphite_def_p => (StudioTheme::GraphitePro, AccentPreset::ThemeDefault, CornerStyle::Precision2px),
    tm_graphite_def_r => (StudioTheme::GraphitePro, AccentPreset::ThemeDefault, CornerStyle::Soft4px),
    tm_graphite_slate_s => (StudioTheme::GraphitePro, AccentPreset::SteelSlate, CornerStyle::Sharp1px),
    tm_graphite_slate_p => (StudioTheme::GraphitePro, AccentPreset::SteelSlate, CornerStyle::Precision2px),
    tm_graphite_slate_r => (StudioTheme::GraphitePro, AccentPreset::SteelSlate, CornerStyle::Soft4px),
    tm_graphite_amb_s => (StudioTheme::GraphitePro, AccentPreset::TungstenAmber, CornerStyle::Sharp1px),
    tm_graphite_amb_p => (StudioTheme::GraphitePro, AccentPreset::TungstenAmber, CornerStyle::Precision2px),
    tm_graphite_amb_r => (StudioTheme::GraphitePro, AccentPreset::TungstenAmber, CornerStyle::Soft4px),
    tm_graphite_cop_s => (StudioTheme::GraphitePro, AccentPreset::OxideCopper, CornerStyle::Sharp1px),
    tm_graphite_cop_p => (StudioTheme::GraphitePro, AccentPreset::OxideCopper, CornerStyle::Precision2px),
    tm_graphite_cop_r => (StudioTheme::GraphitePro, AccentPreset::OxideCopper, CornerStyle::Soft4px),
    tm_graphite_sage_s => (StudioTheme::GraphitePro, AccentPreset::MineralSage, CornerStyle::Sharp1px),
    tm_graphite_sage_p => (StudioTheme::GraphitePro, AccentPreset::MineralSage, CornerStyle::Precision2px),
    tm_graphite_sage_r => (StudioTheme::GraphitePro, AccentPreset::MineralSage, CornerStyle::Soft4px),
    tm_graphite_silv_s => (StudioTheme::GraphitePro, AccentPreset::MonochromeSilver, CornerStyle::Sharp1px),
    tm_graphite_silv_p => (StudioTheme::GraphitePro, AccentPreset::MonochromeSilver, CornerStyle::Precision2px),
    tm_graphite_silv_r => (StudioTheme::GraphitePro, AccentPreset::MonochromeSilver, CornerStyle::Soft4px),

    // OledReference (18)
    tm_oled_def_s => (StudioTheme::OledReference, AccentPreset::ThemeDefault, CornerStyle::Sharp1px),
    tm_oled_def_p => (StudioTheme::OledReference, AccentPreset::ThemeDefault, CornerStyle::Precision2px),
    tm_oled_def_r => (StudioTheme::OledReference, AccentPreset::ThemeDefault, CornerStyle::Soft4px),
    tm_oled_slate_s => (StudioTheme::OledReference, AccentPreset::SteelSlate, CornerStyle::Sharp1px),
    tm_oled_slate_p => (StudioTheme::OledReference, AccentPreset::SteelSlate, CornerStyle::Precision2px),
    tm_oled_slate_r => (StudioTheme::OledReference, AccentPreset::SteelSlate, CornerStyle::Soft4px),
    tm_oled_amb_s => (StudioTheme::OledReference, AccentPreset::TungstenAmber, CornerStyle::Sharp1px),
    tm_oled_amb_p => (StudioTheme::OledReference, AccentPreset::TungstenAmber, CornerStyle::Precision2px),
    tm_oled_amb_r => (StudioTheme::OledReference, AccentPreset::TungstenAmber, CornerStyle::Soft4px),
    tm_oled_cop_s => (StudioTheme::OledReference, AccentPreset::OxideCopper, CornerStyle::Sharp1px),
    tm_oled_cop_p => (StudioTheme::OledReference, AccentPreset::OxideCopper, CornerStyle::Precision2px),
    tm_oled_cop_r => (StudioTheme::OledReference, AccentPreset::OxideCopper, CornerStyle::Soft4px),
    tm_oled_sage_s => (StudioTheme::OledReference, AccentPreset::MineralSage, CornerStyle::Sharp1px),
    tm_oled_sage_p => (StudioTheme::OledReference, AccentPreset::MineralSage, CornerStyle::Precision2px),
    tm_oled_sage_r => (StudioTheme::OledReference, AccentPreset::MineralSage, CornerStyle::Soft4px),
    tm_oled_silv_s => (StudioTheme::OledReference, AccentPreset::MonochromeSilver, CornerStyle::Sharp1px),
    tm_oled_silv_p => (StudioTheme::OledReference, AccentPreset::MonochromeSilver, CornerStyle::Precision2px),
    tm_oled_silv_r => (StudioTheme::OledReference, AccentPreset::MonochromeSilver, CornerStyle::Soft4px),

    // CarbonTungsten (18)
    tm_tung_def_s => (StudioTheme::CarbonTungsten, AccentPreset::ThemeDefault, CornerStyle::Sharp1px),
    tm_tung_def_p => (StudioTheme::CarbonTungsten, AccentPreset::ThemeDefault, CornerStyle::Precision2px),
    tm_tung_def_r => (StudioTheme::CarbonTungsten, AccentPreset::ThemeDefault, CornerStyle::Soft4px),
    tm_tung_slate_s => (StudioTheme::CarbonTungsten, AccentPreset::SteelSlate, CornerStyle::Sharp1px),
    tm_tung_slate_p => (StudioTheme::CarbonTungsten, AccentPreset::SteelSlate, CornerStyle::Precision2px),
    tm_tung_slate_r => (StudioTheme::CarbonTungsten, AccentPreset::SteelSlate, CornerStyle::Soft4px),
    tm_tung_amb_s => (StudioTheme::CarbonTungsten, AccentPreset::TungstenAmber, CornerStyle::Sharp1px),
    tm_tung_amb_p => (StudioTheme::CarbonTungsten, AccentPreset::TungstenAmber, CornerStyle::Precision2px),
    tm_tung_amb_r => (StudioTheme::CarbonTungsten, AccentPreset::TungstenAmber, CornerStyle::Soft4px),
    tm_tung_cop_s => (StudioTheme::CarbonTungsten, AccentPreset::OxideCopper, CornerStyle::Sharp1px),
    tm_tung_cop_p => (StudioTheme::CarbonTungsten, AccentPreset::OxideCopper, CornerStyle::Precision2px),
    tm_tung_cop_r => (StudioTheme::CarbonTungsten, AccentPreset::OxideCopper, CornerStyle::Soft4px),
    tm_tung_sage_s => (StudioTheme::CarbonTungsten, AccentPreset::MineralSage, CornerStyle::Sharp1px),
    tm_tung_sage_p => (StudioTheme::CarbonTungsten, AccentPreset::MineralSage, CornerStyle::Precision2px),
    tm_tung_sage_r => (StudioTheme::CarbonTungsten, AccentPreset::MineralSage, CornerStyle::Soft4px),
    tm_tung_silv_s => (StudioTheme::CarbonTungsten, AccentPreset::MonochromeSilver, CornerStyle::Sharp1px),
    tm_tung_silv_p => (StudioTheme::CarbonTungsten, AccentPreset::MonochromeSilver, CornerStyle::Precision2px),
    tm_tung_silv_r => (StudioTheme::CarbonTungsten, AccentPreset::MonochromeSilver, CornerStyle::Soft4px),

    // NordicSlate (18)
    tm_nord_def_s => (StudioTheme::NordicSlate, AccentPreset::ThemeDefault, CornerStyle::Sharp1px),
    tm_nord_def_p => (StudioTheme::NordicSlate, AccentPreset::ThemeDefault, CornerStyle::Precision2px),
    tm_nord_def_r => (StudioTheme::NordicSlate, AccentPreset::ThemeDefault, CornerStyle::Soft4px),
    tm_nord_slate_s => (StudioTheme::NordicSlate, AccentPreset::SteelSlate, CornerStyle::Sharp1px),
    tm_nord_slate_p => (StudioTheme::NordicSlate, AccentPreset::SteelSlate, CornerStyle::Precision2px),
    tm_nord_slate_r => (StudioTheme::NordicSlate, AccentPreset::SteelSlate, CornerStyle::Soft4px),
    tm_nord_amb_s => (StudioTheme::NordicSlate, AccentPreset::TungstenAmber, CornerStyle::Sharp1px),
    tm_nord_amb_p => (StudioTheme::NordicSlate, AccentPreset::TungstenAmber, CornerStyle::Precision2px),
    tm_nord_amb_r => (StudioTheme::NordicSlate, AccentPreset::TungstenAmber, CornerStyle::Soft4px),
    tm_nord_cop_s => (StudioTheme::NordicSlate, AccentPreset::OxideCopper, CornerStyle::Sharp1px),
    tm_nord_cop_p => (StudioTheme::NordicSlate, AccentPreset::OxideCopper, CornerStyle::Precision2px),
    tm_nord_cop_r => (StudioTheme::NordicSlate, AccentPreset::OxideCopper, CornerStyle::Soft4px),
    tm_nord_sage_s => (StudioTheme::NordicSlate, AccentPreset::MineralSage, CornerStyle::Sharp1px),
    tm_nord_sage_p => (StudioTheme::NordicSlate, AccentPreset::MineralSage, CornerStyle::Precision2px),
    tm_nord_sage_r => (StudioTheme::NordicSlate, AccentPreset::MineralSage, CornerStyle::Soft4px),
    tm_nord_silv_s => (StudioTheme::NordicSlate, AccentPreset::MonochromeSilver, CornerStyle::Sharp1px),
    tm_nord_silv_p => (StudioTheme::NordicSlate, AccentPreset::MonochromeSilver, CornerStyle::Precision2px),
    tm_nord_silv_r => (StudioTheme::NordicSlate, AccentPreset::MonochromeSilver, CornerStyle::Soft4px),

    // DaylightPaper (18)
    tm_day_def_s => (StudioTheme::DaylightPaper, AccentPreset::ThemeDefault, CornerStyle::Sharp1px),
    tm_day_def_p => (StudioTheme::DaylightPaper, AccentPreset::ThemeDefault, CornerStyle::Precision2px),
    tm_day_def_r => (StudioTheme::DaylightPaper, AccentPreset::ThemeDefault, CornerStyle::Soft4px),
    tm_day_slate_s => (StudioTheme::DaylightPaper, AccentPreset::SteelSlate, CornerStyle::Sharp1px),
    tm_day_slate_p => (StudioTheme::DaylightPaper, AccentPreset::SteelSlate, CornerStyle::Precision2px),
    tm_day_slate_r => (StudioTheme::DaylightPaper, AccentPreset::SteelSlate, CornerStyle::Soft4px),
    tm_day_amb_s => (StudioTheme::DaylightPaper, AccentPreset::TungstenAmber, CornerStyle::Sharp1px),
    tm_day_amb_p => (StudioTheme::DaylightPaper, AccentPreset::TungstenAmber, CornerStyle::Precision2px),
    tm_day_amb_r => (StudioTheme::DaylightPaper, AccentPreset::TungstenAmber, CornerStyle::Soft4px),
    tm_day_cop_s => (StudioTheme::DaylightPaper, AccentPreset::OxideCopper, CornerStyle::Sharp1px),
    tm_day_cop_p => (StudioTheme::DaylightPaper, AccentPreset::OxideCopper, CornerStyle::Precision2px),
    tm_day_cop_r => (StudioTheme::DaylightPaper, AccentPreset::OxideCopper, CornerStyle::Soft4px),
    tm_day_sage_s => (StudioTheme::DaylightPaper, AccentPreset::MineralSage, CornerStyle::Sharp1px),
    tm_day_sage_p => (StudioTheme::DaylightPaper, AccentPreset::MineralSage, CornerStyle::Precision2px),
    tm_day_sage_r => (StudioTheme::DaylightPaper, AccentPreset::MineralSage, CornerStyle::Soft4px),
    tm_day_silv_s => (StudioTheme::DaylightPaper, AccentPreset::MonochromeSilver, CornerStyle::Sharp1px),
    tm_day_silv_p => (StudioTheme::DaylightPaper, AccentPreset::MonochromeSilver, CornerStyle::Precision2px),
    tm_day_silv_r => (StudioTheme::DaylightPaper, AccentPreset::MonochromeSilver, CornerStyle::Soft4px),
}

#[test]
fn ui_density_and_checkerboard_presets_are_distinct_and_ordered() {
    assert!(UiDensity::Compact90.zoom_factor() < UiDensity::Standard100.zoom_factor());
    assert!(UiDensity::Standard100.zoom_factor() < UiDensity::Comfortable110.zoom_factor());
    assert!(UiDensity::Comfortable110.zoom_factor() < UiDensity::HiDpi125.zoom_factor());

    for is_light in [false, true] {
        for cc in CheckerContrast::ALL {
            let (a, b) = cc.colors(is_light);
            assert_ne!(a, b);
        }
    }
}

#[test]
fn app_preferences_handles_empty_partial_extra_fields_and_roundtrips() {
    let empty: AppPreferences = serde_json::from_str("{}").unwrap();
    assert!(!empty.check_updates_on_startup);
    assert_eq!(empty.github_repo, DEFAULT_GITHUB_REPO);
    assert_eq!(empty.png_cell_height(), 36);

    let future_json = r#"{"theme":"CarbonTungsten","video_crf":15,"png_scale_preset":3,"future_unknown_field":12345}"#;
    let parsed: AppPreferences = serde_json::from_str(future_json).unwrap();
    assert_eq!(parsed.theme, StudioTheme::CarbonTungsten);
    assert_eq!(parsed.video_crf, 15);
    assert_eq!(parsed.png_cell_height(), 54);

    let s = serde_json::to_string_pretty(&parsed).unwrap();
    let rt: AppPreferences = serde_json::from_str(&s).unwrap();
    assert_eq!(rt.theme, StudioTheme::CarbonTungsten);
    assert_eq!(rt.video_crf, 15);
}

#[test]
fn main_rs_enforces_windows_gui_subsystem_with_zero_alloc_console() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let main_src = std::fs::read_to_string(manifest_dir.join("src/main.rs")).unwrap();
    assert!(
        main_src.contains("windows_subsystem = \"windows\""),
        "src/main.rs MUST declare windows_subsystem = \"windows\" so Windows never spawns an attached terminal window!"
    );
    let code_only: String = main_src
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !code_only.contains("AllocConsole("),
        "src/main.rs must NEVER call AllocConsole()"
    );
}

#[test]
fn gui_defaults_to_empty_canvas_with_zero_sample_scenes() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let gui_src = std::fs::read_to_string(manifest_dir.join("src/gui.rs")).unwrap();
    assert!(
        gui_src.contains("source_kind: SourceKind::Empty"),
        "LumaGlyphApp::new must boot into SourceKind::Empty by default"
    );
    for forbidden in [
        "SamplePreset",
        "generate_sample_rgba",
        "SourceKind::Sample",
        "SourceKind::LiveMotion",
        "menu_button(\"Scenes\"",
        "\"Live 60fps\"",
    ] {
        assert!(
            !gui_src.contains(forbidden),
            "src/gui.rs must not reference sample scenes ('{forbidden}')"
        );
    }
}

#[test]
fn entire_source_tree_and_readme_contain_zero_emojis() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = vec![
        manifest_dir.join("Cargo.toml"),
        manifest_dir.join("build.rs"),
        manifest_dir.join("README.md"),
    ];
    for entry in std::fs::read_dir(manifest_dir.join("src")).unwrap() {
        let p = entry.unwrap().path();
        if p.extension().and_then(|s| s.to_str()) == Some("rs") {
            files.push(p);
        }
    }

    for path in files {
        let text = std::fs::read_to_string(&path).unwrap();
        for ch in text.chars() {
            let cp = ch as u32;
            let is_emoji_or_dingbat =
                (0x2190..=0x2BFF).contains(&cp) || (0xFE00..=0xFE0F).contains(&cp) || cp >= 0x1F000;
            assert!(
                !is_emoji_or_dingbat,
                "Forbidden emoji/symbol U+{cp:04X} ('{ch}') found in {}",
                path.display()
            );
        }
    }
}
