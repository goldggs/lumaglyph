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
    assert!(
        !manifest_dir.join("src/samples.rs").exists(),
        "src/samples.rs must not exist in the codebase"
    );
    let lib_src = std::fs::read_to_string(manifest_dir.join("src/lib.rs")).unwrap();
    assert!(!lib_src.contains("pub mod samples"));
    let gui_src = std::fs::read_to_string(manifest_dir.join("src/gui.rs")).unwrap();
    assert!(
        gui_src.contains("source_kind: SourceKind::Empty"),
        "LumaGlyphApp::new must boot into SourceKind::Empty by default"
    );
    assert!(
        gui_src.contains("inspector_open: false"),
        "Fresh window must start cleanly without side inspector clutter"
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

#[test]
fn workstation_hygiene_no_split_wipe_no_theme_footer_and_normal_defaults() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let gui_src = std::fs::read_to_string(manifest_dir.join("src/gui.rs")).unwrap();
    let cli_src = std::fs::read_to_string(manifest_dir.join("src/cli.rs")).unwrap();

    // Split-Wipe feature completely removed
    assert!(!gui_src.contains("Split-Wipe"));
    assert!(!gui_src.contains("split_enabled"));
    assert!(!gui_src.contains("split_ratio"));
    assert!(!gui_src.contains("orig_tex"));
    assert!(!cli_src.contains("split_wipe"));

    // Theme footer on bottom right completely removed
    assert!(!gui_src.contains("Theme: "));

    // Reset to Default actions present
    assert!(gui_src.contains("fn reset_ascii_to_default"));
    assert!(gui_src.contains("fn reset_all_settings_to_default"));
    assert!(gui_src.contains("Reset to Default"));

    // Normal, balanced default RenderParams
    let def = lumaglyph::engine::RenderParams::default();
    assert_eq!(def.brightness, 0.0);
    assert_eq!(def.contrast, 1.0);
    assert_eq!(def.saturation, 1.0);
    assert_eq!(def.gamma, 1.0);
    assert_eq!(def.dither, 0.04);
    assert_eq!(def.directional_crunch, 1.25);
    assert!(!def.oled_black);
    assert!(!def.transparent_bg);
    assert!(!def.wireframe_only);
    assert!(!def.crt_bloom);
    assert!(!def.scanlines);
    assert!(!def.invert);

    // Self-update CLI subcommand attaches parent console on Windows
    assert!(lumaglyph::cli::should_attach_parent_console(&[
        "lumaglyph.exe",
        "self-update"
    ]));
    assert!(lumaglyph::cli::should_attach_parent_console(&[
        "lumaglyph.exe",
        "update"
    ]));
}

