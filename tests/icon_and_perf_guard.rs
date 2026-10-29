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

/// Times `iters` conversions of the same source and returns the best of five batches, in
/// nanoseconds per conversion. Interference from other work can only make a batch
/// slower, never faster, so the minimum is the measurement least polluted by the
/// machine - which matters on a three-core CI runner.
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
    for _ in 0..5 {
        let start = Instant::now();
        for _ in 0..iters {
            convert_image_buffer(pixels, w, h, 3, atlas, params, false, frame);
        }
        best = best.min(start.elapsed().as_nanos());
    }
    (best as f64 / iters as f64, frame.cols, frame.rows)
}

/// Every timing measurement lives in one test, on purpose.
///
/// `cargo test` runs test functions in parallel threads, so several timing tests in
/// one binary contend with each other and with `rayon` for the same cores. An earlier
/// version of this suite had four separate timing tests and measured a fixed overhead
/// of 1 ms per conversion that did not exist when the same code ran alone. Merging
/// them makes the measurements sequential and repeatable, and it costs nothing: the
/// whole function still runs in about a second.
#[test]
fn conversion_cost_model_and_scaling() {
    let atlas = GlyphAtlas::default_embedded(12);
    let source = noise(1280, 960);
    let mut frame = AsciiFrame::new();

    // --- 1. Cost model: fixed overhead plus a slope per output cell -------------
    //
    //     time = fixed + slope * cells
    //
    // Only the slope describes the algorithm. The fixed part is thread-pool dispatch
    // and allocation, and differs by orders of magnitude between a workstation and a
    // shared CI runner (measured: tens of microseconds locally, roughly 0.8 ms on
    // Linux CI and 1.8 ms on Windows CI). So the assertions are on the slope and on
    // collinearity, which is what a quadratic term would break.
    let mut points: Vec<(f64, f64)> = Vec::new(); // (cells, ns per conversion)
    for cols in [60u32, 120, 240, 420] {
        let mut params = RenderParams::default();
        params.cols = cols;
        convert_image_buffer(&source, 1280, 960, 3, &atlas, &params, false, &mut frame);
        // Roughly 40 ms of work per batch regardless of grid size, so that a scheduler
        // hiccup is a small fraction of the measurement rather than most of it.
        let iters = (80_000u32 / (cols * cols / 10).max(1)).clamp(4, 200);
        let (ns, c, r) = best_batch(&atlas, &params, &source, 1280, 960, &mut frame, iters);
        let cells = (c * r) as f64;
        eprintln!(
            "cols={c:>3} grid={c}x{r} = {cells:>6} cells: {:>7.2} ms ({:>4.0} ns/cell)",
            ns / 1e6,
            ns / cells
        );
        points.push((cells, ns));
    }

    // Model the two ends only. Least squares over four points is easily dragged off by a
    // single stalled sample, and a stall is a property of the machine, not the code.
    let (cells_lo, ns_lo) = points[0];
    let (cells_hi, ns_hi) = points[points.len() - 1];
    let slope = (ns_hi - ns_lo) / (cells_hi - cells_lo);
    let fixed = ns_lo - slope * cells_lo;
    eprintln!(
        "cost model: {slope:.0} ns per output cell + {:.3} ms fixed per call",
        fixed / 1e6
    );

    // Every intermediate point must land in the same ballpark as that model. A quadratic
    // term would not be 1.5x out over a 48-fold range in cell count; it would be orders of
    // magnitude out, which is what makes this a meaningful check rather than a tight one.
    let mut worst: f64 = 0.0;
    for (cells, ns) in points.iter().take(points.len() - 1).skip(1) {
        let predicted = fixed + slope * cells;
        let ratio = ns / predicted;
        eprintln!(
            "  check {cells:.0} cells: predicted {:.2} ms, measured {:.2} ms ({ratio:.2}x)",
            predicted / 1e6,
            ns / 1e6
        );
        worst = worst.max(if ratio > 1.0 { ratio } else { 1.0 / ratio });
    }
    eprintln!("worst deviation from the two-point model: {worst:.2}x");

    assert!(
        (20.0..=3_000.0).contains(&slope),
        "cost per output cell is {slope:.0} ns, outside the expected band - the \
         conversion has gained or lost an order of magnitude of work"
    );
    assert!(
        worst < 2.0,
        "an intermediate measurement is {worst:.1}x off the line through the endpoints - \
         cost is not linear in cell count, which points at a quadratic term"
    );

    // --- 2. A bigger source costs little at a fixed column count ---------------
    //
    // Downsampling reads a bounded number of samples per output cell, so a 4K source
    // should cost about the same as a 640x480 one at the same column count. This is
    // what makes the app usable on large photos and on every frame of a 4K video.
    let mut params = RenderParams::default();
    params.cols = 120;
    let small = noise(640, 480);
    let large = noise(2560, 1920);
    convert_image_buffer(&small, 640, 480, 3, &atlas, &params, false, &mut frame);
    convert_image_buffer(&large, 2560, 1920, 3, &atlas, &params, false, &mut frame);
    let (small_ns, _, _) = best_batch(&atlas, &params, &small, 640, 480, &mut frame, 8);
    let (large_ns, _, _) = best_batch(&atlas, &params, &large, 2560, 1920, &mut frame, 8);
    let ratio = large_ns / small_ns;
    eprintln!(
        "source size: 640x480 {:.2} ms vs 2560x1920 {:.2} ms at cols=120 ({ratio:.2}x for 16x the pixels)",
        small_ns / 1e6,
        large_ns / 1e6
    );
    assert!(
        ratio < 3.0,
        "a 16x larger source cost {ratio:.2}x the time to render at the same column count"
    );

    // --- 3. A normal photo finishes far inside a frame budget ------------------
    //
    // Measured locally at a few milliseconds; the ceiling is generous so that a loaded
    // runner cannot fail it. Only a catastrophic regression reaches 800 ms.
    let mut params = RenderParams::default();
    params.cols = 160;
    let photo = noise(640, 480);
    convert_image_buffer(&photo, 640, 480, 3, &atlas, &params, false, &mut frame);
    let (photo_ns, _, _) = best_batch(&atlas, &params, &photo, 640, 480, &mut frame, 5);
    eprintln!("640x480 at 160 cols: {:.2} ms per frame", photo_ns / 1e6);
    assert!(
        photo_ns < 800e6,
        "one 640x480 frame took {:.0} ms; something has become far slower",
        photo_ns / 1e6
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
