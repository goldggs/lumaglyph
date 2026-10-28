use serde::{Deserialize, Serialize};

/// Default deep matte studio obsidian background (`#090B10`) for standard LCD/IPS displays.
pub const DEFAULT_BG: [u8; 3] = [9, 11, 16];

/// True pitch-black (`#000000`) shutting physical OLED subpixels completely off.
pub const OLED_BG: [u8; 3] = [0, 0, 0];

#[inline(always)]
pub fn active_canvas_bg(oled_black: bool) -> [u8; 3] {
    if oled_black {
        OLED_BG
    } else {
        DEFAULT_BG
    }
}

/// 4x4 normalized Bayer ordered dithering matrix in `[-0.5, +0.5]`.
pub const BAYER_4X4: [[f32; 4]; 4] = [
    [-0.46875, 0.03125, -0.34375, 0.15625],
    [0.28125, -0.21875, 0.40625, -0.09375],
    [-0.28125, 0.21875, -0.40625, 0.09375],
    [0.46875, -0.03125, 0.34375, -0.15625],
];

/// Custom 3-stop color gradient (`Shadow -> Midtone -> Highlight`) for poster/duotone/tritone looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriTonePalette {
    pub shadow: [u8; 3],
    pub midtone: [u8; 3],
    pub highlight: [u8; 3],
}

impl Default for TriTonePalette {
    fn default() -> Self {
        Self::SOLAR_FLARE
    }
}

impl TriTonePalette {
    pub const SOLAR_FLARE: Self = Self {
        shadow: [42, 10, 82],      // Deep Violet
        midtone: [225, 29, 72],    // Electric Crimson
        highlight: [253, 224, 71], // Solar Gold
    };

    pub const TOKYO_MIDNIGHT: Self = Self {
        shadow: [18, 24, 92],      // Indigo Night
        midtone: [192, 38, 211],   // Neon Orchid
        highlight: [56, 236, 255], // Laser Cyan
    };

    pub const BLADE_TEAL_AMBER: Self = Self {
        shadow: [10, 58, 74],      // Deep Petrol Teal
        midtone: [20, 148, 168],   // Cinema Cyan
        highlight: [255, 158, 44], // Sodium Vapor Amber
    };

    pub const EMERALD_ICE: Self = Self {
        shadow: [8, 52, 42],        // Deep Pine
        midtone: [16, 185, 129],    // Emerald
        highlight: [224, 247, 255], // Glacial Ice
    };

    pub const PRESETS: [(&'static str, Self); 4] = [
        ("Solar Flare", Self::SOLAR_FLARE),
        ("Tokyo", Self::TOKYO_MIDNIGHT),
        ("Blade", Self::BLADE_TEAL_AMBER),
        ("Emerald", Self::EMERALD_ICE),
    ];

    pub fn sample(self, y: f32) -> [u8; 3] {
        let t_clamped = y.clamp(0.0, 1.0);
        let (c0, c1, t) = if t_clamped < 0.5 {
            (self.shadow, self.midtone, t_clamped * 2.0)
        } else {
            (self.midtone, self.highlight, (t_clamped - 0.5) * 2.0)
        };
        [
            (c0[0] as f32 + (c1[0] as f32 - c0[0] as f32) * t).round() as u8,
            (c0[1] as f32 + (c1[1] as f32 - c0[1] as f32) * t).round() as u8,
            (c0[2] as f32 + (c1[2] as f32 - c0[2] as f32) * t).round() as u8,
        ]
    }

    pub fn to_hex_triplet(self) -> String {
        format!(
            "{:02X}{:02X}{:02X}-{:02X}{:02X}{:02X}-{:02X}{:02X}{:02X}",
            self.shadow[0],
            self.shadow[1],
            self.shadow[2],
            self.midtone[0],
            self.midtone[1],
            self.midtone[2],
            self.highlight[0],
            self.highlight[1],
            self.highlight[2],
        )
    }

    pub fn from_hex_triplet(s: &str) -> Option<Self> {
        let sep = if s.contains('-') { '-' } else { ',' };
        let mut parts = s.trim().split(sep);
        let sh = parse_hex_rgb(parts.next()?)?;
        let mid = parse_hex_rgb(parts.next()?)?;
        let hi = parse_hex_rgb(parts.next()?)?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            shadow: sh,
            midtone: mid,
            highlight: hi,
        })
    }
}

