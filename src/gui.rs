use crate::charset::{CharsetPreset, ASCII_COUNT, ASCII_MIN};
use crate::color::{ColorMode, TriTonePalette};
use crate::engine::{convert_image_buffer_with_audio, rasterize_rgba, AsciiFrame, RenderParams};
use crate::export::{
    export_ansi, export_html, export_png, export_svg, export_txt, frame_to_ansi_string,
};
use crate::glyph_atlas::GlyphAtlas;
use crate::samples::{generate_sample_rgba, SamplePreset};
use crate::telemetry::format_rss_mb;
use crate::updater::{spawn_release_check, AppPreferences, ReleaseCheckOutcome, APP_VERSION};
use crate::video::{
    detect_ffmpeg_version, transcode_video_streaming, LiveVideoStream, TranscodeProgress,
    TranscodeSummary,
};
use eframe::egui::{
    self, Align, Color32, ColorImage, CornerRadius, FontFamily, FontId, Frame, Layout, Margin,
    Pos2, Rect, RichText, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Vec2,
};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone, Default)]
pub struct GuiBootOptions {
    pub initial_media: Option<PathBuf>,
    pub initial_preset: Option<SamplePreset>,
    pub initial_params: Option<RenderParams>,
    pub split_wipe: Option<f32>,
    pub initial_zoom: Option<f32>,
    pub live_motion: bool,
    pub show_update_modal: bool,
    /// Optional automated screenshot path: renders N frames so GPU textures settle, captures/exits.
    pub auto_exit_after_frames: Option<u32>,
}

enum SourceKind {
    Sample(SamplePreset),
    StaticImage { name: String },
    LiveMotion(SamplePreset),
    VideoFile(LiveVideoStream),
}

struct ActiveTranscodeJob {
    output_path: PathBuf,
    progress: Arc<TranscodeProgress>,
    rx: Receiver<Result<TranscodeSummary, String>>,
}

pub struct LumaGlyphApp {
    atlas: GlyphAtlas,
    params: RenderParams,
    look_code_buf: String,
    source_kind: SourceKind,
    source_rgba: Vec<u8>,
    source_channels: usize,
    source_w: u32,
    source_h: u32,
    source_dirty: bool,
    ascii_dirty: bool,

    ascii_frame: AsciiFrame,
    raster_rgba: Vec<u8>,
    ascii_tex: Option<TextureHandle>,
    orig_tex: Option<TextureHandle>,
    ascii_px_w: usize,
    ascii_px_h: usize,

    // Viewport state
    split_enabled: bool,
    split_ratio: f32,
    zoom: f32,
    auto_fit: bool,
    pan_offset: Vec2,

    // Motion, audio-reactive & video state
    motion_paused: bool,
    motion_start: Instant,
    live_audio_energy: f32,
    video_frames_seen: u64,
    ffmpeg_info: Option<String>,
    transcode_job: Option<ActiveTranscodeJob>,
    export_png_scale_2x: bool,

    // Telemetry & status
    rss_label: String,
    last_rss_poll: Instant,
    status_toast: Option<(String, Instant)>,

    // Offline-by-default preferences & opt-in GitHub release notifier
    prefs: AppPreferences,
    show_update_window: bool,
    update_rx: Option<Receiver<Result<ReleaseCheckOutcome, String>>>,
    update_status: Option<Result<ReleaseCheckOutcome, String>>,

    // Dev helper for automated CI/demo window capture
    frames_drawn: u32,
    auto_exit_after_frames: Option<u32>,
}

impl LumaGlyphApp {
    pub fn new(cc: &eframe::CreationContext<'_>, boot: GuiBootOptions) -> Self {
        apply_studio_theme(&cc.egui_ctx);

        let atlas = GlyphAtlas::default_embedded(20);
        let params = boot.initial_params.unwrap_or_default();
        let look_code_buf = params.to_look_code();
        let preset = boot.initial_preset.unwrap_or(SamplePreset::CyberSculpture);

        let sample_w = 960;
        let sample_h = 600;
        let source_rgba = generate_sample_rgba(preset, sample_w, sample_h, 0.0);

        let prefs = AppPreferences::load();
        let update_rx = if prefs.check_updates_on_startup {
            Some(spawn_release_check(prefs.github_repo.clone()))
        } else {
            None
        };

        let mut app = Self {
            atlas,
            params,
            look_code_buf,
            source_kind: if boot.live_motion {
                SourceKind::LiveMotion(preset)
            } else {
                SourceKind::Sample(preset)
            },
            source_rgba,
            source_channels: 4,
            source_w: sample_w,
            source_h: sample_h,
            source_dirty: true,
            ascii_dirty: true,
            ascii_frame: AsciiFrame::new(),
            raster_rgba: Vec::new(),
            ascii_tex: None,
            orig_tex: None,
            ascii_px_w: 0,
            ascii_px_h: 0,
            split_enabled: boot.split_wipe != Some(0.0),
            split_ratio: boot.split_wipe.unwrap_or(0.34),
            zoom: boot.initial_zoom.unwrap_or(1.0),
            auto_fit: boot.initial_zoom.is_none(),
            pan_offset: Vec2::ZERO,
            motion_paused: false,
            motion_start: Instant::now(),
            live_audio_energy: 0.0,
            video_frames_seen: 0,
            ffmpeg_info: detect_ffmpeg_version(),
            transcode_job: None,
            export_png_scale_2x: true,
            rss_label: format_rss_mb(),
            last_rss_poll: Instant::now(),
            status_toast: None,
            prefs,
            show_update_window: boot.show_update_modal,
            update_rx,
            update_status: None,
            frames_drawn: 0,
            auto_exit_after_frames: boot.auto_exit_after_frames,
        };

        if let Some(path) = boot.initial_media {
            app.load_media_path(path);
        }

        app
    }

    fn set_toast(&mut self, msg: impl Into<String>) {
        self.status_toast = Some((msg.into(), Instant::now()));
    }

    fn sync_look_code(&mut self) {
        self.look_code_buf = self.params.to_look_code();
    }

    fn load_sample(&mut self, preset: SamplePreset, animated: bool) {
        let w = 960;
        let h = 600;
        self.source_rgba = generate_sample_rgba(preset, w, h, 0.0);
        self.source_channels = 4;
        self.source_w = w;
        self.source_h = h;
        self.live_audio_energy = 0.0;
        self.source_kind = if animated {
            self.motion_start = Instant::now();
            self.motion_paused = false;
            SourceKind::LiveMotion(preset)
        } else {
            SourceKind::Sample(preset)
        };
        self.ascii_frame.prev_glyphs.clear();
        self.source_dirty = true;
        self.ascii_dirty = true;
    }

