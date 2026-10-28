use crate::charset::{ASCII_COUNT, ASCII_MIN};
use fontdue::{Font, FontSettings};

/// Embedded SIL Open Font License monospace font so LumaGlyph runs out of the box
/// as a single standalone executable across Linux, macOS, and Windows.
pub const EMBEDDED_JETBRAINS_MONO: &[u8] =
    include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf");

/// 32-byte aligned 8D shape + structure-tensor orientation descriptor per ASCII glyph.
/// - `v[0..6]`: Normalized spatial ink coverage across a 2x3 staggered cell grid
///   (top-left, top-right, mid-left, mid-right, bot-left, bot-right).
/// - `v[6..8]`: Structure-tensor orientation `(Jxx - Jyy, 2 * Jxy)` capturing vertical (`|`),
///   horizontal (`_`, `-`), and diagonal (`/`, `\`) strokes without sign cancellation.
#[repr(C, align(32))]
#[derive(Debug, Clone, Copy, Default)]
pub struct GlyphVector {
    pub v: [f32; 8],
}

/// Pre-calibrated font measurement + crisp monochrome glyph bitmap atlas.
#[derive(Clone)]
pub struct GlyphAtlas {
    pub font_name: String,
    pub cell_w: usize,
    pub cell_h: usize,
    /// Aspect ratio `cell_w / cell_h` (~0.60 for standard coding monospace fonts).
    pub aspect: f32,
    /// 8D shape+orientation vectors for all 95 printable ASCII characters (`0x20..=0x7E`).
    pub vectors: [GlyphVector; ASCII_COUNT],
    /// Normalized overall ink density `[0.0..=1.0]` per glyph for luminance color compensation.
    pub coverage: [f32; ASCII_COUNT],
    /// Contiguous alpha bitmap `[ASCII_COUNT * cell_w * cell_h]` for blazing-fast CPU/GPU blitting.
    pub bitmaps: Vec<u8>,
    raw_font_bytes: Vec<u8>,
}

impl GlyphAtlas {
    pub fn default_embedded(cell_h: usize) -> Self {
        Self::from_font_bytes(
            "JetBrains Mono (Embedded)".to_string(),
            EMBEDDED_JETBRAINS_MONO.to_vec(),
            cell_h,
        )
        .expect("embedded JetBrainsMono-Regular.ttf must parse cleanly")
    }

