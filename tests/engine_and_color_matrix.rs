//! Exhaustive 8D shape-vector engine, 9 ColorModes x 6 matte/effect combinations,
//! odd/extreme image dimensions, overflow & invalid-channel guards, `yuv420p` even-pixel
//! rasterization, Tri-Tone hex parsing, and audio/temporal stability matrix (~240 `#[test]`s).

use lumaglyph::charset::{ASCII_MAX, ASCII_MIN};
use lumaglyph::color::{
    active_canvas_bg, parse_hex_rgb, ColorMode, TriTonePalette, BAYER_4X4, DEFAULT_BG, OLED_BG,
};
use lumaglyph::engine::{
    convert_image_buffer, convert_image_buffer_with_audio, rasterize_rgb24_even, rasterize_rgba,
    AsciiFrame, RenderParams,
};
use lumaglyph::glyph_atlas::GlyphAtlas;
use std::sync::OnceLock;

fn shared_atlas() -> &'static GlyphAtlas {
    static ATLAS: OnceLock<GlyphAtlas> = OnceLock::new();
    ATLAS.get_or_init(|| GlyphAtlas::default_embedded(20))
}

fn synth_test_rgba(w: u32, h: u32, phase: f32) -> Vec<u8> {
    let mut out = vec![0u8; (w * h * 4) as usize];
    let wf = w.max(1) as f32;
    let hf = h.max(1) as f32;
    for y in 0..h {
        let ny = (y as f32 / hf) * 2.0 - 1.0;
        for x in 0..w {
            let nx = (x as f32 / wf) * 2.0 - 1.0;
            let dist = (nx * nx + ny * ny).sqrt();
            if dist > 0.94 {
                let idx = ((y * w + x) * 4) as usize;
                out[idx + 3] = 255;
                continue;
            }
            let angle = ny.atan2(nx) + phase;
            let spoke = ((angle * 10.0).sin() * 4.0).clamp(-1.0, 1.0) * 0.5 + 0.5;
            let ring = ((dist * 24.0 + phase).cos() * 3.5).clamp(-1.0, 1.0) * 0.5 + 0.5;
            let gx = ((nx * 17.0).sin() * 4.5).clamp(-1.0, 1.0) * 0.5 + 0.5;
            let gy = ((ny * 15.0).cos() * 4.5).clamp(-1.0, 1.0) * 0.5 + 0.5;
            let ramp = ((nx + 1.0) * 0.5) * 0.65 + ((1.0 - ny) * 0.5) * 0.35;
            let idx = ((y * w + x) * 4) as usize;
            out[idx] = ((ring * spoke * 0.65 + ramp * 0.40).clamp(0.0, 1.0) * 255.0) as u8;
            out[idx + 1] = ((gx * gy * 0.65 + ramp * 0.35).clamp(0.0, 1.0) * 255.0) as u8;
            out[idx + 2] =
                (((1.0 - dist) * spoke * 0.75 + gx * 0.25).clamp(0.0, 1.0) * 255.0) as u8;
            out[idx + 3] = 255;
        }
    }
    out
}

fn shared_sample_rgba() -> &'static Vec<u8> {
    static SAMPLE: OnceLock<Vec<u8>> = OnceLock::new();
    SAMPLE.get_or_init(|| synth_test_rgba(240, 150, 0.2))
}

#[derive(Clone, Copy)]
enum PipelineVariant {
    Standard,
    OledBlack,
    TransparentAlpha,
    WireframeAlpha,
    BloomScanlines,
    InvertedDither,
}

