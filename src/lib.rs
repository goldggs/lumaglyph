pub mod charset;
pub mod cli;
pub mod color;
pub mod engine;
pub mod export;
pub mod glyph_atlas;
pub mod gui;
pub mod look;
pub mod samples;
pub mod telemetry;
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
    use crate::samples::{generate_sample_rgba, SamplePreset};
    use crate::updater::is_newer_semver;

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
    fn genuine95_uses_wide_character_diversity_without_reallocating() {
        let atlas = GlyphAtlas::default_embedded(20);
        let rgba = generate_sample_rgba(SamplePreset::CyberSculpture, 640, 400, 0.0);
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
        // Simulate noisy dark video frame (`RGB = [5, 6, 7]`) with a bright neon center bar
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

        // Top-left cell must be gated cleanly to `' '` + `[0, 0, 0]`
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
        // Large bright white square on dark background: interior is flat, border is sharp contour
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

        // Center of the solid white square must be dropped to `' '` by wireframe mode!
        let mid_c = (frame.cols / 2) as usize;
        let mid_r = (frame.rows / 2) as usize;
        assert_eq!(
            frame.cells[mid_r * frame.cols as usize + mid_c].ch,
            b' ',
            "flat interior must be empty in wireframe mode"
        );
        // Meanwhile contour edges must have active non-space strokes
        let non_space = frame.cells.iter().filter(|c| c.ch != b' ').count();
        assert!(non_space > 20, "expected active wireframe contour cells");

        let mut rgba = Vec::new();
        rasterize_rgba(&frame, &atlas, &params, 0.0, &mut rgba);
        // Top-left corner pixel must be 100% transparent `[0, 0, 0, 0]`
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
        let rgba = generate_sample_rgba(SamplePreset::SynthwaveHorizon, 320, 200, 0.0);
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
        assert!(is_newer_semver("v0.2.0", "0.1.0"));
        assert!(is_newer_semver("0.3.0", "0.2.0"));
        assert!(is_newer_semver("1.0.0", "0.9.9"));
        assert!(!is_newer_semver("v0.2.0", "0.2.0"));
        assert!(!is_newer_semver("v0.1.0", "0.2.0"));
    }
}
