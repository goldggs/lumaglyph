//! Engine stress, invariants and fuzz.
//!
//! The conversion engine is a pure function from pixels to glyphs. That makes it
//! an ideal fuzz target: it takes attacker-shaped input (arbitrary image sizes,
//! arbitrary parameter values, arbitrary byte counts) and must never panic, never
//! read out of bounds, and never produce a frame whose dimensions disagree with
//! its cell buffer.
//!
//! No external fuzz crate is used on purpose. The project keeps its dependency
//! list short enough to audit by hand, and a deterministic generator with a fixed
//! seed gives reproducible failures, which is worth more here than coverage-guided
//! mutation.
//!
//! `field_reassign_with_default` is allowed here on purpose: these tests build a
//! parameter set by mutating a default one field at a time, sometimes conditionally
//! inside a loop. Rewriting that as a struct literal would bury the point of each
//! test (which values are hostile) inside a wall of `..Default::default()`.
#![allow(clippy::field_reassign_with_default)]

use lumaglyph::charset::CharsetPreset;
use lumaglyph::color::{ColorMode, TriTonePalette};
use lumaglyph::engine::{convert_image_buffer, rasterize_rgba, AsciiFrame, RenderParams};
use lumaglyph::glyph_atlas::GlyphAtlas;

/// Small deterministic PRNG (xorshift64*), so a failure reproduces exactly.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }
    fn range(&mut self, lo: u32, hi: u32) -> u32 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_u32() % (hi - lo))
    }
    fn unit(&mut self) -> f32 {
        (self.next_u32() % 100_000) as f32 / 100_000.0
    }
}

fn atlas() -> GlyphAtlas {
    GlyphAtlas::default_embedded(12)
}

/// A source image of the requested size with deterministic content.
fn image(w: u32, h: u32, channels: usize, seed: u64) -> Vec<u8> {
    let mut rng = Rng::new(seed);
    (0..(w as usize * h as usize * channels))
        .map(|_| (rng.next_u32() & 0xFF) as u8)
        .collect()
}

// ---------------------------------------------------------------------------
// Regression: hostile dimensions
// ---------------------------------------------------------------------------

#[test]
fn zero_and_degenerate_dimensions_are_refused_without_panicking() {
    let atlas = atlas();
    let params = RenderParams::default();
    let mut frame = AsciiFrame::new();
    for (w, h) in [(0u32, 0u32), (0, 10), (10, 0)] {
        let pixels = vec![0u8; 4];
        convert_image_buffer(&pixels, w, h, 3, &atlas, &params, false, &mut frame);
        assert_eq!(frame.cols, 0, "a {w}x{h} image produced a grid");
    }
}

#[test]
fn an_undersized_pixel_buffer_is_rejected() {
    let atlas = atlas();
    let params = RenderParams::default();
    let mut frame = AsciiFrame::new();
    // Claim 100x100 RGB but hand over 10 bytes: must return, not read past the end.
    convert_image_buffer(&[1u8; 10], 100, 100, 3, &atlas, &params, false, &mut frame);
    assert_eq!(frame.cols, 0);
}

#[test]
fn unsupported_channel_counts_are_rejected() {
    let atlas = atlas();
    let params = RenderParams::default();
    for channels in [0usize, 1, 2, 5, 8] {
        let mut frame = AsciiFrame::new();
        let pixels = vec![128u8; 64 * 64 * 8];
        convert_image_buffer(
            &pixels, 64, 64, channels, &atlas, &params, false, &mut frame,
        );
        assert_eq!(frame.cols, 0, "{channels} channels was accepted");
    }
}

#[test]
fn extreme_aspect_ratios_keep_the_grid_at_least_one_row() {
    let atlas = atlas();
    let params = RenderParams::default();
    for (w, h) in [(100_000u32, 1u32), (1, 100_000), (65_535, 2), (2, 65_535)] {
        let pixels = image(w.min(256), h.min(256), 3, w as u64 + h as u64);
        let iw = w.min(256);
        let ih = h.min(256);
        let mut frame = AsciiFrame::new();
        convert_image_buffer(&pixels, iw, ih, 3, &atlas, &params, false, &mut frame);
        assert!(frame.cols > 0 && frame.rows > 0, "{w}x{h} produced no grid");
        assert_eq!(frame.cells.len(), (frame.cols * frame.rows) as usize);
    }
}

#[test]
fn columns_are_clamped_to_the_supported_range() {
    let atlas = atlas();
    let pixels = image(64, 64, 3, 7);
    for cols in [0u32, 1, 15, 16, 420, 421, 100_000] {
        let mut params = RenderParams::default();
        params.cols = cols;
        let mut frame = AsciiFrame::new();
        convert_image_buffer(&pixels, 64, 64, 3, &atlas, &params, false, &mut frame);
        assert!(
            (16..=420).contains(&frame.cols),
            "cols {cols} produced {} columns",
            frame.cols
        );
    }
}

