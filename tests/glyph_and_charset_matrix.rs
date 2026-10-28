//! Exhaustive per-glyph calibration (`0x20..=0x7E` = 95 individual tests),
//! multi-scale GlyphAtlas geometry, embedded IBM Plex Sans + JetBrains Mono verification,
//! and CharsetPreset / Custom mask boundary matrix (~230 individual `#[test]`s).

use lumaglyph::charset::{CharsetPreset, ASCII_COUNT, ASCII_MAX, ASCII_MIN};
use lumaglyph::glyph_atlas::GlyphAtlas;
use lumaglyph::theme::{JETBRAINS_MONO_REGULAR, PLEX_SANS_MEDIUM, PLEX_SANS_REGULAR};
use std::sync::OnceLock;

fn shared_atlas_20() -> &'static GlyphAtlas {
    static ATLAS: OnceLock<GlyphAtlas> = OnceLock::new();
    ATLAS.get_or_init(|| GlyphAtlas::default_embedded(20))
}

fn verify_single_printable_glyph(ch_byte: u8) {
    assert!((ASCII_MIN..=ASCII_MAX).contains(&ch_byte));
    let atlas = shared_atlas_20();
    let idx = (ch_byte - ASCII_MIN) as usize;
    let vec = atlas.vectors[idx];
    let cov = atlas.coverage[idx];
    let bmp = atlas.glyph_slice(idx);

    assert_eq!(bmp.len(), atlas.cell_w * atlas.cell_h);
    assert!(
        cov.is_finite() && (0.0..=1.0).contains(&cov),
        "Invalid coverage {cov} for glyph 0x{ch_byte:02X}"
    );
    for (dim, &val) in vec.v.iter().enumerate() {
        assert!(
            val.is_finite() && (-2.5..=2.5).contains(&val),
            "Non-finite or out-of-range vector dim {dim}={val} for glyph 0x{ch_byte:02X}"
        );
    }

    if ch_byte == b' ' {
        assert_eq!(cov, 0.0, "Space 0x20 must have zero ink coverage");
        assert!(
            bmp.iter().all(|&a| a == 0),
            "Space 0x20 bitmap must be 100% transparent"
        );
    } else {
        assert!(
            cov > 0.0,
            "Visible printable ASCII glyph '{}' (0x{ch_byte:02X}) must have positive ink coverage",
            ch_byte as char
        );
        assert!(
            bmp.iter().any(|&a| a > 0),
            "Visible printable ASCII glyph '{}' (0x{ch_byte:02X}) must render non-zero bitmap alpha",
            ch_byte as char
        );
    }
}

macro_rules! per_glyph_tests {
    ($($name:ident => $byte:expr),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                verify_single_printable_glyph($byte);
            }
        )+
    };
}

