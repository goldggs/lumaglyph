//! Golden-image visual regression tests.
//!
//! Unit tests prove that functions return the values they are supposed to. They do
//! not prove that a rendered picture still *looks* right. This suite renders fixed
//! scenes through the real pipeline (convert -> rasterise -> PNG) and compares the
//! result against a committed reference image.
//!
//! Comparison is tolerance-based rather than a hash. A hash would fail on a
//! one-bit change from a different FPU, a different libm, or a different SIMD path
//! on another architecture, and CI runs on three of them. Instead we allow a small
//! per-channel drift and a small fraction of differing pixels, which absorbs that
//! noise while still catching real changes such as "the mipmap fix regressed",
//! "bloom got twice as strong", or "transparency stopped working".
//!
//! Re-blessing after an intentional visual change:
//!
//! ```text
//! LUMAGLYPH_BLESS=1 cargo test --release --test golden_render
//! git add tests/golden
//! ```
//!
//! `field_reassign_with_default` is allowed so a scene reads as the handful of
//! settings it actually varies, rather than a 40-field struct literal.
#![allow(clippy::field_reassign_with_default)]
//!
//! Always look at the diff before blessing. A blessed regression is a silent one.

use lumaglyph::charset::CharsetPreset;
use lumaglyph::color::{ColorMode, TriTonePalette};
use lumaglyph::engine::{convert_image_buffer, rasterize_rgba, AsciiFrame, RenderParams};
use lumaglyph::glyph_atlas::GlyphAtlas;
use std::path::{Path, PathBuf};

/// Largest per-channel difference that is considered "the same picture".
const CHANNEL_TOLERANCE: i32 = 6;

/// Share of pixels allowed to exceed `CHANNEL_TOLERANCE`, as a fraction.
const DIFFERING_FRACTION: f64 = 0.02;

/// Column count for reference renders.
///
/// Deliberately smaller than the app default (156) so the committed references stay
/// a few hundred kilobytes instead of megabytes, while still being wide enough that
/// glyph choice, edges and colour grading are all exercised.
const GOLDEN_COLS: u32 = 72;

fn base_params() -> RenderParams {
    let mut params = RenderParams::default();
    params.cols = GOLDEN_COLS;
    params
}

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn blessing() -> bool {
    std::env::var("LUMAGLYPH_BLESS")
        .map(|v| v == "1")
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Deterministic source images
// ---------------------------------------------------------------------------

/// A scene is a name plus an RGB24 buffer at a fixed size.
struct Scene {
    name: &'static str,
    width: u32,
    height: u32,
    rgb: Vec<u8>,
}

fn scene_smooth_gradient() -> Scene {
    let (w, h) = (128u32, 96u32);
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let r = (x as f32 / w as f32 * 255.0) as u8;
            let g = (y as f32 / h as f32 * 255.0) as u8;
            let b = ((x + y) as f32 / (w + h) as f32 * 255.0) as u8;
            rgb.extend_from_slice(&[r, g, b]);
        }
    }
    Scene {
        name: "smooth-gradient",
        width: w,
        height: h,
        rgb,
    }
}

fn scene_hard_edges() -> Scene {
    // Rectangles and a diagonal: exercises the structure tensor and glyph choice.
    let (w, h) = (128u32, 96u32);
    let mut rgb = vec![0u8; (w * h * 3) as usize];
    let mut put = |x: u32, y: u32, c: [u8; 3]| {
        if x < w && y < h {
            let i = ((y * w + x) * 3) as usize;
            rgb[i..i + 3].copy_from_slice(&c);
        }
    };
    for y in 0..h {
        for x in 0..w {
            let on_diagonal = (x as i32 - y as i32).abs() < 3;
            let in_box = (20..70).contains(&x) && (20..70).contains(&y);
            let in_stripe = (90..110).contains(&x);
            let c = if on_diagonal {
                [250, 250, 250]
            } else if in_box {
                [220, 40, 40]
            } else if in_stripe {
                [40, 220, 40]
            } else {
                [8, 8, 10]
            };
            put(x, y, c);
        }
    }
    Scene {
        name: "hard-edges",
        width: w,
        height: h,
        rgb,
    }
}