    fn load_media_path(&mut self, path: PathBuf) {
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let is_video = matches!(ext.as_str(), "mp4" | "mkv" | "mov" | "webm" | "avi" | "m4v");

        if is_video {
            match LiveVideoStream::open(&path) {
                Ok(stream) => {
                    let fname = path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("video")
                        .to_string();
                    let has_audio = !stream.audio_envelope.is_empty();
                    self.source_w = stream.dec_w;
                    self.source_h = stream.dec_h;
                    self.source_channels = 3;
                    self.source_rgba = vec![0u8; (stream.dec_w * stream.dec_h * 3) as usize];
                    self.source_kind = SourceKind::VideoFile(stream);
                    self.video_frames_seen = 0;
                    self.live_audio_energy = 0.0;
                    self.ascii_frame.prev_glyphs.clear();
                    self.source_dirty = true;
                    self.ascii_dirty = true;
                    let audio_note = if has_audio {
                        "audio envelope synced"
                    } else {
                        "silent stream"
                    };
                    self.set_toast(format!("Streaming video: {fname} ({audio_note})"));
                }
                Err(e) => self.set_toast(e),
            }
        } else {
            match image::open(&path) {
                Ok(img) => {
                    let rgba = img.to_rgba8();
                    let (w, h) = rgba.dimensions();
                    let fname = path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("image")
                        .to_string();
                    self.source_w = w;
                    self.source_h = h;
                    self.source_channels = 4;
                    self.source_rgba = rgba.into_raw();
                    self.live_audio_energy = 0.0;
                    self.source_kind = SourceKind::StaticImage {
                        name: fname.clone(),
                    };
                    self.ascii_frame.prev_glyphs.clear();
                    self.source_dirty = true;
                    self.ascii_dirty = true;
                    self.set_toast(format!("Loaded {fname} ({w}x{h})"));
                }
                Err(e) => self.set_toast(format!("Image load error: {e}")),
            }
        }
    }

    fn refresh_pipelines(&mut self, ctx: &egui::Context) {
        // 1. Poll live motion or streaming video frame (+ synchronized audio beat energy)
        match &mut self.source_kind {
            SourceKind::LiveMotion(preset) => {
                if !self.motion_paused {
                    let t = self.motion_start.elapsed().as_secs_f32();
                    self.source_rgba =
                        generate_sample_rgba(*preset, self.source_w, self.source_h, t);
                    self.source_channels = 4;
                    // Synthetic 124-BPM studio pulse when testing Audio-Reactive on procedural scenes
                    if self.params.audio_reactive > 0.001 {
                        let phase = (t * (124.0 / 60.0)).fract();
                        self.live_audio_energy = (1.0 - phase).powi(4);
                    } else {
                        self.live_audio_energy = 0.0;
                    }
                    self.source_dirty = true;
                    self.ascii_dirty = true;
                    ctx.request_repaint();
                }
            }
            SourceKind::VideoFile(stream) => {
                if let Some((rgb, audio_e)) = stream.poll_latest_frame() {
                    self.source_rgba = rgb;
                    self.source_channels = 3;
                    self.live_audio_energy = audio_e;
                    self.video_frames_seen += 1;
                    self.source_dirty = true;
                    self.ascii_dirty = true;
                }
                if !stream.paused.load(Ordering::Relaxed) {
                    ctx.request_repaint_after(std::time::Duration::from_millis(14));
                }
            }
            _ => {
                self.live_audio_energy = 0.0;
            }
        }

        // 2. Upload original source texture ONLY when split-wipe is enabled and source changed
        if self.split_enabled && (self.source_dirty || self.orig_tex.is_none()) {
            let color_img = if self.source_channels == 4 {
                ColorImage::from_rgba_unmultiplied(
                    [self.source_w as usize, self.source_h as usize],
                    &self.source_rgba,
                )
            } else {
                ColorImage::from_rgb(
                    [self.source_w as usize, self.source_h as usize],
                    &self.source_rgba,
                )
            };
            match &mut self.orig_tex {
                Some(tex) => tex.set(color_img, TextureOptions::LINEAR),
                None => {
                    self.orig_tex =
                        Some(ctx.load_texture("orig_media", color_img, TextureOptions::LINEAR));
                }
            }
            self.source_dirty = false;
        }

        // 3. Recompute genuine 95-char shape-vector ASCII + rasterize atlas to single GPU texture
        if self.ascii_dirty || self.ascii_tex.is_none() {
            self.sync_look_code();
            let use_temporal = matches!(
                self.source_kind,
                SourceKind::LiveMotion(_) | SourceKind::VideoFile(_)
            );
            convert_image_buffer_with_audio(
                &self.source_rgba,
                self.source_w,
                self.source_h,
                self.source_channels,
                &self.atlas,
                &self.params,
                use_temporal,
                self.live_audio_energy,
                &mut self.ascii_frame,
            );
            let bloom_boost = self.live_audio_energy * self.params.audio_reactive * 0.65;
            let (pw, ph) = rasterize_rgba(
                &self.ascii_frame,
                &self.atlas,
                &self.params,
                bloom_boost,
                &mut self.raster_rgba,
            );
            self.ascii_px_w = pw;
            self.ascii_px_h = ph;

            if pw > 0 && ph > 0 {
                let cimg = ColorImage::from_rgba_unmultiplied([pw, ph], &self.raster_rgba);
                let opts = if self.zoom >= 1.8 {
                    TextureOptions::NEAREST
                } else {
                    TextureOptions::LINEAR
                };
                match &mut self.ascii_tex {
                    Some(tex) => tex.set(cimg, opts),
                    None => {
                        self.ascii_tex = Some(ctx.load_texture("ascii_canvas", cimg, opts));
                    }
                }
            }
            self.ascii_dirty = false;
        }

        // 4. Poll background video transcode job
        if let Some(job) = &self.transcode_job {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
            if let Ok(result) = job.rx.try_recv() {
                let out_name = job
                    .output_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("video")
                    .to_string();
                match result {
                    Ok(sum) => self.set_toast(format!(
                        "Exported {out_name}: {} frames in {:.1}s ({:.1} FPS)",
                        sum.frames_rendered, sum.elapsed_secs, sum.avg_fps
                    )),
                    Err(e) => self.set_toast(format!("Video export error: {e}")),
                }
                self.transcode_job = None;
            }
        }

        // 5. Poll opt-in GitHub release check
        if let Some(rx) = &self.update_rx {
            if let Ok(res) = rx.try_recv() {
                if let Ok(ref ok) = res {
                    if ok.update_available {
                        self.set_toast(format!(
                            "New GitHub release available: {} (current v{})",
                            ok.latest_tag, APP_VERSION
                        ));
                    }
                }
                self.update_status = Some(res);
                self.update_rx = None;
            }
        }

        // 6. Refresh low-overhead RSS telemetry every 600ms
        if self.last_rss_poll.elapsed().as_millis() > 600 {
            self.rss_label = format_rss_mb();
            self.last_rss_poll = Instant::now();
        }
    }