per_glyph_tests! {
    glyph_0x20_space => 0x20, glyph_0x21_bang => 0x21, glyph_0x22_dquote => 0x22, glyph_0x23_hash => 0x23,
    glyph_0x24_dollar => 0x24, glyph_0x25_percent => 0x25, glyph_0x26_amp => 0x26, glyph_0x27_squote => 0x27,
    glyph_0x28_lparen => 0x28, glyph_0x29_rparen => 0x29, glyph_0x2a_star => 0x2A, glyph_0x2b_plus => 0x2B,
    glyph_0x2c_comma => 0x2C, glyph_0x2d_minus => 0x2D, glyph_0x2e_dot => 0x2E, glyph_0x2f_slash => 0x2F,
    glyph_0x30_0 => 0x30, glyph_0x31_1 => 0x31, glyph_0x32_2 => 0x32, glyph_0x33_3 => 0x33,
    glyph_0x34_4 => 0x34, glyph_0x35_5 => 0x35, glyph_0x36_6 => 0x36, glyph_0x37_7 => 0x37,
    glyph_0x38_8 => 0x38, glyph_0x39_9 => 0x39, glyph_0x3a_colon => 0x3A, glyph_0x3b_semicolon => 0x3B,
    glyph_0x3c_lt => 0x3C, glyph_0x3d_eq => 0x3D, glyph_0x3e_gt => 0x3E, glyph_0x3f_quest => 0x3F,
    glyph_0x40_at => 0x40, glyph_0x41_upper_a => 0x41, glyph_0x42_upper_b => 0x42, glyph_0x43_upper_c => 0x43,
    glyph_0x44_upper_d => 0x44, glyph_0x45_upper_e => 0x45, glyph_0x46_upper_f => 0x46, glyph_0x47_upper_g => 0x47,
    glyph_0x48_upper_h => 0x48, glyph_0x49_upper_i => 0x49, glyph_0x4a_upper_j => 0x4A, glyph_0x4b_upper_k => 0x4B,
    glyph_0x4c_upper_l => 0x4C, glyph_0x4d_upper_m => 0x4D, glyph_0x4e_upper_n => 0x4E, glyph_0x4f_upper_o => 0x4F,
    glyph_0x50_upper_p => 0x50, glyph_0x51_upper_q => 0x51, glyph_0x52_upper_r => 0x52, glyph_0x53_upper_s => 0x53,
    glyph_0x54_upper_t => 0x54, glyph_0x55_upper_u => 0x55, glyph_0x56_upper_v => 0x56, glyph_0x57_upper_w => 0x57,
    glyph_0x58_upper_x => 0x58, glyph_0x59_upper_y => 0x59, glyph_0x5a_upper_z => 0x5A, glyph_0x5b_lbrack => 0x5B,
    glyph_0x5c_bslash => 0x5C, glyph_0x5d_rbrack => 0x5D, glyph_0x5e_caret => 0x5E, glyph_0x5f_under => 0x5F,
    glyph_0x60_backtick => 0x60, glyph_0x61_lower_a => 0x61, glyph_0x62_lower_b => 0x62, glyph_0x63_lower_c => 0x63,
    glyph_0x64_lower_d => 0x64, glyph_0x65_lower_e => 0x65, glyph_0x66_lower_f => 0x66, glyph_0x67_lower_g => 0x67,
    glyph_0x68_lower_h => 0x68, glyph_0x69_lower_i => 0x69, glyph_0x6a_lower_j => 0x6A, glyph_0x6b_lower_k => 0x6B,
    glyph_0x6c_lower_l => 0x6C, glyph_0x6d_lower_m => 0x6D, glyph_0x6e_lower_n => 0x6E, glyph_0x6f_lower_o => 0x6F,
    glyph_0x70_lower_p => 0x70, glyph_0x71_lower_q => 0x71, glyph_0x72_lower_r => 0x72, glyph_0x73_lower_s => 0x73,
    glyph_0x74_lower_t => 0x74, glyph_0x75_lower_u => 0x75, glyph_0x76_lower_v => 0x76, glyph_0x77_lower_w => 0x77,
    glyph_0x78_lower_x => 0x78, glyph_0x79_lower_y => 0x79, glyph_0x7a_lower_z => 0x7A, glyph_0x7b_lbrace => 0x7B,
    glyph_0x7c_pipe => 0x7C, glyph_0x7d_rbrace => 0x7D, glyph_0x7e_tilde => 0x7E,
}

#[test]
fn directional_glyph_pairs_have_opposite_diagonal_shears() {
    let atlas = shared_atlas_20();
    let slash = atlas.vectors[(b'/' - ASCII_MIN) as usize].v;
    let bslash = atlas.vectors[(b'\\' - ASCII_MIN) as usize].v;
    assert!(slash[7] * bslash[7] < 0.0, "/ and \\ must oppose on Jxy");

    let bar = atlas.vectors[(b'|' - ASCII_MIN) as usize].v;
    let dash = atlas.vectors[(b'-' - ASCII_MIN) as usize].v;
    assert!(
        bar[6] != dash[6],
        "| and - must differ on Jxx - Jyy axis orientation"
    );
}

macro_rules! atlas_height_tests {
    ($($name:ident => $h:expr),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                let a = shared_atlas_20().with_cell_height($h);
                assert!(a.cell_w >= 4 && a.cell_h >= 8);
                assert!(a.aspect > 0.2 && a.aspect < 1.2);
                assert_eq!(a.bitmaps.len(), ASCII_COUNT * a.cell_w * a.cell_h);
            }
        )+
    };
}