fn scene_dark_and_detail() -> Scene {
    // Mostly dark with a few bright specular pixels, to catch the shadow gate
    // and OLED black floor drifting.
    let (w, h) = (96u32, 96u32);
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let vignette = {
                let dx = (x as f32 - w as f32 / 2.0) / (w as f32 / 2.0);
                let dy = (y as f32 - h as f32 / 2.0) / (h as f32 / 2.0);
                (1.0 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0)
            };
            let spike = if x % 24 < 2 && y % 24 < 2 { 255.0 } else { 0.0 };
            let v = (vignette * 40.0 + spike) as u8;
            rgb.extend_from_slice(&[v, v.saturating_sub(4), v.saturating_sub(8)]);
        }
    }
    Scene {
        name: "dark-and-detail",
        width: w,
        height: h,
        rgb,
    }
}

fn scene_checker() -> Scene {
    // High-frequency detail: a regression in minification or dithering shows up fast.
    let (w, h) = (64u32, 64u32);
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let on = ((x / 8) + (y / 8)) % 2 == 0;
            let v = if on { 230 } else { 25 };
            rgb.extend_from_slice(&[v, v / 2, 255 - v]);
        }
    }
    Scene {
        name: "checker",
        width: w,
        height: h,
        rgb,
    }
}

// ---------------------------------------------------------------------------
// Render helpers
// ---------------------------------------------------------------------------

struct Render<'a> {
    scene: &'a Scene,
    params: RenderParams,
    cell_h: usize,
}

fn render(render: &Render<'_>) -> (Vec<u8>, u32, u32) {
    let atlas = GlyphAtlas::default_embedded(render.cell_h);
    let mut frame = AsciiFrame::new();
    convert_image_buffer(
        &render.scene.rgb,
        render.scene.width,
        render.scene.height,
        3,
        &atlas,
        &render.params,
        false,
        &mut frame,
    );
    assert!(
        frame.cols > 0 && frame.rows > 0,
        "{}: engine produced no cells",
        render.scene.name
    );
    let mut rgba = Vec::new();
    let (w, h) = rasterize_rgba(&frame, &atlas, &render.params, 0.0, &mut rgba);
    assert!(
        w > 0 && h > 0,
        "{}: rasteriser produced no pixels",
        render.scene.name
    );
    (rgba, w as u32, h as u32)
}

/// Compare two RGBA buffers, returning `(mean_channel_delta, differing_fraction)`.
fn compare(a: &[u8], b: &[u8]) -> (f64, f64) {
    assert_eq!(
        a.len(),
        b.len(),
        "image sizes differ, so the layout changed"
    );
    if a.is_empty() {
        return (0.0, 0.0);
    }
    let mut total_delta: u64 = 0;
    let mut differing: u64 = 0;
    let pixels = (a.len() / 4) as u64;
    for (pa, pb) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
        let mut worst = 0i32;
        for c in 0..4 {
            let d = (pa[c] as i32 - pb[c] as i32).abs();
            total_delta += d as u64;
            worst = worst.max(d);
        }
        if worst > CHANNEL_TOLERANCE {
            differing += 1;
        }
    }
    (
        total_delta as f64 / a.len() as f64,
        differing as f64 / pixels as f64,
    )
}

fn check_golden(name: &str, rgba: &[u8], w: u32, h: u32) {
    let dir = golden_dir();
    std::fs::create_dir_all(&dir).expect("create golden dir");
    let path = dir.join(format!("{name}.png"));

    if blessing() || !path.exists() {
        let img: image::RgbaImage =
            image::ImageBuffer::from_raw(w, h, rgba.to_vec()).expect("buffer size");
        img.save(&path).expect("write golden");
        eprintln!("blessed golden image: {}", path.display());
        return;
    }

    let reference = image::open(&path)
        .unwrap_or_else(|e| panic!("{}: unreadable golden: {e}", path.display()))
        .to_rgba8();
    if reference.width() != w || reference.height() != h {
        panic!(
            "{name}: size changed from {}x{} to {w}x{h}. If that was intended, re-bless.",
            reference.width(),
            reference.height()
        );
    }

    let (mean_delta, differing) = compare(rgba, reference.as_raw());
    assert!(
        mean_delta <= CHANNEL_TOLERANCE as f64,
        "{name}: mean channel drift {mean_delta:.2} exceeds tolerance {CHANNEL_TOLERANCE}"
    );
    assert!(
        differing <= DIFFERING_FRACTION,
        "{name}: {:.1}% of pixels changed (limit {:.1}%)",
        differing * 100.0,
        DIFFERING_FRACTION * 100.0
    );
}

// ---------------------------------------------------------------------------
// Golden scenes
// ---------------------------------------------------------------------------

#[test]
fn golden_truecolor_standard_scene() {
    let scene = scene_smooth_gradient();
    let (rgba, w, h) = render(&Render {
        scene: &scene,
        params: base_params(),
        cell_h: 12,
    });
    check_golden("truecolor-standard", &rgba, w, h);
}

