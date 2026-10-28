//! Exhaustive `lg2:` Look Code round-trip, clamping, `NaN`/`inf` injection defense,
//! all 5 exporters (`PNG`, `SVG`, `HTML`, `ANSI`, `TXT`) including XML/HTML XSS escaping,
//! Windows single-app console detachment (`should_attach_parent_console`), and `clap` CLI matrix (~260 `#[test]`s).

use clap::Parser;
use lumaglyph::charset::CharsetPreset;
use lumaglyph::cli::{parse_tritone_arg, should_attach_parent_console, Cli};
use lumaglyph::color::ColorMode;
use lumaglyph::engine::{AsciiCell, AsciiFrame, RenderParams};
use lumaglyph::export::{
    export_ansi, export_html, export_png, export_svg, export_txt, frame_to_ansi_string,
};
use lumaglyph::glyph_atlas::GlyphAtlas;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static EXP_SEQ: AtomicUsize = AtomicUsize::new(1);

fn temp_file(ext: &str) -> PathBuf {
    let id = EXP_SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("lumaglyph_exp_{}_{id}.{ext}", std::process::id()))
}

// ============================================================================
// 1. LOOK CODE (`lg2:`) EXHAUSTIVE KEY, CLAMP & NaN/ADVERSARIAL MATRIX (~95 TESTS)
// ============================================================================

macro_rules! look_valid_tests {
    ($($name:ident => ($code:expr, $check:expr)),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                let mut p = RenderParams::default();
                p.apply_look_code($code).unwrap();
                let checker: fn(&RenderParams) = $check;
                checker(&p);
            }
        )+
    };
}

