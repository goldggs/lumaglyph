use crate::charset::CharsetPreset;
use crate::color::{ColorMode, TriTonePalette};
use crate::engine::RenderParams;

fn parse_finite_f32(v: &str) -> Option<f32> {
    v.trim().parse::<f32>().ok().filter(|x| x.is_finite())
}

impl RenderParams {
    /// Encode all visual & physics settings into a shareable one-line `lg2:...` Look Code.
    pub fn to_look_code(&self) -> String {
        let b = |flag: bool| -> u8 { u8::from(flag) };
        let mut s = format!(
            "lg2:cols={},cs={},cm={},oled={},gate={:.3},alpha={},wf={},wfs={:.2},ar={:.2},cr={:.2},ew={:.2},lum={:.2},bg={:.2},dt={:.2},vig={:.2},st={:.2},br={:.2},ct={:.2},sat={:.2},gam={:.2},bl={},sc={},inv={},tri={}",
            self.cols,
            self.charset.cli_name(),
            self.color_mode.cli_name(),
            b(self.oled_black),
            self.black_gate,
            b(self.transparent_bg),
            b(self.wireframe_only),
            self.wireframe_sensitivity,
            self.audio_reactive,
            self.directional_crunch,
            self.edge_weight,
            self.luminance_comp,
            self.bg_intensity,
            self.dither,
            self.vignette_focus,
            self.temporal_stability,
            self.brightness,
            self.contrast,
            self.saturation,
            self.gamma,
            b(self.crt_bloom),
            b(self.scanlines),
            b(self.invert),
            self.tritone.to_hex_triplet(),
        );
        if self.charset == CharsetPreset::Custom && !self.custom_chars.is_empty() {
            s.push_str(",cc=");
            for &byte in self.custom_chars.as_bytes().iter().take(256) {
                if (0x20..=0x7E).contains(&byte) {
                    s.push_str(&format!("{byte:02X}"));
                }
            }
        }
        s
    }

