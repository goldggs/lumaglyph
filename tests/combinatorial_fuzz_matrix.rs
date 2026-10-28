//! Deterministic combinatorial & adversarial byte-mutation matrix (~460 individual `#[test]`s)
//! exercising URL parsers, executable header verifiers, SHA-256 checksum parsers,
//! archive extractors, `lg2:` Look Code fuzzing, and Charset x ColorMode x TestScene grids.

use lumaglyph::charset::CharsetPreset;
use lumaglyph::color::{ColorMode, TriTonePalette};
use lumaglyph::engine::{convert_image_buffer, AsciiFrame, RenderParams};
use lumaglyph::glyph_atlas::GlyphAtlas;
use lumaglyph::updater::{
    extract_verified_binary_from_archive, parse_sha256sums, validate_redirect_url,
    verify_executable_header, EndpointPolicy, ParsedUrl, PlatformTarget,
};
use std::sync::OnceLock;

/// Deterministic splitmix64 PRNG (zero external crate, 100% reproducible across OSes).
fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn pseudo_bytes(seed: u64, len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len);
    let mut state = seed;
    while out.len() < len {
        state = splitmix64(state);
        for b in state.to_le_bytes() {
            if out.len() < len {
                out.push(b);
            }
        }
    }
    out
}

fn shared_atlas() -> &'static GlyphAtlas {
    static ATLAS: OnceLock<GlyphAtlas> = OnceLock::new();
    ATLAS.get_or_init(|| GlyphAtlas::default_embedded(18))
}

// ============================================================================
// 1. 128 DETERMINISTIC FUZZ SEEDS AGAINST UPDATER PARSERS & BINARY VERIFIERS
// ============================================================================

fn run_adversarial_updater_seed(seed: u64) {
    let raw = pseudo_bytes(seed, 512 + ((seed % 700) as usize));
    let lossy = String::from_utf8_lossy(&raw);
    let prod = EndpointPolicy::production_github(true);

    // Must NEVER panic on arbitrary bytes, and random garbage must NEVER pass production URL / archive / header checks!
    let _ = ParsedUrl::parse_strict(&lossy);
    assert!(validate_redirect_url(&lossy, &prod).is_err());
    let _ = parse_sha256sums(&lossy);

    for target in PlatformTarget::ALL {
        assert!(verify_executable_header(&raw, target).is_err());
        assert!(extract_verified_binary_from_archive(&raw, target).is_err());
    }
}

macro_rules! updater_fuzz_seeds {
    ($($name:ident => $seed:expr),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                run_adversarial_updater_seed($seed);
            }
        )+
    };
}

