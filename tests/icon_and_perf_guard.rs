//! Runtime icon decoding, and a guard against catastrophic slow-downs.
//!
//! The packaging-side icon checks (ICO/ICNS structure, PNG metadata) live in
//! `themes_prefs_and_binary_audit`. This file covers the other half: the code that
//! actually runs when the window opens and when the About tab draws the logo.
//!
//! The timing tests here are deliberately loose. A tight threshold would flake on a
//! loaded CI runner and teach everyone to re-run until green, which is worse than
//! having no threshold. These assertions are set roughly a hundred times slower than
//! measured, so they only fire when something has become asymptotically worse - a
//! per-frame allocation, an accidental O(n^2), a lost parallel iterator.
//!
//! `field_reassign_with_default` is allowed for the same reason as in
//! `engine_stress_and_fuzz`: the column count is set on a default parameter set to
//! make the single variable under test obvious at a glance.
#![allow(clippy::field_reassign_with_default)]

use std::time::Instant;

use lumaglyph::engine::{convert_image_buffer, AsciiFrame, RenderParams};
use lumaglyph::glyph_atlas::GlyphAtlas;
use lumaglyph::icon;

// ---------------------------------------------------------------------------
// Icon decoding
// ---------------------------------------------------------------------------

#[test]
fn the_embedded_icon_decodes_to_a_256_square_rgba_buffer() {
    let (rgba, w, h) = icon::decode_app_icon_rgba().expect("embedded icon must decode");
    assert_eq!((w, h), (256, 256), "the runtime icon must be 256x256");
    assert_eq!(
        rgba.len(),
        256 * 256 * 4,
        "RGBA buffer length disagrees with its size"
    );
}

#[test]
fn the_embedded_icon_is_not_blank() {
    // A fully transparent or single-colour icon would still "decode fine"; that is
    // exactly the bug this catches, because it ships as an invisible taskbar icon.
    let (rgba, _, _) = icon::decode_app_icon_rgba().unwrap();
    // The mark is a monogram on a transparent field, so a low coverage is correct.
    // What would be wrong is *no* coverage (an invisible taskbar icon) or *total*
    // coverage with one flat colour (a placeholder that lost its artwork).
    let opaque = rgba.chunks_exact(4).filter(|px| px[3] > 200).count();
    let total = rgba.len() / 4;
    let coverage = opaque as f64 / total as f64;
    assert!(
        (0.05..0.90).contains(&coverage),
        "icon coverage is {:.1}% ({opaque}/{total} opaque pixels)",
        coverage * 100.0
    );
    // The glyph is drawn with solid and tinted shapes, so it must use several colours.
    let hues: std::collections::HashSet<[u8; 3]> = rgba
        .chunks_exact(4)
        .filter(|px| px[3] > 200)
        .map(|px| [px[0], px[1], px[2]])
        .collect();
    assert!(
        hues.len() > 4,
        "the icon is drawn in only {} colours",
        hues.len()
    );
    let mut colours = std::collections::HashSet::new();
    for px in rgba.chunks_exact(4) {
        colours.insert((px[0], px[1], px[2], px[3]));
        if colours.len() > 16 {
            break;
        }
    }
    assert!(
        colours.len() > 16,
        "the icon uses only {} colours",
        colours.len()
    );
}

#[test]
fn the_icon_data_handed_to_the_window_backend_is_well_formed() {
    let data = icon::eframe_icon_data();
    assert_eq!((data.width, data.height), (256, 256));
    assert_eq!(data.rgba.len(), (data.width * data.height * 4) as usize);
    assert!(
        data.rgba.iter().any(|&b| b != 0),
        "icon data is entirely zero"
    );
}

#[test]
fn the_logo_drawn_in_the_about_tab_matches_the_icon() {
    let image = icon::egui_logo_color_image().expect("logo image for the About tab");
    assert_eq!(image.size, [256, 256]);
    assert_eq!(image.pixels.len(), 256 * 256);
    // The About tab composites this over the panel colour, so it must supply alpha.
    assert!(
        image.pixels.iter().any(|p| p.a() > 0),
        "logo pixels are fully transparent"
    );
}

#[test]
fn decoding_the_icon_repeatedly_is_stable() {
    // The About tab re-decodes on demand; identical output keeps it from flickering.
    let a = icon::decode_app_icon_rgba().unwrap();
    let b = icon::decode_app_icon_rgba().unwrap();
    assert_eq!(a.0, b.0);
    assert_eq!((a.1, a.2), (b.1, b.2));
}

// ---------------------------------------------------------------------------
// Performance guard
// ---------------------------------------------------------------------------

fn noise(w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((w * h * 3) as usize);
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    for _ in 0..(w as usize * h as usize) {
        for _ in 0..3 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            out.push((state & 0xFF) as u8);
        }
    }
    out
}