    fn draw_top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 7.0;

            // Brand badge
            Frame::new()
                .fill(Color32::from_rgb(20, 29, 44))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(56, 189, 248)))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::symmetric(10, 4))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("LUMAGLYPH")
                            .strong()
                            .color(Color32::from_rgb(125, 211, 252))
                            .size(13.5),
                    );
                });

            ui.label(
                RichText::new(format!("v{APP_VERSION}"))
                    .color(Color32::from_rgb(120, 132, 156))
                    .size(11.5),
            );

            ui.separator();

            if ui.button(RichText::new("Open Image").size(12.0)).clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Images", &["png", "jpg", "jpeg", "webp", "gif", "bmp"])
                    .pick_file()
                {
                    self.load_media_path(path);
                }
            }

            if ui.button(RichText::new("Open Video").size(12.0)).clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Videos", &["mp4", "mkv", "mov", "webm", "avi", "m4v"])
                    .pick_file()
                {
                    self.load_media_path(path);
                }
            }

            egui::ComboBox::from_id_salt("showcase_combo")
                .selected_text("Scenes")
                .width(105.0)
                .show_ui(ui, |ui| {
                    for p in SamplePreset::ALL {
                        if ui.selectable_label(false, p.label()).clicked() {
                            self.load_sample(p, false);
                        }
                    }
                    ui.separator();
                    for p in SamplePreset::ALL {
                        if ui
                            .selectable_label(false, format!("Live Motion: {}", p.label()))
                            .clicked()
                        {
                            self.load_sample(p, true);
                        }
                    }
                });

            let is_live = matches!(self.source_kind, SourceKind::LiveMotion(_));
            if ui
                .selectable_label(is_live, RichText::new("Live 60fps").size(11.8))
                .clicked()
            {
                let cur = match self.source_kind {
                    SourceKind::Sample(p) | SourceKind::LiveMotion(p) => p,
                    _ => SamplePreset::CyberSculpture,
                };
                self.load_sample(cur, !is_live);
            }

            ui.separator();

            // Quick Studio Mode Pills (v0.2.0 highlights!)
            if ui
                .selectable_label(
                    self.params.oled_black,
                    RichText::new("OLED #000000").size(11.8),
                )
                .on_hover_text("True #000000 pitch-black canvas for zero OLED subpixel glow")
                .clicked()
            {
                self.params.oled_black = !self.params.oled_black;
                if self.params.oled_black {
                    self.params.transparent_bg = false;
                }
                self.ascii_dirty = true;
            }

            if ui
                .selectable_label(
                    self.params.transparent_bg,
                    RichText::new("Alpha Cutout").size(11.8),
                )
                .on_hover_text("True RGBA [0,0,0,0] transparent background for overlays")
                .clicked()
            {
                self.params.transparent_bg = !self.params.transparent_bg;
                self.ascii_dirty = true;
            }

            if ui
                .selectable_label(
                    self.params.wireframe_only,
                    RichText::new("Neon Wireframe").size(11.8),
                )
                .on_hover_text("Contour-only edge extraction dropping flat interior fills")
                .clicked()
            {
                self.params.wireframe_only = !self.params.wireframe_only;
                self.ascii_dirty = true;
            }

            ui.separator();

            if ui
                .selectable_label(self.split_enabled, RichText::new("Split-Wipe").size(11.8))
                .clicked()
            {
                self.split_enabled = !self.split_enabled;
                if self.split_enabled {
                    self.source_dirty = true;
                }
            }

            if ui
                .selectable_label(self.auto_fit, RichText::new("Fit").size(11.8))
                .clicked()
            {
                self.auto_fit = true;
                self.pan_offset = Vec2::ZERO;
            }
            for &(label, z) in &[("100%", 1.0f32), ("200%", 2.0), ("350%", 3.5)] {
                let active = !self.auto_fit && (self.zoom - z).abs() < 0.08;
                if ui
                    .selectable_label(active, RichText::new(label).size(11.8))
                    .clicked()
                {
                    self.auto_fit = false;
                    self.zoom = z;
                    self.ascii_dirty = true;
                }
            }

            // Right-aligned live telemetry pills
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let offline_label = if self.prefs.check_updates_on_startup {
                    "Releases (Auto)"
                } else {
                    "Offline · Updates"
                };
                let has_new = self
                    .update_status
                    .as_ref()
                    .and_then(|r| r.as_ref().ok())
                    .map(|o| o.update_available)
                    .unwrap_or(false);
                let upd_color = if has_new {
                    Color32::from_rgb(251, 191, 36)
                } else {
                    Color32::from_rgb(148, 163, 184)
                };
                if ui
                    .button(RichText::new(offline_label).color(upd_color).size(11.2))
                    .clicked()
                {
                    self.show_update_window = !self.show_update_window;
                }

                let fps_est = if self.ascii_frame.render_ms > 0.05 {
                    (1000.0 / self.ascii_frame.render_ms).min(9999.0)
                } else {
                    9999.0
                };
                telemetry_pill(
                    ui,
                    &format!(
                        "{:.2} ms ({:.0} FPS) · {} RAM",
                        self.ascii_frame.render_ms, fps_est, self.rss_label
                    ),
                    Color32::from_rgb(19, 24, 36),
                    Color32::from_rgb(245, 158, 11),
                );

                telemetry_pill(
                    ui,
                    &format!("{} / 95 GLYPHS", self.ascii_frame.unique_glyphs),
                    Color32::from_rgb(14, 33, 29),
                    Color32::from_rgb(52, 211, 153),
                );
            });
        });
    }

    fn draw_control_deck(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 5.0;

                // 1. OLED Pitch-Black, Alpha Cutout, Neon Wireframe & Audio-Reactive
                studio_card(ui, "OLED #000000 · ALPHA · WIREFRAME · AUDIO", |ui| {
                    let mut c = false;
                    ui.horizontal(|ui| {
                        if ui
                            .checkbox(&mut self.params.oled_black, "True OLED #000000")
                            .changed()
                        {
                            if self.params.oled_black {
                                self.params.transparent_bg = false;
                            }
                            c = true;
                        }
                        if ui
                            .checkbox(&mut self.params.transparent_bg, "Alpha RGBA")
                            .changed()
                        {
                            if self.params.transparent_bg {
                                self.params.oled_black = false;
                            }
                            c = true;
                        }
                    });

                    c |= labeled_slider(
                        ui,
                        "Shadow Noise Gate",
                        &mut self.params.black_gate,
                        0.00..=0.25,
                    );

                    ui.horizontal(|ui| {
                        c |= ui
                            .checkbox(
                                &mut self.params.wireframe_only,
                                "Neon Wireframe (Contour-Only)",
                            )
                            .changed();
                    });
                    if self.params.wireframe_only {
                        c |= labeled_slider(
                            ui,
                            "Contour Sensitivity",
                            &mut self.params.wireframe_sensitivity,
                            0.35..=2.50,
                        );
                    }

                    c |= labeled_slider(
                        ui,
                        "Audio-Reactive Beat",
                        &mut self.params.audio_reactive,
                        0.00..=2.00,
                    );
                    if self.params.audio_reactive > 0.001 {
                        let meter = (self.live_audio_energy * self.params.audio_reactive * 0.75)
                            .clamp(0.0, 1.0);
                        ui.add(
                            egui::ProgressBar::new(meter)
                                .desired_height(6.0)
                                .fill(Color32::from_rgb(236, 72, 153)),
                        );
                    }

                    if c {
                        self.ascii_dirty = true;
                    }
                });

                // 2. Color Engine & Custom Tri-Tone Ramp Picker
                studio_card(ui, "COLOR ENGINE & CUSTOM TRI-TONE", |ui| {
                    let mut c = false;
                    egui::ComboBox::from_id_salt("color_mode_combo")
                        .selected_text(self.params.color_mode.label())
                        .width(ui.available_width())
                        .show_ui(ui, |ui| {
                            for m in ColorMode::ALL {
                                if ui
                                    .selectable_value(&mut self.params.color_mode, m, m.label())
                                    .changed()
                                {
                                    c = true;
                                }
                            }
                        });

                    if self.params.color_mode == ColorMode::TriTone {
                        ui.add_space(2.0);
                        draw_tritone_bar(ui, &self.params.tritone);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Ramp:").size(11.2));
                            c |= ui
                                .color_edit_button_srgb(&mut self.params.tritone.shadow)
                                .on_hover_text("Shadow stop")
                                .changed();
                            ui.label("->");
                            c |= ui
                                .color_edit_button_srgb(&mut self.params.tritone.midtone)
                                .on_hover_text("Midtone stop")
                                .changed();
                            ui.label("->");
                            c |= ui
                                .color_edit_button_srgb(&mut self.params.tritone.highlight)
                                .on_hover_text("Highlight stop")
                                .changed();
                        });
                        ui.horizontal(|ui| {
                            for (lbl, pal) in TriTonePalette::PRESETS {
                                if ui.small_button(lbl).clicked() {
                                    self.params.tritone = pal;
                                    c = true;
                                }
                            }
                        });
                    }

                    c |= labeled_slider(
                        ui,
                        "Ink Luminance Boost",
                        &mut self.params.luminance_comp,
                        0.0..=1.0,
                    );
                    if self.params.color_mode == ColorMode::DualColor && !self.params.transparent_bg
                    {
                        c |= labeled_slider(
                            ui,
                            "Dual-BG Depth",
                            &mut self.params.bg_intensity,
                            0.04..=0.55,
                        );
                    }
                    c |= labeled_slider(ui, "Bayer 4x4 Dither", &mut self.params.dither, 0.0..=1.0);
                    c |=
                        labeled_slider(ui, "Brightness", &mut self.params.brightness, -0.40..=0.40);
                    c |= labeled_slider(ui, "Contrast", &mut self.params.contrast, 0.50..=2.40);
                    c |= labeled_slider(ui, "Saturation", &mut self.params.saturation, 0.0..=2.20);
                    c |= labeled_slider(ui, "Gamma", &mut self.params.gamma, 0.45..=2.00);

                    ui.horizontal(|ui| {
                        c |= ui
                            .checkbox(&mut self.params.crt_bloom, "CRT Bloom")
                            .changed();
                        c |= ui
                            .checkbox(&mut self.params.scanlines, "Scanlines")
                            .changed();
                        c |= ui.checkbox(&mut self.params.invert, "Invert").changed();
                    });

                    if ui.small_button("Reset Grading Defaults").clicked() {
                        let cols = self.params.cols;
                        self.params = RenderParams {
                            cols,
                            ..RenderParams::default()
                        };
                        c = true;
                    }

                    if c {
                        self.ascii_dirty = true;
                    }
                });

                // 3. Grid & Genuine ASCII Typography
                studio_card(ui, "TYPOGRAPHY & GLYPH SET", |ui| {
                    let mut changed = false;

                    ui.horizontal(|ui| {
                        ui.label("Columns");
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(
                                RichText::new(format!(
                                    "{}x{} cells",
                                    self.ascii_frame.cols, self.ascii_frame.rows
                                ))
                                .color(Color32::from_rgb(125, 211, 252))
                                .size(11.5),
                            );
                        });
                    });
                    changed |= ui
                        .add(egui::Slider::new(&mut self.params.cols, 40..=320))
                        .changed();

                    ui.label("Character Set");
                    egui::ComboBox::from_id_salt("charset_combo")
                        .selected_text(self.params.charset.label())
                        .width(ui.available_width())
                        .show_ui(ui, |ui| {
                            for preset in CharsetPreset::ALL {
                                if ui
                                    .selectable_value(
                                        &mut self.params.charset,
                                        preset,
                                        preset.label(),
                                    )
                                    .changed()
                                {
                                    changed = true;
                                }
                            }
                        });

                    if self.params.charset == CharsetPreset::Custom {
                        ui.label("Custom Characters / Phrase");
                        if ui
                            .text_edit_singleline(&mut self.params.custom_chars)
                            .changed()
                        {
                            changed = true;
                        }
                    }

                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(&self.atlas.font_name)
                                .size(11.0)
                                .color(Color32::from_rgb(148, 163, 184)),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.small_button("Load .TTF").clicked() {
                                if let Some(p) = rfd::FileDialog::new()
                                    .add_filter("Font", &["ttf", "otf"])
                                    .pick_file()
                                {
                                    if let Ok(bytes) = std::fs::read(&p) {
                                        let name = p
                                            .file_stem()
                                            .and_then(|s| s.to_str())
                                            .unwrap_or("Custom Font")
                                            .to_string();
                                        match GlyphAtlas::from_font_bytes(name, bytes, 20) {
                                            Ok(new_atlas) => {
                                                self.atlas = new_atlas;
                                                changed = true;
                                            }
                                            Err(e) => self.set_toast(e),
                                        }
                                    }
                                }
                            }
                        });
                    });

                    if changed {
                        self.ascii_dirty = true;
                    }
                });

                // 4. 8D Shape-Vector & Edge Lock Physics
                studio_card(ui, "8D SHAPE-VECTOR & EDGE PHYSICS", |ui| {
                    let mut c = false;
                    c |= labeled_slider(
                        ui,
                        "Directional Crunch",
                        &mut self.params.directional_crunch,
                        0.0..=3.0,
                    );
                    c |= labeled_slider(
                        ui,
                        "Structure-Tensor Edge",
                        &mut self.params.edge_weight,
                        0.0..=2.0,
                    );
                    c |= labeled_slider(
                        ui,
                        "Video Anti-Flicker",
                        &mut self.params.temporal_stability,
                        0.0..=0.45,
                    );
                    c |= labeled_slider(
                        ui,
                        "Subject Vignette",
                        &mut self.params.vignette_focus,
                        0.0..=1.0,
                    );
                    if c {
                        self.ascii_dirty = true;
                    }
                });

                // 5. One-Line Shareable Look Codes (lg2:)
                studio_card(ui, "SHAREABLE LOOK CODE (lg2:)", |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.look_code_buf)
                            .font(FontId::new(10.8, FontFamily::Monospace))
                            .desired_width(ui.available_width()),
                    );
                    ui.horizontal(|ui| {
                        if ui.small_button("Copy Look").clicked() {
                            let code = self.params.to_look_code();
                            self.look_code_buf = code.clone();
                            ui.ctx().copy_text(code);
                            self.set_toast("Copied shareable lg2: Look Code to clipboard");
                        }
                        if ui.small_button("Apply Edited Code").clicked() {
                            let code = self.look_code_buf.clone();
                            match self.params.apply_look_code(&code) {
                                Ok(()) => {
                                    self.ascii_dirty = true;
                                    self.set_toast("Applied lg2: Look Code");
                                }
                                Err(e) => self.set_toast(e),
                            }
                        }
                    });
                });

                // 6. Live Active Glyph Spectrum
                studio_card(ui, "ACTIVE GLYPH SPECTRUM", |ui| {
                    let mut pairs: Vec<(u8, u32)> = (0..ASCII_COUNT)
                        .filter_map(|i| {
                            let cnt = self.ascii_frame.histogram[i];
                            if cnt > 0 && i > 0 {
                                Some((ASCII_MIN + i as u8, cnt))
                            } else {
                                None
                            }
                        })
                        .collect();
                    pairs.sort_by_key(|a| std::cmp::Reverse(a.1));

                    let top_preview: String =
                        pairs.iter().take(34).map(|&(ch, _)| ch as char).collect();
                    ui.label(
                        RichText::new(format!("{} unique non-space chars active:", pairs.len()))
                            .size(11.0)
                            .color(Color32::from_rgb(148, 163, 184)),
                    );
                    Frame::new()
                        .fill(Color32::from_rgb(10, 13, 20))
                        .corner_radius(CornerRadius::same(4))
                        .inner_margin(Margin::symmetric(8, 5))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(if top_preview.is_empty() {
                                    "(blank)"
                                } else {
                                    &top_preview
                                })
                                .font(FontId::new(12.5, FontFamily::Monospace))
                                .color(Color32::from_rgb(56, 189, 248)),
                            );
                        });
                });

                // 7. Export Studio (Image + Alpha Video + Terminal ANSI)
                studio_card(ui, "EXPORT STUDIO", |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Copy ASCII").clicked() {
                            ui.ctx().copy_text(self.ascii_frame.to_plain_text());
                            self.set_toast("Copied plain ASCII to clipboard");
                        }
                        if ui.button("Copy TrueColor ANSI").clicked() {
                            ui.ctx()
                                .copy_text(frame_to_ansi_string(&self.ascii_frame, &self.params));
                            self.set_toast("Copied 24-bit ANSI escape art to clipboard");
                        }
                    });

                    ui.checkbox(&mut self.export_png_scale_2x, "High-DPI 2x PNG raster");

                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Save PNG").clicked() {
                            if let Some(p) = rfd::FileDialog::new()
                                .set_file_name("lumaglyph.png")
                                .add_filter("PNG", &["png"])
                                .save_file()
                            {
                                let h_px = if self.export_png_scale_2x { 36 } else { 20 };
                                match export_png(
                                    &self.ascii_frame,
                                    &self.atlas,
                                    h_px,
                                    &self.params,
                                    &p,
                                ) {
                                    Ok((w, h)) => self.set_toast(format!("Saved PNG ({w}x{h})")),
                                    Err(e) => self.set_toast(e),
                                }
                            }
                        }
                        if ui.button("Save SVG").clicked() {
                            if let Some(p) = rfd::FileDialog::new()
                                .set_file_name("lumaglyph.svg")
                                .add_filter("SVG", &["svg"])
                                .save_file()
                            {
                                match export_svg(&self.ascii_frame, &self.params, &p) {
                                    Ok(()) => self.set_toast("Saved vector SVG"),
                                    Err(e) => self.set_toast(e),
                                }
                            }
                        }
                        if ui.button("Save HTML").clicked() {
                            if let Some(p) = rfd::FileDialog::new()
                                .set_file_name("lumaglyph.html")
                                .add_filter("HTML", &["html"])
                                .save_file()
                            {
                                match export_html(&self.ascii_frame, &self.params, &p) {
                                    Ok(()) => self.set_toast("Saved standalone HTML"),
                                    Err(e) => self.set_toast(e),
                                }
                            }
                        }
                        if ui.button("Save .ANS").clicked() {
                            if let Some(p) = rfd::FileDialog::new()
                                .set_file_name("lumaglyph.ans")
                                .add_filter("ANSI Art", &["ans"])
                                .save_file()
                            {
                                match export_ansi(&self.ascii_frame, &self.params, &p) {
                                    Ok(()) => self.set_toast("Saved 24-bit terminal .ans"),
                                    Err(e) => self.set_toast(e),
                                }
                            }
                        }
                        if ui.button("Save .TXT").clicked() {
                            if let Some(p) = rfd::FileDialog::new()
                                .set_file_name("lumaglyph.txt")
                                .add_filter("Text", &["txt"])
                                .save_file()
                            {
                                match export_txt(&self.ascii_frame, &p) {
                                    Ok(()) => self.set_toast("Saved plain ASCII .txt"),
                                    Err(e) => self.set_toast(e),
                                }
                            }
                        }
                    });

                    // Video file transcode section
                    if let SourceKind::VideoFile(stream) = &self.source_kind {
                        let in_path = stream.path.clone();
                        let est_frames = stream
                            .meta
                            .duration_secs
                            .map(|d| (d * stream.meta.fps).round() as u64)
                            .unwrap_or(0);
                        ui.separator();
                        ui.label(
                            RichText::new("Streaming Video Transcoder (Flat RAM + Alpha)")
                                .strong()
                                .color(Color32::from_rgb(125, 211, 252)),
                        );
                        let default_ext = if self.params.transparent_bg {
                            "lumaglyph_alpha.webm"
                        } else {
                            "lumaglyph_ascii.mp4"
                        };
                        if let Some(job) = &self.transcode_job {
                            let done = job.progress.frames_done.load(Ordering::Relaxed);
                            let frac = if job.progress.total_frames_est > 0 {
                                (done as f32 / job.progress.total_frames_est as f32).clamp(0.0, 1.0)
                            } else {
                                0.5
                            };
                            ui.add(
                                egui::ProgressBar::new(frac)
                                    .text(format!("Encoding frame {done}...")),
                            );
                            if ui.small_button("Cancel Encode").clicked() {
                                job.progress.cancel_requested.store(true, Ordering::Relaxed);
                            }
                        } else if ui
                            .button("Export ASCII Video (.mp4 / .webm / .mov / .gif / .ans)")
                            .clicked()
                        {
                            if let Some(out_path) = rfd::FileDialog::new()
                                .set_file_name(default_ext)
                                .add_filter(
                                    "Video / Animation",
                                    &["mp4", "webm", "mov", "gif", "ans", "mkv"],
                                )
                                .save_file()
                            {
                                let progress = TranscodeProgress::new(est_frames);
                                let prog_clone = Arc::clone(&progress);
                                let atlas_clone = self.atlas.clone();
                                let params_clone = self.params.clone();
                                let out_clone = out_path.clone();
                                let (tx, rx) = mpsc::channel();
                                std::thread::spawn(move || {
                                    let res = transcode_video_streaming(
                                        &in_path,
                                        &out_clone,
                                        &atlas_clone,
                                        &params_clone,
                                        None,
                                        prog_clone,
                                    );
                                    let _ = tx.send(res);
                                });
                                self.transcode_job = Some(ActiveTranscodeJob {
                                    output_path: out_path,
                                    progress,
                                    rx,
                                });
                            }
                        }
                    }
                });
            });
    }

    fn draw_viewport(&mut self, ui: &mut egui::Ui) {
        let avail = ui.available_rect_before_wrap();
        let (response, painter) = ui.allocate_painter(avail.size(), Sense::click_and_drag());

        // True #000000 pitch-black surround when OLED mode is active, otherwise deep studio obsidian
        let surround = if self.params.oled_black {
            Color32::BLACK
        } else {
            Color32::from_rgb(8, 10, 15)
        };
        painter.rect_filled(avail, 0.0, surround);

        if self.ascii_px_w == 0 || self.ascii_px_h == 0 {
            return;
        }

        if response.hovered() {
            let scroll_y = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll_y.abs() > 0.5 {
                self.auto_fit = false;
                let factor = (1.0 + scroll_y * 0.0025).clamp(0.8, 1.25);
                self.zoom = (self.zoom * factor).clamp(0.20, 6.0);
            }
        }

        let base_w = self.ascii_px_w as f32;
        let base_h = self.ascii_px_h as f32;
        if self.auto_fit {
            let pad = 28.0;
            let sx = ((avail.width() - pad) / base_w).max(0.1);
            let sy = ((avail.height() - pad) / base_h).max(0.1);
            self.zoom = sx.min(sy);
            self.pan_offset = Vec2::ZERO;
        }

        let draw_size = Vec2::new(base_w * self.zoom, base_h * self.zoom);
        let center = avail.center() + self.pan_offset;
        let dest_rect = Rect::from_center_size(center, draw_size);

        let split_x = dest_rect.left() + dest_rect.width() * self.split_ratio;
        let pointer_pos = response.interact_pointer_pos();
        let near_split = self.split_enabled
            && pointer_pos
                .map(|p| (p.x - split_x).abs() < 18.0 && dest_rect.contains(p))
                .unwrap_or(false);

        if near_split {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }

        if response.dragged() {
            if self.split_enabled
                && pointer_pos
                    .map(|p| (p.x - split_x).abs() < 42.0)
                    .unwrap_or(false)
            {
                if let Some(p) = pointer_pos {
                    self.split_ratio =
                        ((p.x - dest_rect.left()) / dest_rect.width().max(1.0)).clamp(0.03, 0.97);
                }
            } else if !self.auto_fit {
                self.pan_offset += response.drag_delta();
            }
        }

        // Draw transparency checkerboard behind `dest_rect` when `transparent_bg` is enabled
        if self.params.transparent_bg {
            draw_alpha_checkerboard(&painter, dest_rect.intersect(avail));
        }

        if !self.params.oled_black {
            painter.rect_stroke(
                dest_rect.expand(1.0),
                CornerRadius::same(3),
                Stroke::new(1.0_f32, Color32::from_rgb(36, 44, 64)),
                StrokeKind::Outside,
            );
        }

        let uv_full = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0));

        if let Some(ascii_tex) = &self.ascii_tex {
            painter
                .with_clip_rect(avail)
                .image(ascii_tex.id(), dest_rect, uv_full, Color32::WHITE);
        }

        if self.split_enabled {
            if let Some(orig_tex) = &self.orig_tex {
                let wipe_x = (dest_rect.left() + dest_rect.width() * self.split_ratio)
                    .clamp(avail.left(), avail.right());
                let left_clip = Rect::from_min_max(
                    Pos2::new(avail.left(), avail.top()),
                    Pos2::new(wipe_x, avail.bottom()),
                );
                painter.with_clip_rect(left_clip).image(
                    orig_tex.id(),
                    dest_rect,
                    uv_full,
                    Color32::WHITE,
                );

                let line_top = dest_rect.top().max(avail.top());
                let line_bot = dest_rect.bottom().min(avail.bottom());
                if line_bot > line_top {
                    painter.line_segment(
                        [Pos2::new(wipe_x, line_top), Pos2::new(wipe_x, line_bot)],
                        Stroke::new(2.0_f32, Color32::from_rgb(56, 189, 248)),
                    );
                    let handle_center = Pos2::new(wipe_x, (line_top + line_bot) * 0.5);
                    painter.circle_filled(handle_center, 13.0, Color32::from_rgb(15, 23, 42));
                    painter.circle_stroke(
                        handle_center,
                        13.0,
                        Stroke::new(1.8_f32, Color32::from_rgb(56, 189, 248)),
                    );
                    painter.text(
                        handle_center,
                        egui::Align2::CENTER_CENTER,
                        "<>",
                        FontId::new(10.0, FontFamily::Monospace),
                        Color32::from_rgb(186, 230, 253),
                    );

                    let badge_y = line_top + 14.0;
                    if wipe_x - dest_rect.left() > 95.0 {
                        draw_canvas_badge(
                            &painter,
                            Pos2::new(dest_rect.left() + 12.0, badge_y),
                            "ORIGINAL SOURCE",
                            Color32::from_rgb(148, 163, 184),
                        );
                    }
                    if dest_rect.right() - wipe_x > 165.0 {
                        let right_tag = if self.params.wireframe_only && self.params.transparent_bg
                        {
                            "NEON WIREFRAME · RGBA ALPHA"
                        } else if self.params.oled_black {
                            "TRUE OLED #000000 · 95-CHAR ASCII"
                        } else if self.params.color_mode == ColorMode::TriTone {
                            "CUSTOM TRI-TONE · 95-CHAR ASCII"
                        } else {
                            "GENUINE 95-CHAR ASCII"
                        };
                        draw_canvas_badge(
                            &painter,
                            Pos2::new(wipe_x + 12.0, badge_y),
                            right_tag,
                            Color32::from_rgb(56, 189, 248),
                        );
                    }
                }
            }
        }

        // Hover Cell Inspector HUD
        if let Some(hover) = response.hover_pos() {
            if dest_rect.contains(hover) && self.ascii_frame.cols > 0 && self.ascii_frame.rows > 0 {
                let rel_x = ((hover.x - dest_rect.left()) / dest_rect.width()).clamp(0.0, 0.9999);
                let rel_y = ((hover.y - dest_rect.top()) / dest_rect.height()).clamp(0.0, 0.9999);
                let c = (rel_x * self.ascii_frame.cols as f32) as usize;
                let r = (rel_y * self.ascii_frame.rows as f32) as usize;
                let cell = self.ascii_frame.cells[r * self.ascii_frame.cols as usize + c];
                let bg_desc = if self.params.transparent_bg {
                    "RGBA(0,0,0,0)".to_string()
                } else {
                    format!("#{:02X}{:02X}{:02X}", cell.bg[0], cell.bg[1], cell.bg[2])
                };
                let info = format!(
                    "Cell ({c}, {r})  Glyph '{}' (0x{:02X})  FG #{:02X}{:02X}{:02X}  BG {bg_desc}",
                    cell.ch as char, cell.ch, cell.fg[0], cell.fg[1], cell.fg[2]
                );
                draw_canvas_badge(
                    &painter,
                    Pos2::new(avail.left() + 14.0, avail.bottom() - 34.0),
                    &info,
                    Color32::from_rgb(125, 211, 252),
                );
            }
        }

        if let Some((msg, when)) = &self.status_toast {
            if when.elapsed().as_secs_f32() < 5.0 {
                draw_canvas_badge(
                    &painter,
                    Pos2::new(avail.left() + 14.0, avail.top() + 14.0),
                    msg,
                    Color32::from_rgb(52, 211, 153),
                );
            } else {
                self.status_toast = None;
            }
        }
    }

    fn draw_bottom_transport(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            match &mut self.source_kind {
                SourceKind::LiveMotion(preset) => {
                    let p_label = preset.label();
                    if ui
                        .button(if self.motion_paused { "Play" } else { "Pause" })
                        .clicked()
                    {
                        self.motion_paused = !self.motion_paused;
                    }
                    ui.label(
                        RichText::new(format!("Live Procedural Stream: {p_label}"))
                            .color(Color32::from_rgb(125, 211, 252))
                            .size(12.0),
                    );
                }
                SourceKind::VideoFile(stream) => {
                    let is_paused = stream.paused.load(Ordering::Relaxed);
                    if ui
                        .button(if is_paused { "Play" } else { "Pause" })
                        .clicked()
                    {
                        stream.paused.store(!is_paused, Ordering::Relaxed);
                    }
                    ui.label(
                        RichText::new(format!(
                            "Video: {}x{} @ {:.1} FPS · Frame #{} · Beat {:.0}%",
                            stream.meta.width,
                            stream.meta.height,
                            stream.meta.fps,
                            self.video_frames_seen,
                            (self.live_audio_energy * 100.0).min(100.0)
                        ))
                        .color(Color32::from_rgb(125, 211, 252))
                        .size(12.0),
                    );
                }
                SourceKind::StaticImage { name } => {
                    ui.label(
                        RichText::new(format!(
                            "Image: {name} ({}x{})",
                            self.source_w, self.source_h
                        ))
                        .color(Color32::from_rgb(148, 163, 184))
                        .size(12.0),
                    );
                }
                SourceKind::Sample(p) => {
                    ui.label(
                        RichText::new(format!(
                            "Scene: {} ({}x{}) — Look Code {}",
                            p.label(),
                            self.source_w,
                            self.source_h,
                            &self.look_code_buf[..self.look_code_buf.len().min(36)]
                        ))
                        .color(Color32::from_rgb(148, 163, 184))
                        .size(12.0),
                    );
                }
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let ff = self
                    .ffmpeg_info
                    .as_deref()
                    .unwrap_or("ffmpeg not on PATH (Image/GIF ready)");
                ui.label(
                    RichText::new(ff)
                        .color(Color32::from_rgb(100, 116, 139))
                        .size(11.0),
                );
            });
        });
    }

    fn draw_update_modal(&mut self, ctx: &egui::Context) {
        if !self.show_update_window {
            return;
        }
        let mut open = self.show_update_window;
        egui::Window::new("Offline Privacy & GitHub Release Notifier")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(430.0)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("Zero Telemetry · 100% Native Offline by Default")
                        .strong()
                        .color(Color32::from_rgb(52, 211, 153)),
                );
                ui.label(
                    "LumaGlyph never opens network sockets unless you click 'Check Now' or opt in below to check GitHub Releases for new versions.",
                );
                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    ui.label("GitHub Repo:");
                    ui.text_edit_singleline(&mut self.prefs.github_repo);
                });

                if ui
                    .checkbox(
                        &mut self.prefs.check_updates_on_startup,
                        "Check GitHub Releases automatically on launch (opt-in)",
                    )
                    .changed()
                {
                    let _ = self.prefs.save();
                }

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let busy = self.update_rx.is_some();
                    if ui
                        .add_enabled(!busy, egui::Button::new("Check GitHub Releases Now"))
                        .clicked()
                    {
                        let _ = self.prefs.save();
                        self.update_rx = Some(spawn_release_check(self.prefs.github_repo.clone()));
                    }
                    if busy {
                        ui.label("Checking api.github.com...");
                    }
                });

                if let Some(res) = &self.update_status {
                    ui.separator();
                    match res {
                        Ok(info) => {
                            if info.update_available {
                                ui.label(
                                    RichText::new(format!(
                                        "Update available: {} (installed: v{APP_VERSION})",
                                        info.latest_tag
                                    ))
                                    .color(Color32::from_rgb(251, 191, 36))
                                    .strong(),
                                );
                                ui.label(&info.summary);
                                ui.hyperlink_to("Open GitHub Release Page", &info.html_url);
                            } else {
                                ui.label(
                                    RichText::new(format!(
                                        "You are up to date (latest: {}).",
                                        info.latest_tag
                                    ))
                                    .color(Color32::from_rgb(52, 211, 153)),
                                );
                            }
                        }
                        Err(err) => {
                            ui.label(
                                RichText::new(err).color(Color32::from_rgb(248, 113, 113)),
                            );
                        }
                    }
                }
            });
        self.show_update_window = open;
    }
}

