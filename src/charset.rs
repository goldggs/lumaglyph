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
#[cfg(test)]
mod charset_unit_tests {
    use super::*;

    #[test]
    fn every_preset_round_trips_through_its_cli_name() {
        let mut seen = std::collections::HashSet::new();
        for preset in CharsetPreset::ALL {
            let name = preset.cli_name();
            assert!(seen.insert(name), "duplicate cli_name {name}");
            assert_eq!(CharsetPreset::from_cli(name), Some(preset));
            assert!(!preset.label().is_empty());
        }
    }

    #[test]
    fn ascii_count_matches_the_printable_range() {
        assert_eq!(ASCII_COUNT, 0x7E - 0x20 + 1);
        assert_eq!(ASCII_MIN, 0x20);
        assert_eq!(ASCII_COUNT, 95);
    }

    #[test]
    fn genuine95_activates_every_glyph() {
        let mask = CharsetPreset::Genuine95.active_mask("");
        assert_eq!(mask.len(), ASCII_COUNT);
        assert!(mask.iter().all(|&b| b), "Genuine95 must use all 95 glyphs");
    }

    #[test]
    fn subset_presets_always_keep_space() {
        for preset in CharsetPreset::ALL {
            let mask = preset.active_mask("");
            assert!(
                mask[0],
                "{preset:?} dropped the space glyph, breaking background cells"
            );
        }
    }

    #[test]
    fn custom_mask_reflects_only_the_supplied_characters() {
        let mask = CharsetPreset::active_mask(CharsetPreset::Custom, "ABC");
        let active: Vec<char> = mask
            .iter()
            .enumerate()
            .filter(|(_, &on)| on)
            .map(|(i, _)| (i as u8 + ASCII_MIN) as char)
            .collect();
        // Space is always retained, plus the three requested glyphs.
        assert_eq!(active, vec![' ', 'A', 'B', 'C']);
    }

    #[test]
    fn custom_mask_ignores_out_of_range_and_duplicate_characters() {
        let with_junk = CharsetPreset::active_mask(CharsetPreset::Custom, "A\nA\t\u{1F600}AA");
        let count = with_junk.iter().filter(|&&b| b).count();
        assert_eq!(count, 2, "expected space + 'A', got {count}");
    }

    #[test]
    fn empty_custom_charset_still_leaves_a_drawable_glyph() {
        let mask = CharsetPreset::active_mask(CharsetPreset::Custom, "");
        assert!(
            mask.iter().any(|&b| b),
            "an empty custom set must not disable every glyph"
        );
    }

    #[test]
    fn masks_are_stable_across_repeated_calls() {
        for preset in CharsetPreset::ALL {
            assert_eq!(preset.active_mask("XYZ"), preset.active_mask("XYZ"));
        }
    }
}