atlas_height_tests! {
    atlas_h_01_clamped => 1,
    atlas_h_04_clamped => 4,
    atlas_h_08 => 8,
    atlas_h_10 => 10,
    atlas_h_12 => 12,
    atlas_h_14 => 14,
    atlas_h_16 => 16,
    atlas_h_18 => 18,
    atlas_h_20 => 20,
    atlas_h_22 => 22,
    atlas_h_24 => 24,
    atlas_h_28 => 28,
    atlas_h_32 => 32,
    atlas_h_36 => 36,
    atlas_h_42 => 42,
    atlas_h_48 => 48,
    atlas_h_54 => 54,
    atlas_h_64 => 64,
}

#[test]
fn custom_font_calibration_works_with_ibm_plex_sans_and_rejects_garbage() {
    let reg = GlyphAtlas::from_font_bytes(
        "IBMPlexSans-Regular".to_string(),
        PLEX_SANS_REGULAR.to_vec(),
        20,
    )
    .unwrap();
    let med = GlyphAtlas::from_font_bytes(
        "IBMPlexSans-Medium".to_string(),
        PLEX_SANS_MEDIUM.to_vec(),
        20,
    )
    .unwrap();
    let mono = GlyphAtlas::from_font_bytes(
        "JetBrainsMono".to_string(),
        JETBRAINS_MONO_REGULAR.to_vec(),
        20,
    )
    .unwrap();
    assert_eq!(reg.vectors.len(), ASCII_COUNT);
    assert_eq!(med.vectors.len(), ASCII_COUNT);
    assert_eq!(mono.vectors.len(), ASCII_COUNT);
    assert!(GlyphAtlas::from_font_bytes(
        "corrupt.ttf".to_string(),
        b"not a real font".to_vec(),
        20
    )
    .is_err());
    assert!(GlyphAtlas::from_font_bytes("empty.ttf".to_string(), Vec::new(), 20).is_err());
}

macro_rules! charset_cli_tests {
    ($($name:ident => ($arg:expr, $expect:expr)),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                assert_eq!(CharsetPreset::from_cli($arg), $expect);
            }
        )+
    };
}

charset_cli_tests! {
    cs_cli_genuine95 => ("genuine95", Some(CharsetPreset::Genuine95)),
    cs_cli_full => ("FULL", Some(CharsetPreset::Genuine95)),
    cs_cli_95 => (" 95 ", Some(CharsetPreset::Genuine95)),
    cs_cli_genuine => ("Genuine", Some(CharsetPreset::Genuine95)),
    cs_cli_structural => ("structural", Some(CharsetPreset::StructuralEdge)),
    cs_cli_edge => ("edge", Some(CharsetPreset::StructuralEdge)),
    cs_cli_arch => ("ARCH", Some(CharsetPreset::StructuralEdge)),
    cs_cli_dense => ("dense", Some(CharsetPreset::TypographicDense)),
    cs_cli_typographic => ("typographic", Some(CharsetPreset::TypographicDense)),
    cs_cli_retro => ("retro", Some(CharsetPreset::RetroClassic)),
    cs_cli_classic => ("classic", Some(CharsetPreset::RetroClassic)),
    cs_cli_basic => ("basic", Some(CharsetPreset::RetroClassic)),
    cs_cli_custom => ("custom", Some(CharsetPreset::Custom)),
    cs_cli_unknown => ("nonexistent", None),
    cs_cli_empty => ("", None),
}

// 95 custom single-character mask tests verifying space + char active, and fallback when only space/non-ASCII
macro_rules! custom_char_mask_tests {
    ($($name:ident => $byte:expr),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                let s = String::from_utf8(vec![$byte]).unwrap();
                let mask = CharsetPreset::Custom.active_mask(&s);
                let active_count = mask.iter().filter(|&&b| b).count();
                assert!(mask[0], "Space must always be active in Custom charset");
                assert!(
                    active_count >= 2,
                    "Custom charset for 0x{:02X} must guarantee >= 2 active glyphs, got {}",
                    $byte,
                    active_count
                );
                if $byte != b' ' {
                    assert!(mask[($byte - ASCII_MIN) as usize]);
                }
            }
        )+
    };
}

