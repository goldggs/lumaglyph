use serde::{Deserialize, Serialize};

/// Default deep obsidian canvas background for pure foreground modes.
pub const DEFAULT_BG: [u8; 3] = [9, 11, 16];

/// 4x4 normalized Bayer ordered dithering matrix in `[-0.5, +0.5]`.
pub const BAYER_4X4: [[f32; 4]; 4] = [
    [-0.46875, 0.03125, -0.34375, 0.15625],
    [0.28125, -0.21875, 0.40625, -0.09375],
    [-0.28125, 0.21875, -0.40625, 0.09375],
    [0.46875, -0.03125, 0.34375, -0.15625],
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorMode {
    /// 24-bit TrueColor with ink-coverage luminance compensation so sparse glyphs stay vibrant.
    TrueColor,
    /// Demoscene/BBS Dual-Color: crisp foreground glyph + shadowed ambient cell background.
    DualColor,
    /// Warm vintage P3 Amber CRT phosphor curve.
    PhosphorAmber,
    /// Classic P1 Green terminal phosphor curve.
    PhosphorGreen,
    /// Electric cyan/magenta/violet neon synth grading.
    Cyberpunk,
    /// Authentic 16-color ANSI terminal palette.
    Ansi16,
    /// 256-color xterm (6x6x6 cube + 24 grayscale).
    Xterm256,
    /// High-contrast typographic silver-ivory monochrome.
    Monochrome,
}

impl ColorMode {
    pub const ALL: [ColorMode; 8] = [
        ColorMode::TrueColor,
        ColorMode::DualColor,
        ColorMode::PhosphorAmber,
        ColorMode::PhosphorGreen,
        ColorMode::Cyberpunk,
        ColorMode::Ansi16,
        ColorMode::Xterm256,
        ColorMode::Monochrome,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ColorMode::TrueColor => "TrueColor 24-bit",
            ColorMode::DualColor => "Dual-Color (FG + Ambient BG)",
            ColorMode::PhosphorAmber => "CRT Amber Phosphor",
            ColorMode::PhosphorGreen => "CRT Green Phosphor",
            ColorMode::Cyberpunk => "Cyberpunk Neon",
            ColorMode::Ansi16 => "ANSI-16 Terminal",
            ColorMode::Xterm256 => "xterm-256 Palette",
            ColorMode::Monochrome => "Monochrome Ivory",
        }
    }

    pub fn cli_name(self) -> &'static str {
        match self {
            ColorMode::TrueColor => "truecolor",
            ColorMode::DualColor => "dual",
            ColorMode::PhosphorAmber => "amber",
            ColorMode::PhosphorGreen => "green",
            ColorMode::Cyberpunk => "cyberpunk",
            ColorMode::Ansi16 => "ansi16",
            ColorMode::Xterm256 => "xterm256",
            ColorMode::Monochrome => "mono",
        }
    }

    pub fn from_cli(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "truecolor" | "24bit" | "rgb" => Some(Self::TrueColor),
            "dual" | "dualcolor" | "bbs" => Some(Self::DualColor),
            "amber" | "crt-amber" => Some(Self::PhosphorAmber),
            "green" | "crt-green" | "matrix" => Some(Self::PhosphorGreen),
            "cyberpunk" | "neon" => Some(Self::Cyberpunk),
            "ansi16" | "16" => Some(Self::Ansi16),
            "xterm256" | "256" => Some(Self::Xterm256),
            "mono" | "monochrome" | "bw" => Some(Self::Monochrome),
            _ => None,
        }
    }
}

const ANSI_16_PALETTE: [[u8; 3]; 16] = [
    [12, 14, 20],    // Black
    [205, 49, 49],   // Red
    [13, 188, 121],  // Green
    [229, 229, 16],  // Yellow
    [36, 114, 200],  // Blue
    [188, 63, 188],  // Magenta
    [17, 168, 205],  // Cyan
    [229, 229, 229], // White
    [102, 102, 115], // Bright Black
    [241, 76, 76],   // Bright Red
    [35, 209, 139],  // Bright Green
    [245, 245, 67],  // Bright Yellow
    [59, 142, 234],  // Bright Blue
    [214, 112, 214], // Bright Magenta
    [41, 184, 219],  // Bright Cyan
    [255, 255, 255], // Bright White
];