// ---------------------------------------------------------------------------
// Invariants across the whole parameter space
// ---------------------------------------------------------------------------

#[test]
fn frame_dimensions_always_agree_with_the_cell_buffer() {
    let atlas = atlas();
    let mut rng = Rng::new(0xBEEF);
    for _ in 0..60 {
        let mut params = RenderParams::default();
        params.cols = rng.range(16, 220);
        params.color_mode = ColorMode::ALL[(rng.next_u32() % 9) as usize];
        params.charset = CharsetPreset::ALL[(rng.next_u32() % 5) as usize];
        params.dither = rng.unit();
        params.edge_weight = rng.unit() * 2.0;
        let (w, h) = (rng.range(8, 200), rng.range(8, 200));
        let pixels = image(w, h, 3, rng.next_u64());
        let mut frame = AsciiFrame::new();
        convert_image_buffer(&pixels, w, h, 3, &atlas, &params, false, &mut frame);
        assert!(frame.cols >= 16);
        assert!(frame.rows >= 1);
        assert_eq!(
            frame.cells.len(),
            (frame.cols as usize) * (frame.rows as usize),
            "cell buffer length disagrees with {cols}x{rows}",
            cols = frame.cols,
            rows = frame.rows
        );
        // Every cell must be a printable ASCII byte, never a control character.
        for cell in &frame.cells {
            assert!(
                (0x20..=0x7E).contains(&cell.ch),
                "engine emitted byte {:#04X}, which is not printable ASCII",
                cell.ch
            );
        }
    }
}

#[test]
fn every_glyph_in_the_histogram_exists_in_the_active_charset() {
    let atlas = atlas();
    let pixels = image(96, 96, 3, 11);
    for preset in CharsetPreset::ALL {
        let mut params = RenderParams::default();
        params.charset = preset;
        params.cols = 80;
        let mut frame = AsciiFrame::new();
        convert_image_buffer(&pixels, 96, 96, 3, &atlas, &params, false, &mut frame);
        let mask = preset.active_mask(&params.custom_chars);
        for cell in &frame.cells {
            let idx = (cell.ch - 0x20) as usize;
            assert!(
                mask[idx],
                "{preset:?} emitted {:?} which is outside the active set",
                cell.ch as char
            );
        }
    }
}

#[test]
fn unique_glyph_count_matches_the_histogram() {
    let atlas = atlas();
    let pixels = image(120, 80, 3, 13);
    let params = RenderParams::default();
    let mut frame = AsciiFrame::new();
    convert_image_buffer(&pixels, 120, 80, 3, &atlas, &params, false, &mut frame);
    let histogram_total: u32 = frame.histogram.iter().sum();
    assert_eq!(
        histogram_total as usize,
        frame.cells.len(),
        "histogram does not account for every cell"
    );
    let distinct = frame.histogram.iter().filter(|&&n| n > 0).count();
    assert_eq!(
        frame.unique_glyphs, distinct,
        "unique_glyphs disagrees with histogram"
    );
}

#[test]
fn a_black_image_renders_as_blanks() {
    let atlas = atlas();
    let mut params = RenderParams::default();
    params.oled_black = true;
    params.black_gate = 0.1;
    let pixels = vec![0u8; 64 * 64 * 3];
    let mut frame = AsciiFrame::new();
    convert_image_buffer(&pixels, 64, 64, 3, &atlas, &params, false, &mut frame);
    let ink = frame.cells.iter().filter(|c| c.ch != b' ').count();
    assert_eq!(ink, 0, "{ink} cells drew ink on a pure black frame");
}

#[test]
fn a_white_image_renders_dense_glyphs() {
    let atlas = atlas();
    let params = RenderParams::default();
    let pixels = vec![255u8; 64 * 64 * 3];
    let mut frame = AsciiFrame::new();
    convert_image_buffer(&pixels, 64, 64, 3, &atlas, &params, false, &mut frame);
    let ink = frame.cells.iter().filter(|c| c.ch != b' ').count();
    let total = frame.cells.len();
    assert!(
        ink as f64 / total as f64 > 0.9,
        "a white frame produced only {ink}/{total} inked cells"
    );
}

#[test]
fn brighter_input_produces_at_least_as_much_ink() {
    // Monotonicity: the engine must not invert brightness.
    let atlas = atlas();
    let params = RenderParams::default();
    let mut previous = 0usize;
    for step in 0..=8 {
        let v = (step * 255 / 8) as u8;
        let pixels = vec![v; 48 * 48 * 3];
        let mut frame = AsciiFrame::new();
        convert_image_buffer(&pixels, 48, 48, 3, &atlas, &params, false, &mut frame);
        let ink = frame.cells.iter().filter(|c| c.ch != b' ').count();
        assert!(
            ink + 8 >= previous,
            "grey {v} produced less ink ({ink}) than the darker step ({previous})"
        );
        previous = previous.max(ink);
    }
}

