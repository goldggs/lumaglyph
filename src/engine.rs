use crate::charset::{CharsetPreset, ASCII_COUNT, ASCII_MIN};
use crate::color::{luma_rgb, resolve_cell_colors, ColorMode, BAYER_4X4, DEFAULT_BG};
use crate::glyph_atlas::{GlyphAtlas, GlyphVector};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Compact 8-byte cell representation (`#[repr(C)]`) so an entire 200x100 frame
/// occupies just 160 KB (fitting comfortably inside L2 CPU cache).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AsciiCell {
    pub ch: u8,
    pub flags: u8,
    pub fg: [u8; 3],
    pub bg: [u8; 3],
}

impl Default for AsciiCell {
    fn default() -> Self {
        Self {
            ch: b' ',
            flags: 0,
            fg: [220, 224, 232],
            bg: DEFAULT_BG,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderParams {
    pub cols: u32,
    pub charset: CharsetPreset,
    pub custom_chars: String,
    pub color_mode: ColorMode,
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub gamma: f32,
    /// Local external-neighbor contrast stretching (`0.0..=3.0`) that snaps curves & silhouettes.
    pub directional_crunch: f32,
    /// Structure-tensor orientation weight (`0.0..=2.0`) locking `/ \ | _ ( )` onto real edges.
    pub edge_weight: f32,
    /// Compensates foreground brightness inversely to glyph ink coverage (`0.0..=1.0`).
    pub luminance_comp: f32,
    /// Ambient cell background richness in `DualColor` mode (`0.0..=0.60`).
    pub bg_intensity: f32,
    /// Ordered 4x4 Bayer dithering strength (`0.0..=1.0`).
    pub dither: f32,
    /// Optional radial vignette luma focus to quiet noisy backgrounds (`0.0..=1.0`).
    pub vignette_focus: f32,
    /// Video anti-flicker hysteresis (`0.0..=0.50`) preventing compression noise boiling.
    pub temporal_stability: f32,
    pub crt_bloom: bool,
    pub scanlines: bool,
    pub invert: bool,
}

impl Default for RenderParams {
    fn default() -> Self {
        Self {
            cols: 156,
            charset: CharsetPreset::Genuine95,
            custom_chars: " .:-=+*#%@".to_string(),
            color_mode: ColorMode::TrueColor,
            brightness: 0.02,
            contrast: 1.18,
            saturation: 1.22,
            gamma: 0.96,
            directional_crunch: 1.40,
            edge_weight: 0.92,
            luminance_comp: 0.68,
            bg_intensity: 0.26,
            dither: 0.08,
            vignette_focus: 0.0,
            temporal_stability: 0.16,
            crt_bloom: false,
            scanlines: false,
            invert: false,
        }
    }
}

/// Reusable zero-allocation frame workspace for both image studio and streaming video.
#[derive(Clone)]
pub struct AsciiFrame {
    pub cols: u32,
    pub rows: u32,
    pub cells: Vec<AsciiCell>,
    pub prev_glyphs: Vec<u8>,
    pub histogram: [u32; ASCII_COUNT],
    pub unique_glyphs: usize,
    pub render_ms: f64,
}

impl Default for AsciiFrame {
    fn default() -> Self {
        Self::new()
    }
}

impl AsciiFrame {
    pub fn new() -> Self {
        Self {
            cols: 0,
            rows: 0,
            cells: Vec::new(),
            prev_glyphs: Vec::new(),
            histogram: [0; ASCII_COUNT],
            unique_glyphs: 0,
            render_ms: 0.0,
        }
    }

    /// Plain UTF-8 text representation (lines separated by `\n`).
    pub fn to_plain_text(&self) -> String {
        let cap = (self.cols as usize + 1) * (self.rows as usize);
        let mut out = String::with_capacity(cap);
        for r in 0..(self.rows as usize) {
            let row_slice = &self.cells[r * self.cols as usize..(r + 1) * self.cols as usize];
            for cell in row_slice {
                out.push(cell.ch as char);
            }
            if r + 1 < self.rows as usize {
                out.push('\n');
            }
        }
        out
    }
}

#[derive(Clone, Copy)]
struct ActiveGlyph {
    idx: u8,
    vec: GlyphVector,
}

/// Convert a packed RGB24 (`stride = 3`) or RGBA32 (`stride = 4`) image buffer into `AsciiFrame`.
/// Reuses `frame.cells` and `frame.prev_glyphs` in place for flat memory consumption.
#[allow(clippy::too_many_arguments)]
pub fn convert_image_buffer(
    pixels: &[u8],
    img_w: u32,
    img_h: u32,
    channels: usize,
    atlas: &GlyphAtlas,
    params: &RenderParams,
    use_temporal: bool,
    frame: &mut AsciiFrame,
) {
    assert!(channels == 3 || channels == 4, "channels must be 3 or 4");
    if img_w == 0 || img_h == 0 || pixels.len() < (img_w as usize) * (img_h as usize) * channels {
        return;
    }

    let t0 = Instant::now();
    let cols = params.cols.clamp(16, 420);
    let rows = ((cols as f32) * (img_h as f32 / img_w as f32) * atlas.aspect)
        .round()
        .max(1.0) as u32;
    let total_cells = (cols as usize) * (rows as usize);

    // If grid resolution changed, reset temporal history cleanly
    let has_valid_history = use_temporal
        && params.temporal_stability > 0.001
        && frame.cols == cols
        && frame.rows == rows
        && frame.prev_glyphs.len() == total_cells;

    frame.cols = cols;
    frame.rows = rows;
    if frame.cells.len() != total_cells {
        frame.cells.resize(total_cells, AsciiCell::default());
    }

    // Build compact active glyph table in L1 cache (<= 95 * 36 bytes = 3.4 KB)
    let mask = params.charset.active_mask(&params.custom_chars);
    let mut active_list = [ActiveGlyph {
        idx: 0,
        vec: GlyphVector::default(),
    }; ASCII_COUNT];
    let mut active_len = 0usize;
    let mut direct_vec_by_idx = [None::<GlyphVector>; ASCII_COUNT];

    let ew = params.edge_weight;
    for idx in 0..ASCII_COUNT {
        if mask[idx] {
            let mut gv = atlas.vectors[idx];
            gv.v[6] *= ew;
            gv.v[7] *= ew;
            active_list[active_len] = ActiveGlyph {
                idx: idx as u8,
                vec: gv,
            };
            direct_vec_by_idx[idx] = Some(gv);
            active_len += 1;
        }
    }
    let active = &active_list[..active_len];

    let cell_px_w = (img_w as f32) / (cols as f32);
    let cell_px_h = (img_h as f32) / (rows as f32);
    let w_max = (img_w - 1) as isize;
    let h_max = (img_h - 1) as isize;
    let img_w_usize = img_w as usize;

    // Precompute 256-entry 1D tone LUT (brightness + contrast + gamma + invert) in L1 cache
    // to eliminate ~1M `powf` calls per frame!
    let mut tone_lut = [0.0f32; 256];
    let inv_gamma = 1.0 / params.gamma.max(0.15);
    let has_gamma = (params.gamma - 1.0).abs() > 1e-3;
    for (byte, out) in tone_lut.iter_mut().enumerate() {
        let mut x = (byte as f32) * (1.0 / 255.0);
        if params.invert {
            x = 1.0 - x;
        }
        x = ((x - 0.5) * params.contrast + 0.5 + params.brightness).clamp(0.0, 1.0);
        if has_gamma {
            x = x.powf(inv_gamma);
        }
        *out = x;
    }

    // Precompute 256-entry directional crunch curve LUT
    let crunch_exp = 1.0 + params.directional_crunch.clamp(0.0, 3.5);
    let do_crunch = params.directional_crunch > 0.01;
    let mut crunch_lut = [0.0f32; 256];
    if do_crunch {
        for (i, out) in crunch_lut.iter_mut().enumerate() {
            *out = ((i as f32) * (1.0 / 255.0)).powf(crunch_exp);
        }
    }

    // Precompute per-glyph ink luminance boost table (95 floats)
    let mut boost_lut = [1.0f32; ASCII_COUNT];
    if params.luminance_comp > 0.001 {
        let exp = params.luminance_comp * 0.58;
        for (out_b, &cov) in boost_lut.iter_mut().zip(atlas.coverage.iter()) {
            if cov > 0.01 {
                *out_b = (1.0 / cov.max(0.24)).powf(exp);
            }
        }
    }

    let sat = params.saturation;

    // Fast clamped pixel fetcher returning graded `(r, g, b, luma)` with zero `powf`!
    let sample_point = |fx: f32, fy: f32| -> (f32, f32, f32, f32) {
        let step_x = (cell_px_w * 0.14).max(0.5);
        let step_y = (cell_px_h * 0.14).max(0.5);
        let offsets = [
            (-step_x, -step_y),
            (step_x, -step_y),
            (-step_x, step_y),
            (step_x, step_y),
        ];
        let mut sr = 0.0f32;
        let mut sg = 0.0f32;
        let mut sb = 0.0f32;
        for &(ox, oy) in &offsets {
            let ix = ((fx + ox) as isize).clamp(0, w_max) as usize;
            let iy = ((fy + oy) as isize).clamp(0, h_max) as usize;
            let base = (iy * img_w_usize + ix) * channels;
            let mut r = tone_lut[pixels[base] as usize];
            let mut g = tone_lut[pixels[base + 1] as usize];
            let mut b = tone_lut[pixels[base + 2] as usize];
            if channels == 4 {
                let a = pixels[base + 3] as f32 * (1.0 / 255.0);
                r *= a;
                g *= a;
                b *= a;
            }
            sr += r;
            sg += g;
            sb += b;
        }
        let r0 = sr * 0.25;
        let g0 = sg * 0.25;
        let b0 = sb * 0.25;
        let y = luma_rgb(r0, g0, b0);
        let r = (y + (r0 - y) * sat).clamp(0.0, 1.0);
        let g = (y + (g0 - y) * sat).clamp(0.0, 1.0);
        let b = (y + (b0 - y) * sat).clamp(0.0, 1.0);
        (r, g, b, luma_rgb(r, g, b))
    };

    // Sub-cell centers in normalized [0..1] cell coordinates (staggered 2x3)
    const SUB_POS: [(f32, f32); 6] = [
        (0.26, 0.18), // TL
        (0.74, 0.18), // TR
        (0.24, 0.50), // ML
        (0.76, 0.50), // MR
        (0.26, 0.82), // BL
        (0.74, 0.82), // BR
    ];
    // External neighbor offsets around the cell for directional contrast crunching
    const EXT_POS: [(f32, f32); 4] = [
        (0.50, -0.24), // Top neighbor
        (0.50, 1.24),  // Bottom neighbor
        (-0.26, 0.50), // Left neighbor
        (1.26, 0.50),  // Right neighbor
    ];

    let dither_amp = params.dither * 0.22;
    let vig_strength = params.vignette_focus.clamp(0.0, 1.0);
    let hysteresis = params.temporal_stability * 0.14;
    let prev_slice = frame.prev_glyphs.as_slice();
    let cols_usize = cols as usize;
    let rows_f = rows as f32;
    let cols_f = cols as f32;

    frame
        .cells
        .par_chunks_mut(cols_usize)
        .enumerate()
        .for_each(|(row_idx, row_cells)| {
            let y0 = (row_idx as f32) * cell_px_h;
            let ny_norm = ((row_idx as f32 + 0.5) / rows_f) * 2.0 - 1.0;
            let bayer_row = &BAYER_4X4[row_idx & 3];

            for (col_idx, out_cell) in row_cells.iter_mut().enumerate() {
                let x0 = (col_idx as f32) * cell_px_w;

                // Optional vignette focus falloff toward frame edges
                let vig_mul = if vig_strength > 0.001 {
                    let nx_norm = ((col_idx as f32 + 0.5) / cols_f) * 2.0 - 1.0;
                    let r2 = (nx_norm * nx_norm + ny_norm * ny_norm) * 0.62;
                    (1.0 - vig_strength * r2.clamp(0.0, 1.0)).clamp(0.08, 1.0)
                } else {
                    1.0
                };

                let dither_delta = if dither_amp > 0.0005 {
                    bayer_row[col_idx & 3] * dither_amp
                } else {
                    0.0
                };

                let mut sub_luma = [0.0f32; 6];
                let mut sum_r = 0.0f32;
                let mut sum_g = 0.0f32;
                let mut sum_b = 0.0f32;
                let mut weight_rgb = 0.0f32;

                for k in 0..6 {
                    let (sx, sy) = SUB_POS[k];
                    let (r, g, b, y) = sample_point(x0 + sx * cell_px_w, y0 + sy * cell_px_h);
                    let yd = (y * vig_mul + dither_delta).clamp(0.0, 1.0);
                    sub_luma[k] = yd;
                    // Weight foreground color toward illuminated parts of the cell so dark edges don't muddy hues
                    let cw = 0.15 + yd;
                    sum_r += r * vig_mul * cw;
                    sum_g += g * vig_mul * cw;
                    sum_b += b * vig_mul * cw;
                    weight_rgb += cw;
                }

                // Compute internal structure-tensor orientation BEFORE contrast crunching
                let gx = ((sub_luma[1] - sub_luma[0])
                    + 1.5 * (sub_luma[3] - sub_luma[2])
                    + (sub_luma[5] - sub_luma[4]))
                    * 0.45;
                let gy = ((sub_luma[4] + sub_luma[5]) - (sub_luma[0] + sub_luma[1])) * 0.65;
                let jxx = gx * gx;
                let jyy = gy * gy;
                let jxy = gx * gy;
                let trace = jxx + jyy;
                let (o0, o1) = if trace > 1e-5 && ew > 0.001 {
                    let edge_mag = (trace.sqrt() * 2.35).min(1.0);
                    (
                        ((jxx - jyy) / trace) * edge_mag * ew,
                        ((2.0 * jxy) / trace) * edge_mag * ew,
                    )
                } else {
                    (0.0, 0.0)
                };

                // Directional contrast crunch using 4 external neighbor samples
                if do_crunch {
                    let mut local_max = sub_luma.iter().copied().fold(0.0f32, f32::max);
                    for &(ex, ey) in &EXT_POS {
                        let (_, _, _, y_ext) =
                            sample_point(x0 + ex * cell_px_w, y0 + ey * cell_px_h);
                        local_max = local_max.max((y_ext * vig_mul).clamp(0.0, 1.0));
                    }
                    if local_max > 0.02 {
                        let scale255 = 255.0 / local_max;
                        for v in &mut sub_luma {
                            let idx = ((*v * scale255).clamp(0.0, 255.0) + 0.5) as usize;
                            *v = crunch_lut[idx] * local_max;
                        }
                    }
                }

                let target = [
                    sub_luma[0],
                    sub_luma[1],
                    sub_luma[2],
                    sub_luma[3],
                    sub_luma[4],
                    sub_luma[5],
                    o0,
                    o1,
                ];

                // SIMD-friendly 8-float squared Euclidean distance scan across active glyphs
                let mut best_idx = active[0].idx;
                let mut best_dist = f32::MAX;

                for ag in active {
                    let gv = &ag.vec.v;
                    let d0 = target[0] - gv[0];
                    let d1 = target[1] - gv[1];
                    let d2 = target[2] - gv[2];
                    let d3 = target[3] - gv[3];
                    let d4 = target[4] - gv[4];
                    let d5 = target[5] - gv[5];
                    let d6 = target[6] - gv[6];
                    let d7 = target[7] - gv[7];
                    let dist = (d0 * d0 + d1 * d1)
                        + (d2 * d2 + d3 * d3)
                        + (d4 * d4 + d5 * d5)
                        + (d6 * d6 + d7 * d7);
                    if dist < best_dist {
                        best_dist = dist;
                        best_idx = ag.idx;
                    }
                }

                // Video temporal hysteresis: keep previous frame's glyph unless the new match wins decisively
                if has_valid_history {
                    let flat_i = row_idx * cols_usize + col_idx;
                    let prev_g = prev_slice[flat_i] as usize;
                    if prev_g < ASCII_COUNT && prev_g != best_idx as usize {
                        if let Some(pgv) = &direct_vec_by_idx[prev_g] {
                            let gv = &pgv.v;
                            let d0 = target[0] - gv[0];
                            let d1 = target[1] - gv[1];
                            let d2 = target[2] - gv[2];
                            let d3 = target[3] - gv[3];
                            let d4 = target[4] - gv[4];
                            let d5 = target[5] - gv[5];
                            let d6 = target[6] - gv[6];
                            let d7 = target[7] - gv[7];
                            let prev_dist = (d0 * d0 + d1 * d1)
                                + (d2 * d2 + d3 * d3)
                                + (d4 * d4 + d5 * d5)
                                + (d6 * d6 + d7 * d7);
                            if prev_dist - hysteresis <= best_dist {
                                best_idx = prev_g as u8;
                            }
                        }
                    }
                }

                let inv_w = 1.0 / weight_rgb.max(1e-4);
                let cell_r = sum_r * inv_w;
                let cell_g = sum_g * inv_w;
                let cell_b = sum_b * inv_w;

                let boost = boost_lut[best_idx as usize];
                let (fg, bg) = resolve_cell_colors(
                    params.color_mode,
                    cell_r,
                    cell_g,
                    cell_b,
                    boost,
                    params.bg_intensity,
                );

                *out_cell = AsciiCell {
                    ch: ASCII_MIN + best_idx,
                    flags: 0,
                    fg,
                    bg,
                };
            }
        });

    // Update temporal history & glyph diversity statistics in one linear pass
    if frame.prev_glyphs.len() != total_cells {
        frame.prev_glyphs.resize(total_cells, 0);
    }
    frame.histogram.fill(0);
    for (i, cell) in frame.cells.iter().enumerate() {
        let g_idx = (cell.ch - ASCII_MIN) as usize;
        frame.prev_glyphs[i] = g_idx as u8;
        frame.histogram[g_idx] += 1;
    }
    frame.unique_glyphs = frame.histogram.iter().filter(|&&c| c > 0).count();
    frame.render_ms = t0.elapsed().as_secs_f64() * 1000.0;
}

/// Rasterize `AsciiFrame` into a contiguous RGBA32 pixel buffer (`out_w * out_h * 4` bytes)
/// using parallel row blitting from `GlyphAtlas`. Zero allocations once `out_rgba` is sized.
pub fn rasterize_rgba(
    frame: &AsciiFrame,
    atlas: &GlyphAtlas,
    crt_bloom: bool,
    scanlines: bool,
    out_rgba: &mut Vec<u8>,
) -> (usize, usize) {
    let out_w = (frame.cols as usize) * atlas.cell_w;
    let out_h = (frame.rows as usize) * atlas.cell_h;
    let needed = out_w * out_h * 4;
    if out_rgba.len() != needed {
        out_rgba.resize(needed, 255);
    }
    if out_w == 0 || out_h == 0 {
        return (0, 0);
    }

    let cell_w = atlas.cell_w;
    let cell_h = atlas.cell_h;
    let cols = frame.cols as usize;
    let row_pixel_bytes = out_w * 4;

    // Parallelize by character row (`cell_h` pixel scanlines per task)
    out_rgba
        .par_chunks_mut(row_pixel_bytes * cell_h)
        .enumerate()
        .for_each(|(cell_row, band_bytes)| {
            let cells_row = &frame.cells[cell_row * cols..(cell_row + 1) * cols];
            for local_y in 0..cell_h {
                let abs_y = cell_row * cell_h + local_y;
                let scan_mul: u16 = if scanlines && (abs_y & 1 == 1) {
                    208 // subtle ~18% CRT scanline attenuation
                } else {
                    256
                };
                let dst_line =
                    &mut band_bytes[local_y * row_pixel_bytes..(local_y + 1) * row_pixel_bytes];

                for (cell_col, cell) in cells_row.iter().enumerate() {
                    let g_idx = (cell.ch.saturating_sub(ASCII_MIN) as usize).min(ASCII_COUNT - 1);
                    let g_bmp = atlas.glyph_slice(g_idx);
                    let src_line = &g_bmp[local_y * cell_w..(local_y + 1) * cell_w];
                    let fg = cell.fg;
                    let bg = cell.bg;
                    let px_base = cell_col * cell_w * 4;

                    for gx in 0..cell_w {
                        let a = src_line[gx] as u16;
                        let inv_a = 255 - a;
                        let mut r = (fg[0] as u16 * a + bg[0] as u16 * inv_a) / 255;
                        let mut g = (fg[1] as u16 * a + bg[1] as u16 * inv_a) / 255;
                        let mut b = (fg[2] as u16 * a + bg[2] as u16 * inv_a) / 255;

                        if crt_bloom && a < 210 {
                            // Sample immediate neighbors inside glyph cell for soft phosphor halo
                            let left_a = if gx > 0 { src_line[gx - 1] as u16 } else { 0 };
                            let right_a = if gx + 1 < cell_w {
                                src_line[gx + 1] as u16
                            } else {
                                0
                            };
                            let up_a = if local_y > 0 {
                                g_bmp[(local_y - 1) * cell_w + gx] as u16
                            } else {
                                0
                            };
                            let dn_a = if local_y + 1 < cell_h {
                                g_bmp[(local_y + 1) * cell_w + gx] as u16
                            } else {
                                0
                            };
                            let halo = (left_a + right_a + up_a + dn_a) >> 4; // / 16
                            if halo > 0 {
                                r = (r + ((fg[0] as u16 * halo) >> 8)).min(255);
                                g = (g + ((fg[1] as u16 * halo) >> 8)).min(255);
                                b = (b + ((fg[2] as u16 * halo) >> 8)).min(255);
                            }
                        }

                        let o = px_base + gx * 4;
                        dst_line[o] = ((r * scan_mul) >> 8) as u8;
                        dst_line[o + 1] = ((g * scan_mul) >> 8) as u8;
                        dst_line[o + 2] = ((b * scan_mul) >> 8) as u8;
                        dst_line[o + 3] = 255;
                    }
                }
            }
        });

    (out_w, out_h)
}

/// Rasterize `AsciiFrame` into an even-dimensioned packed RGB24 buffer (`out_w * out_h * 3`)
/// ready to pipe straight into `ffmpeg -f rawvideo -pix_fmt rgb24` (`yuv420p` safe).
pub fn rasterize_rgb24_even(
    frame: &AsciiFrame,
    atlas: &GlyphAtlas,
    crt_bloom: bool,
    scanlines: bool,
    out_rgb: &mut Vec<u8>,
) -> (usize, usize) {
    let raw_w = (frame.cols as usize) * atlas.cell_w;
    let raw_h = (frame.rows as usize) * atlas.cell_h;
    // Pad up by 1px if odd so H.264 yuv420p encoders accept any column/font geometry
    let out_w = (raw_w + 1) & !1;
    let out_h = (raw_h + 1) & !1;
    let needed = out_w * out_h * 3;
    if out_rgb.len() != needed {
        out_rgb.resize(needed, DEFAULT_BG[0]);
    }
    if raw_w == 0 || raw_h == 0 {
        return (0, 0);
    }

    let cell_w = atlas.cell_w;
    let cell_h = atlas.cell_h;
    let cols = frame.cols as usize;
    let row_pixel_bytes = out_w * 3;

    out_rgb[..raw_h * row_pixel_bytes]
        .par_chunks_mut(row_pixel_bytes * cell_h)
        .enumerate()
        .for_each(|(cell_row, band_bytes)| {
            let cells_row = &frame.cells[cell_row * cols..(cell_row + 1) * cols];
            for local_y in 0..cell_h {
                let abs_y = cell_row * cell_h + local_y;
                let scan_mul: u16 = if scanlines && (abs_y & 1 == 1) {
                    208
                } else {
                    256
                };
                let dst_line =
                    &mut band_bytes[local_y * row_pixel_bytes..(local_y + 1) * row_pixel_bytes];

                for (cell_col, cell) in cells_row.iter().enumerate() {
                    let g_idx = (cell.ch.saturating_sub(ASCII_MIN) as usize).min(ASCII_COUNT - 1);
                    let g_bmp = atlas.glyph_slice(g_idx);
                    let src_line = &g_bmp[local_y * cell_w..(local_y + 1) * cell_w];
                    let fg = cell.fg;
                    let bg = cell.bg;
                    let px_base = cell_col * cell_w * 3;

                    for gx in 0..cell_w {
                        let a = src_line[gx] as u16;
                        let inv_a = 255 - a;
                        let mut r = (fg[0] as u16 * a + bg[0] as u16 * inv_a) / 255;
                        let mut g = (fg[1] as u16 * a + bg[1] as u16 * inv_a) / 255;
                        let mut b = (fg[2] as u16 * a + bg[2] as u16 * inv_a) / 255;

                        if crt_bloom && a < 210 {
                            let left_a = if gx > 0 { src_line[gx - 1] as u16 } else { 0 };
                            let right_a = if gx + 1 < cell_w {
                                src_line[gx + 1] as u16
                            } else {
                                0
                            };
                            let halo = (left_a + right_a) >> 3;
                            if halo > 0 {
                                r = (r + ((fg[0] as u16 * halo) >> 8)).min(255);
                                g = (g + ((fg[1] as u16 * halo) >> 8)).min(255);
                                b = (b + ((fg[2] as u16 * halo) >> 8)).min(255);
                            }
                        }

                        let o = px_base + gx * 3;
                        dst_line[o] = ((r * scan_mul) >> 8) as u8;
                        dst_line[o + 1] = ((g * scan_mul) >> 8) as u8;
                        dst_line[o + 2] = ((b * scan_mul) >> 8) as u8;
                    }
                }
                if out_w > raw_w {
                    let o = raw_w * 3;
                    dst_line[o] = DEFAULT_BG[0];
                    dst_line[o + 1] = DEFAULT_BG[1];
                    dst_line[o + 2] = DEFAULT_BG[2];
                }
            }
        });

    if out_h > raw_h {
        let last_start = raw_h * row_pixel_bytes;
        for px in out_rgb[last_start..].chunks_exact_mut(3) {
            px.copy_from_slice(&DEFAULT_BG);
        }
    }

    (out_w, out_h)
}