#[test]
fn golden_oled_black_scene() {
    let scene = scene_dark_and_detail();
    let mut params = base_params();
    params.oled_black = true;
    params.black_gate = 0.06;
    let (rgba, w, h) = render(&Render {
        scene: &scene,
        params,
        cell_h: 12,
    });
    check_golden("oled-black", &rgba, w, h);
}

#[test]
fn golden_edge_geometry_scene() {
    let scene = scene_hard_edges();
    let mut params = base_params();
    params.edge_weight = 1.4;
    params.directional_crunch = 1.6;
    let (rgba, w, h) = render(&Render {
        scene: &scene,
        params,
        cell_h: 12,
    });
    check_golden("edge-geometry", &rgba, w, h);
}

#[test]
fn golden_tritone_scene() {
    let scene = scene_smooth_gradient();
    let mut params = base_params();
    params.color_mode = ColorMode::TriTone;
    params.tritone = TriTonePalette::TOKYO_MIDNIGHT;
    let (rgba, w, h) = render(&Render {
        scene: &scene,
        params,
        cell_h: 12,
    });
    check_golden("tritone-tokyo", &rgba, w, h);
}

#[test]
fn golden_transparent_scene() {
    // Transparency is a promise of the app, so it gets its own reference frame.
    let scene = scene_hard_edges();
    let mut params = base_params();
    params.transparent_bg = true;
    let (rgba, w, h) = render(&Render {
        scene: &scene,
        params,
        cell_h: 12,
    });
    // Glyph antialiasing tops out just under 255 at small cell sizes, so check
    // for "effectively opaque" rather than exactly 255.
    let max_alpha = rgba.chunks_exact(4).map(|px| px[3]).max().unwrap_or(0);
    assert!(
        max_alpha >= 250,
        "ink is translucent everywhere (max alpha {max_alpha})"
    );
    assert!(
        rgba.chunks_exact(4).filter(|px| px[3] > 200).count() > 0,
        "transparent export produced nothing at all"
    );
    assert!(
        rgba.chunks_exact(4).any(|px| px[3] == 0),
        "transparent export has no clear pixels"
    );
    check_golden("transparent", &rgba, w, h);
}

#[test]
fn golden_dense_charset_scene() {
    let scene = scene_checker();
    let mut params = base_params();
    params.charset = CharsetPreset::TypographicDense;
    params.dither = 0.08;
    let (rgba, w, h) = render(&Render {
        scene: &scene,
        params,
        cell_h: 12,
    });
    check_golden("dense-charset", &rgba, w, h);
}

#[test]
fn golden_wireframe_scene() {
    let scene = scene_hard_edges();
    let mut params = base_params();
    params.wireframe_only = true;
    params.wireframe_sensitivity = 1.15;
    let (rgba, w, h) = render(&Render {
        scene: &scene,
        params,
        cell_h: 12,
    });
    check_golden("wireframe", &rgba, w, h);
}

// ---------------------------------------------------------------------------
// Determinism, which is what makes the references meaningful at all
// ---------------------------------------------------------------------------

#[test]
fn rendering_the_same_scene_twice_is_bit_identical() {
    let scene = scene_hard_edges();
    let params = base_params();
    let (a, aw, ah) = render(&Render {
        scene: &scene,
        params: params.clone(),
        cell_h: 12,
    });
    let (b, bw, bh) = render(&Render {
        scene: &scene,
        params,
        cell_h: 12,
    });
    assert_eq!((aw, ah), (bw, bh));
    assert_eq!(a, b, "the pipeline is not deterministic within one run");
}

#[test]
fn rendering_does_not_mutate_its_input() {
    let scene = scene_smooth_gradient();
    let snapshot = scene.rgb.clone();
    let params = base_params();
    let _ = render(&Render {
        scene: &scene,
        params,
        cell_h: 12,
    });
    assert_eq!(
        scene.rgb, snapshot,
        "the engine wrote into the caller's source buffer"
    );
}

#[test]
fn every_golden_reference_exists_after_the_suite_runs() {
    // Guards against a renamed scene silently dropping its reference image.
    let dir = golden_dir();
    for name in [
        "truecolor-standard",
        "oled-black",
        "edge-geometry",
        "tritone-tokyo",
        "transparent",
        "dense-charset",
        "wireframe",
    ] {
        assert!(
            dir.join(format!("{name}.png")).exists() || blessing(),
            "missing golden reference for {name}"
        );
    }
}