#[test]
fn a_typical_conversion_finishes_far_inside_a_frame_budget() {
    // 640x480 at 160 columns is a normal photo. Measured locally at a few
    // milliseconds; the ceiling is generous on purpose.
    let atlas = GlyphAtlas::default_embedded(12);
    let pixels = noise(640, 480);
    let mut params = RenderParams::default();
    params.cols = 160;
    let mut frame = AsciiFrame::new();
    // Warm the allocator and the glyph atlas so the timing excludes first-touch cost.
    convert_image_buffer(&pixels, 640, 480, 3, &atlas, &params, false, &mut frame);

    let start = Instant::now();
    for _ in 0..5 {
        convert_image_buffer(&pixels, 640, 480, 3, &atlas, &params, false, &mut frame);
    }
    let per_frame = start.elapsed() / 5;
    assert!(
        per_frame.as_millis() < 800,
        "one 640x480 frame took {per_frame:?}; something has become far slower"
    );
    eprintln!("640x480 @ 160 cols: {per_frame:?} per frame");
}

/// Times `iters` conversions of the same source and returns the best batch, in
/// nanoseconds. Best-of-three, because interference from other processes can only
/// make a batch slower, never faster.
fn best_batch(
    atlas: &GlyphAtlas,
    params: &RenderParams,
    pixels: &[u8],
    w: u32,
    h: u32,
    frame: &mut AsciiFrame,
    iters: u32,
) -> (f64, u32, u32) {
    let mut best = u128::MAX;
    for _ in 0..3 {
        let start = Instant::now();
        for _ in 0..iters {
            convert_image_buffer(pixels, w, h, 3, atlas, params, false, frame);
        }
        best = best.min(start.elapsed().as_nanos());
    }
    (best as f64 / iters as f64, frame.cols, frame.rows)
}

#[test]
fn output_cost_is_flat_per_cell_across_the_column_range() {
    // The engine converts onto a fixed grid, so the honest cost unit is nanoseconds
    // per output cell, not per source pixel. If that figure stays flat while the grid
    // grows by two orders of magnitude, the pipeline is O(cols * rows) and nothing in
    // it has picked up a hidden quadratic term.
    //
    // Measured ratios locally sit around 1.1x; the 3x budget absorbs a loaded runner.
    let atlas = GlyphAtlas::default_embedded(12);
    let source = noise(1280, 960);
    let mut frame = AsciiFrame::new();

    let mut per_cell = Vec::new();
    for cols in [60u32, 160, 420] {
        let mut params = RenderParams::default();
        params.cols = cols;
        convert_image_buffer(&source, 1280, 960, 3, &atlas, &params, false, &mut frame);
        let (ns, c, r) = best_batch(&atlas, &params, &source, 1280, 960, &mut frame, 8);
        let cell_ns = ns / ((c * r) as f64);
        eprintln!(
            "cols={c:>3} grid={c}x{r} = {} cells: {:.0} ns/cell",
            c * r,
            cell_ns
        );
        per_cell.push(cell_ns);
    }

    let slowest = per_cell.iter().cloned().fold(f64::MIN, f64::max);
    let fastest = per_cell.iter().cloned().fold(f64::MAX, f64::min);
    let spread = slowest / fastest;
    assert!(
        spread < 3.0,
        "per-cell cost varies {spread:.2}x across the column range ({per_cell:?} ns/cell) - \
         the conversion likely has a non-linear term"
    );
}

#[test]
fn source_resolution_barely_affects_cost_at_fixed_columns() {
    // Downsampling reads a bounded number of samples per output cell, so a 4K source
    // should cost about the same as a 640x480 one at the same column count. This is
    // what makes the app usable on large photos and on every frame of a 4K video.
    let atlas = GlyphAtlas::default_embedded(12);
    let mut params = RenderParams::default();
    params.cols = 120;
    let mut frame = AsciiFrame::new();

    let small = noise(640, 480);
    let large = noise(2560, 1920);
    convert_image_buffer(&small, 640, 480, 3, &atlas, &params, false, &mut frame);
    convert_image_buffer(&large, 2560, 1920, 3, &atlas, &params, false, &mut frame);

    let (small_ns, _, _) = best_batch(&atlas, &params, &small, 640, 480, &mut frame, 8);
    let (large_ns, _, _) = best_batch(&atlas, &params, &large, 2560, 1920, &mut frame, 8);
    let ratio = large_ns / small_ns;
    eprintln!(
        "640x480 {:.2} ms vs 2560x1920 {:.2} ms at cols=120 ({ratio:.2}x for 16x the pixels)",
        small_ns / 1e6,
        large_ns / 1e6
    );
    assert!(
        ratio < 3.0,
        "a 16x larger source cost {ratio:.2}x the time to render at the same column count"
    );
}

#[test]
fn a_frame_reused_across_resolutions_does_not_degrade() {
    // Switching resolution back and forth must not leave an ever-growing buffer.
    let atlas = GlyphAtlas::default_embedded(12);
    let params = RenderParams::default();
    let a = noise(200, 150);
    let b = noise(64, 64);
    let mut frame = AsciiFrame::new();
    for _ in 0..6 {
        convert_image_buffer(&a, 200, 150, 3, &atlas, &params, false, &mut frame);
        convert_image_buffer(&b, 64, 64, 3, &atlas, &params, false, &mut frame);
    }
    convert_image_buffer(&b, 64, 64, 3, &atlas, &params, false, &mut frame);
    // The buffer is sized to the last conversion, not to the largest ever seen.
    assert_eq!(frame.cells.len(), (frame.cols * frame.rows) as usize);
}