    /// Parse and apply a shareable `lg2:...` Look Code string onto `self`.
    /// Rejects non-finite floats (`NaN`, `inf`, `-inf`), oversized inputs, and control bytes.
    pub fn apply_look_code(&mut self, raw: &str) -> Result<(), String> {
        if raw.len() > 2048 {
            return Err("Look code exceeds maximum allowed length (2048 bytes)".to_string());
        }
        let trimmed = raw.trim().trim_matches(['`', '"', '\'']).trim();
        let body = trimmed
            .strip_prefix("lg2:")
            .or_else(|| trimmed.strip_prefix("lg1:"))
            .ok_or_else(|| "Look code must start with 'lg2:'".to_string())?;

        let parse_bool = |v: &str| -> Option<bool> {
            match v.trim().to_ascii_lowercase().as_str() {
                "1" | "true" | "yes" | "on" => Some(true),
                "0" | "false" | "no" | "off" => Some(false),
                _ => None,
            }
        };

        let mut applied = 0usize;
        for token in body.split(',') {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            let Some((k, v)) = token.split_once('=') else {
                continue;
            };
            match k.trim() {
                "cols" => {
                    if let Ok(n) = v.trim().parse::<u32>() {
                        self.cols = n.clamp(16, 420);
                        applied += 1;
                    }
                }
                "cs" => {
                    if let Some(cs) = CharsetPreset::from_cli(v) {
                        self.charset = cs;
                        applied += 1;
                    }
                }
                "cm" => {
                    if let Some(cm) = ColorMode::from_cli(v) {
                        self.color_mode = cm;
                        applied += 1;
                    }
                }
                "oled" => {
                    if let Some(f) = parse_bool(v) {
                        self.oled_black = f;
                        if f {
                            self.transparent_bg = false;
                        }
                        applied += 1;
                    }
                }
                "gate" => {
                    if let Some(x) = parse_finite_f32(v) {
                        self.black_gate = x.clamp(0.0, 0.35);
                        applied += 1;
                    }
                }
                "alpha" => {
                    if let Some(f) = parse_bool(v) {
                        self.transparent_bg = f;
                        if f {
                            self.oled_black = false;
                        }
                        applied += 1;
                    }
                }
                "wf" => {
                    if let Some(f) = parse_bool(v) {
                        self.wireframe_only = f;
                        applied += 1;
                    }
                }
                "wfs" => {
                    if let Some(x) = parse_finite_f32(v) {
                        self.wireframe_sensitivity = x.clamp(0.2, 3.0);
                        applied += 1;
                    }
                }
                "ar" => {
                    if let Some(x) = parse_finite_f32(v) {
                        self.audio_reactive = x.clamp(0.0, 2.5);
                        applied += 1;
                    }
                }
                "cr" => {
                    if let Some(x) = parse_finite_f32(v) {
                        self.directional_crunch = x.clamp(0.0, 3.5);
                        applied += 1;
                    }
                }
                "ew" => {
                    if let Some(x) = parse_finite_f32(v) {
                        self.edge_weight = x.clamp(0.0, 2.5);
                        applied += 1;
                    }
                }
                "lum" => {
                    if let Some(x) = parse_finite_f32(v) {
                        self.luminance_comp = x.clamp(0.0, 1.5);
                        applied += 1;
                    }
                }
                "bg" => {
                    if let Some(x) = parse_finite_f32(v) {
                        self.bg_intensity = x.clamp(0.0, 0.65);
                        applied += 1;
                    }
                }
                "dt" => {
                    if let Some(x) = parse_finite_f32(v) {
                        self.dither = x.clamp(0.0, 1.0);
                        applied += 1;
                    }
                }
                "vig" => {
                    if let Some(x) = parse_finite_f32(v) {
                        self.vignette_focus = x.clamp(0.0, 1.0);
                        applied += 1;
                    }
                }
                "st" => {
                    if let Some(x) = parse_finite_f32(v) {
                        self.temporal_stability = x.clamp(0.0, 0.5);
                        applied += 1;
                    }
                }
                "br" => {
                    if let Some(x) = parse_finite_f32(v) {
                        self.brightness = x.clamp(-0.5, 0.5);
                        applied += 1;
                    }
                }
                "ct" => {
                    if let Some(x) = parse_finite_f32(v) {
                        self.contrast = x.clamp(0.3, 3.0);
                        applied += 1;
                    }
                }
                "sat" => {
                    if let Some(x) = parse_finite_f32(v) {
                        self.saturation = x.clamp(0.0, 3.0);
                        applied += 1;
                    }
                }
                "gam" => {
                    if let Some(x) = parse_finite_f32(v) {
                        self.gamma = x.clamp(0.25, 3.0);
                        applied += 1;
                    }
                }
                "bl" => {
                    if let Some(f) = parse_bool(v) {
                        self.crt_bloom = f;
                        applied += 1;
                    }
                }
                "sc" => {
                    if let Some(f) = parse_bool(v) {
                        self.scanlines = f;
                        applied += 1;
                    }
                }
                "inv" => {
                    if let Some(f) = parse_bool(v) {
                        self.invert = f;
                        applied += 1;
                    }
                }
                "tri" => {
                    if let Some(pal) = TriTonePalette::from_hex_triplet(v) {
                        self.tritone = pal;
                        applied += 1;
                    }
                }
                "cc" => {
                    let h = v.trim();
                    if !h.is_empty()
                        && h.len() <= 512
                        && h.len() % 2 == 0
                        && h.bytes().all(|b| b.is_ascii_hexdigit())
                    {
                        let mut chars = String::with_capacity(h.len() / 2);
                        let mut ok = true;
                        for i in (0..h.len()).step_by(2) {
                            if let Ok(b) = u8::from_str_radix(&h[i..i + 2], 16) {
                                if (0x20..=0x7E).contains(&b) {
                                    chars.push(b as char);
                                }
                            } else {
                                ok = false;
                                break;
                            }
                        }
                        if ok && !chars.is_empty() {
                            self.custom_chars = chars;
                            applied += 1;
                        }
                    }
                }
                _ => {}
            }
        }

        if applied == 0 {
            return Err("No valid look parameters found after 'lg2:'".to_string());
        }
        Ok(())
    }
}
#[cfg(test)]
mod look_code_tests {
    // Test fixtures read more clearly as `default()` plus explicit overrides.
    #![allow(clippy::field_reassign_with_default)]