pub fn parse_hex_rgb(hex: &str) -> Option<[u8; 3]> {
    let h = hex.trim().trim_start_matches('#');
    if h.len() != 6 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let r = u8::from_str_radix(&h[0..2], 16).ok()?;
    let g = u8::from_str_radix(&h[2..4], 16).ok()?;
    let b = u8::from_str_radix(&h[4..6], 16).ok()?;
    Some([r, g, b])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorMode {
    /// 24-bit TrueColor with ink-coverage luminance compensation so sparse glyphs stay vibrant.
    TrueColor,
    /// Demoscene/BBS Dual-Color: crisp foreground glyph + shadowed ambient cell background.
    DualColor,
    /// Custom 3-stop Shadow -> Midtone -> Highlight gradient ramp.
    TriTone,
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
    pub const ALL: [ColorMode; 9] = [
        ColorMode::TrueColor,
        ColorMode::DualColor,
        ColorMode::TriTone,
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
            ColorMode::TriTone => "Custom Tri-Tone (Shadow/Mid/High)",
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
            ColorMode::TriTone => "tritone",
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
            "tritone" | "duotone" | "ramp" => Some(Self::TriTone),
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

/// Compute `(fg, bg)` colors for a matched cell, honoring `oled_black` (`#000000`) and `TriTone`.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn resolve_cell_colors(
    mode: ColorMode,
    r: f32,
    g: f32,
    b: f32,
    boost: f32,
    bg_intensity: f32,
    oled_black: bool,
    tritone: &TriTonePalette,
) -> ([u8; 3], [u8; 3]) {
    let base_bg = active_canvas_bg(oled_black);

    let apply_boost = |cr: f32, cg: f32, cb: f32, factor: f32| -> [u8; 3] {
        let mr = cr * factor;
        let mg = cg * factor;
        let mb = cb * factor;
        let peak = mr.max(mg).max(mb);
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
        ColorMode::TrueColor => (apply_boost(r, g, b, boost), base_bg),
        ColorMode::DualColor => {
            let fg = apply_boost(r, g, b, boost * 1.06);
            let bg_scale = bg_intensity.clamp(0.0, 0.65);
            // In OLED mode, square low-luma background contribution so dark areas drop cleanly to #000000
            let oled_toe = if oled_black {
                luma_rgb(r, g, b).clamp(0.0, 1.0).sqrt()
            } else {
                1.0
            };
            let s = bg_scale * oled_toe;
            let bg = [
                ((base_bg[0] as f32) * (1.0 - s) + (r * 255.0) * s) as u8,
                ((base_bg[1] as f32) * (1.0 - s) + (g * 255.0) * s) as u8,
                ((base_bg[2] as f32) * (1.0 - s) + (b * 255.0) * s) as u8,
            ];
            (fg, bg)
        }
        ColorMode::TriTone => {
            let y = luma_rgb(r, g, b).clamp(0.0, 1.0);
            let (c0, c1, t) = if y < 0.5 {
                (tritone.shadow, tritone.midtone, y * 2.0)
            } else {
                (tritone.midtone, tritone.highlight, (y - 0.5) * 2.0)
            };
            let cr = (c0[0] as f32 + (c1[0] as f32 - c0[0] as f32) * t) * (1.0 / 255.0);
            let cg = (c0[1] as f32 + (c1[1] as f32 - c0[1] as f32) * t) * (1.0 / 255.0);
            let cb = (c0[2] as f32 + (c1[2] as f32 - c0[2] as f32) * t) * (1.0 / 255.0);
            // Attenuate deep shadow stop slightly by luma so near-black doesn't glow artificially bright
            let env = (0.28 + 0.72 * y).min(1.0);
            (apply_boost(cr * env, cg * env, cb * env, boost), base_bg)
        }
        ColorMode::PhosphorAmber => {
            let y = (luma_rgb(r, g, b) * boost).clamp(0.0, 1.15);
            let fg = [
                (y.min(1.0) * 255.0) as u8,
                ((y.powf(1.22) * 184.0 + (y - 0.75).max(0.0) * 180.0).clamp(0.0, 245.0)) as u8,
                ((y.powf(2.4) * 72.0).clamp(0.0, 95.0)) as u8,
            ];
            (fg, if oled_black { OLED_BG } else { [14, 9, 4] })
        }
        ColorMode::PhosphorGreen => {
            let y = (luma_rgb(r, g, b) * boost).clamp(0.0, 1.15);
            let fg = [
                ((y.powf(2.0) * 95.0).clamp(0.0, 120.0)) as u8,
                (y.min(1.0) * 255.0) as u8,
                ((y.powf(1.35) * 125.0).clamp(0.0, 155.0)) as u8,
            ];
            (fg, if oled_black { OLED_BG } else { [5, 14, 8] })
        }
        ColorMode::Cyberpunk => {
            let y = luma_rgb(r, g, b);
            let warmth = (r - b).clamp(-1.0, 1.0) * 0.5 + 0.5;
            let cr = (0.12 + 0.88 * warmth) * (0.28 + 0.85 * y);
            let cg = (0.95 - 0.65 * warmth + 0.25 * g) * (0.22 + 0.85 * y);
            let cb = (1.00 - 0.15 * warmth) * (0.35 + 0.75 * y);
            (
                apply_boost(cr, cg, cb, boost * 0.95),
                if oled_black { OLED_BG } else { [12, 8, 22] },
            )
        }
        ColorMode::Ansi16 => {
            let raw = apply_boost(r, g, b, boost);
            (nearest_ansi16(raw), base_bg)
        }
        ColorMode::Xterm256 => {
            let raw = apply_boost(r, g, b, boost);
            (quantize_xterm256(raw), base_bg)
        }
        ColorMode::Monochrome => {
            let y = (luma_rgb(r, g, b) * boost).clamp(0.0, 1.0);
            let fg = [(y * 246.0) as u8, (y * 244.0) as u8, (y * 238.0) as u8];
            (fg, if oled_black { OLED_BG } else { [10, 11, 14] })
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
