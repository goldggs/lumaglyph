pub mod charset;
pub mod cli;
pub mod color;
pub mod engine;
pub mod export;
pub mod glyph_atlas;
pub mod gui;
pub mod samples;
pub mod telemetry;
pub mod updater;
pub mod video;

#[cfg(test)]
mod tests {
    use crate::charset::{CharsetPreset, ASCII_COUNT};
    use crate::engine::{convert_image_buffer, AsciiFrame, RenderParams};
    use crate::glyph_atlas::GlyphAtlas;
    use crate::samples::{generate_sample_rgba, SamplePreset};
    use crate::updater::is_newer_semver;

    #[test]
    fn embedded_font_calibrates_all_95_printable_ascii_glyphs() {
        let atlas = GlyphAtlas::default_embedded(20);
        assert!(atlas.cell_w >= 8 && atlas.cell_h == 20);
        // Space (' ') has zero ink coverage
        assert_eq!(atlas.coverage[0], 0.0);
        // Dense characters ('@', '#', 'W') have strong non-zero coverage
        let at_idx = (b'@' - 0x20) as usize;
        let slash_idx = (b'/' - 0x20) as usize;
        let bslash_idx = (b'\\' - 0x20) as usize;
        assert!(atlas.coverage[at_idx] > 0.45);
        // Forward slash '/' and backslash '\' must have opposite diagonal orientation signs (`v[7]`)
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

        // Genuine 95 shape matching should utilize a wide range of printable ASCII (well over 35 unique chars)
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
    fn semver_release_comparison_is_accurate() {
        assert!(is_newer_semver("v0.2.0", "0.1.0"));
        assert!(is_newer_semver("1.0.0", "0.9.9"));
        assert!(!is_newer_semver("v0.1.0", "0.1.0"));
        assert!(!is_newer_semver("v0.0.9", "0.1.0"));
    }
}
