use crate::charset::{CharsetPreset, ASCII_COUNT, ASCII_MIN};
use crate::color::{ColorMode, TriTonePalette};
use crate::engine::{convert_image_buffer_with_audio, rasterize_rgba, AsciiFrame, RenderParams};
use crate::export::{
    export_ansi, export_html, export_png, export_svg, export_txt, frame_to_ansi_string,
};
use crate::glyph_atlas::GlyphAtlas;
use crate::samples::{generate_sample_rgba, SamplePreset};
use crate::telemetry::format_rss_mb;
use crate::theme::{
    apply_theme_to_context, install_studio_fonts, mono_font, plex_medium, resolve_palette,
    AccentPreset, CheckerContrast, CornerStyle, StudioTheme, ThemePalette, UiDensity,
};
use crate::updater::{
    format_build_report, spawn_release_check, spawn_verified_release_download, AppPreferences,
    ReleaseCheckOutcome, VerifiedDownloadOutcome, APP_VERSION, BUILD_GIT_SHA, BUILD_PROFILE,
    BUILD_RUSTC_VERSION, BUILD_TARGET_TRIPLE, DEFAULT_GITHUB_REPO,
};
use crate::video::{
    detect_ffmpeg_version, set_video_pipeline_prefs, transcode_video_streaming, LiveVideoStream,
    TranscodeProgress, TranscodeSummary,
};
use eframe::egui::{
    self, Align, Color32, ColorImage, CornerRadius, DragValue, Frame, Key, Layout, Margin, Pos2,
    Rect, RichText, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Vec2,
};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    Appearance,
    EngineDefaults,
    ExportVideo,
    Shortcuts,
    AboutBuild,
}

impl SettingsTab {
    pub const ALL: [Self; 5] = [
        Self::Appearance,
        Self::EngineDefaults,
        Self::ExportVideo,
        Self::Shortcuts,
        Self::AboutBuild,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::EngineDefaults => "Engine & Defaults",
            Self::ExportVideo => "Export & Video",
            Self::Shortcuts => "Shortcuts",
            Self::AboutBuild => "About & Build",
        }
    }

    pub fn from_cli(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "appearance" | "theme" | "themes" => Some(Self::Appearance),
            "engine" | "defaults" => Some(Self::EngineDefaults),
            "export" | "video" => Some(Self::ExportVideo),
            "shortcuts" | "keys" => Some(Self::Shortcuts),
            "about" | "build" | "version" | "updates" => Some(Self::AboutBuild),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InspectorTab {
    #[default]
    All,
    Grading,
    Geometry,
    Output,
}

impl InspectorTab {
    pub const ALL: [Self; 4] = [Self::All, Self::Grading, Self::Geometry, Self::Output];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Grading => "Color",
            Self::Geometry => "Geometry",
            Self::Output => "Output",
        }
    }
}

#[derive(Clone, Default)]
pub struct GuiBootOptions {
    pub initial_media: Option<PathBuf>,
    pub initial_preset: Option<SamplePreset>,
    pub initial_params: Option<RenderParams>,
    pub theme_override: Option<StudioTheme>,
    pub open_settings_tab: Option<SettingsTab>,
    pub split_wipe: Option<f32>,
    pub initial_zoom: Option<f32>,
    pub live_motion: bool,
    pub show_update_modal: bool,
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

    // Workstation layout & viewport state
    inspector_tab: InspectorTab,
    zen_mode: bool,
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
    gpu_renderer: String,
    transcode_job: Option<ActiveTranscodeJob>,

    // Telemetry & status
    rss_label: String,
    last_rss_poll: Instant,
    status_toast: Option<(String, Instant)>,

    // Persistent preferences, theme palette & multi-tab Settings window
    prefs: AppPreferences,
    palette: ThemePalette,
    style_dirty: bool,
    show_settings_window: bool,
    settings_tab: SettingsTab,
    update_rx: Option<Receiver<Result<ReleaseCheckOutcome, String>>>,
    update_status: Option<Result<ReleaseCheckOutcome, String>>,
    download_rx: Option<Receiver<Result<VerifiedDownloadOutcome, String>>>,
    download_status: Option<Result<VerifiedDownloadOutcome, String>>,

    // Automated frame capture exit
    frames_drawn: u32,
    auto_exit_after_frames: Option<u32>,
}

impl LumaGlyphApp {
    pub fn new(cc: &eframe::CreationContext<'_>, boot: GuiBootOptions) -> Self {
        install_studio_fonts(&cc.egui_ctx);

        let mut prefs = AppPreferences::load();
        if let Some(t) = boot.theme_override {
            prefs.theme = t;
        }
        set_video_pipeline_prefs(&prefs.ffmpeg_custom_dir, prefs.video_crf);

        let palette = resolve_palette(prefs.theme, prefs.accent, prefs.corner_style);
        apply_theme_to_context(&cc.egui_ctx, &palette, prefs.ui_density);

        let gpu_renderer = cc
            .gl
            .as_ref()
            .map(|gl| unsafe {
                use eframe::glow::HasContext;
                let r = gl.get_parameter_string(eframe::glow::RENDERER);
                let v = gl.get_parameter_string(eframe::glow::VENDOR);
                if v.is_empty() {
                    r
                } else {
                    format!("{r} ({v})")
                }
            })
            .unwrap_or_else(|| "OpenGL Hardware Context".to_string());

        let cell_h = prefs.atlas_cell_height.clamp(12, 36);
        let atlas = GlyphAtlas::default_embedded(cell_h);

        let params = boot.initial_params.unwrap_or_else(|| RenderParams {
            cols: prefs.default_cols.clamp(40, 320),
            charset: prefs.default_charset,
            color_mode: prefs.default_color_mode,
            oled_black: prefs.default_oled_black,
            ..RenderParams::default()
        });
        let look_code_buf = params.to_look_code();
        let preset = boot.initial_preset.unwrap_or(SamplePreset::CyberSculpture);

        let sample_w = 960;
        let sample_h = 600;
        let source_rgba = generate_sample_rgba(preset, sample_w, sample_h, 0.0);

        let update_rx = if prefs.check_updates_on_startup {
            Some(spawn_release_check(prefs.github_repo.clone()))
        } else {
            None
        };

        let initial_tab = boot.open_settings_tab.unwrap_or(if boot.show_update_modal {
            SettingsTab::AboutBuild
        } else {
            SettingsTab::Appearance
        });
        let show_settings_window = boot.open_settings_tab.is_some() || boot.show_update_modal;

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
            inspector_tab: InspectorTab::All,
            zen_mode: false,
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
            gpu_renderer,
            transcode_job: None,
            rss_label: format_rss_mb(),
            last_rss_poll: Instant::now(),
            status_toast: None,
            prefs,
            palette,
            style_dirty: false,
            show_settings_window,
            settings_tab: initial_tab,
            update_rx,
            update_status: None,
            download_rx: None,
            download_status: None,
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

    fn persist_prefs(&mut self) {
        set_video_pipeline_prefs(&self.prefs.ffmpeg_custom_dir, self.prefs.video_crf);
        self.palette =
            resolve_palette(self.prefs.theme, self.prefs.accent, self.prefs.corner_style);
        self.style_dirty = true;
        let _ = self.prefs.save();
    }

    fn open_image_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Images", &["png", "jpg", "jpeg", "webp", "gif", "bmp"])
            .pick_file()
        {
            self.load_media_path(path);
        }
    }