fn verify_color_and_variant(cm: ColorMode, variant: PipelineVariant) {
    let atlas = shared_atlas();
    let src = shared_sample_rgba();
    let mut params = RenderParams {
        cols: 64,
        color_mode: cm,
        ..RenderParams::default()
    };
    match variant {
        PipelineVariant::Standard => {}
        PipelineVariant::OledBlack => {
            params.oled_black = true;
            params.black_gate = 0.08;
        }
        PipelineVariant::TransparentAlpha => {
            params.transparent_bg = true;
        }
        PipelineVariant::WireframeAlpha => {
            params.transparent_bg = true;
            params.wireframe_only = true;
            params.wireframe_sensitivity = 1.25;
        }
        PipelineVariant::BloomScanlines => {
            params.crt_bloom = true;
            params.scanlines = true;
        }
        PipelineVariant::InvertedDither => {
            params.invert = true;
            params.dither = 0.85;
        }
    }

    let mut frame = AsciiFrame::new();
    convert_image_buffer(src, 240, 150, 4, atlas, &params, false, &mut frame);
    assert_eq!(frame.cols, 64);
    assert!(frame.rows >= 1);
    assert_eq!(frame.cells.len(), (frame.cols * frame.rows) as usize);

    for cell in &frame.cells {
        assert!(
            (ASCII_MIN..=ASCII_MAX).contains(&cell.ch),
            "Out-of-range ASCII byte 0x{:02X}",
            cell.ch
        );
        if matches!(variant, PipelineVariant::OledBlack) && cm != ColorMode::DualColor {
            assert_eq!(cell.bg, OLED_BG);
        }
    }

    let mut rgba = Vec::new();
    let (pw, ph) = rasterize_rgba(&frame, atlas, &params, 0.25, &mut rgba);
    assert_eq!(rgba.len(), pw * ph * 4);
    if params.transparent_bg && !params.crt_bloom {
        assert!(
            rgba.chunks_exact(4).any(|px| px[3] == 0),
            "Transparent mode must produce zero-alpha background pixels"
        );
    } else if !params.transparent_bg {
        assert!(
            rgba.chunks_exact(4).all(|px| px[3] == 255),
            "Opaque mode must keep alpha == 255 on every pixel"
        );
    }
}

macro_rules! color_pipeline_matrix {
    ($($name:ident => ($cm:expr, $var:expr)),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                verify_color_and_variant($cm, $var);
            }
        )+
    };
}