    pub fn from_font_bytes(
        font_name: String,
        raw_font_bytes: Vec<u8>,
        cell_h: usize,
    ) -> Result<Self, String> {
        let cell_h = cell_h.clamp(8, 64);
        let font = Font::from_bytes(raw_font_bytes.as_slice(), FontSettings::default())
            .map_err(|e| format!("Failed to load font: {e}"))?;

        // Calibrate cell width from the font's horizontal advance at `px_size`.
        let px_size = (cell_h as f32) * 0.82;
        let advance = font.metrics('M', px_size).advance_width;
        let cell_w = (advance.round() as usize).clamp(5, 48);
        let aspect = (cell_w as f32) / (cell_h as f32);

        // 1. Measure supersampled 24x40 glyph bitmaps to extract accurate 8D shape vectors.
        let meas_h: usize = 40;
        let meas_px = (meas_h as f32) * 0.82;
        let meas_w = ((font.metrics('M', meas_px).advance_width).round() as usize).clamp(12, 40);
        let baseline_meas = (meas_h as isize) - ((meas_h as f32 * 0.22).round() as isize);

        let mut raw_sub = [[0.0f32; 6]; ASCII_COUNT];
        let mut raw_orient = [[0.0f32; 2]; ASCII_COUNT];
        let mut raw_cov = [0.0f32; ASCII_COUNT];

        // Staggered 2x3 spatial windows (in normalized [0..1] cell coordinates)
        let regions: [(f32, f32, f32, f32); 6] = [
            (0.00, 0.56, 0.00, 0.38), // Top-Left
            (0.44, 1.00, 0.00, 0.38), // Top-Right
            (0.00, 0.56, 0.31, 0.69), // Mid-Left
            (0.44, 1.00, 0.31, 0.69), // Mid-Right
            (0.00, 0.56, 0.62, 1.00), // Bot-Left
            (0.44, 1.00, 0.62, 1.00), // Bot-Right
        ];

        let mut canvas = vec![0.0f32; meas_w * meas_h];

        for idx in 0..ASCII_COUNT {
            canvas.fill(0.0);
            let ch = (ASCII_MIN + idx as u8) as char;
            let (metrics, bitmap) = font.rasterize(ch, meas_px);

            let x0 = metrics.xmin as isize;
            let y0 = baseline_meas - (metrics.height as isize) - (metrics.ymin as isize);

            for gy in 0..metrics.height {
                let cy = y0 + gy as isize;
                if cy < 0 || cy >= meas_h as isize {
                    continue;
                }
                for gx in 0..metrics.width {
                    let cx = x0 + gx as isize;
                    if cx < 0 || cx >= meas_w as isize {
                        continue;
                    }
                    canvas[cy as usize * meas_w + cx as usize] =
                        bitmap[gy * metrics.width + gx] as f32 / 255.0;
                }
            }

            // Total ink coverage
            let sum_ink: f32 = canvas.iter().sum();
            raw_cov[idx] = sum_ink / ((meas_w * meas_h) as f32);

            // Sample the 6 staggered spatial regions with soft tent weights
            for (r_i, &(x_min, x_max, y_min, y_max)) in regions.iter().enumerate() {
                let cx_mid = 0.5 * (x_min + x_max);
                let cy_mid = 0.5 * (y_min + y_max);
                let rx = 0.5 * (x_max - x_min);
                let ry = 0.5 * (y_max - y_min);

                let mut weighted_sum = 0.0f32;
                let mut weight_total = 0.0f32;

                for py in 0..meas_h {
                    let ny = (py as f32 + 0.5) / (meas_h as f32);
                    let wy = (1.0 - ((ny - cy_mid).abs() / ry)).max(0.0);
                    if wy <= 0.0 {
                        continue;
                    }
                    for px in 0..meas_w {
                        let nx = (px as f32 + 0.5) / (meas_w as f32);
                        let wx = (1.0 - ((nx - cx_mid).abs() / rx)).max(0.0);
                        if wx <= 0.0 {
                            continue;
                        }
                        let w = wx * wy;
                        weighted_sum += canvas[py * meas_w + px] * w;
                        weight_total += w;
                    }
                }
                if weight_total > 0.0 {
                    raw_sub[idx][r_i] = weighted_sum / weight_total;
                }
            }

            // Compute structure tensor (Jxx - Jyy, 2*Jxy) over the supersampled glyph
            let mut jxx = 0.0f32;
            let mut jyy = 0.0f32;
            let mut jxy = 0.0f32;
            for py in 1..(meas_h - 1) {
                for px in 1..(meas_w - 1) {
                    let tl = canvas[(py - 1) * meas_w + (px - 1)];
                    let tc = canvas[(py - 1) * meas_w + px];
                    let tr = canvas[(py - 1) * meas_w + (px + 1)];
                    let ml = canvas[py * meas_w + (px - 1)];
                    let mr = canvas[py * meas_w + (px + 1)];
                    let bl = canvas[(py + 1) * meas_w + (px - 1)];
                    let bc = canvas[(py + 1) * meas_w + px];
                    let br = canvas[(py + 1) * meas_w + (px + 1)];

                    let gx = (tr + 2.0 * mr + br) - (tl + 2.0 * ml + bl);
                    let gy = (bl + 2.0 * bc + br) - (tl + 2.0 * tc + tr);
                    jxx += gx * gx;
                    jyy += gy * gy;
                    jxy += gx * gy;
                }
            }
            let trace = jxx + jyy;
            if trace > 1e-4 {
                // Anisotropy strength weighted by ink presence so tiny dots don't masquerade as full lines
                let coherence = (((jxx - jyy).powi(2) + 4.0 * jxy * jxy).sqrt() / trace)
                    * (raw_cov[idx] * 4.5).min(1.0);
                raw_orient[idx][0] = ((jxx - jyy) / trace) * coherence;
                raw_orient[idx][1] = ((2.0 * jxy) / trace) * coherence;
            }
        }

        // Normalize the 6 spatial dimensions using the 92nd-percentile ink density
        // (rather than single outlier `@`) with a gentle tonal toe/shoulder so dense
        // glyphs (`#`, `$`, `%`, `&`, `W`, `M`, `B`, `8`, `Q`, `@`, `m`, `w`, `g`, `q`)
        // spread evenly across upper midtones & highlights instead of `@`/`Q` hogging 0.70..1.0.
        let mut sorted_subs: Vec<f32> = raw_sub
            .iter()
            .skip(1) // skip space
            .flat_map(|s| s.iter().copied())
            .filter(|&v| v > 0.01)
            .collect();
        sorted_subs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let p92_idx =
            ((sorted_subs.len() as f32 * 0.92) as usize).min(sorted_subs.len().saturating_sub(1));
        let ref_sub = sorted_subs.get(p92_idx).copied().unwrap_or(0.5).max(1e-4);
        let max_cov = raw_cov.iter().copied().fold(1e-4f32, f32::max);

        let mut vectors = [GlyphVector::default(); ASCII_COUNT];
        let mut coverage = [0.0f32; ASCII_COUNT];
        for idx in 0..ASCII_COUNT {
            for (dst, &src) in vectors[idx].v[..6].iter_mut().zip(raw_sub[idx].iter()) {
                let norm = (src / (ref_sub * 1.06)).clamp(0.0, 1.0);
                *dst = norm.powf(0.86);
            }
            vectors[idx].v[6] = raw_orient[idx][0];
            vectors[idx].v[7] = raw_orient[idx][1];
            coverage[idx] = (raw_cov[idx] / max_cov).clamp(0.0, 1.0);
        }

        // 2. Rasterize crisp target-resolution atlas bitmaps (`cell_w x cell_h`)
        let mut bitmaps = vec![0u8; ASCII_COUNT * cell_w * cell_h];
        let baseline = (cell_h as isize) - ((cell_h as f32 * 0.22).round() as isize);

        for idx in 0..ASCII_COUNT {
            let ch = (ASCII_MIN + idx as u8) as char;
            let (metrics, glyph_bmp) = font.rasterize(ch, px_size);
            // Center horizontally if slightly narrower/wider than advance
            let x_pad = ((cell_w as isize - metrics.width as isize) / 2).max(0);
            let x0 = metrics.xmin.max(0) as isize;
            let start_x = if metrics.xmin > 0 {
                (metrics.xmin as isize).min(x_pad + 1)
            } else {
                x0
            };
            let start_y = baseline - (metrics.height as isize) - (metrics.ymin as isize);

            let base = idx * cell_w * cell_h;
            for gy in 0..metrics.height {
                let cy = start_y + gy as isize;
                if cy < 0 || cy >= cell_h as isize {
                    continue;
                }
                for gx in 0..metrics.width {
                    let cx = start_x + gx as isize;
                    if cx < 0 || cx >= cell_w as isize {
                        continue;
                    }
                    bitmaps[base + (cy as usize) * cell_w + (cx as usize)] =
                        glyph_bmp[gy * metrics.width + gx];
                }
            }
        }

        Ok(Self {
            font_name,
            cell_w,
            cell_h,
            aspect,
            vectors,
            coverage,
            bitmaps,
            raw_font_bytes,
        })
    }

    /// Rebuild this same font at a different pixel height (e.g. 2x/3x high-DPI PNG export).
    pub fn with_cell_height(&self, new_cell_h: usize) -> Self {
        if new_cell_h == self.cell_h {
            return self.clone();
        }
        Self::from_font_bytes(
            self.font_name.clone(),
            self.raw_font_bytes.clone(),
            new_cell_h,
        )
        .unwrap_or_else(|_| Self::default_embedded(new_cell_h))
    }

    #[inline(always)]
    pub fn glyph_slice(&self, glyph_idx: usize) -> &[u8] {
        let stride = self.cell_w * self.cell_h;
        let start = glyph_idx * stride;
        &self.bitmaps[start..start + stride]
    }
}