    fn open_video_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Videos", &["mp4", "mkv", "mov", "webm", "avi", "m4v"])
            .pick_file()
        {
            self.load_media_path(path);
        }
    }

    fn open_font_dialog(&mut self) {
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
                let ch = self.prefs.atlas_cell_height.clamp(12, 36);
                match GlyphAtlas::from_font_bytes(name.clone(), bytes, ch) {
                    Ok(new_atlas) => {
                        self.atlas = new_atlas;
                        self.ascii_dirty = true;
                        self.set_toast(format!("Calibrated font: {name}"));
                    }
                    Err(e) => self.set_toast(e),
                }
            }
        }
    }

    fn save_png_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .set_file_name("lumaglyph.png")
            .add_filter("PNG", &["png"])
            .save_file()
        {
            let h_px = self.prefs.png_cell_height();
            match export_png(&self.ascii_frame, &self.atlas, h_px, &self.params, &p) {
                Ok((w, h)) => self.set_toast(format!("Exported PNG ({w}x{h} px)")),
                Err(e) => self.set_toast(e),
            }
        }
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
                        "video stream"
                    };
                    self.set_toast(format!("Loaded {fname} ({audio_note})"));
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

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        ctx.input(|i| {
            if i.modifiers.command && i.key_pressed(Key::Comma) {
                self.show_settings_window = !self.show_settings_window;
            }
            if i.modifiers.command && i.key_pressed(Key::O) {
                self.open_image_dialog();
            }
            if i.modifiers.command && !i.modifiers.shift && i.key_pressed(Key::S) {
                self.save_png_dialog();
            }
            if i.modifiers.command && i.modifiers.shift && i.key_pressed(Key::C) {
                let code = self.params.to_look_code();
                ctx.copy_text(code);
                self.set_toast("Copied lg2: Look Code to clipboard");
            }
            if !i.modifiers.any() {
                if i.key_pressed(Key::Tab) {
                    self.zen_mode = !self.zen_mode;
                }
                if i.key_pressed(Key::W) {
                    self.split_enabled = !self.split_enabled;
                    if self.split_enabled {
                        self.source_dirty = true;
                    }
                }
                if i.key_pressed(Key::B) {
                    self.params.oled_black = !self.params.oled_black;
                    if self.params.oled_black {
                        self.params.transparent_bg = false;
                    }
                    self.ascii_dirty = true;
                }
                if i.key_pressed(Key::N) {
                    self.params.wireframe_only = !self.params.wireframe_only;
                    self.ascii_dirty = true;
                }
                if i.key_pressed(Key::Space) {
                    match &mut self.source_kind {
                        SourceKind::LiveMotion(_) => self.motion_paused = !self.motion_paused,
                        SourceKind::VideoFile(stream) => {
                            let p = stream.paused.load(Ordering::Relaxed);
                            stream.paused.store(!p, Ordering::Relaxed);
                        }
                        _ => {}
                    }
                }
                if i.key_pressed(Key::Num0) {
                    self.auto_fit = true;
                    self.pan_offset = Vec2::ZERO;
                }
                if i.key_pressed(Key::Num1) {
                    self.auto_fit = false;
                    self.zoom = 1.0;
                    self.ascii_dirty = true;
                }
                if i.key_pressed(Key::Num2) {
                    self.auto_fit = false;
                    self.zoom = 2.0;
                    self.ascii_dirty = true;
                }
                if i.key_pressed(Key::Num3) {
                    self.auto_fit = false;
                    self.zoom = 3.5;
                    self.ascii_dirty = true;
                }
                if i.key_pressed(Key::OpenBracket) {
                    self.params.cols = self.params.cols.saturating_sub(8).max(40);
                    self.ascii_dirty = true;
                }
                if i.key_pressed(Key::CloseBracket) {
                    self.params.cols = (self.params.cols + 8).min(320);
                    self.ascii_dirty = true;
                }
            }
        });
    }

    fn apply_grading_preset(&mut self, preset_id: &str) {
        let cols = self.params.cols;
        match preset_id {
            "neutral" => {
                self.params = RenderParams {
                    cols,
                    ..RenderParams::default()
                };
            }
            "oled_contrast" => {
                self.params = RenderParams {
                    cols,
                    oled_black: true,
                    black_gate: 0.055,
                    contrast: 1.28,
                    directional_crunch: 1.55,
                    crt_bloom: true,
                    ..RenderParams::default()
                };
            }
            "rgba_contour" => {
                self.params = RenderParams {
                    cols,
                    transparent_bg: true,
                    wireframe_only: true,
                    wireframe_sensitivity: 1.20,
                    color_mode: ColorMode::Cyberpunk,
                    crt_bloom: true,
                    ..RenderParams::default()
                };
            }
            "tokyo_tritone" => {
                self.params = RenderParams {
                    cols,
                    color_mode: ColorMode::TriTone,
                    tritone: TriTonePalette::TOKYO_MIDNIGHT,
                    oled_black: true,
                    black_gate: 0.04,
                    ..RenderParams::default()
                };
            }
            "solar_audio" => {
                self.params = RenderParams {
                    cols,
                    color_mode: ColorMode::TriTone,
                    tritone: TriTonePalette::SOLAR_FLARE,
                    oled_black: true,
                    black_gate: 0.05,
                    audio_reactive: 1.30,
                    crt_bloom: true,
                    ..RenderParams::default()
                };
            }
            "amber_crt" => {
                self.params = RenderParams {
                    cols,
                    color_mode: ColorMode::PhosphorAmber,
                    oled_black: true,
                    crt_bloom: true,
                    scanlines: true,
                    ..RenderParams::default()
                };
            }
            _ => {}
        }
        self.ascii_dirty = true;
    }

    fn refresh_pipelines(&mut self, ctx: &egui::Context) {
        if self.style_dirty {
            apply_theme_to_context(ctx, &self.palette, self.prefs.ui_density);
            self.style_dirty = false;
        }

        match &mut self.source_kind {
            SourceKind::LiveMotion(preset) => {
                if !self.motion_paused {
                    let t = self.motion_start.elapsed().as_secs_f32();
                    self.source_rgba =
                        generate_sample_rgba(*preset, self.source_w, self.source_h, t);
                    self.source_channels = 4;
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

        if let Some(rx) = &self.update_rx {
            ctx.request_repaint_after(std::time::Duration::from_millis(80));
            if let Ok(res) = rx.try_recv() {
                if let Ok(ref ok) = res {
                    if ok.update_available {
                        self.set_toast(format!(
                            "Update available on GitHub: {} (installed v{})",
                            ok.latest_tag, APP_VERSION
                        ));
                    }
                }
                self.update_status = Some(res);
                self.update_rx = None;
            }
        }

        if let Some(rx) = &self.download_rx {
            ctx.request_repaint_after(std::time::Duration::from_millis(80));
            if let Ok(res) = rx.try_recv() {
                match &res {
                    Ok(done) => self.set_toast(format!(
                        "Verified SHA-256 & saved {}",
                        done.saved_path.display()
                    )),
                    Err(err) => self.set_toast(format!("Update verification blocked: {err}")),
                }
                self.download_status = Some(res);
                self.download_rx = None;
            }
        }

        if self.last_rss_poll.elapsed().as_millis() > 600 {
            self.rss_label = format_rss_mb();
            self.last_rss_poll = Instant::now();
        }
    }

    fn draw_menu_strip(&mut self, ui: &mut egui::Ui) {
        let pal = self.palette;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;

            ui.label(
                RichText::new("LUMAGLYPH")
                    .font(plex_medium(12.5))
                    .color(pal.text_primary),
            );
            ui.label(
                RichText::new(format!("v{APP_VERSION}"))
                    .font(mono_font(10.8))
                    .color(pal.text_muted),
            );

            ui.separator();

            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open Image...          Ctrl+O").clicked() {
                        ui.close_menu();
                        self.open_image_dialog();
                    }
                    if ui.button("Open Video...").clicked() {
                        ui.close_menu();
                        self.open_video_dialog();
                    }
                    if ui.button("Load Font (.ttf/.otf)...").clicked() {
                        ui.close_menu();
                        self.open_font_dialog();
                    }
                    ui.separator();
                    if ui.button("Export PNG...          Ctrl+S").clicked() {
                        ui.close_menu();
                        self.save_png_dialog();
                    }
                    if ui.button("Copy Look Code   Ctrl+Shift+C").clicked() {
                        ui.close_menu();
                        ui.ctx().copy_text(self.params.to_look_code());
                        self.set_toast("Copied lg2: Look Code");
                    }
                    ui.separator();
                    if ui.button("Settings...            Ctrl+,").clicked() {
                        ui.close_menu();
                        self.settings_tab = SettingsTab::Appearance;
                        self.show_settings_window = true;
                    }
                    ui.separator();
                    if ui.button("Exit").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button("View", |ui| {
                    if ui
                        .checkbox(&mut self.split_enabled, "Split-Wipe Curtain       W")
                        .changed()
                        && self.split_enabled
                    {
                        self.source_dirty = true;
                    }
                    ui.checkbox(&mut self.zen_mode, "Zen Canvas Mode        Tab");
                    if ui
                        .checkbox(&mut self.prefs.show_cell_hud, "Hover Cell Readout")
                        .changed()
                    {
                        self.persist_prefs();
                    }
                    if ui
                        .checkbox(&mut self.prefs.show_status_bar, "Bottom Status Strip")
                        .changed()
                    {
                        self.persist_prefs();
                    }
                    ui.separator();
                    if ui.button("Zoom to Fit              0").clicked() {
                        self.auto_fit = true;
                        self.pan_offset = Vec2::ZERO;
                        ui.close_menu();
                    }
                    for (lbl, z) in [
                        ("Zoom 100%                1", 1.0f32),
                        ("Zoom 200%                2", 2.0),
                        ("Zoom 350% Loupe          3", 3.5),
                    ] {
                        if ui.button(lbl).clicked() {
                            self.auto_fit = false;
                            self.zoom = z;
                            self.ascii_dirty = true;
                            ui.close_menu();
                        }
                    }
                    ui.separator();
                    ui.menu_button("Theme", |ui| {
                        for t in StudioTheme::ALL {
                            if ui
                                .radio_value(&mut self.prefs.theme, t, t.label())
                                .changed()
                            {
                                self.persist_prefs();
                                ui.close_menu();
                            }
                        }
                    });
                });

                ui.menu_button("Scenes", |ui| {
                    ui.label(
                        RichText::new("Static Calibration Plates")
                            .font(plex_medium(11.0))
                            .color(pal.text_muted),
                    );
                    for p in SamplePreset::ALL {
                        if ui.button(p.label()).clicked() {
                            self.load_sample(p, false);
                            ui.close_menu();
                        }
                    }
                    ui.separator();
                    ui.label(
                        RichText::new("Real-Time 60fps Streams")
                            .font(plex_medium(11.0))
                            .color(pal.text_muted),
                    );
                    for p in SamplePreset::ALL {
                        if ui.button(format!("{} (Live)", p.label())).clicked() {
                            self.load_sample(p, true);
                            ui.close_menu();
                        }
                    }
                });

                ui.menu_button("Presets", |ui| {
                    for (id, lbl) in [
                        ("neutral", "Calibrated TrueColor (Default)"),
                        ("oled_contrast", "OLED #000000 High-Contrast"),
                        ("rgba_contour", "Neon Contour Overlay (RGBA Alpha)"),
                        ("tokyo_tritone", "Tokyo Midnight Tri-Tone"),
                        ("solar_audio", "Solar Flare Audio-Reactive"),
                        ("amber_crt", "Vintage P3 Amber CRT"),
                    ] {
                        if ui.button(lbl).clicked() {
                            self.apply_grading_preset(id);
                            ui.close_menu();
                        }
                    }
                });

                ui.menu_button("Help", |ui| {
                    if ui.button("Settings & Preferences...").clicked() {
                        self.settings_tab = SettingsTab::Appearance;
                        self.show_settings_window = true;
                        ui.close_menu();
                    }
                    if ui.button("Keyboard Shortcuts").clicked() {
                        self.settings_tab = SettingsTab::Shortcuts;
                        self.show_settings_window = true;
                        ui.close_menu();
                    }
                    if ui.button("Check GitHub Releases...").clicked() {
                        self.settings_tab = SettingsTab::AboutBuild;
                        self.show_settings_window = true;
                        if self.update_rx.is_none() {
                            self.update_rx =
                                Some(spawn_release_check(self.prefs.github_repo.clone()));
                        }
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("About & Build Info").clicked() {
                        self.settings_tab = SettingsTab::AboutBuild;
                        self.show_settings_window = true;
                        ui.close_menu();
                    }
                });
            });

            // Right-aligned understated tabular technical telemetry + Settings trigger
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let has_update = self
                    .update_status
                    .as_ref()
                    .and_then(|r| r.as_ref().ok())
                    .map(|o| o.update_available)
                    .unwrap_or(false);

                let settings_lbl = if has_update {
                    "Settings (Update)"
                } else {
                    "Settings"
                };
                if workstation_toggle(ui, &pal, self.show_settings_window, settings_lbl)
                    .on_hover_text("Workstation Settings, Themes & Build Info (Ctrl+,)")
                    .clicked()
                {
                    self.show_settings_window = !self.show_settings_window;
                }

                ui.separator();

                let fps_est = if self.ascii_frame.render_ms > 0.05 {
                    (1000.0 / self.ascii_frame.render_ms).min(9999.0)
                } else {
                    9999.0
                };
                let metrics = format!(
                    "{}x{}   |   {:02}/95 CHARS   |   {:.2} ms ({:.0} FPS)   |   RSS {}",
                    self.ascii_frame.cols,
                    self.ascii_frame.rows,
                    self.ascii_frame.unique_glyphs,
                    self.ascii_frame.render_ms,
                    fps_est,
                    self.rss_label
                );
                ui.label(
                    RichText::new(metrics)
                        .font(mono_font(11.0))
                        .color(pal.text_secondary),
                );
            });
        });
    }

    fn draw_command_strip(&mut self, ui: &mut egui::Ui) {
        let pal = self.palette;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;

            if ui.button("Open Image").clicked() {
                self.open_image_dialog();
            }
            if ui.button("Open Video").clicked() {
                self.open_video_dialog();
            }

            let is_live = matches!(self.source_kind, SourceKind::LiveMotion(_));
            if workstation_toggle(ui, &pal, is_live, "Live 60fps").clicked() {
                let cur = match self.source_kind {
                    SourceKind::Sample(p) | SourceKind::LiveMotion(p) => p,
                    _ => SamplePreset::CyberSculpture,
                };
                self.load_sample(cur, !is_live);
            }

            ui.separator();

            if workstation_toggle(ui, &pal, self.params.oled_black, "OLED #000000")
                .on_hover_text("True #000000 pitch-black canvas (Key: B)")
                .clicked()
            {
                self.params.oled_black = !self.params.oled_black;
                if self.params.oled_black {
                    self.params.transparent_bg = false;
                }
                self.ascii_dirty = true;
            }

            if workstation_toggle(ui, &pal, self.params.transparent_bg, "Alpha RGBA")
                .on_hover_text("Transparent RGBA [0,0,0,0] background cutout")
                .clicked()
            {
                self.params.transparent_bg = !self.params.transparent_bg;
                if self.params.transparent_bg {
                    self.params.oled_black = false;
                }
                self.ascii_dirty = true;
            }

            if workstation_toggle(ui, &pal, self.params.wireframe_only, "Wireframe")
                .on_hover_text("Contour-only edge extraction (Key: N)")
                .clicked()
            {
                self.params.wireframe_only = !self.params.wireframe_only;
                self.ascii_dirty = true;
            }

            if workstation_toggle(ui, &pal, self.params.crt_bloom, "CRT Bloom").clicked() {
                self.params.crt_bloom = !self.params.crt_bloom;
                self.ascii_dirty = true;
            }

            ui.separator();

            if workstation_toggle(ui, &pal, self.split_enabled, "Split-Wipe")
                .on_hover_text("Source / ASCII comparison curtain (Key: W)")
                .clicked()
            {
                self.split_enabled = !self.split_enabled;
                if self.split_enabled {
                    self.source_dirty = true;
                }
            }

            // Right side: viewport zoom presets + Zen mode
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if workstation_toggle(ui, &pal, self.zen_mode, "Zen")
                    .on_hover_text("Hide inspector and status bars (Key: Tab)")
                    .clicked()
                {
                    self.zen_mode = !self.zen_mode;
                }

                ui.separator();

                for &(label, z) in &[("350%", 3.5f32), ("200%", 2.0), ("100%", 1.0)] {
                    let active = !self.auto_fit && (self.zoom - z).abs() < 0.08;
                    if workstation_toggle(ui, &pal, active, label).clicked() {
                        self.auto_fit = false;
                        self.zoom = z;
                        self.ascii_dirty = true;
                    }
                }
                if workstation_toggle(ui, &pal, self.auto_fit, "Fit").clicked() {
                    self.auto_fit = true;
                    self.pan_offset = Vec2::ZERO;
                }
            });
        });
    }

    fn draw_inspector_dock(&mut self, ui: &mut egui::Ui) {
        let pal = self.palette;

        // Segmented workspace tab bar at top of Inspector
        ui.horizontal(|ui| {
            let total_w = ui.available_width();
            let tab_w = (total_w - 12.0) / (InspectorTab::ALL.len() as f32);
            ui.spacing_mut().item_spacing.x = 4.0;
            for tab in InspectorTab::ALL {
                let active = self.inspector_tab == tab;
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(tab_w, 23.0), Sense::click());
                let fill = if active {
                    pal.accent_fill
                } else if resp.hovered() {
                    pal.control_hover
                } else {
                    pal.section_header_bg
                };
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(pal.radius), fill);
                ui.painter().rect_stroke(
                    rect,
                    CornerRadius::same(pal.radius),
                    Stroke::new(
                        1.0_f32,
                        if active {
                            pal.accent
                        } else {
                            pal.border_subtle
                        },
                    ),
                    StrokeKind::Inside,
                );
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    tab.label(),
                    plex_medium(11.3),
                    if active {
                        pal.accent_text
                    } else {
                        pal.text_secondary
                    },
                );
                if resp.clicked() {
                    self.inspector_tab = tab;
                }
            }
        });

        ui.add_space(4.0);

        let show_grading = matches!(
            self.inspector_tab,
            InspectorTab::All | InspectorTab::Grading
        );
        let show_geom = matches!(
            self.inspector_tab,
            InspectorTab::All | InspectorTab::Geometry
        );
        let show_output = matches!(self.inspector_tab, InspectorTab::All | InspectorTab::Output);

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;

                if show_grading {
                    inspector_section(ui, &pal, "CANVAS, MATTE & AUDIO", |ui| {
                        let mut c = false;
                        ui.horizontal(|ui| {
                            if ui
                                .checkbox(&mut self.params.oled_black, "OLED #000000")
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
                            c |= ui
                                .checkbox(&mut self.params.wireframe_only, "Wireframe")
                                .changed();
                        });

                        c |= scrub_row_f32(
                            ui,
                            &pal,
                            "Shadow Gate",
                            &mut self.params.black_gate,
                            0.00..=0.25,
                            0.03,
                            3,
                        );
                        if self.params.wireframe_only {
                            c |= scrub_row_f32(
                                ui,
                                &pal,
                                "Contour Sens.",
                                &mut self.params.wireframe_sensitivity,
                                0.35..=2.50,
                                1.15,
                                2,
                            );
                        }
                        c |= scrub_row_f32(
                            ui,
                            &pal,
                            "Audio Pulse",
                            &mut self.params.audio_reactive,
                            0.00..=2.00,
                            0.00,
                            2,
                        );
                        if self.params.audio_reactive > 0.001 {
                            let meter =
                                (self.live_audio_energy * self.params.audio_reactive * 0.75)
                                    .clamp(0.0, 1.0);
                            draw_level_meter(ui, &pal, "Beat Envelope", meter);
                        }
                        if c {
                            self.ascii_dirty = true;
                        }
                    });

                    inspector_section(ui, &pal, "COLOR GRADING & TRI-TONE", |ui| {
                        let mut c = false;
                        property_combo_row(ui, &pal, "Color Mode", |ui| {
                            egui::ComboBox::from_id_salt("color_mode_combo")
                                .selected_text(self.params.color_mode.label())
                                .width(ui.available_width())
                                .show_ui(ui, |ui| {
                                    for m in ColorMode::ALL {
                                        if ui
                                            .selectable_value(
                                                &mut self.params.color_mode,
                                                m,
                                                m.label(),
                                            )
                                            .changed()
                                        {
                                            c = true;
                                        }
                                    }
                                });
                        });

                        if self.params.color_mode == ColorMode::TriTone {
                            draw_tritone_bar(ui, &pal, &self.params.tritone);
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Stops").size(11.3).color(pal.text_secondary),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    c |= ui
                                        .color_edit_button_srgb(&mut self.params.tritone.highlight)
                                        .on_hover_text("Highlight")
                                        .changed();
                                    ui.label(
                                        RichText::new("High").size(10.8).color(pal.text_muted),
                                    );
                                    c |= ui
                                        .color_edit_button_srgb(&mut self.params.tritone.midtone)
                                        .on_hover_text("Midtone")
                                        .changed();
                                    ui.label(RichText::new("Mid").size(10.8).color(pal.text_muted));
                                    c |= ui
                                        .color_edit_button_srgb(&mut self.params.tritone.shadow)
                                        .on_hover_text("Shadow")
                                        .changed();
                                    ui.label(
                                        RichText::new("Shad").size(10.8).color(pal.text_muted),
                                    );
                                });
                            });
                            ui.horizontal(|ui| {
                                for (lbl, preset_pal) in TriTonePalette::PRESETS {
                                    if ui.small_button(lbl).clicked() {
                                        self.params.tritone = preset_pal;
                                        c = true;
                                    }
                                }
                            });
                        }

                        c |= scrub_row_f32(
                            ui,
                            &pal,
                            "Luminance Comp",
                            &mut self.params.luminance_comp,
                            0.00..=1.00,
                            0.68,
                            2,
                        );
                        if self.params.color_mode == ColorMode::DualColor
                            && !self.params.transparent_bg
                        {
                            c |= scrub_row_f32(
                                ui,
                                &pal,
                                "Dual-BG Depth",
                                &mut self.params.bg_intensity,
                                0.04..=0.55,
                                0.26,
                                2,
                            );
                        }
                        c |= scrub_row_f32(
                            ui,
                            &pal,
                            "Brightness",
                            &mut self.params.brightness,
                            -0.40..=0.40,
                            0.02,
                            2,
                        );
                        c |= scrub_row_f32(
                            ui,
                            &pal,
                            "Contrast",
                            &mut self.params.contrast,
                            0.50..=2.40,
                            1.18,
                            2,
                        );
                        c |= scrub_row_f32(
                            ui,
                            &pal,
                            "Saturation",
                            &mut self.params.saturation,
                            0.00..=2.20,
                            1.22,
                            2,
                        );
                        c |= scrub_row_f32(
                            ui,
                            &pal,
                            "Gamma",
                            &mut self.params.gamma,
                            0.45..=2.00,
                            0.96,
                            2,
                        );
                        c |= scrub_row_f32(
                            ui,
                            &pal,
                            "Bayer Dither",
                            &mut self.params.dither,
                            0.00..=1.00,
                            0.08,
                            2,
                        );

                        ui.horizontal(|ui| {
                            c |= ui.checkbox(&mut self.params.crt_bloom, "Bloom").changed();
                            c |= ui
                                .checkbox(&mut self.params.scanlines, "Scanlines")
                                .changed();
                            c |= ui.checkbox(&mut self.params.invert, "Invert").changed();
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.small_button("Reset").clicked() {
                                    let cols = self.params.cols;
                                    self.params = RenderParams {
                                        cols,
                                        ..RenderParams::default()
                                    };
                                    c = true;
                                }
                            });
                        });

                        if c {
                            self.ascii_dirty = true;
                        }
                    });
                }

                if show_geom {
                    inspector_section(ui, &pal, "TYPOGRAPHY & 8D SHAPE TENSOR", |ui| {
                        let mut c = false;
                        c |= scrub_row_u32(
                            ui,
                            &pal,
                            "Grid Columns",
                            &mut self.params.cols,
                            40..=320,
                            self.prefs.default_cols,
                        );

                        property_combo_row(ui, &pal, "Character Set", |ui| {
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
                                            c = true;
                                        }
                                    }
                                });
                        });

                        if self.params.charset == CharsetPreset::Custom
                            && ui
                                .add(
                                    egui::TextEdit::singleline(&mut self.params.custom_chars)
                                        .font(mono_font(11.2))
                                        .desired_width(ui.available_width()),
                                )
                                .changed()
                        {
                            c = true;
                        }

                        c |= scrub_row_f32(
                            ui,
                            &pal,
                            "Dir. Crunch",
                            &mut self.params.directional_crunch,
                            0.00..=3.00,
                            1.40,
                            2,
                        );
                        c |= scrub_row_f32(
                            ui,
                            &pal,
                            "Edge Tensor",
                            &mut self.params.edge_weight,
                            0.00..=2.00,
                            0.92,
                            2,
                        );
                        c |= scrub_row_f32(
                            ui,
                            &pal,
                            "Anti-Flicker",
                            &mut self.params.temporal_stability,
                            0.00..=0.45,
                            0.16,
                            2,
                        );
                        c |= scrub_row_f32(
                            ui,
                            &pal,
                            "Vignette Focus",
                            &mut self.params.vignette_focus,
                            0.00..=1.00,
                            0.00,
                            2,
                        );

                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(&self.atlas.font_name)
                                    .size(11.0)
                                    .color(pal.text_muted),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.small_button("Load Font...").clicked() {
                                    self.open_font_dialog();
                                }
                            });
                        });

                        if c {
                            self.ascii_dirty = true;
                        }
                    });
                }

                // Look Code & Active Spectrum
                if show_grading || show_output {
                    inspector_section(ui, &pal, "LOOK CODE (lg2:)", |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.look_code_buf)
                                .font(mono_font(10.6))
                                .desired_width(ui.available_width()),
                        );
                        ui.horizontal(|ui| {
                            if ui.small_button("Copy Look").clicked() {
                                let code = self.params.to_look_code();
                                self.look_code_buf = code.clone();
                                ui.ctx().copy_text(code);
                                self.set_toast("Copied lg2: Look Code");
                            }
                            if ui.small_button("Apply Code").clicked() {
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
                }

                if show_geom && self.inspector_tab == InspectorTab::Geometry {
                    inspector_section(ui, &pal, "ACTIVE GLYPH DISTRIBUTION", |ui| {
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
                            pairs.iter().take(36).map(|&(ch, _)| ch as char).collect();
                        ui.label(
                            RichText::new(format!("{} non-space characters active:", pairs.len()))
                                .size(11.0)
                                .color(pal.text_muted),
                        );
                        Frame::new()
                            .fill(pal.track_bg)
                            .stroke(Stroke::new(1.0_f32, pal.border_subtle))
                            .corner_radius(CornerRadius::same(pal.radius))
                            .inner_margin(Margin::symmetric(8, 5))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(if top_preview.is_empty() {
                                        "(empty)"
                                    } else {
                                        &top_preview
                                    })
                                    .font(mono_font(12.0))
                                    .color(pal.text_primary),
                                );
                            });
                    });
                }

                if show_output {
                    inspector_section(ui, &pal, "EXPORT & TRANSCODE", |ui| {
                        ui.horizontal(|ui| {
                            if ui.button("Copy Text").clicked() {
                                ui.ctx().copy_text(self.ascii_frame.to_plain_text());
                                self.set_toast("Copied plain ASCII text");
                            }
                            if ui.button("Copy 24-bit ANSI").clicked() {
                                ui.ctx().copy_text(frame_to_ansi_string(
                                    &self.ascii_frame,
                                    &self.params,
                                ));
                                self.set_toast("Copied TrueColor ANSI escape sequence");
                            }
                        });

                        ui.horizontal_wrapped(|ui| {
                            if ui.button("PNG").clicked() {
                                self.save_png_dialog();
                            }
                            if ui.button("SVG").clicked() {
                                if let Some(p) = rfd::FileDialog::new()
                                    .set_file_name("lumaglyph.svg")
                                    .add_filter("SVG", &["svg"])
                                    .save_file()
                                {
                                    match export_svg(&self.ascii_frame, &self.params, &p) {
                                        Ok(()) => self.set_toast("Exported vector SVG"),
                                        Err(e) => self.set_toast(e),
                                    }
                                }
                            }
                            if ui.button("HTML").clicked() {
                                if let Some(p) = rfd::FileDialog::new()
                                    .set_file_name("lumaglyph.html")
                                    .add_filter("HTML", &["html"])
                                    .save_file()
                                {
                                    match export_html(&self.ascii_frame, &self.params, &p) {
                                        Ok(()) => self.set_toast("Exported standalone HTML"),
                                        Err(e) => self.set_toast(e),
                                    }
                                }
                            }
                            if ui.button(".ANS").clicked() {
                                if let Some(p) = rfd::FileDialog::new()
                                    .set_file_name("lumaglyph.ans")
                                    .add_filter("ANSI Art", &["ans"])
                                    .save_file()
                                {
                                    match export_ansi(&self.ascii_frame, &self.params, &p) {
                                        Ok(()) => self.set_toast("Exported 24-bit .ans"),
                                        Err(e) => self.set_toast(e),
                                    }
                                }
                            }
                            if ui.button(".TXT").clicked() {
                                if let Some(p) = rfd::FileDialog::new()
                                    .set_file_name("lumaglyph.txt")
                                    .add_filter("Text", &["txt"])
                                    .save_file()
                                {
                                    match export_txt(&self.ascii_frame, &p) {
                                        Ok(()) => self.set_toast("Exported plain .txt"),
                                        Err(e) => self.set_toast(e),
                                    }
                                }
                            }
                        });

                        if let SourceKind::VideoFile(stream) = &self.source_kind {
                            let in_path = stream.path.clone();
                            let est_frames = stream
                                .meta
                                .duration_secs
                                .map(|d| (d * stream.meta.fps).round() as u64)
                                .unwrap_or(0);
                            ui.separator();
                            let default_ext = if self.params.transparent_bg {
                                "lumaglyph_alpha.webm"
                            } else {
                                "lumaglyph_ascii.mp4"
                            };
                            if let Some(job) = &self.transcode_job {
                                let done = job.progress.frames_done.load(Ordering::Relaxed);
                                let frac = if job.progress.total_frames_est > 0 {
                                    (done as f32 / job.progress.total_frames_est as f32)
                                        .clamp(0.0, 1.0)
                                } else {
                                    0.5
                                };
                                draw_level_meter(ui, &pal, &format!("Encoding #{done}"), frac);
                                if ui.small_button("Cancel").clicked() {
                                    job.progress.cancel_requested.store(true, Ordering::Relaxed);
                                }
                            } else if ui
                                .button("Transcode Video (.mp4 / .webm / .mov / .gif)")
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
                }
            });
    }

    fn draw_viewport(&mut self, ui: &mut egui::Ui) {
        let pal = self.palette;
        let avail = ui.available_rect_before_wrap();
        let (response, painter) = ui.allocate_painter(avail.size(), Sense::click_and_drag());

        let surround = if self.params.oled_black {
            Color32::BLACK
        } else {
            pal.viewport_bg
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
                .map(|p| (p.x - split_x).abs() < 16.0 && dest_rect.contains(p))
                .unwrap_or(false);

        if near_split {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }

        if response.dragged() {
            if self.split_enabled
                && pointer_pos
                    .map(|p| (p.x - split_x).abs() < 40.0)
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

        if self.params.transparent_bg {
            draw_alpha_checkerboard(
                &painter,
                dest_rect.intersect(avail),
                self.prefs.checker_contrast,
                pal.is_light,
            );
        }

        if self.prefs.show_canvas_border && !self.params.oled_black {
            painter.rect_stroke(
                dest_rect.expand(1.0),
                CornerRadius::same(1),
                Stroke::new(1.0_f32, pal.border),
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

                // Precision hairline wipe rule + rectangular grip
                let line_top = dest_rect.top().max(avail.top());
                let line_bot = dest_rect.bottom().min(avail.bottom());
                if line_bot > line_top {
                    let rule_col = Color32::from_rgba_unmultiplied(220, 222, 228, 195);
                    painter.line_segment(
                        [Pos2::new(wipe_x, line_top), Pos2::new(wipe_x, line_bot)],
                        Stroke::new(1.0_f32, rule_col),
                    );
                    let grip = Rect::from_center_size(
                        Pos2::new(wipe_x, (line_top + line_bot) * 0.5),
                        Vec2::new(14.0, 26.0),
                    );
                    painter.rect_filled(
                        grip,
                        CornerRadius::same(2),
                        Color32::from_rgba_unmultiplied(20, 20, 23, 232),
                    );
                    painter.rect_stroke(
                        grip,
                        CornerRadius::same(2),
                        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(180, 184, 192, 210)),
                        StrokeKind::Inside,
                    );
                    for dx in [-2.0f32, 2.0] {
                        painter.line_segment(
                            [
                                Pos2::new(grip.center().x + dx, grip.top() + 6.0),
                                Pos2::new(grip.center().x + dx, grip.bottom() - 6.0),
                            ],
                            Stroke::new(1.0_f32, Color32::from_rgb(175, 178, 186)),
                        );
                    }

                    let badge_y = line_top + 10.0;
                    let visible_left = dest_rect.left().max(avail.left());
                    let visible_right = dest_rect.right().min(avail.right());
                    if wipe_x - visible_left > 84.0 {
                        draw_viewport_tag(
                            &painter,
                            Pos2::new(visible_left + 10.0, badge_y),
                            "SOURCE",
                        );
                    }
                    if visible_right - wipe_x > 160.0 {
                        let mode_tag = if self.params.wireframe_only && self.params.transparent_bg {
                            "ASCII  |  WIREFRAME RGBA"
                        } else if self.params.oled_black {
                            "ASCII  |  OLED #000000"
                        } else if self.params.color_mode == ColorMode::TriTone {
                            "ASCII  |  TRI-TONE"
                        } else {
                            "ASCII  |  95-CHAR OUTPUT"
                        };
                        draw_viewport_tag(&painter, Pos2::new(wipe_x + 10.0, badge_y), mode_tag);
                    }
                }
            }
        }

        if self.prefs.show_cell_hud {
            if let Some(hover) = response.hover_pos() {
                if dest_rect.contains(hover)
                    && self.ascii_frame.cols > 0
                    && self.ascii_frame.rows > 0
                {
                    let rel_x =
                        ((hover.x - dest_rect.left()) / dest_rect.width()).clamp(0.0, 0.9999);
                    let rel_y =
                        ((hover.y - dest_rect.top()) / dest_rect.height()).clamp(0.0, 0.9999);
                    let c = (rel_x * self.ascii_frame.cols as f32) as usize;
                    let r = (rel_y * self.ascii_frame.rows as f32) as usize;
                    let cell = self.ascii_frame.cells[r * self.ascii_frame.cols as usize + c];
                    let bg_desc = if self.params.transparent_bg {
                        "RGBA(0,0,0,0)".to_string()
                    } else {
                        format!("#{:02X}{:02X}{:02X}", cell.bg[0], cell.bg[1], cell.bg[2])
                    };
                    let info = format!(
                        "CELL ({c:03}, {r:03})   CHAR '{}' (0x{:02X})   FG #{:02X}{:02X}{:02X}   BG {bg_desc}",
                        cell.ch as char, cell.ch, cell.fg[0], cell.fg[1], cell.fg[2]
                    );
                    draw_viewport_tag(
                        &painter,
                        Pos2::new(avail.left() + 10.0, avail.bottom() - 28.0),
                        &info,
                    );
                }
            }
        }

        if let Some((msg, when)) = &self.status_toast {
            if when.elapsed().as_secs_f32() < 4.5 {
                draw_viewport_tag(
                    &painter,
                    Pos2::new(avail.left() + 10.0, avail.top() + 10.0),
                    msg,
                );
            } else {
                self.status_toast = None;
            }
        }
    }

    fn draw_status_strip(&mut self, ui: &mut egui::Ui) {
        let pal = self.palette;
        ui.horizontal(|ui| {
            match &mut self.source_kind {
                SourceKind::LiveMotion(preset) => {
                    let p_label = preset.label();
                    if ui
                        .small_button(if self.motion_paused { "Play" } else { "Pause" })
                        .clicked()
                    {
                        self.motion_paused = !self.motion_paused;
                    }
                    ui.label(
                        RichText::new(format!("LIVE 60FPS  |  {p_label}"))
                            .font(mono_font(10.8))
                            .color(pal.text_secondary),
                    );
                }
                SourceKind::VideoFile(stream) => {
                    let is_paused = stream.paused.load(Ordering::Relaxed);
                    if ui
                        .small_button(if is_paused { "Play" } else { "Pause" })
                        .clicked()
                    {
                        stream.paused.store(!is_paused, Ordering::Relaxed);
                    }
                    ui.label(
                        RichText::new(format!(
                            "VIDEO  |  {}x{} @ {:.1} FPS  |  FRAME #{}  |  BEAT {:02.0}%",
                            stream.meta.width,
                            stream.meta.height,
                            stream.meta.fps,
                            self.video_frames_seen,
                            (self.live_audio_energy * 100.0).min(100.0)
                        ))
                        .font(mono_font(10.8))
                        .color(pal.text_secondary),
                    );
                }
                SourceKind::StaticImage { name } => {
                    ui.label(
                        RichText::new(format!(
                            "IMAGE  |  {name} ({}x{})  |  ZOOM {:.0}%",
                            self.source_w,
                            self.source_h,
                            self.zoom * 100.0
                        ))
                        .font(mono_font(10.8))
                        .color(pal.text_muted),
                    );
                }
                SourceKind::Sample(p) => {
                    ui.label(
                        RichText::new(format!(
                            "PLATE  |  {} ({}x{})  |  ZOOM {:.0}%",
                            p.label(),
                            self.source_w,
                            self.source_h,
                            self.zoom * 100.0
                        ))
                        .font(mono_font(10.8))
                        .color(pal.text_muted),
                    );
                }
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let ff = self
                    .ffmpeg_info
                    .as_deref()
                    .unwrap_or("ffmpeg not on PATH (Image/GIF active)");
                ui.label(
                    RichText::new(format!(
                        "{}   |   Theme: {}",
                        ff,
                        self.prefs.theme.cli_name()
                    ))
                    .font(mono_font(10.6))
                    .color(pal.text_muted),
                );
            });
        });
    }

    fn draw_settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings_window {
            return;
        }
        let pal = self.palette;
        let mut open = self.show_settings_window;

        egui::Window::new("Workstation Settings")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .fixed_size([736.0, 480.0])
            .frame(
                Frame::window(&ctx.style())
                    .fill(pal.panel_bg)
                    .stroke(Stroke::new(1.0_f32, pal.border))
                    .corner_radius(CornerRadius::same(pal.radius.max(2)))
                    .inner_margin(Margin::same(10)),
            )
            .show(ctx, |ui| {
                let body_h = 464.0;
                ui.set_min_size(Vec2::new(716.0, body_h));
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 14.0;

                    // Full-height left navigation rail (explicit vertical layout inside horizontal_top)
                    ui.allocate_ui_with_layout(
                        Vec2::new(172.0, body_h),
                        Layout::top_down(Align::Min),
                        |ui| {
                            Frame::new()
                                .fill(pal.section_header_bg)
                                .stroke(Stroke::new(1.0_f32, pal.border_subtle))
                                .corner_radius(CornerRadius::same(pal.radius))
                                .inner_margin(Margin::same(8))
                                .show(ui, |ui| {
                                    ui.set_min_size(Vec2::new(156.0, body_h - 16.0));
                                    ui.set_max_width(156.0);
                                    ui.spacing_mut().item_spacing.y = 4.0;
                                    for tab in SettingsTab::ALL {
                                        let active = self.settings_tab == tab;
                                        let (rect, resp) = ui.allocate_exact_size(
                                            Vec2::new(156.0, 28.0),
                                            Sense::click(),
                                        );
                                        let fill = if active {
                                            pal.accent_fill
                                        } else if resp.hovered() {
                                            pal.control_hover
                                        } else {
                                            Color32::TRANSPARENT
                                        };
                                        ui.painter().rect_filled(
                                            rect,
                                            CornerRadius::same(pal.radius),
                                            fill,
                                        );
                                        if active {
                                            let bar = Rect::from_min_size(
                                                rect.left_top() + Vec2::new(0.0, 5.0),
                                                Vec2::new(2.5, rect.height() - 10.0),
                                            );
                                            ui.painter().rect_filled(bar, 0.0, pal.accent);
                                        }
                                        ui.painter().text(
                                            Pos2::new(rect.left() + 10.0, rect.center().y),
                                            egui::Align2::LEFT_CENTER,
                                            tab.label(),
                                            if active {
                                                plex_medium(12.2)
                                            } else {
                                                egui::FontId::proportional(12.2)
                                            },
                                            if active {
                                                pal.accent_text
                                            } else {
                                                pal.text_secondary
                                            },
                                        );
                                        if resp.clicked() {
                                            self.settings_tab = tab;
                                        }
                                    }

                                    let rem = ui.available_height() - 42.0;
                                    if rem > 4.0 {
                                        ui.add_space(rem);
                                    }
                                    ui.separator();
                                    ui.label(
                                        RichText::new(format!("LumaGlyph v{APP_VERSION}"))
                                            .font(mono_font(10.5))
                                            .color(pal.text_secondary),
                                    );
                                    ui.label(
                                        RichText::new(format!("rev {BUILD_GIT_SHA}"))
                                            .font(mono_font(10.5))
                                            .color(pal.text_muted),
                                    );
                                });
                        },
                    );

                    // Right content pane
                    ui.allocate_ui_with_layout(
                        Vec2::new(530.0, body_h),
                        Layout::top_down(Align::Min),
                        |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(body_h)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.set_min_width(518.0);
                                    ui.spacing_mut().item_spacing.y = 8.0;
                                    match self.settings_tab {
                                        SettingsTab::Appearance => {
                                            self.draw_settings_appearance(ui)
                                        }
                                        SettingsTab::EngineDefaults => {
                                            self.draw_settings_engine(ui)
                                        }
                                        SettingsTab::ExportVideo => self.draw_settings_export(ui),
                                        SettingsTab::Shortcuts => self.draw_settings_shortcuts(ui),
                                        SettingsTab::AboutBuild => self.draw_settings_about(ui),
                                    }
                                });
                        },
                    );
                });
            });

        self.show_settings_window = open;
    }

    fn draw_settings_appearance(&mut self, ui: &mut egui::Ui) {
        let pal = self.palette;
        let mut changed = false;

        ui.label(
            RichText::new("Workstation Theme")
                .font(plex_medium(13.0))
                .color(pal.text_primary),
        );
        ui.label(
            RichText::new("Neutral calibrated surfaces ensure UI chrome never competes with canvas color grading.")
                .size(11.3)
                .color(pal.text_muted),
        );

        // Visual theme swatch cards
        for t in StudioTheme::ALL {
            let preview = resolve_palette(t, self.prefs.accent, self.prefs.corner_style);
            let active = self.prefs.theme == t;
            let (rect, resp) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());
            let bg = if active {
                pal.accent_fill
            } else if resp.hovered() {
                pal.control_hover
            } else {
                pal.control_bg
            };
            ui.painter()
                .rect_filled(rect, CornerRadius::same(pal.radius), bg);
            ui.painter().rect_stroke(
                rect,
                CornerRadius::same(pal.radius),
                Stroke::new(1.0_f32, if active { pal.accent } else { pal.border }),
                StrokeKind::Inside,
            );

            // 3 surface chips on left
            for (idx, chip_col) in [preview.viewport_bg, preview.panel_bg, preview.accent]
                .into_iter()
                .enumerate()
            {
                let chip = Rect::from_min_size(
                    Pos2::new(
                        rect.left() + 8.0 + (idx as f32) * 16.0,
                        rect.center().y - 6.0,
                    ),
                    Vec2::new(13.0, 12.0),
                );
                ui.painter()
                    .rect_filled(chip, CornerRadius::same(1), chip_col);
                ui.painter().rect_stroke(
                    chip,
                    CornerRadius::same(1),
                    Stroke::new(1.0_f32, pal.border),
                    StrokeKind::Inside,
                );
            }

            ui.painter().text(
                Pos2::new(rect.left() + 64.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                t.label(),
                if active {
                    plex_medium(11.8)
                } else {
                    egui::FontId::proportional(11.8)
                },
                if active {
                    pal.accent_text
                } else {
                    pal.text_primary
                },
            );

            if resp.clicked() && !active {
                self.prefs.theme = t;
                changed = true;
            }
        }

        ui.separator();

        property_combo_row(ui, &pal, "Indicator Accent", |ui| {
            egui::ComboBox::from_id_salt("pref_accent")
                .selected_text(self.prefs.accent.label())
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for a in AccentPreset::ALL {
                        if ui
                            .selectable_value(&mut self.prefs.accent, a, a.label())
                            .changed()
                        {
                            changed = true;
                        }
                    }
                });
        });

        property_combo_row(ui, &pal, "Interface Density", |ui| {
            egui::ComboBox::from_id_salt("pref_density")
                .selected_text(self.prefs.ui_density.label())
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for d in UiDensity::ALL {
                        if ui
                            .selectable_value(&mut self.prefs.ui_density, d, d.label())
                            .changed()
                        {
                            changed = true;
                        }
                    }
                });
        });

        property_combo_row(ui, &pal, "Corner Geometry", |ui| {
            egui::ComboBox::from_id_salt("pref_corners")
                .selected_text(self.prefs.corner_style.label())
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for cs in CornerStyle::ALL {
                        if ui
                            .selectable_value(&mut self.prefs.corner_style, cs, cs.label())
                            .changed()
                        {
                            changed = true;
                        }
                    }
                });
        });

        property_combo_row(ui, &pal, "Alpha Checkerboard", |ui| {
            egui::ComboBox::from_id_salt("pref_checker")
                .selected_text(self.prefs.checker_contrast.label())
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for cc in CheckerContrast::ALL {
                        if ui
                            .selectable_value(&mut self.prefs.checker_contrast, cc, cc.label())
                            .changed()
                        {
                            changed = true;
                        }
                    }
                });
        });

        ui.separator();
        changed |= ui
            .checkbox(
                &mut self.prefs.show_cell_hud,
                "Show hover cell inspector readout on canvas",
            )
            .changed();
        changed |= ui
            .checkbox(
                &mut self.prefs.show_canvas_border,
                "Draw 1 px frame hairline around active canvas",
            )
            .changed();
        changed |= ui
            .checkbox(
                &mut self.prefs.show_status_bar,
                "Show bottom media transport & status strip",
            )
            .changed();

        if changed {
            self.persist_prefs();
        }
    }

    fn draw_settings_engine(&mut self, ui: &mut egui::Ui) {
        let pal = self.palette;
        let mut changed = false;

        ui.label(
            RichText::new("Engine Calibration & Startup Defaults")
                .font(plex_medium(13.0))
                .color(pal.text_primary),
        );

        changed |= scrub_row_u32(
            ui,
            &pal,
            "Startup Columns",
            &mut self.prefs.default_cols,
            40..=320,
            156,
        );

        property_combo_row(ui, &pal, "Default Charset", |ui| {
            egui::ComboBox::from_id_salt("pref_def_cs")
                .selected_text(self.prefs.default_charset.label())
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for cs in CharsetPreset::ALL {
                        if ui
                            .selectable_value(&mut self.prefs.default_charset, cs, cs.label())
                            .changed()
                        {
                            changed = true;
                        }
                    }
                });
        });

        property_combo_row(ui, &pal, "Default Color Mode", |ui| {
            egui::ComboBox::from_id_salt("pref_def_cm")
                .selected_text(self.prefs.default_color_mode.label())
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for cm in ColorMode::ALL {
                        if ui
                            .selectable_value(&mut self.prefs.default_color_mode, cm, cm.label())
                            .changed()
                        {
                            changed = true;
                        }
                    }
                });
        });

        changed |= ui
            .checkbox(
                &mut self.prefs.default_oled_black,
                "Start new sessions in True OLED #000000 mode",
            )
            .changed();

        ui.separator();
        ui.label(
            RichText::new("8D Glyph Atlas Raster Resolution")
                .font(plex_medium(12.2))
                .color(pal.text_primary),
        );
        ui.horizontal(|ui| {
            for (h, lbl) in [
                (16usize, "16 px (Compact)"),
                (20usize, "20 px (Standard)"),
                (28usize, "28 px (Ultra Crisp)"),
            ] {
                if ui
                    .radio_value(&mut self.prefs.atlas_cell_height, h, lbl)
                    .changed()
                {
                    self.atlas = self.atlas.with_cell_height(h);
                    self.ascii_dirty = true;
                    changed = true;
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("Active Font: {}", self.atlas.font_name))
                    .font(mono_font(11.2))
                    .color(pal.text_secondary),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button("Reset to JetBrains Mono").clicked() {
                    self.atlas =
                        GlyphAtlas::default_embedded(self.prefs.atlas_cell_height.clamp(12, 36));
                    self.ascii_dirty = true;
                }
                if ui.small_button("Load .TTF / .OTF...").clicked() {
                    self.open_font_dialog();
                }
            });
        });

        ui.separator();
        if ui
            .button("Apply Startup Defaults to Current Canvas")
            .clicked()
        {
            self.params.cols = self.prefs.default_cols;
            self.params.charset = self.prefs.default_charset;
            self.params.color_mode = self.prefs.default_color_mode;
            self.params.oled_black = self.prefs.default_oled_black;
            self.ascii_dirty = true;
        }

        if changed {
            self.persist_prefs();
        }
    }

    fn draw_settings_export(&mut self, ui: &mut egui::Ui) {
        let pal = self.palette;
        let mut changed = false;

        ui.label(
            RichText::new("Raster & Streaming Video Pipeline")
                .font(plex_medium(13.0))
                .color(pal.text_primary),
        );

        ui.label(
            RichText::new("PNG Export Scale")
                .size(11.5)
                .color(pal.text_secondary),
        );
        ui.horizontal(|ui| {
            for (preset, lbl) in [
                (1u8, "1x (20 px/cell)"),
                (2u8, "2x HiDPI (36 px/cell)"),
                (3u8, "3x Print (54 px/cell)"),
            ] {
                if ui
                    .radio_value(&mut self.prefs.png_scale_preset, preset, lbl)
                    .changed()
                {
                    changed = true;
                }
            }
        });

        let mut crf_u32 = self.prefs.video_crf as u32;
        if scrub_row_u32(ui, &pal, "H.264 Video CRF", &mut crf_u32, 14..=28, 18) {
            self.prefs.video_crf = crf_u32 as u8;
            changed = true;
        }
        ui.label(
            RichText::new("Lower CRF preserves sharper character edges in MP4 exports (14 = studio master, 18 = high quality, 24 = compact).")
                .size(11.0)
                .color(pal.text_muted),
        );

        ui.separator();
        ui.label(
            RichText::new("FFmpeg / FFprobe Binary Directory (Optional)")
                .font(plex_medium(12.2))
                .color(pal.text_primary),
        );
        ui.label(
            RichText::new("Leave blank to auto-detect alongside executable or system PATH.")
                .size(11.0)
                .color(pal.text_muted),
        );
        ui.horizontal(|ui| {
            if ui
                .add(
                    egui::TextEdit::singleline(&mut self.prefs.ffmpeg_custom_dir)
                        .font(mono_font(11.2))
                        .desired_width(ui.available_width() - 140.0),
                )
                .changed()
            {
                changed = true;
            }
            if ui.button("Browse...").clicked() {
                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                    self.prefs.ffmpeg_custom_dir = dir.to_string_lossy().into_owned();
                    changed = true;
                }
            }
            if ui.button("Detect").clicked() {
                set_video_pipeline_prefs(&self.prefs.ffmpeg_custom_dir, self.prefs.video_crf);
                self.ffmpeg_info = detect_ffmpeg_version();
            }
        });

        let status = self
            .ffmpeg_info
            .as_deref()
            .unwrap_or("Not detected on PATH");
        ui.label(
            RichText::new(format!("Detected: {status}"))
                .font(mono_font(11.0))
                .color(pal.text_secondary),
        );

        if changed {
            self.persist_prefs();
        }
    }

    fn draw_settings_shortcuts(&mut self, ui: &mut egui::Ui) {
        let pal = self.palette;
        ui.label(
            RichText::new("Keyboard & Viewport Controls")
                .font(plex_medium(13.0))
                .color(pal.text_primary),
        );

        let bindings = [
            ("Ctrl + O", "Open image or animated GIF"),
            ("Ctrl + S", "Quick export PNG at configured DPI scale"),
            (
                "Ctrl + Shift + C",
                "Copy one-line lg2: Look Code to clipboard",
            ),
            ("Ctrl + ,", "Open or close Workstation Settings"),
            (
                "Tab",
                "Toggle Zen Canvas Mode (hide side dock & status bar)",
            ),
            ("Space", "Play / pause live 60fps stream or video file"),
            ("W", "Toggle Source / ASCII Split-Wipe curtain"),
            ("B", "Toggle True OLED #000000 pitch-black canvas"),
            ("N", "Toggle Neon Wireframe contour-only mode"),
            ("[  /  ]", "Decrease / increase grid columns by 8"),
            ("0 / 1 / 2 / 3", "Zoom to Fit / 100% / 200% / 350% Loupe"),
            ("Mouse Wheel", "Smooth zoom around viewport"),
            (
                "Double-Click Label",
                "Reset individual grading fader to default",
            ),
        ];

        Frame::new()
            .fill(pal.section_header_bg)
            .stroke(Stroke::new(1.0_f32, pal.border_subtle))
            .corner_radius(CornerRadius::same(pal.radius))
            .inner_margin(Margin::symmetric(10, 8))
            .show(ui, |ui| {
                egui::Grid::new("shortcuts_grid")
                    .num_columns(2)
                    .spacing([18.0, 6.0])
                    .striped(true)
                    .show(ui, |ui| {
                        for (key, desc) in bindings {
                            ui.label(
                                RichText::new(key)
                                    .font(mono_font(11.2))
                                    .color(pal.text_primary),
                            );
                            ui.label(RichText::new(desc).size(11.6).color(pal.text_secondary));
                            ui.end_row();
                        }
                    });
            });
    }

    fn draw_settings_about(&mut self, ui: &mut egui::Ui) {
        let pal = self.palette;

        ui.horizontal(|ui| {
            ui.label(
                RichText::new("LUMAGLYPH")
                    .font(plex_medium(15.0))
                    .color(pal.text_primary),
            );
            ui.label(
                RichText::new(format!("Version {APP_VERSION}"))
                    .font(mono_font(12.0))
                    .color(pal.accent),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button("Copy Build Info").clicked() {
                    let rep = format_build_report(
                        Some(&self.gpu_renderer),
                        self.ffmpeg_info.as_deref(),
                        &self.rss_label,
                    );
                    ui.ctx().copy_text(rep);
                    self.set_toast("Copied build diagnostics to clipboard");
                }
            });
        });
        ui.label(
            RichText::new("High-throughput 95-character 8D shape-vector ASCII imaging and streaming video workstation.")
                .size(11.4)
                .color(pal.text_secondary),
        );

        Frame::new()
            .fill(pal.section_header_bg)
            .stroke(Stroke::new(1.0_f32, pal.border_subtle))
            .corner_radius(CornerRadius::same(pal.radius))
            .inner_margin(Margin::symmetric(10, 8))
            .show(ui, |ui| {
                let threads = format!("{} logical workers (Rayon)", rayon::current_num_threads());
                let ff = self
                    .ffmpeg_info
                    .as_deref()
                    .unwrap_or("Not detected on PATH");
                let rows = [
                    ("Application Version", format!("v{APP_VERSION}")),
                    ("Git Commit", BUILD_GIT_SHA.to_string()),
                    ("Target Architecture", BUILD_TARGET_TRIPLE.to_string()),
                    (
                        "Build Profile",
                        format!("{BUILD_PROFILE} (LTO thin, stripped)"),
                    ),
                    ("Rust Toolchain", BUILD_RUSTC_VERSION.to_string()),
                    ("OpenGL Renderer", self.gpu_renderer.clone()),
                    ("Compute Engine", threads),
                    (
                        "Embedded Typefaces",
                        "IBM Plex Sans + JetBrains Mono (SIL OFL 1.1)".to_string(),
                    ),
                    ("FFmpeg Backend", ff.to_string()),
                    ("Process Memory (RSS)", self.rss_label.clone()),
                    ("License", "MIT License (Open Source)".to_string()),
                ];

                egui::Grid::new("about_build_grid")
                    .num_columns(2)
                    .spacing([18.0, 5.0])
                    .striped(true)
                    .show(ui, |ui| {
                        for (k, v) in rows {
                            ui.label(RichText::new(k).size(11.4).color(pal.text_muted));
                            ui.label(
                                RichText::new(v)
                                    .font(mono_font(11.1))
                                    .color(pal.text_primary),
                            );
                            ui.end_row();
                        }
                    });
            });

        ui.separator();
        ui.label(
            RichText::new("Network Policy & GitHub Release Checks")
                .font(plex_medium(12.2))
                .color(pal.text_primary),
        );
        ui.label(
            RichText::new("100% offline by default. Zero telemetry. Network requests occur only when manually triggered or opted in below.")
                .size(11.1)
                .color(pal.text_muted),
        );

        ui.horizontal(|ui| {
            ui.label(RichText::new("Repository:").size(11.4));
            if ui
                .add(
                    egui::TextEdit::singleline(&mut self.prefs.github_repo)
                        .font(mono_font(11.2))
                        .desired_width(220.0),
                )
                .changed()
            {
                let _ = self.prefs.save();
            }
            let busy = self.update_rx.is_some();
            if ui
                .add_enabled(!busy, egui::Button::new("Check Releases"))
                .clicked()
            {
                let _ = self.prefs.save();
                self.update_rx = Some(spawn_release_check(self.prefs.github_repo.clone()));
            }
        });

        if !self
            .prefs
            .github_repo
            .trim()
            .eq_ignore_ascii_case(DEFAULT_GITHUB_REPO)
        {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!(
                        "Custom repository configured (official: {DEFAULT_GITHUB_REPO})"
                    ))
                    .size(11.0)
                    .color(pal.warn),
                );
                if ui.small_button("Reset to Official").clicked() {
                    self.prefs.github_repo = DEFAULT_GITHUB_REPO.to_string();
                    let _ = self.prefs.save();
                }
            });
        }

        if ui
            .checkbox(
                &mut self.prefs.check_updates_on_startup,
                "Check GitHub Releases automatically on launch (opt-in)",
            )
            .changed()
        {
            let _ = self.prefs.save();
        }

        if let Some(res) = &self.update_status {
            match res {
                Ok(info) => {
                    if info.update_available {
                        ui.label(
                            RichText::new(format!(
                                "New release available: {} (installed v{APP_VERSION})",
                                info.latest_tag
                            ))
                            .font(plex_medium(11.6))
                            .color(pal.warn),
                        );
                        ui.horizontal(|ui| {
                            let dl_busy = self.download_rx.is_some();
                            let dl_label = if dl_busy {
                                "Verifying SHA-256 & Downloading..."
                            } else {
                                "Download Verified Binary..."
                            };
                            if ui
                                .add_enabled(!dl_busy, egui::Button::new(dl_label))
                                .on_hover_text(
                                    "Downloads archive over HTTPS, verifies SHA256SUMS.txt in constant time, and inspects native OS/CPU binary header",
                                )
                                .clicked()
                            {
                                let default_dir = dirs::download_dir()
                                    .or_else(dirs::home_dir)
                                    .unwrap_or_else(|| PathBuf::from("."));
                                if let Some(chosen) = rfd::FileDialog::new()
                                    .set_directory(&default_dir)
                                    .pick_folder()
                                {
                                    self.download_status = None;
                                    self.download_rx = Some(spawn_verified_release_download(
                                        self.prefs.github_repo.clone(),
                                        chosen,
                                    ));
                                }
                            }
                            ui.hyperlink_to("Open GitHub Release Page", &info.html_url);
                        });
                    } else {
                        ui.label(
                            RichText::new(format!(
                                "Up to date (latest published release: {}).",
                                info.latest_tag
                            ))
                            .font(mono_font(11.1))
                            .color(pal.text_secondary),
                        );
                    }
                }
                Err(err) => {
                    ui.label(RichText::new(err).size(11.2).color(pal.warn));
                }
            }
        }

        if let Some(dl) = &self.download_status {
            match dl {
                Ok(done) => {
                    ui.label(
                        RichText::new(format!(
                            "Verified {} (SHA-256 {}...) -> {}",
                            done.tag,
                            &done.sha256_hex[..16],
                            done.saved_path.display()
                        ))
                        .font(mono_font(10.8))
                        .color(pal.accent),
                    );
                }
                Err(err) => {
                    ui.label(RichText::new(err).size(11.1).color(pal.warn));
                }
            }
        }
    }
}