color_pipeline_matrix! {
    cm_truecolor_std => (ColorMode::TrueColor, PipelineVariant::Standard),
    cm_truecolor_oled => (ColorMode::TrueColor, PipelineVariant::OledBlack),
    cm_truecolor_alpha => (ColorMode::TrueColor, PipelineVariant::TransparentAlpha),
    cm_truecolor_wf => (ColorMode::TrueColor, PipelineVariant::WireframeAlpha),
    cm_truecolor_bloom => (ColorMode::TrueColor, PipelineVariant::BloomScanlines),
    cm_truecolor_inv => (ColorMode::TrueColor, PipelineVariant::InvertedDither),

    cm_dual_std => (ColorMode::DualColor, PipelineVariant::Standard),
    cm_dual_oled => (ColorMode::DualColor, PipelineVariant::OledBlack),
    cm_dual_alpha => (ColorMode::DualColor, PipelineVariant::TransparentAlpha),
    cm_dual_wf => (ColorMode::DualColor, PipelineVariant::WireframeAlpha),
    cm_dual_bloom => (ColorMode::DualColor, PipelineVariant::BloomScanlines),
    cm_dual_inv => (ColorMode::DualColor, PipelineVariant::InvertedDither),

    cm_tritone_std => (ColorMode::TriTone, PipelineVariant::Standard),
    cm_tritone_oled => (ColorMode::TriTone, PipelineVariant::OledBlack),
    cm_tritone_alpha => (ColorMode::TriTone, PipelineVariant::TransparentAlpha),
    cm_tritone_wf => (ColorMode::TriTone, PipelineVariant::WireframeAlpha),
    cm_tritone_bloom => (ColorMode::TriTone, PipelineVariant::BloomScanlines),
    cm_tritone_inv => (ColorMode::TriTone, PipelineVariant::InvertedDither),

    cm_amber_std => (ColorMode::PhosphorAmber, PipelineVariant::Standard),
    cm_amber_oled => (ColorMode::PhosphorAmber, PipelineVariant::OledBlack),
    cm_amber_alpha => (ColorMode::PhosphorAmber, PipelineVariant::TransparentAlpha),
    cm_amber_wf => (ColorMode::PhosphorAmber, PipelineVariant::WireframeAlpha),
    cm_amber_bloom => (ColorMode::PhosphorAmber, PipelineVariant::BloomScanlines),
    cm_amber_inv => (ColorMode::PhosphorAmber, PipelineVariant::InvertedDither),

    cm_green_std => (ColorMode::PhosphorGreen, PipelineVariant::Standard),
    cm_green_oled => (ColorMode::PhosphorGreen, PipelineVariant::OledBlack),
    cm_green_alpha => (ColorMode::PhosphorGreen, PipelineVariant::TransparentAlpha),
    cm_green_wf => (ColorMode::PhosphorGreen, PipelineVariant::WireframeAlpha),
    cm_green_bloom => (ColorMode::PhosphorGreen, PipelineVariant::BloomScanlines),
    cm_green_inv => (ColorMode::PhosphorGreen, PipelineVariant::InvertedDither),

    cm_cyber_std => (ColorMode::Cyberpunk, PipelineVariant::Standard),
    cm_cyber_oled => (ColorMode::Cyberpunk, PipelineVariant::OledBlack),
    cm_cyber_alpha => (ColorMode::Cyberpunk, PipelineVariant::TransparentAlpha),
    cm_cyber_wf => (ColorMode::Cyberpunk, PipelineVariant::WireframeAlpha),
    cm_cyber_bloom => (ColorMode::Cyberpunk, PipelineVariant::BloomScanlines),
    cm_cyber_inv => (ColorMode::Cyberpunk, PipelineVariant::InvertedDither),

    cm_ansi16_std => (ColorMode::Ansi16, PipelineVariant::Standard),
    cm_ansi16_oled => (ColorMode::Ansi16, PipelineVariant::OledBlack),
    cm_ansi16_alpha => (ColorMode::Ansi16, PipelineVariant::TransparentAlpha),
    cm_ansi16_wf => (ColorMode::Ansi16, PipelineVariant::WireframeAlpha),
    cm_ansi16_bloom => (ColorMode::Ansi16, PipelineVariant::BloomScanlines),
    cm_ansi16_inv => (ColorMode::Ansi16, PipelineVariant::InvertedDither),

    cm_ansi256_std => (ColorMode::Xterm256, PipelineVariant::Standard),
    cm_ansi256_oled => (ColorMode::Xterm256, PipelineVariant::OledBlack),
    cm_ansi256_alpha => (ColorMode::Xterm256, PipelineVariant::TransparentAlpha),
    cm_ansi256_wf => (ColorMode::Xterm256, PipelineVariant::WireframeAlpha),
    cm_ansi256_bloom => (ColorMode::Xterm256, PipelineVariant::BloomScanlines),
    cm_ansi256_inv => (ColorMode::Xterm256, PipelineVariant::InvertedDither),

    cm_mono_std => (ColorMode::Monochrome, PipelineVariant::Standard),
    cm_mono_oled => (ColorMode::Monochrome, PipelineVariant::OledBlack),
    cm_mono_alpha => (ColorMode::Monochrome, PipelineVariant::TransparentAlpha),
    cm_mono_wf => (ColorMode::Monochrome, PipelineVariant::WireframeAlpha),
    cm_mono_bloom => (ColorMode::Monochrome, PipelineVariant::BloomScanlines),
    cm_mono_inv => (ColorMode::Monochrome, PipelineVariant::InvertedDither),
}

// Even-dimension guarantee for H.264 yuv420p across odd/prime column counts
macro_rules! even_rgb24_cols_tests {
    ($($name:ident => $cols:expr),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                let atlas = shared_atlas();
                let src = shared_sample_rgba();
                let params = RenderParams {
                    cols: $cols,
                    crt_bloom: true,
                    scanlines: true,
                    ..RenderParams::default()
                };
                let mut frame = AsciiFrame::new();
                convert_image_buffer(src, 240, 150, 4, atlas, &params, false, &mut frame);
                let mut rgb24 = Vec::new();
                let (ew, eh) = rasterize_rgb24_even(&frame, atlas, &params, 0.1, &mut rgb24);
                assert_eq!(ew % 2, 0, "yuv420p width must be even for cols={}", $cols);
                assert_eq!(eh % 2, 0, "yuv420p height must be even for cols={}", $cols);
                assert_eq!(rgb24.len(), ew * eh * 3);
            }
        )+
    };
}