impl eframe::App for LumaGlyphApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.refresh_pipelines(ctx);

        egui::TopBottomPanel::top("top_studio_bar")
            .frame(
                Frame::new()
                    .fill(Color32::from_rgb(14, 18, 27))
                    .inner_margin(Margin::symmetric(12, 8))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(30, 38, 56))),
            )
            .show(ctx, |ui| self.draw_top_bar(ui));

        egui::TopBottomPanel::bottom("bottom_transport_bar")
            .frame(
                Frame::new()
                    .fill(Color32::from_rgb(13, 16, 24))
                    .inner_margin(Margin::symmetric(12, 6))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(30, 38, 56))),
            )
            .show(ctx, |ui| self.draw_bottom_transport(ui));

        egui::SidePanel::left("left_control_deck")
            .default_width(324.0)
            .min_width(296.0)
            .max_width(380.0)
            .frame(
                Frame::new()
                    .fill(Color32::from_rgb(15, 19, 28))
                    .inner_margin(Margin::symmetric(10, 10))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(30, 38, 56))),
            )
            .show(ctx, |ui| self.draw_control_deck(ui));

        let central_bg = if self.params.oled_black {
            Color32::BLACK
        } else {
            Color32::from_rgb(8, 10, 15)
        };
        egui::CentralPanel::default()
            .frame(Frame::new().fill(central_bg))
            .show(ctx, |ui| self.draw_viewport(ui));

        self.draw_update_modal(ctx);

        self.frames_drawn = self.frames_drawn.saturating_add(1);
        if let Some(limit) = self.auto_exit_after_frames {
            if self.frames_drawn < limit {
                ctx.request_repaint();
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

fn draw_tritone_bar(ui: &mut egui::Ui, pal: &TriTonePalette) {
    let desired = Vec2::new(ui.available_width(), 12.0);
    let (rect, _) = ui.allocate_exact_size(desired, Sense::hover());
    let painter = ui.painter();
    let steps = 32usize;
    let sw = rect.width() / (steps as f32);
    for i in 0..steps {
        let t = (i as f32 + 0.5) / (steps as f32);
        let [r, g, b] = pal.sample(t);
        let seg = Rect::from_min_max(
            Pos2::new(rect.left() + (i as f32) * sw, rect.top()),
            Pos2::new(rect.left() + ((i + 1) as f32) * sw + 0.5, rect.bottom()),
        );
        painter.rect_filled(seg, 0.0, Color32::from_rgb(r, g, b));
    }
    painter.rect_stroke(
        rect,
        CornerRadius::same(2),
        Stroke::new(1.0_f32, Color32::from_rgb(56, 70, 96)),
        StrokeKind::Outside,
    );
}

fn draw_alpha_checkerboard(painter: &egui::Painter, rect: Rect) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let tile = 16.0f32;
    let c_dark = Color32::from_rgb(13, 16, 23);
    let c_light = Color32::from_rgb(24, 30, 42);
    painter.rect_filled(rect, 0.0, c_dark);

    let cols = (rect.width() / tile).ceil() as usize;
    let rows = (rect.height() / tile).ceil() as usize;
    let clipped = painter.with_clip_rect(rect);
    for r in 0..rows {
        for c in 0..cols {
            if (r + c) & 1 == 1 {
                let x0 = rect.left() + (c as f32) * tile;
                let y0 = rect.top() + (r as f32) * tile;
                let sub = Rect::from_min_size(Pos2::new(x0, y0), Vec2::splat(tile));
                clipped.rect_filled(sub, 0.0, c_light);
            }
        }
    }
}

fn apply_studio_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = Color32::from_rgb(15, 19, 28);
    visuals.window_fill = Color32::from_rgb(17, 22, 33);
    visuals.extreme_bg_color = Color32::from_rgb(9, 12, 18);
    visuals.faint_bg_color = Color32::from_rgb(22, 27, 40);
    visuals.selection.bg_fill = Color32::from_rgb(14, 116, 144);
    visuals.selection.stroke = Stroke::new(1.0_f32, Color32::from_rgb(125, 211, 252));
    visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(22, 27, 40);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(36, 45, 65));
    visuals.widgets.noninteractive.fg_stroke =
        Stroke::new(1.0_f32, Color32::from_rgb(214, 222, 235));
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(26, 33, 49);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(45, 56, 80));
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(34, 45, 68);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(56, 189, 248));
    visuals.widgets.active.bg_fill = Color32::from_rgb(14, 116, 144);
    ctx.set_visuals(visuals);
}

