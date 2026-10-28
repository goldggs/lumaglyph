use serde::{Deserialize, Serialize};

/// Printable 7-bit ASCII range: 0x20 (' ') through 0x7E ('~') = 95 glyphs.
pub const ASCII_MIN: u8 = 0x20;
pub const ASCII_MAX: u8 = 0x7E;
pub const ASCII_COUNT: usize = (ASCII_MAX - ASCII_MIN + 1) as usize; // 95

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CharsetPreset {
    /// All 95 printable 7-bit ASCII glyphs (0x20..=0x7E), matched via 8D shape+gradient vectors.
    Genuine95,
    /// Structural/architectural focus: emphasizes slashes, parens, brackets, pipes, punctuation & crisp anchors.
    StructuralEdge,
    /// Rich 70-character typographic ramp + punctuation shapes.
    TypographicDense,
    /// Classic 17-char retro ramp (useful to compare against Genuine 95 shape matching).
    RetroClassic,
    /// User-supplied character set (filtered to unique printable ASCII).
    Custom,
}

impl CharsetPreset {
    pub const ALL: [CharsetPreset; 5] = [
        CharsetPreset::Genuine95,
        CharsetPreset::StructuralEdge,
        CharsetPreset::TypographicDense,
        CharsetPreset::RetroClassic,
        CharsetPreset::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            CharsetPreset::Genuine95 => "Genuine 95 ASCII (Full Range)",
            CharsetPreset::StructuralEdge => "Structural & Edge Geometry",
            CharsetPreset::TypographicDense => "Dense Typographic (70 Chars)",
            CharsetPreset::RetroClassic => "Retro Classic (17 Chars)",
            CharsetPreset::Custom => "Custom Phrase / Code",
        }
    }

    pub fn cli_name(self) -> &'static str {
        match self {
            CharsetPreset::Genuine95 => "genuine95",
            CharsetPreset::StructuralEdge => "structural",
            CharsetPreset::TypographicDense => "dense",
            CharsetPreset::RetroClassic => "retro",
            CharsetPreset::Custom => "custom",
        }
    }

    pub fn from_cli(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "genuine95" | "full" | "95" | "genuine" => Some(Self::Genuine95),
            "structural" | "edge" | "arch" => Some(Self::StructuralEdge),
            "dense" | "typographic" => Some(Self::TypographicDense),
            "retro" | "classic" | "basic" => Some(Self::RetroClassic),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }

    /// Returns a 95-element boolean mask indicating which glyphs (`0x20..=0x7E`) are active.
    pub fn active_mask(self, custom: &str) -> [bool; ASCII_COUNT] {
        let mut mask = [false; ASCII_COUNT];
        match self {
            CharsetPreset::Genuine95 => {
                mask.fill(true);
            }
            CharsetPreset::StructuralEdge => {
                for &b in b" `'.,-~:;!|/\\()[]{}<>^_+=*?vVxXYAwWmM#$%&@" {
                    if (ASCII_MIN..=ASCII_MAX).contains(&b) {
                        mask[(b - ASCII_MIN) as usize] = true;
                    }
                }
            }
            CharsetPreset::TypographicDense => {
                for &b in
                    b" .'`^\",:;Il!i><~+_-?][}{1)(|\\/tfjrxnuvczXYUJCLQ0OZmwqpdbkhao*#MW&8%B@$"
                {
                    if (ASCII_MIN..=ASCII_MAX).contains(&b) {
                        mask[(b - ASCII_MIN) as usize] = true;
                    }
                }
            }
            CharsetPreset::RetroClassic => {
                for &b in b" .,:;+i1tfLCG08@#" {
                    if (ASCII_MIN..=ASCII_MAX).contains(&b) {
                        mask[(b - ASCII_MIN) as usize] = true;
                    }
                }
            }
            CharsetPreset::Custom => {
                mask[0] = true; // Always include space for clean dark regions
                for b in custom.bytes() {
                    if (ASCII_MIN..=ASCII_MAX).contains(&b) {
                        mask[(b - ASCII_MIN) as usize] = true;
                    }
                }
                // Ensure at least 2 active glyphs even if custom string was empty
                let count = mask.iter().filter(|&&x| x).count();
                if count < 2 {
                    for &b in b" .:-=+*#%@" {
                        mask[(b - ASCII_MIN) as usize] = true;
                    }
                }
            }
        }
        mask
    }
}
