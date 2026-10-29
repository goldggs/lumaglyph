pub mod charset;
pub mod cli;
pub mod color;
pub mod engine;
pub mod export;
pub mod glyph_atlas;
pub mod gui;
pub mod icon;
pub mod look;
pub mod telemetry;
pub mod theme;
pub mod updater;
pub mod video;

#[cfg(test)]
mod tests {
    use crate::charset::{CharsetPreset, ASCII_COUNT};
    use crate::color::{ColorMode, TriTonePalette, OLED_BG};
    use crate::engine::{
        convert_image_buffer, convert_image_buffer_with_audio, rasterize_rgb24_even,
        rasterize_rgba, AsciiFrame, RenderParams,
    };
    use crate::glyph_atlas::GlyphAtlas;
    use crate::theme::{
        resolve_palette, AccentPreset, CornerStyle, StudioTheme, JETBRAINS_MONO_REGULAR,
        PLEX_SANS_MEDIUM, PLEX_SANS_REGULAR,
    };
    use crate::updater::{
        format_build_report, is_newer_semver, AppPreferences, APP_VERSION, BUILD_GIT_SHA,
        BUILD_TARGET_TRIPLE,
    };

    fn test_pattern_rgba(w: u32, h: u32) -> Vec<u8> {
        let mut out = vec![0u8; (w * h * 4) as usize];
        let wf = w.max(1) as f32;
        let hf = h.max(1) as f32;
        for y in 0..h {
            let ny = (y as f32 / hf) * 2.0 - 1.0;
            for x in 0..w {
                let nx = (x as f32 / wf) * 2.0 - 1.0;
                let r = (nx * nx + ny * ny).sqrt();
                let angle = ny.atan2(nx);
                let spoke = ((angle * 11.0).sin() * 4.0).clamp(-1.0, 1.0) * 0.5 + 0.5;
                let rings = ((r * 26.0).cos() * 3.5).clamp(-1.0, 1.0) * 0.5 + 0.5;
                let grid_x = ((nx * 19.0).sin() * 5.0).clamp(-1.0, 1.0) * 0.5 + 0.5;
                let grid_y = ((ny * 17.0).cos() * 5.0).clamp(-1.0, 1.0) * 0.5 + 0.5;
                let ramp = ((nx + 1.0) * 0.5) * 0.65 + ((1.0 - ny) * 0.5) * 0.35;
                let mix =
                    (rings * spoke * 0.55 + grid_x * grid_y * 0.30 + ramp * 0.35).clamp(0.0, 1.0);
                let v0 = (mix * ramp + rings * 0.25).clamp(0.0, 1.0);
                let v1 = (grid_x * spoke * 0.70 + ramp * 0.30).clamp(0.0, 1.0);
                let v2 = ((1.0 - (r * 0.85).min(1.0)) * grid_y * 0.80 + mix * 0.20).clamp(0.0, 1.0);
                let i = ((y * w + x) * 4) as usize;
                out[i] = (v0 * 255.0) as u8;
                out[i + 1] = (v1 * 255.0) as u8;
                out[i + 2] = (v2 * 255.0) as u8;
                out[i + 3] = 255;
            }
        }
        out
    }

    #[test]
    fn embedded_font_calibrates_all_95_printable_ascii_glyphs() {
        let atlas = GlyphAtlas::default_embedded(20);
        assert!(atlas.cell_w >= 8 && atlas.cell_h == 20);
        assert_eq!(atlas.coverage[0], 0.0);
        let at_idx = (b'@' - 0x20) as usize;
        let slash_idx = (b'/' - 0x20) as usize;
        let bslash_idx = (b'\\' - 0x20) as usize;
        assert!(atlas.coverage[at_idx] > 0.45);
        assert!(atlas.vectors[slash_idx].v[7] * atlas.vectors[bslash_idx].v[7] < 0.0);
        assert_eq!(atlas.vectors.len(), ASCII_COUNT);
    }

    #[test]
    fn embedded_ibm_plex_sans_and_jetbrains_mono_are_valid_fonts() {
        assert!(
            fontdue::Font::from_bytes(PLEX_SANS_REGULAR, fontdue::FontSettings::default()).is_ok()
        );
        assert!(
            fontdue::Font::from_bytes(PLEX_SANS_MEDIUM, fontdue::FontSettings::default()).is_ok()
        );
        assert!(fontdue::Font::from_bytes(
            JETBRAINS_MONO_REGULAR,
            fontdue::FontSettings::default()
        )
        .is_ok());
    }

