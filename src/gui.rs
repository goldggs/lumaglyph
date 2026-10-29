use crate::charset::{CharsetPreset, ASCII_COUNT, ASCII_MIN};
use crate::color::{ColorMode, TriTonePalette};
use crate::engine::{convert_image_buffer_with_audio, rasterize_rgba, AsciiFrame, RenderParams};
use crate::export::{
    export_ansi, export_html, export_png, export_svg, export_txt, frame_to_ansi_string,
};
use crate::glyph_atlas::GlyphAtlas;
use crate::telemetry::format_rss_mb;
use crate::theme::{
    apply_theme_to_context, canvas_texture_options, header_stat_rects, header_stats_width,
    install_studio_fonts, mono_font, plex_medium, resolve_palette, visible_rows, AccentPreset,
    CheckerContrast, CornerStyle, StudioTheme, ThemePalette, UiDensity, HEADER_STAT_FONT,
    HEADER_STAT_HEIGHT, HEADER_STAT_SEP,
};
use crate::updater::{
    format_build_report, relaunch_executable, spawn_release_check, spawn_verified_self_update,
    AppPreferences, ReleaseCheckOutcome, TextureFilterMode, VerifiedInstallOutcome, APP_VERSION,
    BUILD_GIT_SHA, BUILD_PROFILE, BUILD_RUSTC_VERSION, BUILD_TARGET_TRIPLE, DEFAULT_GITHUB_REPO,
    DOCS_INSTALL_URL, DOCS_QUICKSTART_URL, DOCS_TROUBLESHOOTING_URL, DOCS_USER_GUIDE_URL,
    FPS_CAP_OPTIONS, LATEST_RELEASE_URL, REPO_URL,
};
use crate::video::{
    detect_ffmpeg_version, extract_source_frame, format_timecode, set_video_pipeline_prefs,
    transcode_video_streaming, LiveVideoStream, TranscodeProgress, TranscodeSummary,
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

/// A vertical hairline between two readouts.
///
/// The header strip, the status bar and the canvas readout all use this instead
/// of a typed pipe character, so the dividers line up with each other and with
/// the rule under the transport strip.
fn hairline_divider(ui: &mut egui::Ui, pal: &ThemePalette) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(13.0, 10.0), Sense::hover());
    ui.painter().line_segment(
        [
            Pos2::new(rect.center().x, rect.center().y - 5.0),
            Pos2::new(rect.center().x, rect.center().y + 5.0),
        ],
        Stroke::new(1.0_f32, pal.border_subtle),
    );
}