look_valid_tests! {
    look_cols_min_clamp => ("lg2:cols=1", |p| assert_eq!(p.cols, 16)),
    look_cols_max_clamp => ("lg2:cols=99999", |p| assert_eq!(p.cols, 420)),
    look_cols_exact => ("lg2:cols=204", |p| assert_eq!(p.cols, 204)),
    look_legacy_lg1_prefix => ("lg1:cols=128", |p| assert_eq!(p.cols, 128)),
    look_backtick_wrapped => ("`lg2:cols=144`", |p| assert_eq!(p.cols, 144)),
    look_dquote_wrapped => ("\"lg2:cols=172\"", |p| assert_eq!(p.cols, 172)),
    look_squote_wrapped => ("'lg2:cols=96'", |p| assert_eq!(p.cols, 96)),

    look_cs_genuine95 => ("lg2:cs=genuine95", |p| assert_eq!(p.charset, CharsetPreset::Genuine95)),
    look_cs_structural => ("lg2:cs=structural", |p| assert_eq!(p.charset, CharsetPreset::StructuralEdge)),
    look_cs_dense => ("lg2:cs=dense", |p| assert_eq!(p.charset, CharsetPreset::TypographicDense)),
    look_cs_retro => ("lg2:cs=retro", |p| assert_eq!(p.charset, CharsetPreset::RetroClassic)),
    look_cs_custom_hex => ("lg2:cs=custom,cc=202E3A40", |p| {
        assert_eq!(p.charset, CharsetPreset::Custom);
        assert_eq!(p.custom_chars, " .:@");
    }),

    look_cm_truecolor => ("lg2:cm=truecolor", |p| assert_eq!(p.color_mode, ColorMode::TrueColor)),
    look_cm_dual => ("lg2:cm=dual", |p| assert_eq!(p.color_mode, ColorMode::DualColor)),
    look_cm_tritone => ("lg2:cm=tritone", |p| assert_eq!(p.color_mode, ColorMode::TriTone)),
    look_cm_amber => ("lg2:cm=amber", |p| assert_eq!(p.color_mode, ColorMode::PhosphorAmber)),
    look_cm_green => ("lg2:cm=green", |p| assert_eq!(p.color_mode, ColorMode::PhosphorGreen)),
    look_cm_cyberpunk => ("lg2:cm=cyberpunk", |p| assert_eq!(p.color_mode, ColorMode::Cyberpunk)),
    look_cm_ansi16 => ("lg2:cm=ansi16", |p| assert_eq!(p.color_mode, ColorMode::Ansi16)),
    look_cm_ansi256 => ("lg2:cm=xterm256", |p| assert_eq!(p.color_mode, ColorMode::Xterm256)),
    look_cm_mono => ("lg2:cm=mono", |p| assert_eq!(p.color_mode, ColorMode::Monochrome)),

    look_oled_clears_alpha => ("lg2:alpha=1,oled=1", |p| {
        assert!(p.oled_black);
        assert!(!p.transparent_bg);
    }),
    look_alpha_clears_oled => ("lg2:oled=1,alpha=1", |p| {
        assert!(p.transparent_bg);
        assert!(!p.oled_black);
    }),
    look_bool_words => ("lg2:wf=true,bl=yes,sc=on,inv=false", |p| {
        assert!(p.wireframe_only && p.crt_bloom && p.scanlines && !p.invert);
    }),

    look_gate_clamp_high => ("lg2:gate=9.9", |p| assert!((p.black_gate - 0.35).abs() < 1e-5)),
    look_gate_clamp_low => ("lg2:gate=-2.0", |p| assert_eq!(p.black_gate, 0.0)),
    look_wfs_clamp => ("lg2:wfs=99.0", |p| assert_eq!(p.wireframe_sensitivity, 3.0)),
    look_ar_clamp => ("lg2:ar=99.0", |p| assert_eq!(p.audio_reactive, 2.5)),
    look_cr_clamp => ("lg2:cr=99.0", |p| assert_eq!(p.directional_crunch, 3.5)),
    look_ew_clamp => ("lg2:ew=99.0", |p| assert_eq!(p.edge_weight, 2.5)),
    look_lum_clamp => ("lg2:lum=99.0", |p| assert_eq!(p.luminance_comp, 1.5)),
    look_bg_clamp => ("lg2:bg=99.0", |p| assert_eq!(p.bg_intensity, 0.65)),
    look_dt_clamp => ("lg2:dt=99.0", |p| assert_eq!(p.dither, 1.0)),
    look_vig_clamp => ("lg2:vig=99.0", |p| assert_eq!(p.vignette_focus, 1.0)),
    look_st_clamp => ("lg2:st=99.0", |p| assert_eq!(p.temporal_stability, 0.5)),
    look_br_clamp_low => ("lg2:br=-9.0", |p| assert_eq!(p.brightness, -0.5)),
    look_br_clamp_high => ("lg2:br=9.0", |p| assert_eq!(p.brightness, 0.5)),
    look_ct_clamp_low => ("lg2:ct=0.01", |p| assert_eq!(p.contrast, 0.3)),
    look_ct_clamp_high => ("lg2:ct=99.0", |p| assert_eq!(p.contrast, 3.0)),
    look_sat_clamp => ("lg2:sat=99.0", |p| assert_eq!(p.saturation, 3.0)),
    look_gam_clamp_low => ("lg2:gam=0.001", |p| assert_eq!(p.gamma, 0.25)),
    look_gam_clamp_high => ("lg2:gam=99.0", |p| assert_eq!(p.gamma, 3.0)),
    look_tri_hex => ("lg2:tri=010203-040506-070809", |p| {
        assert_eq!(p.tritone.shadow, [1, 2, 3]);
        assert_eq!(p.tritone.midtone, [4, 5, 6]);
        assert_eq!(p.tritone.highlight, [7, 8, 9]);
    }),
    look_cc_strips_control_bytes => ("lg2:cc=000A41427F", |p| {
        assert_eq!(p.custom_chars, "AB");
    }),
}

// Adversarial NaN / Infinity / malformed Look Code rejection tests
macro_rules! look_reject_tests {
    ($($name:ident => $code:expr),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                let mut p = RenderParams::default();
                assert!(
                    p.apply_look_code($code).is_err(),
                    "Expected Look Code '{}' to be rejected",
                    $code
                );
                // Verify all float fields remained finite!
                assert!(
                    p.black_gate.is_finite()
                        && p.wireframe_sensitivity.is_finite()
                        && p.audio_reactive.is_finite()
                        && p.directional_crunch.is_finite()
                        && p.edge_weight.is_finite()
                        && p.luminance_comp.is_finite()
                        && p.bg_intensity.is_finite()
                        && p.dither.is_finite()
                        && p.vignette_focus.is_finite()
                        && p.temporal_stability.is_finite()
                        && p.brightness.is_finite()
                        && p.contrast.is_finite()
                        && p.saturation.is_finite()
                        && p.gamma.is_finite()
                );
            }
        )+
    };
}