    use super::*;
    use crate::charset::CharsetPreset;
    use crate::color::{ColorMode, TriTonePalette};

    /// Look codes are untrusted input: people paste them from chat and screenshots.
    fn hostile_corpus() -> Vec<String> {
        let mut out = Vec::new();
        for len in [0usize, 1, 5, 2047, 2048, 2049, 4096, 100_000] {
            out.push("lg2:cols=".to_string() + &"9".repeat(len));
        }
        for bad in [
            "lg2:cols=NaN",
            "lg2:br=inf",
            "lg2:ct=-inf",
            "lg2:gam=NaN",
            "lg2:cols=-5",
            "lg2:cols=999999",
            "lg2:cm=../../etc/passwd",
            "lg2:tri=zzzzzz-zzzzzz-zzzzzz",
            "lg2:,,,,",
            "lg2:",
            "lg2",
            "",
            "   ",
            "lg1:cols=100",
            "cols=100",
            "\u{0}lg2:cols=10\u{0}",
            "lg2:cc=\u{0}\u{1}\u{2}",
            "lg2:cols=80,cols=90,cols=100",
            "lg2:gate=99999",
            "lg2:ar=-99999",
        ] {
            out.push(bad.to_string());
        }
        out
    }

    #[test]
    fn default_params_round_trip_through_a_look_code() {
        let original = RenderParams::default();
        let code = original.to_look_code();
        assert!(code.starts_with("lg2:"), "look code must be tagged lg2:");
        let mut parsed = RenderParams::default();
        parsed.apply_look_code(&code).unwrap();
        assert_eq!(parsed.to_look_code(), code, "round trip changed the code");
    }

    #[test]
    fn round_trip_preserves_every_field() {
        let mut p = RenderParams::default();
        p.cols = 220;
        p.charset = CharsetPreset::StructuralEdge;
        p.color_mode = ColorMode::Cyberpunk;
        p.oled_black = true;
        p.black_gate = 0.15;
        p.transparent_bg = false;
        p.wireframe_only = true;
        p.audio_reactive = 1.5;
        p.brightness = -0.2;
        p.contrast = 1.7;
        p.saturation = 0.4;
        p.gamma = 2.2;
        p.dither = 0.5;
        p.vignette_focus = 0.3;
        p.temporal_stability = 0.4;
        p.crt_bloom = true;
        p.scanlines = true;
        p.invert = true;
        p.tritone = TriTonePalette::EMERALD_ICE;

        let mut other = RenderParams::default();
        other.apply_look_code(&p.to_look_code()).unwrap();

        assert_eq!(other.cols, p.cols);
        assert_eq!(other.charset, p.charset);
        assert_eq!(other.color_mode, p.color_mode);
        assert_eq!(other.oled_black, p.oled_black);
        assert!((other.black_gate - p.black_gate).abs() < 1e-3);
        assert_eq!(other.wireframe_only, p.wireframe_only);
        assert!((other.audio_reactive - p.audio_reactive).abs() < 0.01);
        assert!((other.brightness - p.brightness).abs() < 0.01);
        assert!((other.contrast - p.contrast).abs() < 0.01);
        assert!((other.saturation - p.saturation).abs() < 0.01);
        assert!((other.gamma - p.gamma).abs() < 0.01);
        assert!((other.dither - p.dither).abs() < 0.01);
        assert!((other.vignette_focus - p.vignette_focus).abs() < 0.01);
        assert!(other.crt_bloom && other.scanlines && other.invert);
        assert_eq!(other.tritone, p.tritone);
    }

    #[test]
    fn hostile_input_never_panics() {
        for raw in hostile_corpus() {
            let mut p = RenderParams::default();
            // Any result is acceptable; crashing is not.
            let _ = p.apply_look_code(&raw);
        }
    }