    #[test]
    fn workstation_themes_and_preferences_backward_compat() {
        for t in StudioTheme::ALL {
            assert_eq!(StudioTheme::from_cli(t.cli_name()), Some(t));
            for a in AccentPreset::ALL {
                let pal = resolve_palette(t, a, CornerStyle::Precision2px);
                assert_eq!(pal.is_light, t.is_light());
            }
        }

        // Legacy v0.1 / v0.2 JSON must deserialize cleanly into v0.3 AppPreferences
        let legacy_json = r#"{"check_updates_on_startup":false,"github_repo":"goldggs/lumaglyph"}"#;
        let prefs: AppPreferences = serde_json::from_str(legacy_json).unwrap();
        assert_eq!(prefs.theme, StudioTheme::GraphitePro);
        assert_eq!(prefs.default_cols, 156);
        assert_eq!(prefs.video_crf, 18);
        assert!(!prefs.check_updates_on_startup);
    }

    #[test]
    fn build_metadata_is_populated_and_path_free() {
        assert_eq!(APP_VERSION, "0.3.8");
        assert!(!BUILD_GIT_SHA.is_empty());
        assert!(!BUILD_TARGET_TRIPLE.is_empty());
        let report = format_build_report(Some("Test GPU"), Some("ffmpeg 7.1"), "8.0 MB");
        assert!(report.contains("LumaGlyph v0.3.8"));
        assert!(!report.contains("/home/"));
        assert!(!report.contains("/Users/"));
        assert!(!report.contains("C:\\Users\\"));
    }

    #[test]
    fn embedded_application_icon_decodes_to_rgba_256() {
        let (rgba, w, h) = crate::icon::decode_app_icon_rgba().expect("embedded icon must decode");
        assert_eq!((w, h), (256, 256));
        assert_eq!(rgba.len(), 256 * 256 * 4);
        assert_eq!(rgba[3], 0, "top-left corner must be transparent");
        let opaque_pixels = rgba.chunks_exact(4).filter(|px| px[3] > 200).count();
        assert!(
            opaque_pixels > 4_000,
            "icon must contain crisp LG glyph pixels"
        );
    }

    #[test]
    fn genuine95_uses_wide_character_diversity_without_reallocating() {
        let atlas = GlyphAtlas::default_embedded(20);
        let rgba = test_pattern_rgba(640, 400);
        let params = RenderParams {
            cols: 140,
            charset: CharsetPreset::Genuine95,
            ..RenderParams::default()
        };
        let mut frame = AsciiFrame::new();
        convert_image_buffer(&rgba, 640, 400, 4, &atlas, &params, false, &mut frame);

        assert!(
            frame.unique_glyphs >= 35,
            "expected rich ASCII diversity, got {}",
            frame.unique_glyphs
        );

        let ptr_before = frame.cells.as_ptr();
        convert_image_buffer(&rgba, 640, 400, 4, &atlas, &params, true, &mut frame);
        assert_eq!(
            ptr_before,
            frame.cells.as_ptr(),
            "frame workspace must reuse cell buffer with zero reallocation"
        );
    }

    #[test]
    fn oled_black_and_shadow_noise_gate_produce_exact_pitch_black() {
        let atlas = GlyphAtlas::default_embedded(20);
        let w = 160u32;
        let h = 100u32;
        let mut rgb = vec![6u8; (w * h * 3) as usize];
        for y in 40..60 {
            for x in 50..110 {
                let i = ((y * w + x) * 3) as usize;
                rgb[i] = 40;
                rgb[i + 1] = 240;
                rgb[i + 2] = 255;
            }
        }
        let params = RenderParams {
            cols: 60,
            oled_black: true,
            black_gate: 0.08,
            ..RenderParams::default()
        };
        let mut frame = AsciiFrame::new();
        convert_image_buffer(&rgb, w, h, 3, &atlas, &params, false, &mut frame);

        assert_eq!(frame.cells[0].ch, b' ');
        assert_eq!(frame.cells[0].bg, OLED_BG);

        let mut out_rgba = Vec::new();
        rasterize_rgba(&frame, &atlas, &params, 0.0, &mut out_rgba);
        assert_eq!(&out_rgba[0..4], &[0, 0, 0, 255]);

        let mut out_rgb24 = Vec::new();
        rasterize_rgb24_even(&frame, &atlas, &params, 0.0, &mut out_rgb24);
        assert_eq!(&out_rgb24[0..3], &[0, 0, 0]);
    }