look_reject_tests! {
    look_rej_empty => "",
    look_rej_wrong_prefix => "lg3:cols=120",
    look_rej_no_prefix => "cols=120,cm=truecolor",
    look_rej_empty_body => "lg2:",
    look_rej_commas_only => "lg2:,,,,",
    look_rej_unknown_keys => "lg2:foo=1,bar=2",
    look_rej_gate_nan => "lg2:gate=NaN",
    look_rej_gate_inf => "lg2:gate=inf",
    look_rej_gate_neg_inf => "lg2:gate=-inf",
    look_rej_wfs_nan => "lg2:wfs=NaN",
    look_rej_ar_nan => "lg2:ar=NaN",
    look_rej_cr_nan => "lg2:cr=NaN",
    look_rej_ew_nan => "lg2:ew=NaN",
    look_rej_lum_nan => "lg2:lum=NaN",
    look_rej_bg_nan => "lg2:bg=NaN",
    look_rej_dt_nan => "lg2:dt=NaN",
    look_rej_vig_nan => "lg2:vig=NaN",
    look_rej_st_nan => "lg2:st=NaN",
    look_rej_br_nan => "lg2:br=NaN",
    look_rej_ct_nan => "lg2:ct=NaN",
    look_rej_sat_nan => "lg2:sat=NaN",
    look_rej_gam_nan => "lg2:gam=NaN",
    look_rej_gam_inf => "lg2:gam=Infinity",
    look_rej_cc_odd_hex => "lg2:cc=414",
    look_rej_cc_non_hex => "lg2:cc=ZZZZ",
    look_rej_cc_only_control => "lg2:cc=00010203",
}

#[test]
fn look_code_oversize_over_2048_bytes_is_rejected() {
    let huge = format!("lg2:cols=120,cc={}", "41".repeat(1200));
    let mut p = RenderParams::default();
    assert!(p.apply_look_code(&huge).is_err());
}

// ============================================================================
// 2. WINDOWS SINGLE-APP CONSOLE DETACHMENT & CLI MATRIX (~85 TESTS)
// ============================================================================

macro_rules! console_attach_tests {
    ($($name:ident => ($args:expr, $expect_attach:expr)),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                let slice: &[&str] = $args;
                assert_eq!(
                    should_attach_parent_console(slice),
                    $expect_attach,
                    "Unexpected console attachment decision for {:?}",
                    slice
                );
            }
        )+
    };
}

// GUI launches MUST return `false` (zero console attachment on Windows!)
// Headless CLI subcommands / --help / --version MUST return `true`!
console_attach_tests! {
    win_gui_bare_exe => (&["lumaglyph.exe"], false),
    win_gui_bare_unix => (&["lumaglyph"], false),
    win_gui_empty_args => (&[], false),
    win_gui_open_png => (&["lumaglyph.exe", "C:\\Users\\Artist\\render.png"], false),
    win_gui_open_mp4 => (&["lumaglyph.exe", "D:\\Footage\\scene 01.mp4"], false),
    win_gui_open_rel_file => (&["lumaglyph", "photo.jpg"], false),
    win_gui_studio_subcommand => (&["lumaglyph.exe", "studio"], false),
    win_gui_studio_with_theme => (&["lumaglyph.exe", "studio", "--theme", "tungsten"], false),
    win_gui_studio_with_settings => (&["lumaglyph.exe", "studio", "--settings", "about"], false),
    win_gui_studio_with_file_and_oled => (&["lumaglyph.exe", "studio", "clip.mp4", "--oled-black"], false),

    win_cli_help_short => (&["lumaglyph.exe", "-h"], true),
    win_cli_help_long => (&["lumaglyph.exe", "--help"], true),
    win_cli_help_subcmd => (&["lumaglyph.exe", "help"], true),
    win_cli_version_short => (&["lumaglyph.exe", "-V"], true),
    win_cli_version_long => (&["lumaglyph.exe", "--version"], true),
    win_cli_studio_help => (&["lumaglyph.exe", "studio", "--help"], true),
    win_cli_image_subcmd => (&["lumaglyph.exe", "image", "input.png"], true),
    win_cli_video_subcmd => (&["lumaglyph.exe", "video", "in.mp4", "-o", "out.mp4"], true),
    win_cli_bench_subcmd => (&["lumaglyph.exe", "bench"], true),
    win_cli_build_info => (&["lumaglyph.exe", "build-info"], true),
    win_cli_info_alias => (&["lumaglyph.exe", "info"], true),
    win_cli_look_subcmd => (&["lumaglyph.exe", "look"], true),
    win_cli_check_update => (&["lumaglyph.exe", "check-update"], true),
    win_cli_check_update_dl => (&["lumaglyph.exe", "check-update", "--download"], true),
    win_cli_unknown_flag => (&["lumaglyph.exe", "--bad-flag"], true),
}

