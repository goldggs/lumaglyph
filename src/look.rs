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