#[test]
fn windows_ico_container_validates_all_7_resolutions_and_bounds() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let ico = std::fs::read(manifest_dir.join("assets/icons/lumaglyph.ico"))
        .expect("assets/icons/lumaglyph.ico must exist");
    assert!(ico.len() > 80_000, "ICO must contain rich 32-bit frames");

    let reserved = u16::from_le_bytes([ico[0], ico[1]]);
    let ico_type = u16::from_le_bytes([ico[2], ico[3]]);
    let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
    assert_eq!(reserved, 0);
    assert_eq!(ico_type, 1, "ICO type must be 1 (icon)");
    assert_eq!(count, 7, "ICO must carry all 7 Windows DPI sizes");

    let expected_sizes: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];
    let mut cursor_offset = 6 + count * 16;

    for (i, &exp_sz) in expected_sizes.iter().enumerate() {
        let ent = &ico[6 + i * 16..6 + (i + 1) * 16];
        let bw = if ent[0] == 0 { 256u32 } else { ent[0] as u32 };
        let bh = if ent[1] == 0 { 256u32 } else { ent[1] as u32 };
        let planes = u16::from_le_bytes([ent[4], ent[5]]);
        let bpp = u16::from_le_bytes([ent[6], ent[7]]);
        let size = u32::from_le_bytes([ent[8], ent[9], ent[10], ent[11]]) as usize;
        let offset = u32::from_le_bytes([ent[12], ent[13], ent[14], ent[15]]) as usize;

        assert_eq!((bw, bh), (exp_sz, exp_sz));
        assert_eq!(planes, 1);
        assert_eq!(bpp, 32);
        assert_eq!(offset, cursor_offset, "ICO payloads must pack contiguously");
        assert!(offset + size <= ico.len());

        let payload = &ico[offset..offset + size];
        if exp_sz == 256 {
            assert_eq!(&payload[..8], b"\x89PNG\r\n\x1a\n");
            let decoded = image::load_from_memory_with_format(payload, image::ImageFormat::Png)
                .unwrap()
                .into_rgba8();
            assert_eq!(decoded.dimensions(), (256, 256));
        } else {
            let bi_size = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let bi_w = i32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
            let bi_h = i32::from_le_bytes([payload[8], payload[9], payload[10], payload[11]]);
            let bi_planes = u16::from_le_bytes([payload[12], payload[13]]);
            let bi_bit_count = u16::from_le_bytes([payload[14], payload[15]]);
            assert_eq!(bi_size, 40);
            assert_eq!(bi_w, exp_sz as i32);
            assert_eq!(bi_h, (exp_sz * 2) as i32);
            assert_eq!(bi_planes, 1);
            assert_eq!(bi_bit_count, 32);
        }

        cursor_offset += size;
    }
    assert_eq!(cursor_offset, ico.len());
}

#[test]
fn macos_icns_container_validates_all_7_ostypes_and_png_payloads() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let icns = std::fs::read(manifest_dir.join("assets/icons/lumaglyph.icns"))
        .expect("assets/icons/lumaglyph.icns must exist");
    assert_eq!(&icns[0..4], b"icns");
    let total_len = u32::from_be_bytes([icns[4], icns[5], icns[6], icns[7]]) as usize;
    assert_eq!(total_len, icns.len());

    let expected: [(&[u8; 4], u32); 7] = [
        (b"icp4", 16),
        (b"icp5", 32),
        (b"icp6", 64),
        (b"ic07", 128),
        (b"ic08", 256),
        (b"ic09", 512),
        (b"ic10", 1024),
    ];

    let mut pos = 8usize;
    for (ostype, exp_sz) in expected {
        assert!(pos + 8 <= icns.len());
        assert_eq!(&icns[pos..pos + 4], ostype);
        let chunk_len =
            u32::from_be_bytes([icns[pos + 4], icns[pos + 5], icns[pos + 6], icns[pos + 7]])
                as usize;
        assert!(chunk_len > 8 && pos + chunk_len <= icns.len());
        let png_bytes = &icns[pos + 8..pos + chunk_len];
        assert_eq!(&png_bytes[..8], b"\x89PNG\r\n\x1a\n");
        let img = image::load_from_memory_with_format(png_bytes, image::ImageFormat::Png)
            .unwrap()
            .into_rgba8();
        assert_eq!(img.dimensions(), (exp_sz, exp_sz));
        assert_eq!(
            img.get_pixel(0, 0)[3],
            0,
            "ICNS corners must be transparent"
        );
        pos += chunk_len;
    }
    assert_eq!(pos, icns.len());
}