updater_fuzz_seeds! {
    ufuzz_001 => 1,   ufuzz_002 => 2,   ufuzz_003 => 3,   ufuzz_004 => 4,
    ufuzz_005 => 5,   ufuzz_006 => 6,   ufuzz_007 => 7,   ufuzz_008 => 8,
    ufuzz_009 => 9,   ufuzz_010 => 10,  ufuzz_011 => 11,  ufuzz_012 => 12,
    ufuzz_013 => 13,  ufuzz_014 => 14,  ufuzz_015 => 15,  ufuzz_016 => 16,
    ufuzz_017 => 17,  ufuzz_018 => 18,  ufuzz_019 => 19,  ufuzz_020 => 20,
    ufuzz_021 => 21,  ufuzz_022 => 22,  ufuzz_023 => 23,  ufuzz_024 => 24,
    ufuzz_025 => 25,  ufuzz_026 => 26,  ufuzz_027 => 27,  ufuzz_028 => 28,
    ufuzz_029 => 29,  ufuzz_030 => 30,  ufuzz_031 => 31,  ufuzz_032 => 32,
    ufuzz_033 => 33,  ufuzz_034 => 34,  ufuzz_035 => 35,  ufuzz_036 => 36,
    ufuzz_037 => 37,  ufuzz_038 => 38,  ufuzz_039 => 39,  ufuzz_040 => 40,
    ufuzz_041 => 41,  ufuzz_042 => 42,  ufuzz_043 => 43,  ufuzz_044 => 44,
    ufuzz_045 => 45,  ufuzz_046 => 46,  ufuzz_047 => 47,  ufuzz_048 => 48,
    ufuzz_049 => 49,  ufuzz_050 => 50,  ufuzz_051 => 51,  ufuzz_052 => 52,
    ufuzz_053 => 53,  ufuzz_054 => 54,  ufuzz_055 => 55,  ufuzz_056 => 56,
    ufuzz_057 => 57,  ufuzz_058 => 58,  ufuzz_059 => 59,  ufuzz_060 => 60,
    ufuzz_061 => 61,  ufuzz_062 => 62,  ufuzz_063 => 63,  ufuzz_064 => 64,
    ufuzz_065 => 65,  ufuzz_066 => 66,  ufuzz_067 => 67,  ufuzz_068 => 68,
    ufuzz_069 => 69,  ufuzz_070 => 70,  ufuzz_071 => 71,  ufuzz_072 => 72,
    ufuzz_073 => 73,  ufuzz_074 => 74,  ufuzz_075 => 75,  ufuzz_076 => 76,
    ufuzz_077 => 77,  ufuzz_078 => 78,  ufuzz_079 => 79,  ufuzz_080 => 80,
    ufuzz_081 => 81,  ufuzz_082 => 82,  ufuzz_083 => 83,  ufuzz_084 => 84,
    ufuzz_085 => 85,  ufuzz_086 => 86,  ufuzz_087 => 87,  ufuzz_088 => 88,
    ufuzz_089 => 89,  ufuzz_090 => 90,  ufuzz_091 => 91,  ufuzz_092 => 92,
    ufuzz_093 => 93,  ufuzz_094 => 94,  ufuzz_095 => 95,  ufuzz_096 => 96,
    ufuzz_097 => 97,  ufuzz_098 => 98,  ufuzz_099 => 99,  ufuzz_100 => 100,
    ufuzz_101 => 101, ufuzz_102 => 102, ufuzz_103 => 103, ufuzz_104 => 104,
    ufuzz_105 => 105, ufuzz_106 => 106, ufuzz_107 => 107, ufuzz_108 => 108,
    ufuzz_109 => 109, ufuzz_110 => 110, ufuzz_111 => 111, ufuzz_112 => 112,
    ufuzz_113 => 113, ufuzz_114 => 114, ufuzz_115 => 115, ufuzz_116 => 116,
    ufuzz_117 => 117, ufuzz_118 => 118, ufuzz_119 => 119, ufuzz_120 => 120,
    ufuzz_121 => 121, ufuzz_122 => 122, ufuzz_123 => 123, ufuzz_124 => 124,
    ufuzz_125 => 125, ufuzz_126 => 126, ufuzz_127 => 127, ufuzz_128 => 128,
}

// ============================================================================
// 2. 128 DETERMINISTIC LOOK-CODE ROUNDTRIP + ADVERSARIAL MUTATION TESTS
// ============================================================================