even_rgb24_cols_tests! {
    even_cols_001_clamped => 1,
    even_cols_016 => 16,
    even_cols_017 => 17,
    even_cols_019 => 19,
    even_cols_023 => 23,
    even_cols_029 => 29,
    even_cols_031 => 31,
    even_cols_037 => 37,
    even_cols_041 => 41,
    even_cols_043 => 43,
    even_cols_047 => 47,
    even_cols_053 => 53,
    even_cols_059 => 59,
    even_cols_061 => 61,
    even_cols_067 => 67,
    even_cols_071 => 71,
    even_cols_073 => 73,
    even_cols_079 => 79,
    even_cols_083 => 83,
    even_cols_089 => 89,
    even_cols_097 => 97,
    even_cols_101 => 101,
    even_cols_113 => 113,
    even_cols_127 => 127,
    even_cols_131 => 131,
    even_cols_149 => 149,
    even_cols_157 => 157,
    even_cols_173 => 173,
    even_cols_199 => 199,
    even_cols_251 => 251,
    even_cols_317 => 317,
    even_cols_420 => 420,
    even_cols_999_clamped => 999,
}

// Extreme & degenerate image geometries (1x1, 1xN, Nx1, prime sizes, RGB24 & RGBA32)
macro_rules! geometry_tests {
    ($($name:ident => ($w:expr, $h:expr, $ch:expr)),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                let atlas = shared_atlas();
                let buf = vec![180u8; ($w as usize) * ($h as usize) * $ch];
                let params = RenderParams { cols: 32, ..RenderParams::default() };
                let mut frame = AsciiFrame::new();
                convert_image_buffer(&buf, $w, $h, $ch, atlas, &params, false, &mut frame);
                assert!(frame.cols >= 16);
                assert!(frame.rows >= 1);
                assert_eq!(frame.cells.len(), (frame.cols * frame.rows) as usize);
            }
        )+
    };
}

geometry_tests! {
    geom_1x1_rgb => (1, 1, 3),
    geom_1x1_rgba => (1, 1, 4),
    geom_1x64_tall_rgb => (1, 64, 3),
    geom_1x200_tall_rgba => (1, 200, 4),
    geom_64x1_wide_rgb => (64, 1, 3),
    geom_200x1_wide_rgba => (200, 1, 4),
    geom_2x2_rgb => (2, 2, 3),
    geom_2x2_rgba => (2, 2, 4),
    geom_3x3_rgb => (3, 3, 3),
    geom_5x7_rgb => (5, 7, 3),
    geom_7x5_rgba => (7, 5, 4),
    geom_13x17_rgb => (13, 17, 3),
    geom_19x23_rgba => (19, 23, 4),
    geom_31x31_rgb => (31, 31, 3),
    geom_63x65_rgba => (63, 65, 4),
    geom_100x100_rgb => (100, 100, 3),
    geom_128x72_rgba => (128, 72, 4),
}

// Malformed / short / overflow / invalid-channel input safety (zero panics!)
macro_rules! malformed_buffer_tests {
    ($($name:ident => ($len:expr, $w:expr, $h:expr, $ch:expr)),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                let atlas = shared_atlas();
                let buf = vec![0u8; $len];
                let params = RenderParams::default();
                let mut frame = AsciiFrame::new();
                convert_image_buffer(&buf, $w, $h, $ch, atlas, &params, false, &mut frame);
                assert_eq!(frame.cols, 0);
                assert_eq!(frame.rows, 0);
            }
        )+
    };
}

malformed_buffer_tests! {
    malformed_zero_w => (1024, 0, 100, 4),
    malformed_zero_h => (1024, 100, 0, 4),
    malformed_zero_both => (0, 0, 0, 4),
    malformed_short_by_1_rgba => (16 * 16 * 4 - 1, 16, 16, 4),
    malformed_short_by_1_rgb => (16 * 16 * 3 - 1, 16, 16, 3),
    malformed_empty_buf => (0, 64, 64, 4),
    malformed_channels_0 => (1024, 8, 8, 0),
    malformed_channels_1 => (1024, 8, 8, 1),
    malformed_channels_2 => (1024, 8, 8, 2),
    malformed_channels_5 => (1024, 8, 8, 5),
    malformed_channels_8 => (1024, 8, 8, 8),
    malformed_u32_max_overflow => (64, u32::MAX, u32::MAX, 4),
}