#[test]
fn master_png_icons_are_metadata_free_rgba_and_exact_dimensions() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    for sz in [1024u32, 512, 256, 64, 32] {
        let p = manifest_dir.join(format!("assets/icons/lumaglyph-{sz}.png"));
        let raw = std::fs::read(&p).unwrap();
        assert_eq!(&raw[..8], b"\x89PNG\r\n\x1a\n");

        // Walk raw PNG chunks to ensure zero host/timestamp/EXIF text metadata chunks exist
        let mut idx = 8usize;
        while idx + 12 <= raw.len() {
            let len =
                u32::from_be_bytes([raw[idx], raw[idx + 1], raw[idx + 2], raw[idx + 3]]) as usize;
            let ctype = &raw[idx + 4..idx + 8];
            assert!(
                ctype != b"tEXt" && ctype != b"zTXt" && ctype != b"iTXt" && ctype != b"eXIf",
                "Forbidden metadata chunk {:?} in {}",
                std::str::from_utf8(ctype),
                p.display()
            );
            idx += 12 + len;
            if ctype == b"IEND" {
                break;
            }
        }
        assert_eq!(idx, raw.len());

        let img = image::load_from_memory_with_format(&raw, image::ImageFormat::Png)
            .unwrap()
            .into_rgba8();
        assert_eq!(img.dimensions(), (sz, sz));
        assert_eq!(img.get_pixel(0, 0)[3], 0);
        assert_eq!(img.get_pixel(sz - 1, 0)[3], 0);
        assert_eq!(img.get_pixel(0, sz - 1)[3], 0);
        assert_eq!(img.get_pixel(sz - 1, sz - 1)[3], 0);
    }
}

#[test]
fn runtime_eframe_icon_and_desktop_bundle_manifests_are_valid() {
    let icon_data = lumaglyph::icon::eframe_icon_data();
    assert_eq!((icon_data.width, icon_data.height), (256, 256));
    assert_eq!(icon_data.rgba.len(), 256 * 256 * 4);

    let ui_img = lumaglyph::icon::egui_logo_color_image().expect("UI logo ColorImage must decode");
    assert_eq!(ui_img.size, [256, 256]);

    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let desktop =
        std::fs::read_to_string(manifest_dir.join("assets/linux/lumaglyph.desktop")).unwrap();
    assert!(desktop.contains("Name=LumaGlyph"));
    assert!(desktop.contains("Icon=lumaglyph"));
    assert!(desktop.contains("Terminal=false"));

    let plist = std::fs::read_to_string(manifest_dir.join("assets/macos/Info.plist")).unwrap();
    assert!(plist.contains("<string>lumaglyph.icns</string>"));
    assert!(plist.contains("<string>LumaGlyph</string>"));

    let build_rs = std::fs::read_to_string(manifest_dir.join("build.rs")).unwrap();
    assert!(build_rs.contains("assets/icons/lumaglyph.ico"));
    assert!(build_rs.contains("compile_for(\"lumaglyph\")"));
}

#[test]
fn optimisations_and_rendering_preference_defaults_are_fast_and_safe() {
    let prefs = lumaglyph::updater::AppPreferences::default();

    // Background idling is on by default so an unfocused window costs nothing.
    assert!(prefs.idle_when_unfocused);
    assert!(prefs.pause_decode_when_unfocused);
    assert!(prefs.idle_when_minimized);
    assert!(prefs.trim_memory_on_idle);

    // Rendering defaults: 60 FPS cap, vsync, 4x MSAA, auto filtering, standard raster.
    assert_eq!(prefs.fps_cap, 60);
    assert!(prefs.vsync);
    assert_eq!(prefs.msaa_samples, 4);
    assert_eq!(
        prefs.texture_filter,
        lumaglyph::updater::TextureFilterMode::Auto
    );
    assert_eq!(prefs.preview_supersample, 1);
    assert_eq!(prefs.video_preview_max_dim, 640);
    assert!(prefs.audio_envelope_enabled);
    assert!(prefs.frame_dedup_enabled);
    assert_eq!(prefs.worker_threads, 0);
}