/// Perceptual Rec.709 luminance in `[0.0, 1.0]`.
#[inline(always)]
pub fn luma_rgb(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Apply grading (brightness, contrast, gamma, saturation) in normalized `[0..1]` RGB space.
#[allow(clippy::too_many_arguments)]
#[inline(always)]
pub fn grade_rgb(
    mut r: f32,
    mut g: f32,
    mut b: f32,
    brightness: f32,
    contrast: f32,
    saturation: f32,
    gamma: f32,
    invert: bool,
) -> (f32, f32, f32) {
    if invert {
        r = 1.0 - r;
        g = 1.0 - g;
        b = 1.0 - b;
    }

    r = ((r - 0.5) * contrast + 0.5 + brightness).clamp(0.0, 1.0);
    g = ((g - 0.5) * contrast + 0.5 + brightness).clamp(0.0, 1.0);
    b = ((b - 0.5) * contrast + 0.5 + brightness).clamp(0.0, 1.0);

    if (gamma - 1.0).abs() > 1e-3 {
        let inv_g = 1.0 / gamma.max(0.15);
        r = r.powf(inv_g);
        g = g.powf(inv_g);
        b = b.powf(inv_g);
    }

    let y = luma_rgb(r, g, b);
    r = (y + (r - y) * saturation).clamp(0.0, 1.0);
    g = (y + (g - y) * saturation).clamp(0.0, 1.0);
    b = (y + (b - y) * saturation).clamp(0.0, 1.0);
    (r, g, b)
}

/// Compute `(fg, bg)` colors for a matched cell, applying ink-coverage luminance compensation
/// so thin structural glyphs (`/`, `\`, `;`, `(`) retain rich color saturation and brightness.
#[inline]
pub fn resolve_cell_colors(
    mode: ColorMode,
    r: f32,
    g: f32,
    b: f32,
    boost: f32,
    bg_intensity: f32,
) -> ([u8; 3], [u8; 3]) {
    let apply_boost = |cr: f32, cg: f32, cb: f32, factor: f32| -> [u8; 3] {
        let mr = cr * factor;
        let mg = cg * factor;
        let mb = cb * factor;
        let peak = mr.max(mg).max(mb);
        // Soft shoulder roll-off above 1.0 so bright colors don't desaturate into flat white
        let scale = if peak > 1.0 {
            (1.0 + 0.25 * (peak - 1.0)).min(1.12) / peak
        } else {
            1.0
        };
        [
            ((mr * scale).clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
            ((mg * scale).clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
            ((mb * scale).clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
        ]
    };

    match mode {
        ColorMode::TrueColor => (apply_boost(r, g, b, boost), DEFAULT_BG),
        ColorMode::DualColor => {
            let fg = apply_boost(r, g, b, boost * 1.06);
            // Quantize & darken ambient cell background for crisp contrast against foreground ink
            let bg_scale = bg_intensity.clamp(0.0, 0.65);
            let bg = [
                ((DEFAULT_BG[0] as f32) * (1.0 - bg_scale) + (r * 255.0) * bg_scale) as u8,
                ((DEFAULT_BG[1] as f32) * (1.0 - bg_scale) + (g * 255.0) * bg_scale) as u8,
                ((DEFAULT_BG[2] as f32) * (1.0 - bg_scale) + (b * 255.0) * bg_scale) as u8,
            ];
            (fg, bg)
        }
        ColorMode::PhosphorAmber => {
            let y = (luma_rgb(r, g, b) * boost).clamp(0.0, 1.15);
            let fg = [
                (y.min(1.0) * 255.0) as u8,
                ((y.powf(1.22) * 184.0 + (y - 0.75).max(0.0) * 180.0).clamp(0.0, 245.0)) as u8,
                ((y.powf(2.4) * 72.0).clamp(0.0, 95.0)) as u8,
            ];
            (fg, [14, 9, 4])
        }
        ColorMode::PhosphorGreen => {
            let y = (luma_rgb(r, g, b) * boost).clamp(0.0, 1.15);
            let fg = [
                ((y.powf(2.0) * 95.0).clamp(0.0, 120.0)) as u8,
                (y.min(1.0) * 255.0) as u8,
                ((y.powf(1.35) * 125.0).clamp(0.0, 155.0)) as u8,
            ];
            (fg, [5, 14, 8])
        }
        ColorMode::Cyberpunk => {
            let y = luma_rgb(r, g, b);
            let warmth = (r - b).clamp(-1.0, 1.0) * 0.5 + 0.5;
            // Blend electric cyan (0.10, 0.95, 1.0) <-> neon magenta-coral (1.0, 0.22, 0.78)
            let cr = (0.12 + 0.88 * warmth) * (0.28 + 0.85 * y);
            let cg = (0.95 - 0.65 * warmth + 0.25 * g) * (0.22 + 0.85 * y);
            let cb = (1.00 - 0.15 * warmth) * (0.35 + 0.75 * y);
            (apply_boost(cr, cg, cb, boost * 0.95), [12, 8, 22])
        }
        ColorMode::Ansi16 => {
            let raw = apply_boost(r, g, b, boost);
            (nearest_ansi16(raw), DEFAULT_BG)
        }
        ColorMode::Xterm256 => {
            let raw = apply_boost(r, g, b, boost);
            (quantize_xterm256(raw), DEFAULT_BG)
        }
        ColorMode::Monochrome => {
            let y = (luma_rgb(r, g, b) * boost).clamp(0.0, 1.0);
            let fg = [(y * 246.0) as u8, (y * 244.0) as u8, (y * 238.0) as u8];
            (fg, [10, 11, 14])
        }
    }
}

#[inline]
fn nearest_ansi16(rgb: [u8; 3]) -> [u8; 3] {
    let mut best = ANSI_16_PALETTE[0];
    let mut best_d = u32::MAX;
    for &cand in &ANSI_16_PALETTE {
        let dr = rgb[0] as i32 - cand[0] as i32;
        let dg = rgb[1] as i32 - cand[1] as i32;
        let db = rgb[2] as i32 - cand[2] as i32;
        let d = (dr * dr * 3 + dg * dg * 4 + db * db * 2) as u32;
        if d < best_d {
            best_d = d;
            best = cand;
        }
    }
    best
}

#[inline]
fn quantize_xterm256(rgb: [u8; 3]) -> [u8; 3] {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let snap = |v: u8| -> u8 {
        let mut best = LEVELS[0];
        let mut best_d = 255u8;
        for &lv in &LEVELS {
            let d = v.abs_diff(lv);
            if d < best_d {
                best_d = d;
                best = lv;
            }
        }
        best
    };
    let cube = [snap(rgb[0]), snap(rgb[1]), snap(rgb[2])];
    // Compare against 24-step xterm grayscale ramp
    let avg = ((rgb[0] as u16 + rgb[1] as u16 + rgb[2] as u16) / 3) as u8;
    let g_idx = ((avg.saturating_sub(8) as u16 * 23 + 114) / 230).min(23) as u8;
    let g_val = 8 + g_idx * 10;
    let gray = [g_val, g_val, g_val];

    let dist = |a: [u8; 3], b: [u8; 3]| -> i32 {
        let dr = a[0] as i32 - b[0] as i32;
        let dg = a[1] as i32 - b[1] as i32;
        let db = a[2] as i32 - b[2] as i32;
        dr * dr + dg * dg + db * db
    };
    if dist(rgb, gray) < dist(rgb, cube) {
        gray
    } else {
        cube
    }
}