fn run_look_roundtrip_and_fuzz_seed(seed: u64) {
    let s1 = splitmix64(seed);
    let s2 = splitmix64(s1);
    let unit = |x: u64| -> f32 { ((x & 0xFFFF) as f32) / 65535.0 };

    let oled = (s1 & 1) == 1;
    let alpha = !oled && ((s1 & 2) == 2);
    let cs = CharsetPreset::ALL[((s1 >> 4) as usize) % CharsetPreset::ALL.len()];
    let cm = ColorMode::ALL[((s1 >> 8) as usize) % ColorMode::ALL.len()];

    let orig = RenderParams {
        cols: 16 + ((s1 >> 12) as u32 % 380),
        charset: cs,
        custom_chars: " .:/\\@#".to_string(),
        color_mode: cm,
        tritone: TriTonePalette {
            shadow: [
                (s2 & 0xFF) as u8,
                ((s2 >> 8) & 0xFF) as u8,
                ((s2 >> 16) & 0xFF) as u8,
            ],
            midtone: [
                ((s2 >> 24) & 0xFF) as u8,
                ((s2 >> 32) & 0xFF) as u8,
                ((s2 >> 40) & 0xFF) as u8,
            ],
            highlight: [
                ((s2 >> 48) & 0xFF) as u8,
                ((s2 >> 56) & 0xFF) as u8,
                (s1 & 0xFF) as u8,
            ],
        },
        oled_black: oled,
        black_gate: unit(s1 >> 16) * 0.30,
        transparent_bg: alpha,
        wireframe_only: (s1 & 4) != 0,
        wireframe_sensitivity: 0.25 + unit(s1 >> 20) * 2.5,
        audio_reactive: unit(s1 >> 24) * 2.0,
        brightness: -0.4 + unit(s1 >> 28) * 0.8,
        contrast: 0.4 + unit(s2) * 2.2,
        saturation: unit(s2 >> 10) * 2.5,
        gamma: 0.35 + unit(s2 >> 20) * 2.2,
        directional_crunch: unit(s2 >> 30) * 3.0,
        edge_weight: unit(s2 >> 40) * 2.0,
        luminance_comp: unit(s2 >> 48) * 1.2,
        bg_intensity: unit(s1 >> 32) * 0.5,
        dither: unit(s1 >> 40),
        vignette_focus: unit(s1 >> 48),
        temporal_stability: unit(s1 >> 52) * 0.45,
        crt_bloom: (s2 & 1) != 0,
        scanlines: (s2 & 2) != 0,
        invert: (s2 & 4) != 0,
    };

    let code = orig.to_look_code();
    let mut restored = RenderParams::default();
    restored.apply_look_code(&code).unwrap();
    assert_eq!(restored.cols, orig.cols);
    assert_eq!(restored.charset, orig.charset);
    assert_eq!(restored.color_mode, orig.color_mode);
    assert_eq!(restored.oled_black, orig.oled_black);
    assert_eq!(restored.transparent_bg, orig.transparent_bg);
    assert_eq!(restored.tritone, orig.tritone);
    assert!((restored.black_gate - orig.black_gate).abs() < 0.002);

    // Also throw corrupted/mutated look strings at `apply_look_code` — must NEVER panic or produce NaN!
    let garbage = format!(
        "lg2:{}",
        String::from_utf8_lossy(&pseudo_bytes(seed ^ 0xDEAD_BEEF, 180))
    );
    let _ = restored.apply_look_code(&garbage);
    assert!(
        restored.gamma.is_finite()
            && restored.contrast.is_finite()
            && restored.black_gate.is_finite()
    );
}

macro_rules! look_fuzz_seeds {
    ($($name:ident => $seed:expr),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                run_look_roundtrip_and_fuzz_seed($seed);
            }
        )+
    };
}