#[test]
fn fps_cap_maps_to_reparent_intervals_and_thread_budget() {
    use lumaglyph::updater::{AppPreferences, FPS_CAP_OPTIONS};

    assert_eq!(FPS_CAP_OPTIONS, [0, 15, 24, 30, 60, 144]);

    let mut prefs = AppPreferences::default();
    assert!(prefs.repaint_interval().is_some());

    prefs.fps_cap = 0;
    assert!(
        prefs.repaint_interval().is_none(),
        "0 FPS cap means uncapped/event driven"
    );

    for cap in [15u32, 24, 30, 60, 144] {
        prefs.fps_cap = cap;
        let d = prefs
            .repaint_interval()
            .expect("cap must map to an interval");
        let expected = 1.0 / f64::from(cap);
        assert!(
            (d.as_secs_f64() - expected).abs() < 1e-6,
            "cap {cap} produced {:?}",
            d
        );
    }

    prefs.fps_cap = 100_000;
    assert!(prefs.repaint_interval().unwrap().as_secs_f64() > 0.0);

    // Worker threads: 0 = auto, explicit values clamp into a sane range.
    prefs.worker_threads = 0;
    assert!(prefs.effective_worker_threads() >= 1);
    prefs.worker_threads = 6;
    assert_eq!(prefs.effective_worker_threads(), 6);
    prefs.worker_threads = 10_000;
    assert_eq!(prefs.effective_worker_threads(), 64);
}

#[test]
fn texture_filter_mode_resolves_for_every_zoom() {
    use lumaglyph::updater::TextureFilterMode;

    for mode in TextureFilterMode::ALL {
        assert!(!mode.label().is_empty());
        for zoom in [0.2f32, 0.5, 1.0, 1.79, 1.8, 3.5, 6.0] {
            let resolved = mode.resolve(zoom);
            assert!(resolved == "nearest" || resolved == "linear");
            match mode {
                TextureFilterMode::Nearest => assert_eq!(resolved, "nearest"),
                TextureFilterMode::Linear => assert_eq!(resolved, "linear"),
                TextureFilterMode::Auto => {
                    if zoom >= 1.8 {
                        assert_eq!(resolved, "nearest")
                    } else {
                        assert_eq!(resolved, "linear")
                    }
                }
            }
        }
    }
}

#[test]
fn new_settings_tabs_exist_and_are_reachable_from_cli() {
    use lumaglyph::gui::SettingsTab;

    assert_eq!(SettingsTab::ALL.len(), 7);
    assert!(SettingsTab::ALL.contains(&SettingsTab::Rendering));
    assert!(SettingsTab::ALL.contains(&SettingsTab::Optimisations));
    assert_eq!(SettingsTab::Rendering.label(), "Rendering & Quality");
    assert_eq!(SettingsTab::Optimisations.label(), "Optimisations");

    for alias in ["rendering", "quality", "graphics", "gfx"] {
        assert_eq!(SettingsTab::from_cli(alias), Some(SettingsTab::Rendering));
    }
    for alias in [
        "optimisations",
        "optimizations",
        "optimise",
        "optimize",
        "performance",
        "perf",
        "power",
    ] {
        assert_eq!(
            SettingsTab::from_cli(alias),
            Some(SettingsTab::Optimisations)
        );
    }
    assert_eq!(SettingsTab::from_cli("nonsense"), None);
}

#[test]
fn old_preferences_json_still_loads_with_new_optimisation_fields() {
    // A v0.3.5-era config must deserialize into v0.3.6 with sane defaults.
    let legacy = r#"{
        "theme": "GraphitePro",
        "default_cols": 156,
        "video_crf": 18,
        "fps_cap": 60,
        "check_updates_on_startup": false
    }"#;
    let prefs: lumaglyph::updater::AppPreferences = serde_json::from_str(legacy).unwrap();
    assert_eq!(prefs.default_cols, 156);
    assert!(prefs.idle_when_unfocused);
    assert_eq!(prefs.msaa_samples, 4);
    assert_eq!(prefs.video_preview_max_dim, 640);
}