// Hex color & Tri-Tone parser matrix
macro_rules! hex_rgb_tests {
    ($($name:ident => ($s:expr, $expect:expr)),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                assert_eq!(parse_hex_rgb($s), $expect);
            }
        )+
    };
}

hex_rgb_tests! {
    hex_valid_upper => ("FF8000", Some([255, 128, 0])),
    hex_valid_lower => ("00e5ff", Some([0, 229, 255])),
    hex_valid_hash => ("#123456", Some([0x12, 0x34, 0x56])),
    hex_valid_trimmed => ("  #AABBCC  ", Some([0xAA, 0xBB, 0xCC])),
    hex_valid_black => ("000000", Some([0, 0, 0])),
    hex_valid_white => ("FFFFFF", Some([255, 255, 255])),
    hex_reject_empty => ("", None),
    hex_reject_3char => ("#FFF", None),
    hex_reject_5char => ("12345", None),
    hex_reject_7char => ("1234567", None),
    hex_reject_8char_alpha => ("123456FF", None),
    hex_reject_non_hex => ("GG0000", None),
    hex_reject_plus_sign => ("+12345", None),
}

macro_rules! tritone_parse_tests {
    ($($name:ident => ($s:expr, $ok:expr)),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                assert_eq!(TriTonePalette::from_hex_triplet($s).is_some(), $ok);
            }
        )+
    };
}

tritone_parse_tests! {
    tri_valid_hyphen => ("120428-00E5FF-FFE680", true),
    tri_valid_comma_hash => ("#120428,#00E5FF,#FFE680", true),
    tri_valid_lower => ("000000-888888-ffffff", true),
    tri_reject_two_stops => ("120428-00E5FF", false),
    tri_reject_four_stops => ("120428-00E5FF-FFE680-FFFFFF", false),
    tri_reject_bad_stop => ("120428-ZZZZZZ-FFE680", false),
    tri_reject_empty => ("", false),
}

// Non-finite audio_energy & sample preset coverage
macro_rules! audio_safety_tests {
    ($($name:ident => ($audio:expr, $ar:expr)),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                let atlas = shared_atlas();
                let src = shared_sample_rgba();
                let params = RenderParams {
                    cols: 48,
                    audio_reactive: $ar,
                    ..RenderParams::default()
                };
                let mut frame = AsciiFrame::new();
                convert_image_buffer_with_audio(src, 240, 150, 4, atlas, &params, true, $audio, &mut frame);
                assert_eq!(frame.cols, 48);
                assert!(frame.cells.iter().all(|c| (ASCII_MIN..=ASCII_MAX).contains(&c.ch)));
            }
        )+
    };
}

audio_safety_tests! {
    audio_zero => (0.0, 1.0),
    audio_normal_half => (0.5, 1.0),
    audio_peak_one => (1.0, 2.0),
    audio_negative_clamped => (-10.0, 1.5),
    audio_huge_clamped => (1000.0, 10.0),
    audio_nan_guarded => (f32::NAN, 1.5),
    audio_pos_inf_guarded => (f32::INFINITY, 1.5),
    audio_neg_inf_guarded => (f32::NEG_INFINITY, 1.5),
    audio_param_nan_guarded => (0.8, f32::NAN),
    audio_both_nan_guarded => (f32::NAN, f32::INFINITY),
}

#[test]
fn bayer_matrix_is_zero_mean_and_bounded() {
    let mut sum = 0.0f32;
    for row in &BAYER_4X4 {
        for &v in row {
            assert!((-0.5..=0.5).contains(&v));
            sum += v;
        }
    }
    assert!(sum.abs() < 1e-6, "4x4 Bayer matrix must have zero mean");
    assert_eq!(active_canvas_bg(false), DEFAULT_BG);
    assert_eq!(active_canvas_bg(true), OLED_BG);
}

#[test]
fn synthetic_test_patterns_generate_exact_rgba_buffers() {
    for t in [0.0f32, 0.5, 2.75] {
        let rgba = synth_test_rgba(96, 60, t);
        assert_eq!(rgba.len(), 96 * 60 * 4);
        assert!(rgba.chunks_exact(4).all(|px| px[3] == 255));
    }
}