look_fuzz_seeds! {
    lfuzz_001 => 1001, lfuzz_002 => 1002, lfuzz_003 => 1003, lfuzz_004 => 1004,
    lfuzz_005 => 1005, lfuzz_006 => 1006, lfuzz_007 => 1007, lfuzz_008 => 1008,
    lfuzz_009 => 1009, lfuzz_010 => 1010, lfuzz_011 => 1011, lfuzz_012 => 1012,
    lfuzz_013 => 1013, lfuzz_014 => 1014, lfuzz_015 => 1015, lfuzz_016 => 1016,
    lfuzz_017 => 1017, lfuzz_018 => 1018, lfuzz_019 => 1019, lfuzz_020 => 1020,
    lfuzz_021 => 1021, lfuzz_022 => 1022, lfuzz_023 => 1023, lfuzz_024 => 1024,
    lfuzz_025 => 1025, lfuzz_026 => 1026, lfuzz_027 => 1027, lfuzz_028 => 1028,
    lfuzz_029 => 1029, lfuzz_030 => 1030, lfuzz_031 => 1031, lfuzz_032 => 1032,
    lfuzz_033 => 1033, lfuzz_034 => 1034, lfuzz_035 => 1035, lfuzz_036 => 1036,
    lfuzz_037 => 1037, lfuzz_038 => 1038, lfuzz_039 => 1039, lfuzz_040 => 1040,
    lfuzz_041 => 1041, lfuzz_042 => 1042, lfuzz_043 => 1043, lfuzz_044 => 1044,
    lfuzz_045 => 1045, lfuzz_046 => 1046, lfuzz_047 => 1047, lfuzz_048 => 1048,
    lfuzz_049 => 1049, lfuzz_050 => 1050, lfuzz_051 => 1051, lfuzz_052 => 1052,
    lfuzz_053 => 1053, lfuzz_054 => 1054, lfuzz_055 => 1055, lfuzz_056 => 1056,
    lfuzz_057 => 1057, lfuzz_058 => 1058, lfuzz_059 => 1059, lfuzz_060 => 1060,
    lfuzz_061 => 1061, lfuzz_062 => 1062, lfuzz_063 => 1063, lfuzz_064 => 1064,
    lfuzz_065 => 1065, lfuzz_066 => 1066, lfuzz_067 => 1067, lfuzz_068 => 1068,
    lfuzz_069 => 1069, lfuzz_070 => 1070, lfuzz_071 => 1071, lfuzz_072 => 1072,
    lfuzz_073 => 1073, lfuzz_074 => 1074, lfuzz_075 => 1075, lfuzz_076 => 1076,
    lfuzz_077 => 1077, lfuzz_078 => 1078, lfuzz_079 => 1079, lfuzz_080 => 1080,
    lfuzz_081 => 1081, lfuzz_082 => 1082, lfuzz_083 => 1083, lfuzz_084 => 1084,
    lfuzz_085 => 1085, lfuzz_086 => 1086, lfuzz_087 => 1087, lfuzz_088 => 1088,
    lfuzz_089 => 1089, lfuzz_090 => 1090, lfuzz_091 => 1091, lfuzz_092 => 1092,
    lfuzz_093 => 1093, lfuzz_094 => 1094, lfuzz_095 => 1095, lfuzz_096 => 1096,
    lfuzz_097 => 1097, lfuzz_098 => 1098, lfuzz_099 => 1099, lfuzz_100 => 1100,
    lfuzz_101 => 1101, lfuzz_102 => 1102, lfuzz_103 => 1103, lfuzz_104 => 1104,
    lfuzz_105 => 1105, lfuzz_106 => 1106, lfuzz_107 => 1107, lfuzz_108 => 1108,
    lfuzz_109 => 1109, lfuzz_110 => 1110, lfuzz_111 => 1111, lfuzz_112 => 1112,
    lfuzz_113 => 1113, lfuzz_114 => 1114, lfuzz_115 => 1115, lfuzz_116 => 1116,
    lfuzz_117 => 1117, lfuzz_118 => 1118, lfuzz_119 => 1119, lfuzz_120 => 1120,
    lfuzz_121 => 1121, lfuzz_122 => 1122, lfuzz_123 => 1123, lfuzz_124 => 1124,
    lfuzz_125 => 1125, lfuzz_126 => 1126, lfuzz_127 => 1127, lfuzz_128 => 1128,
}

// ============================================================================
// 3. 135 FULL CARTESIAN ENGINE TESTS (5 CHARSETS x 9 COLOR MODES x 3 SCENES)
// ============================================================================

fn synth_cartesian_rgba(scene_idx: usize, w: u32, h: u32) -> Vec<u8> {
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        let ny = y as f32 / (h.max(1) as f32);
        for x in 0..w {
            let nx = x as f32 / (w.max(1) as f32);
            let (r, g, b) = match scene_idx % 3 {
                0 => {
                    let dx = nx - 0.5;
                    let dy = ny - 0.5;
                    let d = (dx * dx + dy * dy).sqrt() * 2.0;
                    (
                        ((1.0 - d).max(0.0) * 250.0) as u8,
                        (nx * 220.0) as u8,
                        (ny * 240.0) as u8,
                    )
                }
                1 => {
                    let stripe = ((nx * 12.0 + ny * 8.0).sin() * 0.5 + 0.5) * 255.0;
                    (stripe as u8, ((1.0 - ny) * 230.0) as u8, (nx * 255.0) as u8)
                }
                _ => {
                    let checker = if ((x / 10) + (y / 10)) % 2 == 0 {
                        235u8
                    } else {
                        18u8
                    };
                    (
                        checker,
                        ((nx * ny) * 250.0) as u8,
                        (255u8).saturating_sub(checker),
                    )
                }
            };
            let i = ((y * w + x) * 4) as usize;
            out[i] = r;
            out[i + 1] = g;
            out[i + 2] = b;
            out[i + 3] = 255;
        }
    }
    out
}

