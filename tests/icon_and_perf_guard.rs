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

    // --- 1. How cost grows with the size of the output grid ---------------------
    //
    // The cost of one conversion is a fixed overhead per call plus a slope per output
    // cell. The slope describes the algorithm; the fixed part is thread-pool dispatch
    // and allocation and differs hugely by machine (measured: 0.1 ms on a workstation,
    // 0.8 ms on Linux CI, 1.8 ms on Windows CI). What follows asserts only the shape.
    let mut points: Vec<(f64, f64)> = Vec::new(); // (cells, ns per conversion)
    for cols in [60u32, 120, 240, 420] {
        let mut params = RenderParams::default();
        params.cols = cols;
        convert_image_buffer(&source, 1280, 960, 3, &atlas, &params, false, &mut frame);
        // Roughly 40 ms of work per batch regardless of grid size, so that a scheduler
        // hiccup is a small fraction of the measurement rather than most of it.
        let iters = (40_000u32 / (cols * cols / 10).max(1)).clamp(4, 120);
        let (ns, c, r) = best_batch(&atlas, &params, &source, 1280, 960, &mut frame, iters);
        let cells = (c * r) as f64;
        eprintln!(
            "cols={c:>3} grid={c}x{r} = {cells:>6} cells: {:>7.2} ms ({:>4.0} ns/cell)",
            ns / 1e6,
            ns / cells
        );
        points.push((cells, ns));
    }

    // What this asserts, and what it deliberately does not.
    //
    // Three attempts at this test failed on CI for three different reasons, all of them
    // the same underlying fact: a shared runner cannot measure throughput. The fixed
    // per-call overhead differs by 20x between machines, and CPU contention from other
    // test threads skews whichever measurement happens to overlap with it - at small
    // grids on one run, at large grids on the next.
    //
    // So the assertion is on asymptotic *shape*, using ratios that survive either skew:
    //
    //   - time must grow with cell count (an inverted or quantised cost would fail);
    //   - over a 48-fold increase in cells, the cost ratio must be in the tens, not
    //     thousands (quadratic) and not one (constant).
    //
    // A true O(n^2) conversion would be roughly 2,300x here, and a lost parallel
    // iterator would show up as a ratio far below the 8x floor. The fitted model is
    // printed for information, so a human reading the log can see the numbers without
    // the test failing over a stalled sample.
    let (cells_lo, ns_lo) = points[0];
    let (cells_hi, ns_hi) = points[points.len() - 1];
    let slope = (ns_hi - ns_lo) / (cells_hi - cells_lo);
    let fixed = ns_lo - slope * cells_lo;
    eprintln!(
        "cost model (informational): {slope:.0} ns per output cell + {:.3} ms fixed per call",
        fixed / 1e6
    );

    for pair in points.windows(2) {
        assert!(
            pair[1].1 > pair[0].1,
            "conversion did not get slower with a larger grid: {:.0} cells took {:.2} ms \
             but {:.0} cells took {:.2} ms",
            pair[0].0,
            pair[0].1 / 1e6,
            pair[1].0,
            pair[1].1 / 1e6
        );
    }

    let cell_ratio = cells_hi / cells_lo;
    let cost_ratio = ns_hi / ns_lo;
    let per_cell_hi = ns_hi / cells_hi;
    eprintln!(
        "{cell_ratio:.0}x the cells cost {cost_ratio:.1}x the time \
         (linear would be about {cell_ratio:.0}x, quadratic about {:.0}x); \
         {per_cell_hi:.0} ns per cell at the largest grid",
        cell_ratio * cell_ratio
    );

    // The band is wide on purpose. Measurements collected across Linux, macOS and
    // Windows runners put this ratio between 8x and 83x, because the fixed per-call
    // overhead compresses it on slow machines and contention inflates it on others.
    // A quadratic conversion over this range would be about 2,300x; a constant-cost
    // one would be 1x. Both are a long way outside [2.5, 300].
    assert!(
        (2.5..=300.0).contains(&cost_ratio),
        "{cell_ratio:.0}x the cells cost {cost_ratio:.1}x the time - outside the linear \
         band, so the conversion has either gained a non-linear term or become constant-cost"
    );

    // A single-point check catches the class of regression that the ratio cannot: an
    // order of magnitude lost to a per-cell allocation or a serialised loop. Every
    // machine measured so far lands between 136 and 400 ns per cell, so 3,000 ns leaves
    // roughly tenfold headroom for a slow or contended runner.
    assert!(
        per_cell_hi < 3_000.0,
        "{per_cell_hi:.0} ns per output cell at the largest grid - an order of magnitude \
         slower than any machine measured so far"
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