    #[test]
    fn oversized_codes_are_rejected_without_being_parsed() {
        let huge = "lg2:cols=100,".repeat(200); // ~2.6 KB
        let mut p = RenderParams::default();
        let before = p.cols;
        let err = p.apply_look_code(&huge).unwrap_err();
        assert!(
            err.contains("length") || err.contains("exceed"),
            "got: {err}"
        );
        assert_eq!(p.cols, before, "a rejected code must not mutate the params");
    }

    #[test]
    fn non_finite_numbers_are_ignored() {
        let mut p = RenderParams::default();
        let defaults = p.to_look_code();
        let _ = p.apply_look_code("lg2:br=NaN,ct=inf,gam=-inf,sat=NaN");
        assert_eq!(
            p.to_look_code(),
            defaults,
            "NaN/inf leaked into the parameters"
        );
        assert!(p.brightness.is_finite() && p.contrast.is_finite() && p.gamma.is_finite());
    }

    #[test]
    fn numeric_fields_clamp_to_their_documented_ranges() {
        let mut p = RenderParams::default();
        p.apply_look_code("lg2:cols=999999,gate=99,ar=-99,ct=99999")
            .unwrap();
        assert!(
            (16..=420).contains(&p.cols),
            "cols escaped its range: {}",
            p.cols
        );
        assert!((0.0..=0.35).contains(&p.black_gate));
        assert!(p.audio_reactive >= 0.0);
        assert!(p.contrast.is_finite());
    }

    #[test]
    fn unknown_keys_are_skipped_but_known_keys_still_apply() {
        let mut p = RenderParams::default();
        p.apply_look_code("lg2:totally_fake=1,cols=99,another=xyz")
            .unwrap();
        assert_eq!(p.cols, 99);
    }

    #[test]
    fn missing_prefix_is_rejected() {
        let mut p = RenderParams::default();
        assert!(p.apply_look_code("cols=100,cm=mono").is_err());
        assert!(p.apply_look_code("lg3:cols=100").is_err());
    }

    #[test]
    fn lg1_codes_are_still_accepted_for_forward_compatibility() {
        let mut p = RenderParams::default();
        p.apply_look_code("lg1:cols=120").unwrap();
        assert_eq!(p.cols, 120);
    }

    #[test]
    fn codes_wrapped_in_quotes_or_backticks_are_accepted() {
        for raw in [
            "`lg2:cols=100`",
            "\"lg2:cols=100\"",
            "'lg2:cols=100'",
            "  lg2:cols=100  ",
        ] {
            let mut p = RenderParams::default();
            p.apply_look_code(raw).unwrap();
            assert_eq!(p.cols, 100, "failed to unwrap {raw:?}");
        }
    }

    #[test]
    fn oled_and_transparency_stay_mutually_exclusive() {
        let mut p = RenderParams::default();
        p.apply_look_code("lg2:oled=1").unwrap();
        assert!(p.oled_black && !p.transparent_bg);
        p.apply_look_code("lg2:alpha=1").unwrap();
        assert!(p.transparent_bg && !p.oled_black, "alpha and OLED both on");
    }

    #[test]
    fn control_bytes_never_reach_the_output_code() {
        for raw in hostile_corpus() {
            let mut p = RenderParams::default();
            if p.apply_look_code(&raw).is_ok() {
                let code = p.to_look_code();
                assert!(
                    !code.bytes().any(|b| b < 0x20 || b == 0x7F),
                    "control byte survived into {code:?}"
                );
            }
        }
    }

    #[test]
    fn duplicate_keys_let_the_last_one_win() {
        let mut p = RenderParams::default();
        p.apply_look_code("lg2:cols=80,cols=200,cols=120").unwrap();
        assert_eq!(p.cols, 120);
    }

    #[test]
    fn custom_charset_survives_a_round_trip() {
        let mut p = RenderParams::default();
        p.charset = CharsetPreset::Custom;
        p.custom_chars = " .:-=+*#%@".to_string();
        let mut other = RenderParams::default();
        other.apply_look_code(&p.to_look_code()).unwrap();
        assert_eq!(other.charset, CharsetPreset::Custom);
        assert_eq!(
            other.custom_chars, p.custom_chars,
            "the custom glyph set must survive a share"
        );
    }
}