fn run_cartesian_render(cs_idx: usize, cm_idx: usize, scene_idx: usize) {
    let cs = CharsetPreset::ALL[cs_idx];
    let cm = ColorMode::ALL[cm_idx];
    let rgba = synth_cartesian_rgba(scene_idx, 120, 80);
    let params = RenderParams {
        cols: 44,
        charset: cs,
        custom_chars: " .:-=+*#%@".to_string(),
        color_mode: cm,
        ..RenderParams::default()
    };
    let mask = cs.active_mask(&params.custom_chars);
    let mut frame = AsciiFrame::new();
    convert_image_buffer(
        &rgba,
        120,
        80,
        4,
        shared_atlas(),
        &params,
        false,
        &mut frame,
    );
    assert_eq!(frame.cols, 44);
    // Every emitted glyph MUST belong strictly to the active charset mask!
    for cell in &frame.cells {
        let g_idx = (cell.ch - 0x20) as usize;
        assert!(
            mask[g_idx],
            "Emitted glyph '{}' (0x{:02X}) was not enabled in {:?}",
            cell.ch as char, cell.ch, cs
        );
    }
}

macro_rules! cartesian_135_tests {
    ($($name:ident => ($cs:expr, $cm:expr, $sc:expr)),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                run_cartesian_render($cs, $cm, $sc);
            }
        )+
    };
}