    #[test]
    fn transparent_alpha_and_neon_wireframe_cut_out_interiors() {
        let atlas = GlyphAtlas::default_embedded(20);
        let w = 240u32;
        let h = 160u32;
        let mut rgb = vec![0u8; (w * h * 3) as usize];
        for y in 30..130 {
            for x in 40..200 {
                let i = ((y * w + x) * 3) as usize;
                rgb[i] = 245;
                rgb[i + 1] = 245;
                rgb[i + 2] = 245;
            }
        }
        let params = RenderParams {
            cols: 80,
            transparent_bg: true,
            wireframe_only: true,
            wireframe_sensitivity: 1.2,
            ..RenderParams::default()
        };
        let mut frame = AsciiFrame::new();
        convert_image_buffer(&rgb, w, h, 3, &atlas, &params, false, &mut frame);

        let mid_c = (frame.cols / 2) as usize;
        let mid_r = (frame.rows / 2) as usize;
        assert_eq!(
            frame.cells[mid_r * frame.cols as usize + mid_c].ch,
            b' ',
            "flat interior must be empty in wireframe mode"
        );
        let non_space = frame.cells.iter().filter(|c| c.ch != b' ').count();
        assert!(non_space > 20, "expected active wireframe contour cells");

        let mut rgba = Vec::new();
        rasterize_rgba(&frame, &atlas, &params, 0.0, &mut rgba);
        assert_eq!(&rgba[0..4], &[0, 0, 0, 0]);
        assert!(
            rgba.chunks_exact(4).any(|px| px[3] > 180),
            "contour ink pixels must have opaque alpha"
        );
    }

    #[test]
    fn tritone_ramp_and_shareable_look_code_roundtrip() {
        let pal = TriTonePalette::from_hex_triplet("#120428,#00E5FF,#FFE680").unwrap();
        assert_eq!(pal.to_hex_triplet(), "120428-00E5FF-FFE680");

        let orig = RenderParams {
            cols: 192,
            charset: CharsetPreset::Custom,
            custom_chars: " .:/\\|@#".to_string(),
            color_mode: ColorMode::TriTone,
            tritone: pal,
            oled_black: true,
            black_gate: 0.07,
            wireframe_only: true,
            wireframe_sensitivity: 1.35,
            audio_reactive: 0.85,
            crt_bloom: true,
            ..RenderParams::default()
        };
        let code = orig.to_look_code();
        assert!(code.starts_with("lg2:"));

        let mut restored = RenderParams::default();
        restored.apply_look_code(&code).unwrap();
        assert_eq!(restored.cols, 192);
        assert_eq!(restored.charset, CharsetPreset::Custom);
        assert_eq!(restored.custom_chars, " .:/\\|@#");
        assert_eq!(restored.color_mode, ColorMode::TriTone);
        assert_eq!(restored.tritone, pal);
        assert!(restored.oled_black);
        assert!(restored.wireframe_only);
        assert!(restored.crt_bloom);
        assert!((restored.black_gate - 0.07).abs() < 0.01);
        assert!((restored.audio_reactive - 0.85).abs() < 0.01);
    }

    #[test]
    fn audio_reactive_beat_modulates_glyph_luminance() {
        let atlas = GlyphAtlas::default_embedded(20);
        let rgba = test_pattern_rgba(320, 200);
        let params = RenderParams {
            cols: 80,
            audio_reactive: 1.25,
            ..RenderParams::default()
        };
        let mut calm = AsciiFrame::new();
        let mut peak = AsciiFrame::new();
        convert_image_buffer_with_audio(&rgba, 320, 200, 4, &atlas, &params, false, 0.0, &mut calm);
        convert_image_buffer_with_audio(&rgba, 320, 200, 4, &atlas, &params, false, 1.0, &mut peak);

        let sum_calm: u64 = calm
            .cells
            .iter()
            .map(|c| c.fg[0] as u64 + c.fg[1] as u64 + c.fg[2] as u64)
            .sum();
        let sum_peak: u64 = peak
            .cells
            .iter()
            .map(|c| c.fg[0] as u64 + c.fg[1] as u64 + c.fg[2] as u64)
            .sum();
        assert!(
            sum_peak > sum_calm,
            "beat peak ({sum_peak}) should boost foreground energy above calm ({sum_calm})"
        );
    }

    #[test]
    fn semver_release_comparison_is_accurate() {
        assert!(is_newer_semver("v0.3.0", "0.2.0"));
        assert!(is_newer_semver("0.4.0", "0.3.0"));
        assert!(is_newer_semver("1.0.0", "0.9.9"));
        assert!(!is_newer_semver("v0.3.0", "0.3.0"));
        assert!(!is_newer_semver("v0.2.0", "0.3.0"));
    }
}