fn studio_card(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    Frame::new()
        .fill(Color32::from_rgb(20, 25, 37))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(34, 43, 62)))
        .corner_radius(CornerRadius::same(7))
        .inner_margin(Margin::symmetric(9, 6))
        .show(ui, |ui| {
            ui.label(
                RichText::new(title)
                    .size(10.0)
                    .strong()
                    .color(Color32::from_rgb(125, 211, 252)),
            );
            ui.add_space(2.0);
            add_contents(ui);
        });
}

fn labeled_slider(
    ui: &mut egui::Ui,
    label: &str,
    val: &mut f32,
    range: std::ops::RangeInclusive<f32>,
) -> bool {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(11.6));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add(egui::Slider::new(val, range).show_value(true))
                .changed()
        })
        .inner
    })
    .inner
}

fn telemetry_pill(ui: &mut egui::Ui, text: &str, bg: Color32, fg: Color32) {
    Frame::new()
        .fill(bg)
        .stroke(Stroke::new(1.0_f32, fg.gamma_multiply(0.45)))
        .corner_radius(CornerRadius::same(5))
        .inner_margin(Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.label(
                RichText::new(text)
                    .font(FontId::new(11.0, FontFamily::Monospace))
                    .color(fg),
            );
        });
}

fn draw_canvas_badge(painter: &egui::Painter, pos: Pos2, text: &str, accent: Color32) {
    let galley = painter.layout_no_wrap(
        text.to_string(),
        FontId::new(11.5, FontFamily::Monospace),
        accent,
    );
    let rect = Rect::from_min_size(pos, galley.size() + Vec2::new(16.0, 10.0));
    painter.rect_filled(
        rect,
        CornerRadius::same(5),
        Color32::from_rgba_unmultiplied(11, 15, 24, 224),
    );
    painter.rect_stroke(
        rect,
        CornerRadius::same(5),
        Stroke::new(1.0_f32, accent.gamma_multiply(0.55)),
        StrokeKind::Outside,
    );
    painter.galley(pos + Vec2::new(8.0, 5.0), galley, accent);
}

pub fn launch_gui(boot: GuiBootOptions) -> Result<(), String> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("LumaGlyph — Genuine 95-Char Color ASCII & Video Studio")
            .with_inner_size([1520.0, 900.0])
            .with_min_inner_size([1024.0, 640.0]),
        ..Default::default()
    };

    eframe::run_native(
        "LumaGlyph",
        native_options,
        Box::new(move |cc| Ok(Box::new(LumaGlyphApp::new(cc, boot)))),
    )
    .map_err(|e| format!("Failed launching native GPU window: {e}"))
}