macro_rules! clap_parse_tests {
    ($($name:ident => ($args:expr, $expect_ok:expr)),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                let slice: &[&str] = $args;
                assert_eq!(Cli::try_parse_from(slice).is_ok(), $expect_ok);
            }
        )+
    };
}

clap_parse_tests! {
    clap_no_args => (&["lumaglyph"], true),
    clap_single_file => (&["lumaglyph", "art.png"], true),
    clap_studio_full => (&[
        "lumaglyph", "studio", "in.png", "--theme", "graphite", "--settings", "appearance",
        "--oled-black", "--black-gate", "0.05", "--split", "0.4", "--zoom", "1.5"
    ], true),
    clap_image_full => (&[
        "lumaglyph", "image", "input.png", "-o", "out.png", "--cols", "160",
        "--charset", "genuine95", "--color", "tritone", "--tritone", "tokyo",
        "--oled-black", "--bloom", "--scanlines"
    ], true),
    clap_image_wireframe_alpha => (&[
        "lumaglyph", "image", "input.png", "-o", "out.svg", "--transparent",
        "--wireframe", "--wireframe-sens", "1.4"
    ], true),
    clap_reject_removed_studio_preset_flag => (&["lumaglyph", "studio", "--preset", "cyber"], false),
    clap_reject_removed_studio_live_flag => (&["lumaglyph", "studio", "--live-motion"], false),
    clap_video_full => (&[
        "lumaglyph", "video", "in.mp4", "-o", "out.webm", "--cols", "140", "--fps", "30",
        "--transparent", "--wireframe", "--audio-reactive", "1.2", "--stability", "0.2"
    ], true),
    clap_build_info => (&["lumaglyph", "build-info"], true),
    clap_info_alias => (&["lumaglyph", "info"], true),
    clap_bench => (&["lumaglyph", "bench", "--cols", "180", "--iters", "50"], true),
    clap_look_default => (&["lumaglyph", "look"], true),
    clap_look_inspect => (&["lumaglyph", "look", "lg2:cols=120"], true),
    clap_check_update_plain => (&["lumaglyph", "check-update"], true),
    clap_check_update_download => (&[
        "lumaglyph", "check-update", "--download", "--dest", "/tmp", "--allow-custom-repo"
    ], true),
    clap_reject_video_missing_output => (&["lumaglyph", "video", "in.mp4"], false),
    clap_reject_image_missing_input => (&["lumaglyph", "image"], false),
    clap_parse_bare_file_open => (&["lumaglyph", "dragged_media.png"], true),
    clap_reject_two_bare_files => (&["lumaglyph", "a.png", "b.png"], false),
}

#[test]
fn tritone_cli_aliases_and_hex_parse_cleanly() {
    for alias in [
        "solar",
        "flare",
        "inferno",
        "tokyo",
        "cyber",
        "neon",
        "blade",
        "amber",
        "emerald",
        "ice",
        "112233-445566-778899",
    ] {
        assert!(parse_tritone_arg(alias).is_ok());
    }
    assert!(parse_tritone_arg("not-a-palette").is_err());
}

// ============================================================================
// 3. EXPORTERS (`PNG`, `SVG`, `HTML`, `ANSI`, `TXT`) & XSS ESCAPING (~25 TESTS)
// ============================================================================

fn build_xss_probe_frame() -> AsciiFrame {
    // Frame containing `<script>&"'` characters to verify strict XML/HTML escaping!
    let chars = b"<script>&\"'/@# ";
    let cols = chars.len() as u32;
    let rows = 2u32;
    let mut cells = Vec::with_capacity((cols * rows) as usize);
    for r in 0..rows {
        for &ch in chars {
            cells.push(AsciiCell {
                ch,
                flags: 0,
                fg: [240, 120 + (r as u8) * 40, 60],
                bg: [9, 11, 16],
            });
        }
    }
    AsciiFrame {
        cols,
        rows,
        cells,
        prev_glyphs: Vec::new(),
        histogram: [0; 95],
        unique_glyphs: 14,
        render_ms: 0.5,
    }
}

#[test]
fn svg_exporter_escapes_xml_special_chars_and_respects_transparency() {
    let frame = build_xss_probe_frame();
    let svg_opaque = temp_file("svg");
    let svg_alpha = temp_file("svg");

    let p_opaque = RenderParams {
        oled_black: true,
        color_mode: ColorMode::DualColor,
        ..RenderParams::default()
    };
    export_svg(&frame, &p_opaque, &svg_opaque).unwrap();
    let content_opaque = std::fs::read_to_string(&svg_opaque).unwrap();
    assert!(!content_opaque.contains("<script>"));
    assert!(content_opaque.contains("&lt;script&gt;&amp;&quot;&#39;"));
    assert!(content_opaque.contains("fill=\"#000000\""));

    let p_alpha = RenderParams {
        transparent_bg: true,
        ..RenderParams::default()
    };
    export_svg(&frame, &p_alpha, &svg_alpha).unwrap();
    let content_alpha = std::fs::read_to_string(&svg_alpha).unwrap();
    assert!(!content_alpha.contains("<rect width=\"100%\""));

    let _ = std::fs::remove_file(svg_opaque);
    let _ = std::fs::remove_file(svg_alpha);
}