impl eframe::App for LumaGlyphApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_shortcuts(ctx);
        self.refresh_pipelines(ctx);
        let pal = self.palette;

        egui::TopBottomPanel::top("workstation_menu_bar")
            .exact_height(27.0)
            .frame(
                Frame::new()
                    .fill(pal.menu_bg)
                    .inner_margin(Margin::symmetric(10, 3))
                    .stroke(Stroke::new(1.0_f32, pal.border_subtle)),
            )
            .show(ctx, |ui| self.draw_menu_strip(ui));

        if !self.zen_mode {
            egui::TopBottomPanel::top("workstation_command_strip")
                .exact_height(31.0)
                .frame(
                    Frame::new()
                        .fill(pal.toolbar_bg)
                        .inner_margin(Margin::symmetric(10, 4))
                        .stroke(Stroke::new(1.0_f32, pal.border_subtle)),
                )
                .show(ctx, |ui| self.draw_command_strip(ui));

            if self.prefs.show_status_bar {
                egui::TopBottomPanel::bottom("workstation_status_strip")
                    .exact_height(24.0)
                    .frame(
                        Frame::new()
                            .fill(pal.menu_bg)
                            .inner_margin(Margin::symmetric(10, 3))
                            .stroke(Stroke::new(1.0_f32, pal.border_subtle)),
                    )
                    .show(ctx, |ui| self.draw_status_strip(ui));
            }

            egui::SidePanel::left("workstation_inspector_dock")
                .default_width(332.0)
                .min_width(304.0)
                .max_width(390.0)
                .frame(
                    Frame::new()
                        .fill(pal.panel_bg)
                        .inner_margin(Margin::symmetric(9, 8))
                        .stroke(Stroke::new(1.0_f32, pal.border_subtle)),
                )
                .show(ctx, |ui| self.draw_inspector_dock(ui));
        }

        let central_bg = if self.params.oled_black {
            Color32::BLACK
        } else {
            pal.viewport_bg
        };
        egui::CentralPanel::default()
            .frame(Frame::new().fill(central_bg))
            .show(ctx, |ui| self.draw_viewport(ui));

        self.draw_settings_window(ctx);

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