#[test]
fn conversion_is_deterministic_for_identical_input() {
    let atlas = atlas();
    let pixels = image(96, 72, 3, 17);
    let params = RenderParams::default();
    let mut a = AsciiFrame::new();
    let mut b = AsciiFrame::new();
    convert_image_buffer(&pixels, 96, 72, 3, &atlas, &params, false, &mut a);
    convert_image_buffer(&pixels, 96, 72, 3, &atlas, &params, false, &mut b);
    assert_eq!(a.to_plain_text(), b.to_plain_text());
    assert_eq!(a.histogram, b.histogram);
}

#[test]
fn rgba_and_rgb_of_the_same_picture_render_identically() {
    let atlas = atlas();
    let params = RenderParams::default();
    let rgb = image(64, 64, 3, 19);
    let rgba: Vec<u8> = rgb
        .chunks_exact(3)
        .flat_map(|p| [p[0], p[1], p[2], 255])
        .collect();
    let mut a = AsciiFrame::new();
    let mut b = AsciiFrame::new();
    convert_image_buffer(&rgb, 64, 64, 3, &atlas, &params, false, &mut a);
    convert_image_buffer(&rgba, 64, 64, 4, &atlas, &params, false, &mut b);
    assert_eq!(a.to_plain_text(), b.to_plain_text());
}

// ---------------------------------------------------------------------------
// Hostile parameter values
// ---------------------------------------------------------------------------

#[test]
fn non_finite_parameters_never_reach_the_pipeline() {
    let atlas = atlas();
    let pixels = image(64, 64, 3, 23);
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut params = RenderParams::default();
        params.brightness = bad;
        params.contrast = bad;
        params.gamma = bad;
        params.saturation = bad;
        params.edge_weight = bad;
        params.dither = bad;
        params.black_gate = bad;
        params.vignette_focus = bad;
        params.luminance_comp = bad;
        params.directional_crunch = bad;
        let mut frame = AsciiFrame::new();
        convert_image_buffer(&pixels, 64, 64, 3, &atlas, &params, false, &mut frame);
        assert!(frame.cols > 0, "NaN parameters produced no output at all");
        for cell in &frame.cells {
            assert!((0x20..=0x7E).contains(&cell.ch));
        }
    }
}

#[test]
fn out_of_range_parameters_are_tolerated() {
    let atlas = atlas();
    let pixels = image(48, 48, 3, 29);
    let mut params = RenderParams::default();
    params.brightness = -99.0;
    params.contrast = 1e6;
    params.gamma = 1e-6;
    params.saturation = 1e6;
    params.dither = 99.0;
    params.edge_weight = 99.0;
    params.black_gate = 99.0;
    params.vignette_focus = 99.0;
    let mut frame = AsciiFrame::new();
    convert_image_buffer(&pixels, 48, 48, 3, &atlas, &params, false, &mut frame);
    assert!(frame.cols > 0);
}

#[test]
fn every_color_mode_produces_a_rasterisable_frame() {
    let atlas = atlas();
    let pixels = image(48, 48, 3, 31);
    let mut rgba = Vec::new();
    for mode in ColorMode::ALL {
        let mut params = RenderParams::default();
        params.color_mode = mode;
        params.tritone = TriTonePalette::SOLAR_FLARE;
        let mut frame = AsciiFrame::new();
        convert_image_buffer(&pixels, 48, 48, 3, &atlas, &params, false, &mut frame);
        let (w, h) = rasterize_rgba(&frame, &atlas, &params, 0.0, &mut rgba);
        assert!(w > 0 && h > 0, "{mode:?} produced an empty raster");
        assert_eq!(rgba.len(), w * h * 4);
    }
}

// ---------------------------------------------------------------------------
// Fuzz: random bytes must never panic
// ---------------------------------------------------------------------------

#[test]
fn random_image_bytes_never_panic() {
    let atlas = atlas();
    let mut rng = Rng::new(0xF0F0);
    for case in 0..40 {
        let w = rng.range(1, 128);
        let h = rng.range(1, 128);
        let channels = if case % 2 == 0 { 3 } else { 4 };
        let pixels = image(w, h, channels, rng.next_u64());
        let mut params = RenderParams::default();
        params.cols = rng.range(16, 200);
        params.color_mode = ColorMode::ALL[(rng.next_u32() % 9) as usize];
        params.dither = rng.unit();
        let mut frame = AsciiFrame::new();
        convert_image_buffer(&pixels, w, h, channels, &atlas, &params, false, &mut frame);
        assert!(frame.cols > 0, "fuzz case {case} produced no output");
    }
}

