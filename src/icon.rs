use std::sync::Arc;

/// Embedded 256x256 RGBA PNG application icon used at runtime for window titlebars,
/// Windows taskbars, macOS Dock, Linux X11/Wayland panels, and the About dialog.
pub const EMBEDDED_ICON_PNG_256: &[u8] = include_bytes!("../assets/icons/lumaglyph-256.png");

/// Decodes the embedded 256x256 RGBA PNG icon into `(rgba_bytes, width, height)`.
pub fn decode_app_icon_rgba() -> Result<(Vec<u8>, u32, u32), String> {
    let img = image::load_from_memory_with_format(EMBEDDED_ICON_PNG_256, image::ImageFormat::Png)
        .map_err(|e| format!("Failed decoding embedded application icon: {e}"))?
        .into_rgba8();
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return Err("Decoded application icon has zero dimensions".to_string());
    }
    Ok((img.into_raw(), w, h))
}

/// Constructs `Arc<egui::IconData>` for `egui::ViewportBuilder::with_icon`.
pub fn eframe_icon_data() -> Arc<egui::IconData> {
    match decode_app_icon_rgba() {
        Ok((rgba, width, height)) => Arc::new(egui::IconData {
            rgba,
            width,
            height,
        }),
        Err(_) => Arc::new(egui::IconData::default()),
    }
}

/// Constructs an `egui::ColorImage` from the embedded 256x256 RGBA icon for UI rendering.
pub fn egui_logo_color_image() -> Option<egui::ColorImage> {
    let (rgba, w, h) = decode_app_icon_rgba().ok()?;
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [w as usize, h as usize],
        &rgba,
    ))
}