/// Architectural flat section block with hairline border and muted medium header.
fn inspector_section(
    ui: &mut egui::Ui,
    pal: &ThemePalette,
    title: &str,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    Frame::new()
        .fill(pal.panel_bg)
        .stroke(Stroke::new(1.0_f32, pal.border_subtle))
        .corner_radius(CornerRadius::same(pal.radius))
        .inner_margin(Margin::ZERO)
        .show(ui, |ui| {
            // Subtle top title bar
            Frame::new()
                .fill(pal.section_header_bg)
                .inner_margin(Margin::symmetric(8, 4))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.label(
                        RichText::new(title)
                            .font(plex_medium(10.2))
                            .color(pal.text_secondary),
                    );
                });
            Frame::new()
                .inner_margin(Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 4.5;
                    add_contents(ui);
                });
        });
}

/// Flat hardware-style toggle button with crisp hairline border and subtle active underline.
fn workstation_toggle(
    ui: &mut egui::Ui,
    pal: &ThemePalette,
    active: bool,
    label: &str,
) -> egui::Response {
    let font = if active {
        plex_medium(11.3)
    } else {
        egui::FontId::proportional(11.3)
    };
    let text_col = if active {
        pal.accent_text
    } else {
        pal.text_secondary
    };
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font, text_col);
    let desired = Vec2::new(galley.size().x + 14.0, 21.0);
    let (rect, resp) = ui.allocate_exact_size(desired, Sense::click());

    let fill = if active {
        pal.accent_fill
    } else if resp.hovered() {
        pal.control_hover
    } else {
        pal.control_bg
    };
    let stroke_col = if active {
        pal.accent
    } else if resp.hovered() {
        pal.border
    } else {
        pal.border_subtle
    };

    ui.painter()
        .rect_filled(rect, CornerRadius::same(pal.radius), fill);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(pal.radius),
        Stroke::new(1.0_f32, stroke_col),
        StrokeKind::Inside,
    );
    if active {
        let bar = Rect::from_min_max(
            Pos2::new(rect.left() + 2.0, rect.bottom() - 2.0),
            Pos2::new(rect.right() - 2.0, rect.bottom() - 0.5),
        );
        ui.painter().rect_filled(bar, 0.0, pal.accent);
    }
    ui.painter()
        .galley(rect.center() - galley.size() * 0.5, galley, text_col);
    resp
}