#[test]
fn truncated_buffers_are_ignored_not_read() {
    let atlas = atlas();
    let params = RenderParams::default();
    // Every possible truncation of an otherwise-valid 8x8 RGB buffer.
    let full = image(8, 8, 3, 37);
    for len in 0..full.len() {
        let mut frame = AsciiFrame::new();
        convert_image_buffer(&full[..len], 8, 8, 3, &atlas, &params, false, &mut frame);
        // Either it refuses (cols == 0) or it succeeds; it must never read past `len`.
        if len < 8 * 8 * 3 {
            assert_eq!(
                frame.cols, 0,
                "a truncated buffer at {len} bytes was accepted"
            );
        }
    }
}

#[test]
fn random_look_codes_never_panic_the_parser() {
    let mut rng = Rng::new(0x1234_5678);
    let alphabet: Vec<char> = "lg2:cols=,cs=cm=tri=0123456789ABCDEF-=\u{0}\u{1}\n\t\"'"
        .chars()
        .collect();
    for _ in 0..400 {
        let len = rng.range(0, 120) as usize;
        let candidate: String = (0..len)
            .map(|_| alphabet[(rng.next_u32() as usize) % alphabet.len()])
            .collect();
        let mut params = RenderParams::default();
        let _ = params.apply_look_code(&candidate);
        // Whatever survived must still serialise back to a valid code.
        let code = params.to_look_code();
        assert!(code.starts_with("lg2:"));
    }
}

#[test]
fn random_parameters_from_the_gui_sliders_never_panic() {
    // Mimics a user dragging every fader to both ends and back.
    let atlas = atlas();
    let pixels = image(64, 64, 3, 41);
    let mut rng = Rng::new(0xABCD);
    for _ in 0..50 {
        let mut params = RenderParams::default();
        params.brightness = rng.unit() * 2.0 - 1.0;
        params.contrast = rng.unit() * 4.0 - 1.0;
        params.saturation = rng.unit() * 3.0;
        params.gamma = rng.unit() * 4.0;
        params.edge_weight = rng.unit() * 2.0;
        params.directional_crunch = rng.unit() * 3.0;
        params.luminance_comp = rng.unit();
        params.bg_intensity = rng.unit() * 0.65;
        params.dither = rng.unit();
        params.vignette_focus = rng.unit();
        params.temporal_stability = rng.unit() * 0.5;
        params.audio_reactive = rng.unit() * 2.0;
        params.wireframe_sensitivity = rng.unit() * 2.0;
        params.oled_black = rng.next_u32() % 2 == 0;
        params.transparent_bg = !params.oled_black && rng.next_u32() % 2 == 0;
        params.crt_bloom = rng.next_u32() % 2 == 0;
        params.scanlines = rng.next_u32() % 2 == 0;
        params.invert = rng.next_u32() % 2 == 0;
        params.charset = CharsetPreset::ALL[(rng.next_u32() % 5) as usize];
        params.color_mode = ColorMode::ALL[(rng.next_u32() % 9) as usize];
        let mut frame = AsciiFrame::new();
        convert_image_buffer(&pixels, 64, 64, 3, &atlas, &params, false, &mut frame);
        assert!(frame.cells.len() == (frame.cols * frame.rows) as usize);
    }
}

// ---------------------------------------------------------------------------
// Resource sanity
// ---------------------------------------------------------------------------

#[test]
fn a_large_frame_stays_within_a_predictable_memory_ceiling() {
    // 420 columns x up to 2000 rows is the documented maximum.
    let atlas = atlas();
    let mut params = RenderParams::default();
    params.cols = 420;
    let pixels = image(200, 400, 3, 43);
    let mut frame = AsciiFrame::new();
    convert_image_buffer(&pixels, 200, 400, 3, &atlas, &params, false, &mut frame);
    // One cell is a handful of bytes; the buffer should never balloon.
    let bytes = frame.cells.len() * std::mem::size_of::<lumaglyph::engine::AsciiCell>();
    assert!(
        bytes < 32 * 1024 * 1024,
        "cell buffer grew to {bytes} bytes"
    );
}

#[test]
fn reusing_a_frame_does_not_accumulate_state() {
    let atlas = atlas();
    let params = RenderParams::default();
    let mut frame = AsciiFrame::new();
    let a = image(64, 64, 3, 47);
    convert_image_buffer(&a, 64, 64, 3, &atlas, &params, false, &mut frame);
    let first = frame.to_plain_text();
    // Convert something completely different, then come back.
    let b = image(32, 96, 3, 53);
    convert_image_buffer(&b, 32, 96, 3, &atlas, &params, false, &mut frame);
    convert_image_buffer(&a, 64, 64, 3, &atlas, &params, false, &mut frame);
    assert_eq!(
        frame.to_plain_text(),
        first,
        "frame reuse left stale state behind"
    );
}