/// Draws the header telemetry strip into fixed-width columns.
///
/// Each value is right-aligned inside its own column, so a value changing length
/// (156x44 -> 1920x1080) only moves text within that column. Nothing else on the
/// strip shifts.
fn paint_header_stats(
    painter: &egui::Painter,
    rect: Rect,
    values: &[String; 4],
    pal: &ThemePalette,
) {
    let columns = header_stat_rects(rect.right(), rect.center().y);
    for (idx, (column, value)) in columns.iter().zip(values.iter()).enumerate() {
        painter.text(
            column.right_center(),
            egui::Align2::RIGHT_CENTER,
            value,
            mono_font(HEADER_STAT_FONT),
            pal.text_secondary,
        );
        if idx > 0 {
            let x = column.left() - HEADER_STAT_SEP * 0.5;
            painter.line_segment(
                [
                    Pos2::new(x, rect.center().y - 5.0),
                    Pos2::new(x, rect.center().y + 5.0),
                ],
                Stroke::new(1.0_f32, pal.border_subtle),
            );
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    Appearance,
    EngineDefaults,
    Rendering,
    Optimisations,
    ExportVideo,
    Shortcuts,
    AboutBuild,
}

impl SettingsTab {
    pub const ALL: [Self; 7] = [
        Self::Appearance,
        Self::EngineDefaults,
        Self::Rendering,
        Self::Optimisations,
        Self::ExportVideo,
        Self::Shortcuts,
        Self::AboutBuild,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::EngineDefaults => "Engine & Defaults",
            Self::Rendering => "Rendering & Quality",
            Self::Optimisations => "Optimisations",
            Self::ExportVideo => "Export & Video",
            Self::Shortcuts => "Shortcuts",
            Self::AboutBuild => "About & Build",
        }
    }

    pub fn from_cli(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "appearance" | "theme" | "themes" => Some(Self::Appearance),
            "engine" | "defaults" => Some(Self::EngineDefaults),
            "rendering" | "quality" | "graphics" | "gfx" => Some(Self::Rendering),
            "optimisations" | "optimizations" | "optimise" | "optimize" | "performance"
            | "perf" | "power" => Some(Self::Optimisations),
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
    pub initial_params: Option<RenderParams>,
    pub theme_override: Option<StudioTheme>,
    pub open_settings_tab: Option<SettingsTab>,
    pub initial_zoom: Option<f32>,
    pub show_update_modal: bool,
    pub auto_exit_after_frames: Option<u32>,
}

#[derive(Default)]
enum SourceKind {
    #[default]
    Empty,
    StaticImage {
        name: String,
    },
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
    ascii_dirty: bool,

    ascii_frame: AsciiFrame,
    raster_rgba: Vec<u8>,
    ascii_tex: Option<TextureHandle>,
    app_logo_tex: Option<TextureHandle>,
    ascii_px_w: usize,
    ascii_px_h: usize,

    // Workstation layout & viewport state
    inspector_open: bool,
    inspector_tab: InspectorTab,
    zen_mode: bool,
    zoom: f32,
    auto_fit: bool,
    pan_offset: Vec2,

    // Audio-reactive & video state
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
    self_update_rx: Option<Receiver<Result<VerifiedInstallOutcome, String>>>,
    self_update_status: Option<Result<VerifiedInstallOutcome, String>>,

    // Performance, quality & background-idle runtime state
    app_idle: bool,
    idle_since: Option<Instant>,
    hires_atlas: Option<(usize, GlyphAtlas)>,
    frame_fingerprint: u64,
    fps_frames: u32,
    fps_window_start: Instant,
    measured_fps: f32,
    timeline_drag: bool,

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

        let app_logo_tex = crate::icon::egui_logo_color_image().map(|img| {
            cc.egui_ctx
                .load_texture("lumaglyph_brand_logo", img, TextureOptions::LINEAR)
        });

        let mut app = Self {
            atlas,
            params,
            look_code_buf,
            source_kind: SourceKind::Empty,
            source_rgba: Vec::new(),
            source_channels: 4,
            source_w: 0,
            source_h: 0,
            ascii_dirty: false,
            ascii_frame: AsciiFrame::new(),
            raster_rgba: Vec::new(),
            ascii_tex: None,
            app_logo_tex,
            ascii_px_w: 0,
            ascii_px_h: 0,
            inspector_open: false,
            inspector_tab: InspectorTab::All,
            zen_mode: false,
            zoom: boot.initial_zoom.unwrap_or(1.0),
            auto_fit: boot.initial_zoom.is_none(),
            pan_offset: Vec2::ZERO,
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
            self_update_rx: None,
            self_update_status: None,
            app_idle: false,
            idle_since: None,
            hires_atlas: None,
            frame_fingerprint: 0,
            fps_frames: 0,
            fps_window_start: Instant::now(),
            measured_fps: 0.0,
            timeline_drag: false,
            frames_drawn: 0,
            auto_exit_after_frames: boot.auto_exit_after_frames,
        };

        if let Some(path) = boot.initial_media {
            app.load_media_path(path);
        }

        app
    }

    #[inline]
    fn has_media(&self) -> bool {
        !matches!(self.source_kind, SourceKind::Empty)
    }

    fn close_media(&mut self) {
        self.source_kind = SourceKind::Empty;
        self.inspector_open = false;
        self.source_rgba.clear();
        self.source_rgba.shrink_to_fit();
        self.raster_rgba.clear();
        self.raster_rgba.shrink_to_fit();
        self.source_w = 0;
        self.source_h = 0;
        self.ascii_px_w = 0;
        self.ascii_px_h = 0;
        self.ascii_frame = AsciiFrame::new();
        self.ascii_tex = None;
        self.ascii_dirty = false;
        self.live_audio_energy = 0.0;
        self.video_frames_seen = 0;
        self.pan_offset = Vec2::ZERO;
    }

    fn reset_ascii_to_default(&mut self) {
        self.params = RenderParams::default();
        self.prefs.default_cols = 156;
        self.prefs.default_charset = CharsetPreset::Genuine95;
        self.prefs.default_color_mode = ColorMode::TrueColor;
        self.prefs.default_oled_black = false;
        self.prefs.atlas_cell_height = 20;
        self.atlas = GlyphAtlas::default_embedded(20);
        self.auto_fit = true;
        self.zoom = 1.0;
        self.pan_offset = Vec2::ZERO;
        let _ = self.prefs.save();
        self.sync_look_code();
        self.ascii_dirty = true;
        self.set_toast("Reset every ASCII parameter");
    }

    fn reset_all_settings_to_default(&mut self) {
        self.prefs = AppPreferences::default();
        self.atlas = GlyphAtlas::default_embedded(self.prefs.atlas_cell_height);
        self.persist_prefs();
        self.params = RenderParams::default();
        self.auto_fit = true;
        self.zoom = 1.0;
        self.pan_offset = Vec2::ZERO;
        self.sync_look_code();
        self.ascii_dirty = true;
        self.set_toast("Settings and ASCII parameters are back to defaults");
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
            self.load_font_path(p);
        }
    }

    fn load_font_path(&mut self, p: PathBuf) {
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
                    if self.has_media() {
                        self.ascii_dirty = true;
                    }
                    self.set_toast(format!("Calibrated font: {name}"));
                }
                Err(e) => self.set_toast(e),
            }
        }
    }

    fn save_png_dialog(&mut self) {
        if !self.has_media() {
            return;
        }
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

    fn load_media_path(&mut self, path: PathBuf) {
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let is_video = matches!(ext.as_str(), "mp4" | "mkv" | "mov" | "webm" | "avi" | "m4v");

        if is_video {
            let max_dim = self.prefs.video_preview_max_dim;
            let analyze_audio = self.prefs.audio_envelope_enabled;
            match LiveVideoStream::open_with_options(&path, max_dim, analyze_audio) {
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
                    self.inspector_open = true;
                    self.video_frames_seen = 0;
                    self.live_audio_energy = 0.0;
                    self.ascii_frame.prev_glyphs.clear();
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
                    self.inspector_open = true;
                    self.ascii_frame.prev_glyphs.clear();
                    self.ascii_dirty = true;
                    self.set_toast(format!("Loaded {fname} ({w}x{h})"));
                }
                Err(e) => self.set_toast(format!("Image load error: {e}")),
            }
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for f in dropped {
            if let Some(path) = f.path {
                let ext = path
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if matches!(ext.as_str(), "ttf" | "otf") {
                    self.load_font_path(path);
                } else {
                    self.load_media_path(path);
                }
                break;
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
            if i.modifiers.command && !i.modifiers.shift && i.key_pressed(Key::O) {
                self.open_image_dialog();
            }
            if i.modifiers.command && i.modifiers.shift && i.key_pressed(Key::O) {
                self.open_video_dialog();
            }
            if i.modifiers.command && !i.modifiers.shift && i.key_pressed(Key::B) {
                self.inspector_open = !self.inspector_open;
            }
            if i.modifiers.command && !i.modifiers.shift && i.key_pressed(Key::W) {
                self.close_media();
            }
            if i.modifiers.command && !i.modifiers.shift && i.key_pressed(Key::S) {
                self.save_png_dialog();
            }
            if i.key_pressed(Key::F1) {
                ctx.open_url(egui::OpenUrl::new_tab(DOCS_QUICKSTART_URL));
            }
            if i.key_pressed(Key::ArrowLeft) || i.key_pressed(Key::ArrowRight) {
                let step = if i.key_pressed(Key::ArrowLeft) {
                    -5.0
                } else {
                    5.0
                };
                if let SourceKind::VideoFile(st) = &self.source_kind {
                    st.seek_by(step);
                }
            }
            if i.modifiers.command && i.modifiers.shift && i.key_pressed(Key::R) {
                self.reset_ascii_to_default();
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
                    if let SourceKind::VideoFile(stream) = &mut self.source_kind {
                        let p = stream.paused.load(Ordering::Relaxed);
                        stream.paused.store(!p, Ordering::Relaxed);
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

        let repaint_interval = self.prefs.repaint_interval();

        if let SourceKind::VideoFile(stream) = &mut self.source_kind {
            // Background idle: park the decoder thread so an unfocused window costs nothing.
            let hold = self.app_idle && self.prefs.pause_decode_when_unfocused;
            stream.set_idle_hold(hold);

            if let Some((rgb, audio_e)) = stream.poll_latest_frame() {
                let fingerprint = frame_fingerprint(&rgb);
                let duplicate = self.prefs.frame_dedup_enabled
                    && fingerprint == self.frame_fingerprint
                    && self.ascii_tex.is_some();
                self.frame_fingerprint = fingerprint;

                self.source_rgba = rgb;
                self.source_channels = 3;
                self.live_audio_energy = audio_e;
                self.video_frames_seen += 1;
                if !duplicate {
                    self.ascii_dirty = true;
                }
            }
            let playing = !stream.paused.load(Ordering::Relaxed) && !hold;
            if playing {
                ctx.request_repaint_after(
                    repaint_interval.unwrap_or(std::time::Duration::from_millis(14)),
                );
            }
        } else {
            self.live_audio_energy = 0.0;
        }

        if !self.has_media() || self.source_w == 0 || self.source_h == 0 {
            if self.ascii_dirty {
                self.sync_look_code();
                self.ascii_dirty = false;
            }
        } else if self.ascii_dirty || self.ascii_tex.is_none() {
            self.sync_look_code();
            let use_temporal = matches!(self.source_kind, SourceKind::VideoFile(_));
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

            // Optional 2x supersampled canvas raster for crisp glyph edges when zoomed.
            let supersample = if self.prefs.preview_supersample >= 2 {
                2usize
            } else {
                1usize
            };
            let target_cell_h = self.atlas.cell_h.saturating_mul(supersample);
            if supersample > 1 && self.hires_atlas.as_ref().map(|(h, _)| *h) != Some(target_cell_h)
            {
                self.hires_atlas =
                    Some((target_cell_h, self.atlas.with_cell_height(target_cell_h)));
            }
            let raster_atlas = if supersample > 1 {
                self.hires_atlas
                    .as_ref()
                    .map(|(_, a)| a)
                    .unwrap_or(&self.atlas)
            } else {
                &self.atlas
            };

            let (pw, ph) = rasterize_rgba(
                &self.ascii_frame,
                raster_atlas,
                &self.params,
                bloom_boost,
                &mut self.raster_rgba,
            );
            self.ascii_px_w = pw;
            self.ascii_px_h = ph;

            if pw > 0 && ph > 0 {
                let cimg = ColorImage::from_rgba_unmultiplied([pw, ph], &self.raster_rgba);
                // Rows that actually fit on screen drive the mip choice: a picture
                // drawn 300 tall never needs all 51 source rows sampled.
                let drawn_h = self.ascii_px_h as f32 * self.zoom;
                let opts = canvas_texture_options(
                    self.prefs.texture_filter.resolve(self.zoom),
                    self.ascii_frame.rows,
                    visible_rows(self.ascii_frame.rows, drawn_h),
                );
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

        if let Some(rx) = &self.self_update_rx {
            ctx.request_repaint_after(std::time::Duration::from_millis(80));
            if let Ok(res) = rx.try_recv() {
                self.self_update_rx = None;
                match res {
                    Ok(done) => {
                        let _ = self.prefs.save();
                        match relaunch_executable(&done.installed_exe, &[]) {
                            Ok(_pid) => {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                std::process::exit(0);
                            }
                            Err(launch_err) => {
                                let msg = format!(
                                    "Installed {}, failed auto-launch: {launch_err}",
                                    done.tag
                                );
                                self.set_toast(&msg);
                                self.self_update_status = Some(Err(msg));
                            }
                        }
                    }
                    Err(err) => {
                        self.set_toast(format!("Update failed: {err}"));
                        self.self_update_status = Some(Err(err));
                    }
                }
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

            if let Some(tex) = &self.app_logo_tex {
                ui.add(
                    egui::Image::from_texture((tex.id(), Vec2::splat(17.0)))
                        .fit_to_exact_size(Vec2::splat(17.0)),
                );
            }

            ui.label(
                RichText::new("LUMAGLYPH")
                    .font(plex_medium(12.2))
                    .color(pal.text_primary),
            );

            ui.separator();

            egui::menu::bar(ui, |ui| {
                let has_media = self.has_media();
                ui.menu_button("File", |ui| {
                    if ui.button("Open Image...          Ctrl+O").clicked() {
                        ui.close_menu();
                        self.open_image_dialog();
                    }
                    if ui.button("Open Video...    Ctrl+Shift+O").clicked() {
                        ui.close_menu();
                        self.open_video_dialog();
                    }
                    if ui.button("Load Font (.ttf/.otf)...").clicked() {
                        ui.close_menu();
                        self.open_font_dialog();
                    }
                    if ui
                        .add_enabled(
                            has_media,
                            egui::Button::new("Close Media            Ctrl+W"),
                        )
                        .clicked()
                    {
                        ui.close_menu();
                        self.close_media();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(
                            has_media,
                            egui::Button::new("Export PNG...          Ctrl+S"),
                        )
                        .clicked()
                    {
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
                    ui.checkbox(&mut self.inspector_open, "Inspector Panel     Ctrl+B");
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
                    if ui.button("Reset to Default  Ctrl+Shift+R").clicked() {
                        self.reset_ascii_to_default();
                        ui.close_menu();
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
                });

                ui.menu_button("Presets", |ui| {
                    if ui.button("Reset to Default         Ctrl+Shift+R").clicked() {
                        self.reset_ascii_to_default();
                        ui.close_menu();
                    }
                    ui.separator();
                    for (id, lbl) in [
                        ("neutral", "Normal TrueColor (Default)"),
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
                    if ui.button("Getting Started (Quick Start)   F1").clicked() {
                        ui.close_menu();
                        ui.ctx()
                            .open_url(egui::OpenUrl::new_tab(DOCS_QUICKSTART_URL));
                    }
                    if ui.button("How to Download & Install").clicked() {
                        ui.close_menu();
                        ui.ctx().open_url(egui::OpenUrl::new_tab(DOCS_INSTALL_URL));
                    }
                    if ui.button("User Guide (every panel & setting)").clicked() {
                        ui.close_menu();
                        ui.ctx()
                            .open_url(egui::OpenUrl::new_tab(DOCS_USER_GUIDE_URL));
                    }
                    if ui.button("Troubleshooting & FAQ").clicked() {
                        ui.close_menu();
                        ui.ctx()
                            .open_url(egui::OpenUrl::new_tab(DOCS_TROUBLESHOOTING_URL));
                    }
                    if ui.button("Latest Downloads on GitHub").clicked() {
                        ui.close_menu();
                        ui.ctx()
                            .open_url(egui::OpenUrl::new_tab(LATEST_RELEASE_URL));
                    }
                    if ui.button("LumaGlyph Repository").clicked() {
                        ui.close_menu();
                        ui.ctx().open_url(egui::OpenUrl::new_tab(REPO_URL));
                    }
                    ui.separator();
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
                    .on_hover_text("Settings, themes and build info (Ctrl+,)")
                    .clicked()
                {
                    self.show_settings_window = !self.show_settings_window;
                }

                if self.has_media() {
                    ui.separator();
                    let fps_est = if self.ascii_frame.render_ms > 0.05 {
                        (1000.0 / self.ascii_frame.render_ms).min(9999.0)
                    } else {
                        9999.0
                    };
                    let values = [
                        format!("{}x{}", self.ascii_frame.cols, self.ascii_frame.rows),
                        format!("{:02}/95 CHARS", self.ascii_frame.unique_glyphs),
                        format!("{:.2} ms", self.ascii_frame.render_ms),
                        format!("{:.0} FPS", fps_est),
                    ];
                    let (rect, _) = ui.allocate_exact_size(
                        Vec2::new(header_stats_width(), HEADER_STAT_HEIGHT),
                        Sense::hover(),
                    );
                    paint_header_stats(ui.painter(), rect, &values, &pal);
                }
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
            if self.has_media()
                && ui
                    .button("Close")
                    .on_hover_text("Close Media (Ctrl+W)")
                    .clicked()
            {
                self.close_media();
            }

            ui.separator();

            if workstation_toggle(ui, &pal, self.params.oled_black, "OLED #000000")
                .on_hover_text("True #000000 black canvas (B)")
                .clicked()
            {
                self.params.oled_black = !self.params.oled_black;
                if self.params.oled_black {
                    self.params.transparent_bg = false;
                }
                self.ascii_dirty = true;
            }

            if workstation_toggle(ui, &pal, self.params.transparent_bg, "Alpha RGBA")
                .on_hover_text("Transparent background")
                .clicked()
            {
                self.params.transparent_bg = !self.params.transparent_bg;
                if self.params.transparent_bg {
                    self.params.oled_black = false;
                }
                self.ascii_dirty = true;
            }

            if workstation_toggle(ui, &pal, self.params.wireframe_only, "Wireframe")
                .on_hover_text("Contours only, no fill (N)")
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

            if ui
                .button("Reset to Default")
                .on_hover_text("Reset every ASCII parameter (Ctrl+Shift+R)")
                .clicked()
            {
                self.reset_ascii_to_default();
            }

            // Right side: viewport zoom presets + Zen mode
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if workstation_toggle(ui, &pal, self.zen_mode, "Zen")
                    .on_hover_text("Hide the inspector and status bars (Tab)")
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

        // Segmented workspace tab bar + one-click Reset to Default at top of Inspector
        ui.horizontal(|ui| {
            let reset_w = 50.0_f32;
            let total_w = (ui.available_width() - reset_w - 6.0).max(160.0);
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

            let (r_rect, r_resp) = ui.allocate_exact_size(Vec2::new(reset_w, 23.0), Sense::click());
            let r_fill = if r_resp.hovered() {
                pal.control_hover
            } else {
                pal.control_bg
            };
            ui.painter()
                .rect_filled(r_rect, CornerRadius::same(pal.radius), r_fill);
            ui.painter().rect_stroke(
                r_rect,
                CornerRadius::same(pal.radius),
                Stroke::new(1.0_f32, pal.border_subtle),
                StrokeKind::Inside,
            );
            ui.painter().text(
                r_rect.center(),
                egui::Align2::CENTER_CENTER,
                "Reset",
                plex_medium(11.1),
                pal.text_secondary,
            );
            if r_resp
                .on_hover_text("Reset every ASCII parameter (Ctrl+Shift+R)")
                .clicked()
            {
                self.reset_ascii_to_default();
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
                            0.02,
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
                            0.65,
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
                                0.22,
                                2,
                            );
                        }
                        c |= scrub_row_f32(
                            ui,
                            &pal,
                            "Brightness",
                            &mut self.params.brightness,
                            -0.40..=0.40,
                            0.00,
                            2,
                        );
                        c |= scrub_row_f32(
                            ui,
                            &pal,
                            "Contrast",
                            &mut self.params.contrast,
                            0.50..=2.40,
                            1.00,
                            2,
                        );
                        c |= scrub_row_f32(
                            ui,
                            &pal,
                            "Saturation",
                            &mut self.params.saturation,
                            0.00..=2.20,
                            1.00,
                            2,
                        );
                        c |= scrub_row_f32(
                            ui,
                            &pal,
                            "Gamma",
                            &mut self.params.gamma,
                            0.45..=2.00,
                            1.00,
                            2,
                        );
                        c |= scrub_row_f32(
                            ui,
                            &pal,
                            "Bayer Dither",
                            &mut self.params.dither,
                            0.00..=1.00,
                            0.04,
                            2,
                        );

                        ui.horizontal(|ui| {
                            c |= ui.checkbox(&mut self.params.crt_bloom, "Bloom").changed();
                            c |= ui
                                .checkbox(&mut self.params.scanlines, "Scanlines")
                                .changed();
                            c |= ui.checkbox(&mut self.params.invert, "Invert").changed();
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.small_button("Reset to Default").clicked() {
                                    self.reset_ascii_to_default();
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
                            1.25,
                            2,
                        );
                        c |= scrub_row_f32(
                            ui,
                            &pal,
                            "Edge Tensor",
                            &mut self.params.edge_weight,
                            0.00..=2.00,
                            0.90,
                            2,
                        );
                        c |= scrub_row_f32(
                            ui,
                            &pal,
                            "Anti-Flicker",
                            &mut self.params.temporal_stability,
                            0.00..=0.45,
                            0.20,
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
                    let has_media = self.has_media();
                    inspector_section(ui, &pal, "EXPORT & TRANSCODE", |ui| {
                        ui.add_enabled_ui(has_media, |ui| {
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

    fn draw_empty_workbench(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        avail: Rect,
        pointer_pos: Option<Pos2>,
        clicked: bool,
    ) {
        let pal = self.palette;
        let center = Pos2::new(
            avail.center().x,
            (avail.center().y - 18.0).max(avail.top() + 120.0),
        );

        // Subtle recessed tone matching VS Code's low-contrast empty-window watermark
        let watermark_col = if pal.is_light {
            Color32::from_rgba_unmultiplied(90, 92, 98, 95)
        } else {
            Color32::from_rgba_unmultiplied(150, 154, 166, 62)
        };
        let wordmark_col = if pal.is_light {
            Color32::from_rgba_unmultiplied(88, 90, 96, 125)
        } else {
            Color32::from_rgba_unmultiplied(155, 160, 172, 92)
        };
        let hint_col = if pal.is_light {
            Color32::from_rgba_unmultiplied(95, 98, 105, 165)
        } else {
            Color32::from_rgba_unmultiplied(146, 150, 162, 138)
        };

        // 1. Subtle recessed LG brand watermark (like VS Code's empty-window mark)
        let mark_c = Pos2::new(center.x, center.y - 90.0);
        if let Some(tex) = &self.app_logo_tex {
            let logo_rect = Rect::from_center_size(mark_c, Vec2::splat(64.0));
            let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            let tint = if pal.is_light {
                Color32::from_rgba_unmultiplied(150, 165, 190, 96)
            } else {
                Color32::from_rgba_unmultiplied(210, 224, 245, 76)
            };
            painter.image(tex.id(), logo_rect, uv, tint);
        } else {
            let half = 21.0_f32;
            let leg = 8.5_f32;
            let st = Stroke::new(1.5_f32, watermark_col);
            for (sx, sy) in [(-1.0_f32, -1.0_f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                let corner = Pos2::new(mark_c.x + sx * half, mark_c.y + sy * half);
                painter.line_segment([corner, Pos2::new(corner.x - sx * leg, corner.y)], st);
                painter.line_segment([corner, Pos2::new(corner.x, corner.y - sy * leg)], st);
            }
        }

        // Recessed wordmark
        painter.text(
            Pos2::new(center.x, center.y - 44.0),
            egui::Align2::CENTER_CENTER,
            "L U M A G L Y P H",
            plex_medium(12.5),
            wordmark_col,
        );

        // 2. VS Code-style two-column shortcut rows
        let rows: [(&str, &[&str], u8); 5] = [
            ("Open Image", &["Ctrl", "O"], 1),
            ("Open Video", &["Ctrl", "Shift", "O"], 2),
            ("Toggle Inspector", &["Ctrl", "B"], 3),
            ("Settings", &["Ctrl", ","], 4),
            ("Documentation", &["F1"], 5),
        ];

        let spine_x = center.x - 6.0;
        let start_y = center.y - 20.0;
        let row_h = 28.0_f32;

        for (idx, (label, keys, action_id)) in rows.iter().enumerate() {
            let cy = start_y + (idx as f32) * row_h;
            let hit_rect = Rect::from_center_size(Pos2::new(center.x, cy), Vec2::new(290.0, 24.0));
            let row_hovered = pointer_pos.is_some_and(|p| hit_rect.contains(p));
            if row_hovered {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }

            let lbl_col = if row_hovered {
                pal.text_primary
            } else {
                pal.text_muted
            };
            painter.text(
                Pos2::new(spine_x - 12.0, cy),
                egui::Align2::RIGHT_CENTER,
                *label,
                egui::FontId::proportional(12.2),
                lbl_col,
            );

            // Keycap badges on right of spine
            let mut kx = spine_x + 6.0;
            for (k_i, k) in keys.iter().enumerate() {
                if k_i > 0 {
                    painter.text(
                        Pos2::new(kx + 4.0, cy),
                        egui::Align2::CENTER_CENTER,
                        "+",
                        mono_font(10.5),
                        hint_col,
                    );
                    kx += 11.0;
                }
                let galley =
                    painter.layout_no_wrap((*k).to_string(), mono_font(10.6), pal.text_secondary);
                let cap_w = (galley.size().x + 11.0).max(22.0);
                let cap_h = 19.0_f32;
                let cap_rect =
                    Rect::from_min_size(Pos2::new(kx, cy - cap_h * 0.5), Vec2::new(cap_w, cap_h));
                painter.rect_filled(cap_rect, CornerRadius::same(3), pal.control_bg);
                painter.rect_stroke(
                    cap_rect,
                    CornerRadius::same(3),
                    Stroke::new(1.0_f32, pal.border_subtle),
                    StrokeKind::Inside,
                );
                painter.galley(
                    cap_rect.center() - galley.size() * 0.5,
                    galley,
                    pal.text_secondary,
                );
                kx += cap_w + 3.0;
            }

            if row_hovered && clicked {
                match *action_id {
                    1 => self.open_image_dialog(),
                    2 => self.open_video_dialog(),
                    3 => self.inspector_open = !self.inspector_open,
                    4 => {
                        self.settings_tab = SettingsTab::Appearance;
                        self.show_settings_window = true;
                    }
                    5 => {
                        ui.ctx()
                            .open_url(egui::OpenUrl::new_tab(DOCS_QUICKSTART_URL));
                    }
                    _ => {}
                }
            }
        }

        // 3. Quiet drop hint underneath
        let foot_y = start_y + (rows.len() as f32) * row_h + 14.0;
        painter.text(
            Pos2::new(center.x, foot_y),
            egui::Align2::CENTER_CENTER,
            "Drop an image, video, or .ttf font file anywhere to open",
            egui::FontId::proportional(11.3),
            hint_col,
        );
    }

    fn draw_viewport(&mut self, ui: &mut egui::Ui) {
        let pal = self.palette;
        let avail = ui.available_rect_before_wrap();
        let (response, painter) = ui.allocate_painter(avail.size(), Sense::click_and_drag());

        // The video transport owns its own docked band beneath the picture, so it can never
        // cover the bottom rows of the artwork.
        let has_video = matches!(self.source_kind, SourceKind::VideoFile(_));
        let transport_h = if has_video && !self.zen_mode {
            46.0_f32.min(avail.height() * 0.35)
        } else {
            0.0
        };
        let canvas_avail =
            Rect::from_min_max(avail.min, Pos2::new(avail.max.x, avail.max.y - transport_h));
        let transport_band =
            Rect::from_min_max(Pos2::new(avail.min.x, avail.max.y - transport_h), avail.max);

        let surround = if !self.has_media() {
            pal.panel_bg
        } else if self.params.oled_black {
            Color32::BLACK
        } else {
            pal.viewport_bg
        };
        painter.rect_filled(canvas_avail, 0.0, surround);

        let dragging_file = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
        if dragging_file {
            painter.rect_stroke(
                avail.shrink(8.0),
                CornerRadius::same(2),
                Stroke::new(1.0_f32, pal.accent),
                StrokeKind::Inside,
            );
        }

        if !self.has_media() || self.ascii_px_w == 0 || self.ascii_px_h == 0 {
            self.draw_empty_workbench(
                ui,
                &painter,
                avail,
                response.hover_pos(),
                response.clicked(),
            );
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
            let sx = ((canvas_avail.width() - pad) / base_w).max(0.1);
            let sy = ((canvas_avail.height() - pad) / base_h).max(0.1);
            self.zoom = sx.min(sy);
            self.pan_offset = Vec2::ZERO;
        }

        let draw_size = Vec2::new(base_w * self.zoom, base_h * self.zoom);
        let center = canvas_avail.center() + self.pan_offset;
        let dest_rect = Rect::from_center_size(center, draw_size);

        if response.dragged()
            && !self.auto_fit
            && !self.timeline_drag
            && !transport_band.contains(response.interact_pointer_pos().unwrap_or(Pos2::ZERO))
        {
            self.pan_offset += response.drag_delta();
        }

        if self.params.transparent_bg {
            draw_alpha_checkerboard(
                &painter,
                dest_rect.intersect(canvas_avail),
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
            painter.with_clip_rect(canvas_avail).image(
                ascii_tex.id(),
                dest_rect,
                uv_full,
                Color32::WHITE,
            );
        }

        if transport_h > 0.0 {
            self.draw_video_transport(ui, &painter, transport_band);
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
                        Pos2::new(canvas_avail.left() + 10.0, canvas_avail.bottom() - 28.0),
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

    /// Docked video transport strip: sits in its own band directly beneath the video so it
    /// never covers the picture. Play/pause, 5-second jumps, a scrubbable timeline, a timecode
    /// readout, and the frame-export menu.
    fn draw_video_transport(&mut self, ui: &mut egui::Ui, painter: &egui::Painter, band: Rect) {
        if !matches!(self.source_kind, SourceKind::VideoFile(_)) || band.height() < 20.0 {
            return;
        }
        let pal = self.palette;

        // Docked strip chrome: flat panel fill plus a single hairline separator above.
        painter.rect_filled(band, 0.0, pal.panel_bg);
        painter.line_segment(
            [
                Pos2::new(band.left(), band.top() + 0.5),
                Pos2::new(band.right(), band.top() + 0.5),
            ],
            Stroke::new(1.0_f32, pal.border_subtle),
        );

        let bar = Rect::from_min_max(
            Pos2::new(band.left() + 10.0, band.center().y - 15.0),
            Pos2::new(band.right() - 10.0, band.center().y + 15.0),
        );
        if bar.width() < 240.0 {
            return;
        }
        let cy = bar.center().y;

        let icon_col = pal.text_primary;
        let dim_col = pal.text_secondary;

        // ---- Play / pause ----
        let play_rect = Rect::from_center_size(Pos2::new(bar.left() + 13.0, cy), Vec2::splat(26.0));
        let play_resp = ui
            .interact(play_rect, ui.id().with("video_play_btn"), Sense::click())
            .on_hover_text("Play / Pause (Space)");
        if play_resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            painter.rect_filled(play_rect, CornerRadius::same(3), pal.control_hover);
        }
        let paused = match &self.source_kind {
            SourceKind::VideoFile(st) => {
                st.paused.load(Ordering::Relaxed)
                    || (self.app_idle && self.prefs.pause_decode_when_unfocused)
            }
            _ => true,
        };
        let c = play_rect.center();
        if paused {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(c.x - 4.0, c.y - 7.0),
                    Pos2::new(c.x - 4.0, c.y + 7.0),
                    Pos2::new(c.x + 7.0, c.y),
                ],
                icon_col,
                Stroke::NONE,
            ));
        } else {
            for dx in [-4.5f32, 1.5] {
                painter.rect_filled(
                    Rect::from_min_size(Pos2::new(c.x + dx, c.y - 7.0), Vec2::new(3.5, 14.0)),
                    0.5,
                    icon_col,
                );
            }
        }
        if play_resp.clicked() {
            if let SourceKind::VideoFile(st) = &self.source_kind {
                st.toggle_pause();
            }
        }

        // ---- Jump back / forward 5 seconds ----
        // Two clean, non-overlapping chevrons per button, mirrored around the button centre.
        let mut jump: Option<f64> = None;
        // Chevrons read as two strokes, not tall spikes: keep them wider than tall
        // and size the hit box to match the play button.
        let tri_w = 5.0_f32;
        let tri_h = 4.2_f32;
        for (idx, delta) in [-5.0f64, 5.0].into_iter().enumerate() {
            let is_back = delta < 0.0;
            let btn = Rect::from_center_size(
                Pos2::new(bar.left() + 49.0 + idx as f32 * 26.0, cy),
                Vec2::splat(24.0),
            );
            let resp = ui
                .interact(btn, ui.id().with(("video_jump", idx)), Sense::click())
                .on_hover_text(if is_back {
                    "Back 5 seconds (Left Arrow)"
                } else {
                    "Forward 5 seconds (Right Arrow)"
                });
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                painter.rect_filled(btn, CornerRadius::same(3), pal.control_hover);
            }
            // Two chevrons, each `tri_w` wide with `gap` between them, mirrored so the pair is
            // optically centred in the button and never overlaps itself.
            let gap = 1.6_f32;
            let span = 2.0 * tri_w + gap;
            for i in 0..2 {
                let shift = i as f32 * (tri_w + gap);
                let (base, tip) = if is_back {
                    let right = btn.center().x + span * 0.5 - shift;
                    (right, right - tri_w)
                } else {
                    let left = btn.center().x - span * 0.5 + shift;
                    (left, left + tri_w)
                };
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        Pos2::new(base, cy - tri_h),
                        Pos2::new(base, cy + tri_h),
                        Pos2::new(tip, cy),
                    ],
                    dim_col,
                    Stroke::NONE,
                ));
            }
            if resp.clicked() {
                jump = Some(delta);
            }
        }

        // ---- Frame export menu, right aligned ----
        let export_rect = Rect::from_min_max(
            Pos2::new(bar.right() - 92.0, cy - 11.0),
            Pos2::new(bar.right(), cy + 11.0),
        );
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(export_rect)
                .layout(Layout::left_to_right(Align::Center)),
        );
        egui::menu::menu_button(&mut child, "Export Frame", |ui| {
            if ui.button("ASCII Frame (PNG)...").clicked() {
                ui.close_menu();
                self.save_png_dialog();
            }
            if ui.button("Source Video Frame (PNG)...").clicked() {
                ui.close_menu();
                self.export_source_frame_dialog();
            }
            if ui.button("Copy ASCII Frame as Text").clicked() {
                ui.close_menu();
                let txt = frame_to_ansi_string(&self.ascii_frame, &self.params);
                ui.ctx().copy_text(txt);
                self.set_toast("Copied current frame as ANSI text");
            }
        });

        // ---- Timecode ----
        let (position, duration) = match &self.source_kind {
            SourceKind::VideoFile(st) => (st.position_secs(), st.duration_secs()),
            _ => (0.0, 0.0),
        };
        let tc = format!(
            "{} / {}",
            format_timecode(position),
            format_timecode(duration)
        );
        let tc_galley = painter.layout_no_wrap(tc, mono_font(10.8), dim_col);
        let tc_x = export_rect.left() - 14.0;
        painter.galley(
            Pos2::new(tc_x - tc_galley.size().x, cy - tc_galley.size().y * 0.5),
            tc_galley.clone(),
            dim_col,
        );

        // ---- Scrubbable timeline ----
        let track_left = bar.left() + 96.0;
        let track_right = tc_x - tc_galley.size().x - 14.0;
        if track_right > track_left + 40.0 {
            let track = Rect::from_min_max(
                Pos2::new(track_left, cy - 1.5),
                Pos2::new(track_right, cy + 1.5),
            );
            let hit = Rect::from_min_max(
                Pos2::new(track_left - 4.0, cy - 9.0),
                Pos2::new(track_right + 4.0, cy + 9.0),
            );
            let resp = ui
                .interact(hit, ui.id().with("video_timeline"), Sense::click_and_drag())
                .on_hover_text("Drag to scrub through the timeline");
            self.timeline_drag = resp.dragged();

            painter.rect_filled(track, 1.0, pal.control_bg);
            let ratio = if duration > 0.0 {
                (position / duration).clamp(0.0, 1.0) as f32
            } else {
                0.0
            };
            painter.rect_filled(
                Rect::from_min_max(
                    track.left_top(),
                    Pos2::new(track.left() + track.width() * ratio, track.bottom()),
                ),
                1.0,
                pal.accent,
            );
            let knob_x = track.left() + track.width() * ratio;
            let knob = Pos2::new(knob_x, cy);
            painter.circle_filled(knob, 4.0, pal.text_primary);
            painter.circle_stroke(knob, 4.0, Stroke::new(1.0_f32, pal.panel_bg));

            if (resp.dragged() || resp.clicked()) && duration > 0.0 {
                if let Some(p) = resp.interact_pointer_pos() {
                    let r = ((p.x - track_left) / track.width()).clamp(0.0, 1.0) as f64;
                    if let SourceKind::VideoFile(st) = &self.source_kind {
                        st.seek_to(r * duration);
                    }
                }
            }
        } else {
            self.timeline_drag = false;
        }

        if let Some(delta) = jump {
            if let SourceKind::VideoFile(st) = &self.source_kind {
                st.seek_by(delta);
            }
        }
    }

    /// Saves the untouched, full-resolution video frame currently on the timeline.
    fn export_source_frame_dialog(&mut self) {
        let SourceKind::VideoFile(stream) = &self.source_kind else {
            return;
        };
        let path = stream.path.clone();
        let secs = stream.position_secs();
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("frame")
            .to_string();
        let stamp = format_timecode(secs).replace(':', "-").replace('.', "_");
        let default_name = format!("{stem}-frame-{stamp}.png");

        let Some(target) = rfd::FileDialog::new()
            .set_file_name(&default_name)
            .add_filter("PNG", &["png"])
            .save_file()
        else {
            return;
        };

        match extract_source_frame(&path, secs) {
            Ok((rgb, w, h)) => {
                let Some(img) = image::ImageBuffer::<image::Rgb<u8>, Vec<u8>>::from_raw(w, h, rgb)
                else {
                    self.set_toast("Failed to build source frame image buffer");
                    return;
                };
                match img.save(&target) {
                    Ok(()) => self.set_toast(format!(
                        "Saved source frame at {} ({w}x{h} px)",
                        format_timecode(secs)
                    )),
                    Err(e) => self.set_toast(format!("Frame save failed: {e}")),
                }
            }
            Err(e) => self.set_toast(format!("Frame export failed: {e}")),
        }
    }

    fn draw_status_strip(&mut self, ui: &mut egui::Ui) {
        let pal = self.palette;
        ui.horizontal(|ui| {
            match &mut self.source_kind {
                SourceKind::Empty => {
                    ui.label(
                        RichText::new("Ready")
                            .font(mono_font(10.6))
                            .color(pal.text_muted),
                    );
                }
                SourceKind::VideoFile(stream) => {
                    let state = if stream.paused.load(Ordering::Relaxed) {
                        "PAUSED"
                    } else if self.app_idle && self.prefs.pause_decode_when_unfocused {
                        "IDLE"
                    } else {
                        "PLAYING"
                    };
                    let parts = [
                        format!(
                            "{}x{} @ {:.1} FPS",
                            stream.meta.width, stream.meta.height, stream.meta.fps
                        ),
                        state.to_string(),
                        format!(
                            "{} / {}",
                            format_timecode(stream.position_secs()),
                            format_timecode(stream.duration_secs())
                        ),
                    ];
                    for (idx, part) in parts.iter().enumerate() {
                        if idx > 0 {
                            hairline_divider(ui, &pal);
                        }
                        ui.label(
                            RichText::new(part)
                                .font(mono_font(10.6))
                                .color(pal.text_secondary),
                        );
                    }
                }
                SourceKind::StaticImage { name } => {
                    ui.label(
                        RichText::new(name.as_str())
                            .font(mono_font(10.6))
                            .color(pal.text_muted),
                    );
                    hairline_divider(ui, &pal);
                    ui.label(
                        RichText::new(format!("{}x{} px", self.source_w, self.source_h))
                            .font(mono_font(10.6))
                            .color(pal.text_muted),
                    );
                }
            }

            if self.has_media() && self.ascii_frame.cols > 0 && self.ascii_frame.rows > 0 {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("{:.0}%", self.zoom * 100.0))
                            .font(mono_font(10.6))
                            .color(pal.text_muted),
                    );
                    hairline_divider(ui, &pal);
                    ui.label(
                        RichText::new(format!(
                            "{}x{} cells",
                            self.ascii_frame.cols, self.ascii_frame.rows
                        ))
                        .font(mono_font(10.6))
                        .color(pal.text_muted),
                    );
                });
            }
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

                                    let rem = ui.available_height() - 72.0;
                                    if rem > 4.0 {
                                        ui.add_space(rem);
                                    }
                                    if ui
                                        .add_sized(
                                            [ui.available_width(), 24.0],
                                            egui::Button::new("Reset to Default"),
                                        )
                                        .on_hover_text(
                                            "Reset every ASCII parameter and startup default (Ctrl+Shift+R)",
                                        )
                                        .clicked()
                                    {
                                        self.reset_ascii_to_default();
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
                                        SettingsTab::Rendering => {
                                            self.draw_settings_rendering(ui)
                                        }
                                        SettingsTab::Optimisations => {
                                            self.draw_settings_optimisations(ui)
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
            RichText::new("Muted greys, so the interface stays out of the way of the picture.")
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
                "Show the cell inspector on hover",
            )
            .changed();
        changed |= ui
            .checkbox(
                &mut self.prefs.show_canvas_border,
                "Draw a hairline around the canvas",
            )
            .changed();
        changed |= ui
            .checkbox(
                &mut self.prefs.show_status_bar,
                "Show the transport strip and status bar",
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
            RichText::new("Startup defaults")
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
            RichText::new("Glyph raster resolution")
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

        ui.label(
            RichText::new(format!("Active Font: {}", self.atlas.font_name))
                .font(mono_font(11.2))
                .color(pal.text_secondary),
        );
        ui.horizontal(|ui| {
            if ui.small_button("Load .TTF / .OTF...").clicked() {
                self.open_font_dialog();
            }
            if ui.small_button("Reset to JetBrains Mono").clicked() {
                self.atlas =
                    GlyphAtlas::default_embedded(self.prefs.atlas_cell_height.clamp(12, 36));
                self.ascii_dirty = true;
            }
        });

        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .button("Reset ASCII Parameters to Normal Default")
                .on_hover_text(
                    "Puts every color, contrast, edge and effect control back to default",
                )
                .clicked()
            {
                self.reset_ascii_to_default();
            }
            if ui
                .button("Restore All Settings to Default")
                .on_hover_text("Puts the preferences and every ASCII parameter back to default")
                .clicked()
            {
                self.reset_all_settings_to_default();
            }
        });

        if changed {
            self.persist_prefs();
        }
    }

    fn draw_settings_rendering(&mut self, ui: &mut egui::Ui) {
        let pal = self.palette;
        let mut changed = false;
        let mut needs_restart = false;

        ui.label(
            RichText::new("Frame rate")
                .font(plex_medium(13.0))
                .color(pal.text_primary),
        );
        ui.label(
            RichText::new(
                "Caps how often the window redraws. Video still decodes at its own rate.",
            )
            .size(11.4)
            .color(pal.text_secondary),
        );

        ui.label(
            RichText::new("Frame Rate Cap")
                .size(11.5)
                .color(pal.text_secondary),
        );
        ui.horizontal(|ui| {
            for opt in FPS_CAP_OPTIONS {
                let lbl = if opt == 0 {
                    "Uncapped".to_string()
                } else {
                    format!("{opt} FPS")
                };
                if ui.radio_value(&mut self.prefs.fps_cap, opt, lbl).changed() {
                    changed = true;
                }
            }
        });
        ui.label(
            RichText::new(format!("Active repaint budget: {}", {
                if self.prefs.fps_cap == 0 {
                    "uncapped (event driven)".to_string()
                } else {
                    format!("{:.2} ms/frame", 1000.0 / f64::from(self.prefs.fps_cap))
                }
            }))
            .font(mono_font(10.6))
            .color(pal.text_muted),
        );

        ui.separator();

        ui.label(
            RichText::new("Texture Sampling")
                .size(11.5)
                .color(pal.text_secondary),
        );
        ui.horizontal(|ui| {
            for mode in TextureFilterMode::ALL {
                if ui
                    .radio_value(&mut self.prefs.texture_filter, mode, mode.label())
                    .changed()
                {
                    changed = true;
                    self.ascii_dirty = true;
                }
            }
        });

        ui.label(
            RichText::new("Supersampling")
                .size(11.5)
                .color(pal.text_secondary),
        );
        ui.horizontal(|ui| {
            for (v, lbl) in [
                (1u8, "1x (Standard)"),
                (2u8, "2x Supersampled (Crisp when zoomed)"),
            ] {
                if ui
                    .radio_value(&mut self.prefs.preview_supersample, v, lbl)
                    .changed()
                {
                    changed = true;
                    self.ascii_dirty = true;
                }
            }
        });

        ui.separator();

        ui.label(
            RichText::new("GPU Presentation")
                .font(plex_medium(12.4))
                .color(pal.text_primary),
        );
        if ui
            .checkbox(&mut self.prefs.vsync, "VSync (wait for vertical blank)")
            .changed()
        {
            changed = true;
            needs_restart = true;
        }
        if ui
            .checkbox(
                &mut self.prefs.animation_smoothing,
                "Smoothed zoom & scrolling",
            )
            .changed()
        {
            changed = true;
        }
        if ui
            .checkbox(
                &mut self.prefs.show_fps_readout,
                "Show live measured FPS in the status strip",
            )
            .changed()
        {
            changed = true;
        }

        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Anti-aliasing (MSAA)")
                    .size(11.5)
                    .color(pal.text_secondary),
            );
            egui::ComboBox::from_id_salt("msaa_combo")
                .selected_text(match self.prefs.msaa_samples {
                    1 => "Off (1x)".to_string(),
                    2 => "2x MSAA".to_string(),
                    4 => "4x MSAA (Default)".to_string(),
                    _ => "8x MSAA".to_string(),
                })
                .show_ui(ui, |ui| {
                    for (v, lbl) in [
                        (1u16, "Off (1x)"),
                        (2u16, "2x MSAA"),
                        (4u16, "4x MSAA (Default)"),
                        (8u16, "8x MSAA"),
                    ] {
                        if ui
                            .selectable_value(&mut self.prefs.msaa_samples, v, lbl)
                            .changed()
                        {
                            changed = true;
                            needs_restart = true;
                        }
                    }
                });
        });

        if needs_restart {
            ui.label(
                RichText::new("VSync and MSAA changes apply the next time LumaGlyph launches.")
                    .font(mono_font(10.4))
                    .color(pal.accent),
            );
        }

        if changed {
            self.persist_prefs();
        }
    }

    fn draw_settings_optimisations(&mut self, ui: &mut egui::Ui) {
        let pal = self.palette;
        let mut changed = false;
        let mut reopen_video = false;

        ui.label(
            RichText::new("Background and focus")
                .font(plex_medium(13.0))
                .color(pal.text_primary),
        );
        ui.label(
            RichText::new("In the background LumaGlyph can stop drawing and decoding altogether.")
                .size(11.4)
                .color(pal.text_secondary),
        );

        if ui
            .checkbox(
                &mut self.prefs.idle_when_unfocused,
                "Stop drawing when the window loses focus",
            )
            .changed()
        {
            changed = true;
        }
        if ui
            .checkbox(
                &mut self.prefs.pause_decode_when_unfocused,
                "Pause video decoding while unfocused",
            )
            .changed()
        {
            changed = true;
        }
        if ui
            .checkbox(
                &mut self.prefs.idle_when_minimized,
                "Stay fully idle while minimized",
            )
            .changed()
        {
            changed = true;
        }
        if ui
            .checkbox(
                &mut self.prefs.trim_memory_on_idle,
                "Release scratch buffers after 10 seconds of idle",
            )
            .changed()
        {
            changed = true;
        }

        ui.label(
            RichText::new(if self.app_idle {
                "Idle. The window is in the background, so redraws are paused."
            } else {
                "Running. The window has focus."
            })
            .font(mono_font(10.6))
            .color(if self.app_idle {
                pal.accent
            } else {
                pal.text_muted
            }),
        );

        ui.separator();

        ui.label(
            RichText::new("Threads")
                .font(plex_medium(12.4))
                .color(pal.text_primary),
        );
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Worker threads")
                    .size(11.5)
                    .color(pal.text_secondary),
            );
            if ui
                .add(
                    DragValue::new(&mut self.prefs.worker_threads)
                        .range(0..=64)
                        .speed(0.2),
                )
                .on_hover_text("0 = automatic (all available cores)")
                .changed()
            {
                changed = true;
            }
            ui.label(
                RichText::new(format!(
                    "(0 = auto, currently {})",
                    self.prefs.effective_worker_threads()
                ))
                .font(mono_font(10.4))
                .color(pal.text_muted),
            );
        });
        ui.label(
            RichText::new("Thread count is applied on the next launch.")
                .font(mono_font(10.2))
                .color(pal.text_muted),
        );
        if ui
            .checkbox(
                &mut self.prefs.frame_dedup_enabled,
                "Skip re-rendering identical consecutive video frames",
            )
            .changed()
        {
            changed = true;
        }

        ui.separator();

        ui.label(
            RichText::new("Video decode")
                .font(plex_medium(12.4))
                .color(pal.text_primary),
        );
        ui.label(
            RichText::new("Live Preview Decode Height Cap")
                .size(11.5)
                .color(pal.text_secondary),
        );
        ui.horizontal(|ui| {
            for (v, lbl) in [
                (480u32, "480p (Fastest)"),
                (640u32, "640p (Balanced)"),
                (854u32, "854p"),
                (1080u32, "1080p (Sharpest)"),
            ] {
                if ui
                    .radio_value(&mut self.prefs.video_preview_max_dim, v, lbl)
                    .changed()
                {
                    changed = true;
                    reopen_video = true;
                }
            }
        });
        if ui
            .checkbox(&mut self.prefs.audio_envelope_enabled, "React to the audio")
            .changed()
        {
            changed = true;
            reopen_video = true;
        }
        if reopen_video {
            ui.label(
                RichText::new("Re-open the video for decode changes to take effect.")
                    .font(mono_font(10.4))
                    .color(pal.accent),
            );
        }

        if changed {
            self.persist_prefs();
        }
    }

    fn draw_settings_export(&mut self, ui: &mut egui::Ui) {
        let pal = self.palette;
        let mut changed = false;

        ui.label(
            RichText::new("Video pipeline")
                .font(plex_medium(13.0))
                .color(pal.text_primary),
        );

        ui.label(
            RichText::new("PNG export scale")
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
            RichText::new("Lower CRF keeps character edges sharper. 14 is master quality, 18 is high, 24 is small.")
                .size(11.0)
                .color(pal.text_muted),
        );

        ui.separator();
        ui.label(
            RichText::new("ffmpeg / ffprobe folder (optional)")
                .font(plex_medium(12.2))
                .color(pal.text_primary),
        );
        ui.label(
            RichText::new("Leave empty to look next to the app, then on PATH.")
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
            RichText::new("Shortcuts")
                .font(plex_medium(13.0))
                .color(pal.text_primary),
        );

        let bindings = [
            ("Ctrl + O", "Open image or animated GIF"),
            ("Ctrl + Shift + O", "Open video file"),
            ("Ctrl + W", "Close current media"),
            ("Ctrl + B", "Show / hide left Inspector side panel"),
            (
                "Ctrl + Shift + R",
                "Reset all ASCII parameters to normal default",
            ),
            ("Ctrl + S", "Export a PNG at the configured scale"),
            ("Ctrl + Shift + C", "Copy the one-line lg2: Look Code"),
            ("Ctrl + ,", "Open or close Workstation Settings"),
            ("F1", "Open the LumaGlyph documentation"),
            (
                "Left / Right Arrow",
                "Seek the loaded video back / forward 5 seconds",
            ),
            ("Tab", "Zen mode: hide the side dock and status bar"),
            ("Space", "Play / pause loaded video file"),
            ("B", "Toggle True OLED #000000 pitch-black canvas"),
            ("N", "Toggle Neon Wireframe contour-only mode"),
            ("[  /  ]", "Change grid columns by 8"),
            ("0 / 1 / 2 / 3", "Zoom to fit, 100%, 200%, 350%"),
            ("Mouse Wheel", "Smooth zoom around viewport"),
            ("Double-Click Label", "Reset this slider"),
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
            if let Some(tex) = &self.app_logo_tex {
                ui.add(
                    egui::Image::from_texture((tex.id(), Vec2::splat(36.0)))
                        .fit_to_exact_size(Vec2::splat(36.0)),
                );
                ui.add_space(4.0);
            }
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
            RichText::new(
                "Turns images and video into ASCII art with 95 characters. Runs offline.",
            )
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
            RichText::new("Documentation")
                .font(plex_medium(12.2))
                .color(pal.text_primary),
        );
        ui.label(
            RichText::new("Guides for people who have never used GitHub.")
                .size(11.4)
                .color(pal.text_secondary),
        );
        ui.horizontal(|ui| {
            if ui.small_button("Quick Start").clicked() {
                ui.ctx()
                    .open_url(egui::OpenUrl::new_tab(DOCS_QUICKSTART_URL));
            }
            if ui.small_button("Download & Install").clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(DOCS_INSTALL_URL));
            }
            if ui.small_button("User Guide").clicked() {
                ui.ctx()
                    .open_url(egui::OpenUrl::new_tab(DOCS_USER_GUIDE_URL));
            }
            if ui.small_button("Troubleshooting").clicked() {
                ui.ctx()
                    .open_url(egui::OpenUrl::new_tab(DOCS_TROUBLESHOOTING_URL));
            }
            if ui.small_button("Latest Downloads").clicked() {
                ui.ctx()
                    .open_url(egui::OpenUrl::new_tab(LATEST_RELEASE_URL));
            }
        });

        ui.separator();
        ui.label(
            RichText::new("Network")
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
                            let up_busy = self.self_update_rx.is_some();
                            let up_label = if up_busy {
                                "Updating & Relaunching...".to_string()
                            } else {
                                format!("Update & Relaunch ({})", info.latest_tag)
                            };
                            if ui
                                .add_enabled(!up_busy, egui::Button::new(up_label))
                                .on_hover_text(
                                    "Rebuilds from the verified release tag (in a local Cargo checkout) or installs the SHA-256-verified release binary in-place, then automatically launches the new release",
                                )
                                .clicked()
                            {
                                let _ = self.prefs.save();
                                self.self_update_status = None;
                                self.self_update_rx =
                                    Some(spawn_verified_self_update(self.prefs.github_repo.clone()));
                            }
                            ui.hyperlink_to("Release Notes", &info.html_url);
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

        if let Some(Err(err)) = &self.self_update_status {
            ui.label(RichText::new(err).size(11.1).color(pal.warn));
        }
    }
}

impl eframe::App for LumaGlyphApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Background-idle policy: an unfocused or minimized window can drop to zero work per second.
        let focused = ctx.input(|i| i.focused);
        let minimized = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        self.app_idle = (self.prefs.idle_when_unfocused && !focused)
            || (self.prefs.idle_when_minimized && minimized);

        self.handle_dropped_files(ctx);
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

        let show_chrome = !self.zen_mode && (self.has_media() || self.inspector_open);

        if show_chrome {
            egui::TopBottomPanel::top("workstation_command_strip")
                .exact_height(31.0)
                .frame(
                    Frame::new()
                        .fill(pal.toolbar_bg)
                        .inner_margin(Margin::symmetric(10, 4))
                        .stroke(Stroke::new(1.0_f32, pal.border_subtle)),
                )
                .show(ctx, |ui| self.draw_command_strip(ui));
        }

        if !self.zen_mode && self.prefs.show_status_bar {
            egui::TopBottomPanel::bottom("workstation_status_strip")
                .exact_height(23.0)
                .frame(
                    Frame::new()
                        .fill(pal.menu_bg)
                        .inner_margin(Margin::symmetric(10, 3))
                        .stroke(Stroke::new(1.0_f32, pal.border_subtle)),
                )
                .show(ctx, |ui| self.draw_status_strip(ui));
        }

        if !self.zen_mode && self.inspector_open {
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

        let central_bg = if !self.has_media() {
            pal.panel_bg
        } else if self.params.oled_black {
            Color32::BLACK
        } else {
            pal.viewport_bg
        };
        egui::CentralPanel::default()
            .frame(Frame::new().fill(central_bg))
            .show(ctx, |ui| self.draw_viewport(ui));

        self.draw_settings_window(ctx);

        // Measured render rate for the optional status-strip readout.
        self.fps_frames = self.fps_frames.saturating_add(1);
        let window_secs = self.fps_window_start.elapsed().as_secs_f32();
        if window_secs >= 0.5 {
            self.measured_fps = self.fps_frames as f32 / window_secs;
            self.fps_frames = 0;
            self.fps_window_start = Instant::now();
        }

        if self.app_idle {
            let since = *self.idle_since.get_or_insert_with(Instant::now);
            if self.prefs.trim_memory_on_idle && since.elapsed().as_secs() >= 10 {
                // Hand scratch memory back to the OS once the window has been idle a while.
                self.raster_rgba = Vec::new();
                self.ascii_frame.cells.shrink_to_fit();
                self.hires_atlas = None;
            }
            if self.prefs.trim_memory_on_idle {
                ctx.request_repaint_after(std::time::Duration::from_secs(2));
            }
        } else {
            self.idle_since = None;
        }

        self.frames_drawn = self.frames_drawn.saturating_add(1);
        if let Some(limit) = self.auto_exit_after_frames {
            if self.frames_drawn < limit {
                ctx.request_repaint_after(
                    self.prefs
                        .repaint_interval()
                        .unwrap_or(std::time::Duration::from_millis(16)),
                );
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

/// Cheap 64-bit content fingerprint over a decimated sample of a frame buffer,
/// used to skip redundant re-renders of unchanged video frames.
fn frame_fingerprint(buf: &[u8]) -> u64 {
    if buf.is_empty() {
        return 0;
    }
    let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
    let stride = (buf.len() / 4096).max(1);
    let mut i = 0usize;
    while i < buf.len() {
        hash ^= u64::from(buf[i]);
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
        i += stride;
    }
    hash ^= buf.len() as u64;
    hash
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
    // Apply the persisted rendering & optimisation preferences before the GPU context exists.
    let prefs = AppPreferences::load();
    if prefs.worker_threads > 0 {
        let _ = rayon::ThreadPoolBuilder::new()
            .num_threads(prefs.effective_worker_threads())
            .thread_name(|i| format!("lumaglyph-worker-{i}"))
            .build_global();
    }

    let base_options = |multisampling: u16| eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("LumaGlyph")
            .with_icon(crate::icon::eframe_icon_data())
            .with_inner_size([1520.0, 900.0])
            .with_min_inner_size([1024.0, 640.0])
            .with_drag_and_drop(true),
        vsync: prefs.vsync,
        multisampling,
        ..Default::default()
    };

    // eframe uses `0` to mean "no multisampling"; glutin rejects an explicit 1-sample request.
    let requested_msaa = if prefs.msaa_samples <= 1 {
        0
    } else {
        prefs.msaa_samples
    };
    let first_boot = boot.clone();
    let first = eframe::run_native(
        "LumaGlyph",
        base_options(requested_msaa),
        Box::new(move |cc| Ok(Box::new(LumaGlyphApp::new(cc, first_boot)))),
    );

    match first {
        Ok(()) => Ok(()),
        // Some drivers (software rasterizers, older GPUs, remote desktops) publish no
        // multisampled framebuffer config. Never fail to boot over a quality preference:
        // fall back to 1x MSAA and remember that the GPU cannot honour the request.
        Err(primary_err) if requested_msaa > 0 => {
            let fallback = eframe::run_native(
                "LumaGlyph",
                base_options(0),
                Box::new(move |cc| Ok(Box::new(LumaGlyphApp::new(cc, boot)))),
            );
            fallback.map_err(|fallback_err| {
                format!(
                    "Failed launching native GPU window: {primary_err} (retried without MSAA: {fallback_err})"
                )
            })
        }
        Err(err) => Err(format!("Failed launching native GPU window: {err}")),
    }
}