fn property_combo_row(
    ui: &mut egui::Ui,
    pal: &ThemePalette,
    label: &str,
    add_right: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            Vec2::new(102.0, 20.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.label(RichText::new(label).size(11.4).color(pal.text_secondary));
            },
        );
        add_right(ui);
    });
}

/// Capture-One / DaVinci-Resolve style two-column fader row with double-click reset and tabular DragValue.
fn scrub_row_f32(
    ui: &mut egui::Ui,
    pal: &ThemePalette,
    label: &str,
    val: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    default_val: f32,
    decimals: usize,
) -> bool {
    let mut changed = false;
    let min = *range.start();
    let max = *range.end();
    let span = (max - min).max(1e-6);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;

        // 1. Property label (double-click resets to default)
        let (lbl_rect, lbl_resp) = ui.allocate_exact_size(Vec2::new(98.0, 18.0), Sense::click());
        let lbl_col = if lbl_resp.hovered() {
            pal.text_primary
        } else {
            pal.text_secondary
        };
        ui.painter().text(
            Pos2::new(lbl_rect.left(), lbl_rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(11.3),
            lbl_col,
        );
        if lbl_resp
            .on_hover_text(format!("Double-click to reset ({default_val:.decimals$})"))
            .double_clicked()
        {
            *val = default_val;
            changed = true;
        }

        // 2. Precision track fader
        let read_w = 50.0f32;
        let track_w = (ui.available_width() - read_w - 6.0).max(48.0);
        let (t_rect, t_resp) =
            ui.allocate_exact_size(Vec2::new(track_w, 18.0), Sense::click_and_drag());
        if t_resp.double_clicked() {
            *val = default_val;
            changed = true;
        } else if (t_resp.dragged() || t_resp.clicked()) && !t_resp.double_clicked() {
            if let Some(pos) = t_resp.interact_pointer_pos() {
                let norm = ((pos.x - t_rect.left()) / t_rect.width().max(1.0)).clamp(0.0, 1.0);
                *val = min + norm * span;
                changed = true;
            }
        }

        let groove = Rect::from_center_size(t_rect.center(), Vec2::new(t_rect.width(), 4.0));
        ui.painter()
            .rect_filled(groove, CornerRadius::same(1), pal.track_bg);
        ui.painter().rect_stroke(
            groove,
            CornerRadius::same(1),
            Stroke::new(1.0_f32, pal.border_subtle),
            StrokeKind::Inside,
        );

        let norm = ((*val - min) / span).clamp(0.0, 1.0);
        let zero_norm = if min < 0.0 && max > 0.0 {
            (-min / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let x_a = groove.left() + groove.width() * zero_norm.min(norm);
        let x_b = groove.left() + groove.width() * zero_norm.max(norm);
        if x_b > x_a {
            let fill_r = Rect::from_min_max(
                Pos2::new(x_a, groove.top()),
                Pos2::new(x_b, groove.bottom()),
            );
            ui.painter().rect_filled(fill_r, 0.0, pal.accent);
        }

        let thumb_x = (groove.left() + groove.width() * norm)
            .clamp(groove.left() + 3.0, groove.right() - 3.0);
        let thumb =
            Rect::from_center_size(Pos2::new(thumb_x, groove.center().y), Vec2::new(6.0, 12.0));
        ui.painter().rect_filled(
            thumb,
            CornerRadius::same(1),
            if t_resp.hovered() || t_resp.dragged() {
                pal.text_primary
            } else {
                pal.text_secondary
            },
        );

        // 3. Tabular monospace DragValue readout
        changed |= ui
            .add_sized(
                [read_w, 18.0],
                DragValue::new(val)
                    .range(range)
                    .speed(span * 0.005)
                    .fixed_decimals(decimals),
            )
            .changed();
    });

    changed
}

fn scrub_row_u32(
    ui: &mut egui::Ui,
    pal: &ThemePalette,
    label: &str,
    val: &mut u32,
    range: std::ops::RangeInclusive<u32>,
    default_val: u32,
) -> bool {
    let mut f = *val as f32;
    let changed = scrub_row_f32(
        ui,
        pal,
        label,
        &mut f,
        (*range.start() as f32)..=(*range.end() as f32),
        default_val as f32,
        0,
    );
    if changed {
        *val = (f.round() as u32).clamp(*range.start(), *range.end());
    }
    changed
}

fn draw_level_meter(ui: &mut egui::Ui, pal: &ThemePalette, label: &str, level: f32) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(10.8).color(pal.text_muted));
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 6.0), Sense::hover());
        ui.painter()
            .rect_filled(rect, CornerRadius::same(1), pal.track_bg);
        let w = rect.width() * level.clamp(0.0, 1.0);
        if w > 0.5 {
            let fill = Rect::from_min_size(rect.left_top(), Vec2::new(w, rect.height()));
            ui.painter()
                .rect_filled(fill, CornerRadius::same(1), pal.accent);
        }
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(1),
            Stroke::new(1.0_f32, pal.border_subtle),
            StrokeKind::Inside,
        );
    });
}