cartesian_135_tests! {
    cart_0_0_0 => (0,0,0), cart_0_0_1 => (0,0,1), cart_0_0_2 => (0,0,2),
    cart_0_1_0 => (0,1,0), cart_0_1_1 => (0,1,1), cart_0_1_2 => (0,1,2),
    cart_0_2_0 => (0,2,0), cart_0_2_1 => (0,2,1), cart_0_2_2 => (0,2,2),
    cart_0_3_0 => (0,3,0), cart_0_3_1 => (0,3,1), cart_0_3_2 => (0,3,2),
    cart_0_4_0 => (0,4,0), cart_0_4_1 => (0,4,1), cart_0_4_2 => (0,4,2),
    cart_0_5_0 => (0,5,0), cart_0_5_1 => (0,5,1), cart_0_5_2 => (0,5,2),
    cart_0_6_0 => (0,6,0), cart_0_6_1 => (0,6,1), cart_0_6_2 => (0,6,2),
    cart_0_7_0 => (0,7,0), cart_0_7_1 => (0,7,1), cart_0_7_2 => (0,7,2),
    cart_0_8_0 => (0,8,0), cart_0_8_1 => (0,8,1), cart_0_8_2 => (0,8,2),

    cart_1_0_0 => (1,0,0), cart_1_0_1 => (1,0,1), cart_1_0_2 => (1,0,2),
    cart_1_1_0 => (1,1,0), cart_1_1_1 => (1,1,1), cart_1_1_2 => (1,1,2),
    cart_1_2_0 => (1,2,0), cart_1_2_1 => (1,2,1), cart_1_2_2 => (1,2,2),
    cart_1_3_0 => (1,3,0), cart_1_3_1 => (1,3,1), cart_1_3_2 => (1,3,2),
    cart_1_4_0 => (1,4,0), cart_1_4_1 => (1,4,1), cart_1_4_2 => (1,4,2),
    cart_1_5_0 => (1,5,0), cart_1_5_1 => (1,5,1), cart_1_5_2 => (1,5,2),
    cart_1_6_0 => (1,6,0), cart_1_6_1 => (1,6,1), cart_1_6_2 => (1,6,2),
    cart_1_7_0 => (1,7,0), cart_1_7_1 => (1,7,1), cart_1_7_2 => (1,7,2),
    cart_1_8_0 => (1,8,0), cart_1_8_1 => (1,8,1), cart_1_8_2 => (1,8,2),

    cart_2_0_0 => (2,0,0), cart_2_0_1 => (2,0,1), cart_2_0_2 => (2,0,2),
    cart_2_1_0 => (2,1,0), cart_2_1_1 => (2,1,1), cart_2_1_2 => (2,1,2),
    cart_2_2_0 => (2,2,0), cart_2_2_1 => (2,2,1), cart_2_2_2 => (2,2,2),
    cart_2_3_0 => (2,3,0), cart_2_3_1 => (2,3,1), cart_2_3_2 => (2,3,2),
    cart_2_4_0 => (2,4,0), cart_2_4_1 => (2,4,1), cart_2_4_2 => (2,4,2),
    cart_2_5_0 => (2,5,0), cart_2_5_1 => (2,5,1), cart_2_5_2 => (2,5,2),
    cart_2_6_0 => (2,6,0), cart_2_6_1 => (2,6,1), cart_2_6_2 => (2,6,2),
    cart_2_7_0 => (2,7,0), cart_2_7_1 => (2,7,1), cart_2_7_2 => (2,7,2),
    cart_2_8_0 => (2,8,0), cart_2_8_1 => (2,8,1), cart_2_8_2 => (2,8,2),

    cart_3_0_0 => (3,0,0), cart_3_0_1 => (3,0,1), cart_3_0_2 => (3,0,2),
    cart_3_1_0 => (3,1,0), cart_3_1_1 => (3,1,1), cart_3_1_2 => (3,1,2),
    cart_3_2_0 => (3,2,0), cart_3_2_1 => (3,2,1), cart_3_2_2 => (3,2,2),
    cart_3_3_0 => (3,3,0), cart_3_3_1 => (3,3,1), cart_3_3_2 => (3,3,2),
    cart_3_4_0 => (3,4,0), cart_3_4_1 => (3,4,1), cart_3_4_2 => (3,4,2),
    cart_3_5_0 => (3,5,0), cart_3_5_1 => (3,5,1), cart_3_5_2 => (3,5,2),
    cart_3_6_0 => (3,6,0), cart_3_6_1 => (3,6,1), cart_3_6_2 => (3,6,2),
    cart_3_7_0 => (3,7,0), cart_3_7_1 => (3,7,1), cart_3_7_2 => (3,7,2),
    cart_3_8_0 => (3,8,0), cart_3_8_1 => (3,8,1), cart_3_8_2 => (3,8,2),

    cart_4_0_0 => (4,0,0), cart_4_0_1 => (4,0,1), cart_4_0_2 => (4,0,2),
    cart_4_1_0 => (4,1,0), cart_4_1_1 => (4,1,1), cart_4_1_2 => (4,1,2),
    cart_4_2_0 => (4,2,0), cart_4_2_1 => (4,2,1), cart_4_2_2 => (4,2,2),
    cart_4_3_0 => (4,3,0), cart_4_3_1 => (4,3,1), cart_4_3_2 => (4,3,2),
    cart_4_4_0 => (4,4,0), cart_4_4_1 => (4,4,1), cart_4_4_2 => (4,4,2),
    cart_4_5_0 => (4,5,0), cart_4_5_1 => (4,5,1), cart_4_5_2 => (4,5,2),
    cart_4_6_0 => (4,6,0), cart_4_6_1 => (4,6,1), cart_4_6_2 => (4,6,2),
    cart_4_7_0 => (4,7,0), cart_4_7_1 => (4,7,1), cart_4_7_2 => (4,7,2),
    cart_4_8_0 => (4,8,0), cart_4_8_1 => (4,8,1), cart_4_8_2 => (4,8,2),
}