#[test]
fn documentation_links_are_https_github_and_match_repository() {
    use lumaglyph::updater::{
        ALL_DOC_LINKS, DOCS_DEVELOPING_URL, DOCS_INDEX_URL, DOCS_INSTALL_URL, DOCS_QUICKSTART_URL,
        DOCS_TROUBLESHOOTING_URL, DOCS_USER_GUIDE_URL, LATEST_RELEASE_URL, REPO_URL,
    };

    let all = [
        REPO_URL,
        LATEST_RELEASE_URL,
        DOCS_INDEX_URL,
        DOCS_INSTALL_URL,
        DOCS_QUICKSTART_URL,
        DOCS_USER_GUIDE_URL,
        DOCS_TROUBLESHOOTING_URL,
        DOCS_DEVELOPING_URL,
    ];
    for url in all {
        assert!(
            url.starts_with("https://github.com/goldggs/lumaglyph"),
            "{url}"
        );
        assert!(!url.contains(' '));
        assert!(!url.contains("../"));
    }
    assert_eq!(ALL_DOC_LINKS.len(), 7);
    for (label, url) in ALL_DOC_LINKS {
        assert!(!label.is_empty());
        assert!(url.starts_with("https://"));
    }
}

#[test]
fn documentation_files_exist_and_are_written_for_non_developers() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let required = [
        "docs/README.md",
        "docs/INSTALL.md",
        "docs/QUICKSTART.md",
        "docs/USER-GUIDE.md",
        "docs/TROUBLESHOOTING.md",
        "docs/DEVELOPING.md",
    ];
    for rel in required {
        let path = manifest_dir.join(rel);
        assert!(path.exists(), "missing documentation file: {rel}");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.len() > 1_500, "{rel} is suspiciously short");
        // Plain-language, no emoji, no smart punctuation hazards.
        assert!(!text.contains('\u{1F600}'));
        assert!(
            text.is_ascii(),
            "{rel} must stay 7-bit ASCII for clean rendering everywhere"
        );
    }

    // The install guide must tell a Windows user exactly which asset to grab.
    let install = std::fs::read_to_string(manifest_dir.join("docs/INSTALL.md")).unwrap();
    for needle in [
        "lumaglyph-windows-x86_64.zip",
        "LumaGlyph-macos-arm64-app.zip",
        "LumaGlyph-macos-x86_64-app.zip",
        "lumaglyph-linux-x86_64.tar.gz",
        "releases/latest",
        "Assets",
        "More info",
    ] {
        assert!(
            install.contains(needle),
            "docs/INSTALL.md must mention '{needle}'"
        );
    }

    // README must put the download story above the technical sections.
    let readme = std::fs::read_to_string(manifest_dir.join("README.md")).unwrap();
    let download_pos = readme
        .find("Download and run it")
        .expect("download section");
    let engine_pos = readme.find("What it does").expect("engine section");
    assert!(
        download_pos < engine_pos,
        "the download guide must come before deep technical sections"
    );
    for needle in [
        "releases/latest",
        "lumaglyph-windows-x86_64.zip",
        "docs/INSTALL.md",
        "docs/QUICKSTART.md",
        "You do not need a GitHub account",
    ] {
        assert!(readme.contains(needle), "README must mention '{needle}'");
    }
}