fn draw_tritone_bar(ui: &mut egui::Ui, pal: &ThemePalette, tri: &TriTonePalette) {
    let desired = Vec2::new(ui.available_width(), 10.0);
    let (rect, _) = ui.allocate_exact_size(desired, Sense::hover());
    let painter = ui.painter();
    let steps = 32usize;
    let sw = rect.width() / (steps as f32);
    for i in 0..steps {
        let t = (i as f32 + 0.5) / (steps as f32);
        let [r, g, b] = tri.sample(t);
        let seg = Rect::from_min_max(
            Pos2::new(rect.left() + (i as f32) * sw, rect.top()),
            Pos2::new(rect.left() + ((i + 1) as f32) * sw + 0.5, rect.bottom()),
        );
        painter.rect_filled(seg, 0.0, Color32::from_rgb(r, g, b));
    }
    painter.rect_stroke(
        rect,
        CornerRadius::same(1),
        Stroke::new(1.0_f32, pal.border),
        StrokeKind::Inside,
    );
}

fn draw_alpha_checkerboard(
    painter: &egui::Painter,
    rect: Rect,
    contrast: CheckerContrast,
    is_light: bool,
) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let tile = 16.0f32;
    let (c_a, c_b) = contrast.colors(is_light);
    painter.rect_filled(rect, 0.0, c_a);

    let cols = (rect.width() / tile).ceil() as usize;
    let rows = (rect.height() / tile).ceil() as usize;
    let clipped = painter.with_clip_rect(rect);
    for r in 0..rows {
        for c in 0..cols {
            if (r + c) & 1 == 1 {
                let x0 = rect.left() + (c as f32) * tile;
                let y0 = rect.top() + (r as f32) * tile;
                let sub = Rect::from_min_size(Pos2::new(x0, y0), Vec2::splat(tile));
                clipped.rect_filled(sub, 0.0, c_b);
            }
        }
    }
}

/// Neutral architectural technical overlay tag (zero neon border glow).
fn draw_viewport_tag(painter: &egui::Painter, pos: Pos2, text: &str) {
    let text_col = Color32::from_rgb(218, 220, 226);
    let galley = painter.layout_no_wrap(text.to_string(), mono_font(10.8), text_col);
    let rect = Rect::from_min_size(pos, galley.size() + Vec2::new(14.0, 8.0));
    painter.rect_filled(
        rect,
        CornerRadius::same(2),
        Color32::from_rgba_unmultiplied(16, 16, 19, 228),
    );
    painter.rect_stroke(
        rect,
        CornerRadius::same(2),
        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(62, 62, 70, 210)),
        StrokeKind::Inside,
    );
    painter.galley(pos + Vec2::new(7.0, 4.0), galley, text_col);
}

pub fn launch_gui(boot: GuiBootOptions) -> Result<(), String> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("LumaGlyph")
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