#[test]
fn html_exporter_escapes_script_tags_and_supports_transparent_and_oled() {
    let frame = build_xss_probe_frame();
    let html_oled = temp_file("html");
    let html_alpha = temp_file("html");

    export_html(
        &frame,
        &RenderParams {
            oled_black: true,
            ..RenderParams::default()
        },
        &html_oled,
    )
    .unwrap();
    let s_oled = std::fs::read_to_string(&html_oled).unwrap();
    assert!(!s_oled.contains("<script>"));
    assert!(s_oled.contains("&lt;"));
    assert!(s_oled.contains("#000000"));

    export_html(
        &frame,
        &RenderParams {
            transparent_bg: true,
            ..RenderParams::default()
        },
        &html_alpha,
    )
    .unwrap();
    let s_alpha = std::fs::read_to_string(&html_alpha).unwrap();
    assert!(s_alpha.contains("transparent"));

    let _ = std::fs::remove_file(html_oled);
    let _ = std::fs::remove_file(html_alpha);
}

#[test]
fn png_ansi_and_txt_exporters_roundtrip_and_reject_empty_frames() {
    let frame = build_xss_probe_frame();
    let atlas = GlyphAtlas::default_embedded(20);

    // Empty frame rejected cleanly by export_png
    let empty = AsciiFrame::new();
    let bad_png = temp_file("png");
    assert!(export_png(&empty, &atlas, 24, &RenderParams::default(), &bad_png).is_err());

    // Transparent PNG export + reload verification
    let alpha_png = temp_file("png");
    let (w, h) = export_png(
        &frame,
        &atlas,
        24,
        &RenderParams {
            transparent_bg: true,
            ..RenderParams::default()
        },
        &alpha_png,
    )
    .unwrap();
    let reloaded = image::open(&alpha_png).unwrap().to_rgba8();
    assert_eq!(reloaded.dimensions(), (w as u32, h as u32));
    assert!(reloaded.pixels().any(|px| px.0[3] == 0));
    assert!(reloaded.pixels().any(|px| px.0[3] > 180));
    let _ = std::fs::remove_file(alpha_png);

    // ANSI & TXT exports
    let ans_file = temp_file("ans");
    let txt_file = temp_file("txt");
    export_ansi(&frame, &RenderParams::default(), &ans_file).unwrap();
    export_txt(&frame, &txt_file).unwrap();

    let ans_text = frame_to_ansi_string(
        &frame,
        &RenderParams {
            oled_black: true,
            ..RenderParams::default()
        },
    );
    assert!(ans_text.contains("\x1b[48;2;"));
    assert!(ans_text.contains("\x1b[38;2;"));
    assert!(ans_text.contains("\x1b[0m"));

    let plain = std::fs::read_to_string(&txt_file).unwrap();
    assert_eq!(plain.lines().count(), 2);

    let _ = std::fs::remove_file(ans_file);
    let _ = std::fs::remove_file(txt_file);
}

#[test]
fn multibyte_utf8_and_plus_sign_boundary_regressions_never_panic() {
    use lumaglyph::color::parse_hex_rgb;
    use lumaglyph::updater::parse_sha256sums;

    // 3x 2-byte UTF-8 chars ("ééé") == 6 bytes total; must return None without UTF-8 slice panic
    assert_eq!(parse_hex_rgb("ééé"), None);
    assert_eq!(parse_hex_rgb("#ééé"), None);
    assert_eq!(parse_hex_rgb("+12345"), None);

    // Look code `cc=` with even byte-length multi-byte UTF-8 or leading '+'
    let mut p = RenderParams::default();
    assert!(p.apply_look_code("lg2:cc=éé").is_err());
    assert!(p.apply_look_code("lg2:cc=+9+9").is_err());

    // SHA256SUMS.txt with multi-byte UTF-8 straddling byte index 64
    let utf8_straddle = "é".repeat(40);
    assert!(parse_sha256sums(&utf8_straddle).is_err());
}