custom_char_mask_tests! {
    cust_0x20 => 0x20, cust_0x21 => 0x21, cust_0x22 => 0x22, cust_0x23 => 0x23,
    cust_0x24 => 0x24, cust_0x25 => 0x25, cust_0x26 => 0x26, cust_0x27 => 0x27,
    cust_0x28 => 0x28, cust_0x29 => 0x29, cust_0x2a => 0x2A, cust_0x2b => 0x2B,
    cust_0x2c => 0x2C, cust_0x2d => 0x2D, cust_0x2e => 0x2E, cust_0x2f => 0x2F,
    cust_0x30 => 0x30, cust_0x31 => 0x31, cust_0x32 => 0x32, cust_0x33 => 0x33,
    cust_0x34 => 0x34, cust_0x35 => 0x35, cust_0x36 => 0x36, cust_0x37 => 0x37,
    cust_0x38 => 0x38, cust_0x39 => 0x39, cust_0x3a => 0x3A, cust_0x3b => 0x3B,
    cust_0x3c => 0x3C, cust_0x3d => 0x3D, cust_0x3e => 0x3E, cust_0x3f => 0x3F,
    cust_0x40 => 0x40, cust_0x41 => 0x41, cust_0x42 => 0x42, cust_0x43 => 0x43,
    cust_0x44 => 0x44, cust_0x45 => 0x45, cust_0x46 => 0x46, cust_0x47 => 0x47,
    cust_0x48 => 0x48, cust_0x49 => 0x49, cust_0x4a => 0x4A, cust_0x4b => 0x4B,
    cust_0x4c => 0x4C, cust_0x4d => 0x4D, cust_0x4e => 0x4E, cust_0x4f => 0x4F,
    cust_0x50 => 0x50, cust_0x51 => 0x51, cust_0x52 => 0x52, cust_0x53 => 0x53,
    cust_0x54 => 0x54, cust_0x55 => 0x55, cust_0x56 => 0x56, cust_0x57 => 0x57,
    cust_0x58 => 0x58, cust_0x59 => 0x59, cust_0x5a => 0x5A, cust_0x5b => 0x5B,
    cust_0x5c => 0x5C, cust_0x5d => 0x5D, cust_0x5e => 0x5E, cust_0x5f => 0x5F,
    cust_0x60 => 0x60, cust_0x61 => 0x61, cust_0x62 => 0x62, cust_0x63 => 0x63,
    cust_0x64 => 0x64, cust_0x65 => 0x65, cust_0x66 => 0x66, cust_0x67 => 0x67,
    cust_0x68 => 0x68, cust_0x69 => 0x69, cust_0x6a => 0x6A, cust_0x6b => 0x6B,
    cust_0x6c => 0x6C, cust_0x6d => 0x6D, cust_0x6e => 0x6E, cust_0x6f => 0x6F,
    cust_0x70 => 0x70, cust_0x71 => 0x71, cust_0x72 => 0x72, cust_0x73 => 0x73,
    cust_0x74 => 0x74, cust_0x75 => 0x75, cust_0x76 => 0x76, cust_0x77 => 0x77,
    cust_0x78 => 0x78, cust_0x79 => 0x79, cust_0x7a => 0x7A, cust_0x7b => 0x7B,
    cust_0x7c => 0x7C, cust_0x7d => 0x7D, cust_0x7e => 0x7E,
}

#[test]
fn preset_masks_have_expected_cardinality() {
    assert_eq!(
        CharsetPreset::Genuine95
            .active_mask("")
            .iter()
            .filter(|&&b| b)
            .count(),
        95
    );
    assert!(
        CharsetPreset::StructuralEdge
            .active_mask("")
            .iter()
            .filter(|&&b| b)
            .count()
            >= 35
    );
    assert!(
        CharsetPreset::TypographicDense
            .active_mask("")
            .iter()
            .filter(|&&b| b)
            .count()
            >= 65
    );
    assert_eq!(
        CharsetPreset::RetroClassic
            .active_mask("")
            .iter()
            .filter(|&&b| b)
            .count(),
        17
    );
    // Non-ASCII / control-only custom input safely falls back to 10-char ramp
    assert_eq!(
        CharsetPreset::Custom
            .active_mask("\0\n\t\u{00E9}")
            .iter()
            .filter(|&&b| b)
            .count(),
        10
    );
}