#[test]
fn gui_wires_video_transport_on_canvas_and_docs_entry_points() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let gui_src = std::fs::read_to_string(manifest_dir.join("src/gui.rs")).unwrap();

    // The transport must be drawn over the canvas, not in the side/status toolbars.
    assert!(gui_src.contains("fn draw_video_transport"));
    // Transport must be docked in its own band beneath the canvas, never drawn over the video.
    assert!(gui_src.contains("draw_video_transport(ui, &painter, transport_band)"));
    assert!(gui_src.contains("let transport_band ="));
    assert!(gui_src.contains("let canvas_avail ="));
    assert!(!gui_src.contains("draw_video_transport(ui, &painter, dest_rect"));
    // Seek chevrons must be a centred pair, not offset copies that read as a misaligned shadow.
    assert!(gui_src.contains("let span = 2.0 * tri_w + gap;"));
    assert!(gui_src.contains("video_timeline"));
    assert!(gui_src.contains("fn export_source_frame_dialog"));
    assert!(gui_src.contains("Source Video Frame (PNG)"));
    assert!(!gui_src.contains("button(if is_paused { \"Play\" } else { \"Pause\" })"));
    assert!(!gui_src.contains("small_button(if is_paused { \"Play\" } else { \"Pause\" })"));

    // Documentation entry points.
    assert!(gui_src.contains("DOCS_QUICKSTART_URL"));
    assert!(gui_src.contains("Key::F1"));
    assert!(gui_src.contains("Documentation"));
    assert!(gui_src.contains("How to Download & Install"));

    // New settings panels exist.
    assert!(gui_src.contains("fn draw_settings_rendering"));
    assert!(gui_src.contains("fn draw_settings_optimisations"));
    assert!(gui_src.contains("SettingsTab::Optimisations =>"));
    assert!(gui_src.contains("SettingsTab::Rendering =>"));

    // Idle policy is consulted every frame.
    assert!(gui_src.contains("self.app_idle = (self.prefs.idle_when_unfocused && !focused)"));
    assert!(gui_src.contains("set_idle_hold"));
}

#[test]
fn readme_opens_with_the_brand_banner() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let readme = std::fs::read_to_string(manifest_dir.join("README.md")).unwrap();
    assert!(
        readme.contains("assets/brand/banner.png"),
        "README should open with the banner"
    );
    assert!(
        manifest_dir.join("assets/brand/banner.png").exists(),
        "banner asset is missing"
    );
    // Banner sits above the download section.
    let banner = readme.find("assets/brand/banner.png").unwrap();
    let download = readme.find("## Download and run it").unwrap();
    assert!(banner < download);
}

#[test]
fn header_telemetry_uses_fixed_columns() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let gui_src = std::fs::read_to_string(manifest_dir.join("src/gui.rs")).unwrap();
    let theme_src = std::fs::read_to_string(manifest_dir.join("src/theme.rs")).unwrap();

    // Values are drawn into pre-sized columns rather than one flowing label.
    assert!(gui_src.contains("paint_header_stats(ui.painter(), rect, &values, &pal)"));
    assert!(gui_src.contains("header_stats_width()"));
    assert!(gui_src.contains("allocate_exact_size"));
    assert!(
        !gui_src.contains("ms ({:.0} FPS)"),
        "old single-line telemetry is back"
    );
    assert!(theme_src.contains("pub const HEADER_STAT_WIDTHS"));
}

#[test]
fn app_copy_stays_plain() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let gui_src = std::fs::read_to_string(manifest_dir.join("src/gui.rs")).unwrap();

    // No parenthetical narrator lines such as the old status sentence.
    assert!(!gui_src.contains("Current state:"));
    assert!(gui_src.contains("Idle. The window is in the background"));
    assert!(gui_src.contains("Running. The window has focus."));

    // Marketing blurbs that read like generated copy stay out of the UI.
    for banned in [
        "high-throughput",
        "ensure UI chrome",
        "plain-language guides",
        "workstation.",
    ] {
        assert!(
            !gui_src.to_lowercase().contains(banned),
            "UI copy should stay plain: found '{banned}'"
        );
    }
}

#[test]
fn social_preview_is_the_size_github_expects() {
    // GitHub renders social previews at a 2:1 ratio and crops anything else,
    // so the shipped asset has to be exactly 1280x640.
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/brand/social-preview.png");
    let bytes = std::fs::read(&path).expect("social preview asset");

    assert_eq!(&bytes[1..4], b"PNG", "social preview must be a real PNG");
    // IHDR is always the first chunk: width and height are big-endian u32s.
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    assert_eq!((width, height), (1280, 640));
    assert!(bytes.len() < 1_000_000, "GitHub rejects files over 1 MB");
}
