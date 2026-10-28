use crate::charset::CharsetPreset;
use crate::color::{ColorMode, TriTonePalette};
use crate::engine::{convert_image_buffer, AsciiFrame, RenderParams};
use crate::export::{
    export_ansi, export_html, export_png, export_svg, export_txt, frame_to_ansi_string,
};
use crate::glyph_atlas::GlyphAtlas;
use crate::gui::{launch_gui, GuiBootOptions, SettingsTab};
use crate::samples::{generate_sample_rgba, SamplePreset};
use crate::telemetry::format_rss_mb;
use crate::theme::StudioTheme;
use crate::updater::{
    check_latest_release_blocking, download_and_verify_release_blocking, format_build_report,
    AppPreferences, APP_VERSION, DEFAULT_GITHUB_REPO,
};
use crate::video::{detect_ffmpeg_version, transcode_video_streaming, TranscodeProgress};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(
    name = "lumaglyph",
    version,
    about = "High-throughput 95-character color ASCII art and streaming video workstation"
)]
pub struct Cli {
    /// Optional media file to open directly in the native workstation when no subcommand is given.
    pub file: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Launch the interactive GPU Workstation (default when run with no subcommand).
    Studio {
        /// Optional image or video file to open on startup.
        file: Option<PathBuf>,
        #[arg(long)]
        preset: Option<String>,
        /// Apply a one-line shareable Look Code (`lg2:...`).
        #[arg(long)]
        look: Option<String>,
        /// Workstation UI theme (`graphite`, `oled`, `tungsten`, `nordic`, `daylight`).
        #[arg(long)]
        theme: Option<String>,
        /// Open Settings dialog to a specific tab (`appearance`, `engine`, `export`, `shortcuts`, `about`).
        #[arg(long)]
        settings: Option<String>,
        #[arg(long)]
        color: Option<String>,
        /// Tri-Tone palette (`solar`, `tokyo`, `blade`, `emerald`, or `RRGGBB-RRGGBB-RRGGBB`).
        #[arg(long)]
        tritone: Option<String>,
        #[arg(long)]
        charset: Option<String>,
        #[arg(long)]
        cols: Option<u32>,
        #[arg(long)]
        oled_black: bool,
        #[arg(long)]
        black_gate: Option<f32>,
        #[arg(long)]
        transparent: bool,
        #[arg(long)]
        wireframe: bool,
        #[arg(long)]
        audio_reactive: Option<f32>,
        #[arg(long)]
        split: Option<f32>,
        #[arg(long)]
        zoom: Option<f32>,
        #[arg(long)]
        bloom: bool,
        #[arg(long)]
        live_motion: bool,
        #[arg(long)]
        show_updates: bool,
        #[arg(long, hide = true)]
        exit_after_frames: Option<u32>,
    },
    /// Convert an image (PNG/JPG/WebP/GIF/BMP) to 95-character ASCII (PNG/SVG/HTML/ANS/TXT or stdout).
    Image {
        /// Input image path (or `sample:cyber`, `sample:synthwave`, `sample:orbs`).
        input: String,
        /// Output file (.png, .svg, .html, .ans, .txt). Prints TrueColor ANSI to stdout if omitted.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Apply a shareable `lg2:...` Look Code before CLI overrides.
        #[arg(long)]
        look: Option<String>,
        #[arg(long)]
        cols: Option<u32>,
        #[arg(long)]
        charset: Option<String>,
        #[arg(long)]
        color: Option<String>,
        /// Tri-Tone palette (`solar`, `tokyo`, `blade`, `emerald`, or `RRGGBB-RRGGBB-RRGGBB`).
        #[arg(long)]
        tritone: Option<String>,
        /// True `#000000` pitch-black background for OLED displays.
        #[arg(long)]
        oled_black: bool,
        /// Shadow noise gate (`0.00..=0.35`, default `0.03`).
        #[arg(long)]
        black_gate: Option<f32>,
        /// Render background with true `RGBA [0,0,0,0]` alpha cutout.
        #[arg(long)]
        transparent: bool,
        /// Contour / Wireframe mode (edges only, flat fills dropped).
        #[arg(long)]
        wireframe: bool,
        #[arg(long)]
        wireframe_sens: Option<f32>,
        #[arg(long)]
        crunch: Option<f32>,
        #[arg(long)]
        edge: Option<f32>,
        #[arg(long)]
        dither: Option<f32>,
        #[arg(long)]
        bloom: bool,
        #[arg(long)]
        scanlines: bool,
    },
    /// Stream-transcode a video file to ASCII video (.mp4 / .webm / .mov / .gif / .ans) with flat memory.
    Video {
        /// Input video path (.mp4, .mkv, .mov, .webm).
        input: PathBuf,
        /// Output video/animation path (.mp4, .webm, .mov, .gif, .ans).
        #[arg(short, long)]
        output: PathBuf,
        /// Apply a shareable `lg2:...` Look Code before CLI overrides.
        #[arg(long)]
        look: Option<String>,
        #[arg(long)]
        cols: Option<u32>,
        #[arg(long)]
        fps: Option<f64>,
        #[arg(long)]
        charset: Option<String>,
        #[arg(long)]
        color: Option<String>,
        /// Tri-Tone palette (`solar`, `tokyo`, `blade`, `emerald`, or `RRGGBB-RRGGBB-RRGGBB`).
        #[arg(long)]
        tritone: Option<String>,
        /// True `#000000` pitch-black background for OLED displays.
        #[arg(long)]
        oled_black: bool,
        /// Shadow noise gate (`0.00..=0.35`, default `0.03`).
        #[arg(long)]
        black_gate: Option<f32>,
        /// Transparent alpha video overlay (`.webm` VP9 yuva420p, `.mov` RGBA, or `.gif`).
        #[arg(long)]
        transparent: bool,
        /// Contour / Wireframe mode.
        #[arg(long)]
        wireframe: bool,
        #[arg(long)]
        wireframe_sens: Option<f32>,
        /// Audio-reactive beat modulation strength (`0.0..=2.0`).
        #[arg(long)]
        audio_reactive: Option<f32>,
        #[arg(long)]
        stability: Option<f32>,
        #[arg(long)]
        crunch: Option<f32>,
        #[arg(long)]
        edge: Option<f32>,
        #[arg(long)]
        bloom: bool,
    },
    /// Print or inspect a `lg2:...` shareable Look Code.
    Look {
        /// Optional `lg2:...` code to inspect; prints default Look Code if omitted.
        code: Option<String>,
    },
    /// Print detailed version, commit revision, target triple, toolchain, and runtime info.
    #[command(alias = "info")]
    BuildInfo,
    /// Run a multi-core throughput and memory footprint benchmark.
    Bench {
        #[arg(long, default_value_t = 180)]
        cols: u32,
        #[arg(long, default_value_t = 120)]
        iters: u32,
    },
    /// Manually check GitHub Releases or download a SHA-256 verified binary update (no background telemetry).
    CheckUpdate {
        #[arg(long)]
        repo: Option<String>,
        /// Download, SHA-256 verify, inspect native binary header, and extract the latest release binary.
        #[arg(long)]
        download: bool,
        /// Destination directory when `--download` is set (defaults to current working directory).
        #[arg(long)]
        dest: Option<PathBuf>,
        /// Permit `--download` from a custom `--repo` other than the canonical repository.
        #[arg(long)]
        allow_custom_repo: bool,
    },
}

/// Determines whether `args` represent a headless CLI / help / version invocation (which should
/// attach to an existing parent shell on Windows) versus a native GUI workstation launch (which
/// runs completely detached with zero console window).
pub fn should_attach_parent_console<S: AsRef<str>>(args: &[S]) -> bool {
    if args.len() <= 1 {
        return false;
    }
    for a in &args[1..] {
        let s = a.as_ref();
        if matches!(s, "-h" | "--help" | "-V" | "--version" | "help") {
            return true;
        }
    }
    let first = args[1].as_ref();
    if first == "studio" {
        return false;
    }
    if matches!(
        first,
        "image" | "video" | "look" | "build-info" | "info" | "bench" | "check-update"
    ) {
        return true;
    }
    if first.starts_with('-') {
        return true;
    }
    // Single positional file path (`lumaglyph.exe C:\media\clip.mp4`) opens GUI directly
    args.len() > 2
}

pub fn parse_tritone_arg(spec: &str) -> Result<TriTonePalette, String> {
    match spec.trim().to_ascii_lowercase().as_str() {
        "solar" | "flare" | "inferno" | "fire" => Ok(TriTonePalette::SOLAR_FLARE),
        "tokyo" | "cyber" | "neon" => Ok(TriTonePalette::TOKYO_MIDNIGHT),
        "blade" | "amber" | "gold" => Ok(TriTonePalette::BLADE_TEAL_AMBER),
        "emerald" | "ice" | "arctic" => Ok(TriTonePalette::EMERALD_ICE),
        _ => TriTonePalette::from_hex_triplet(spec).ok_or_else(|| {
            format!(
                "Invalid --tritone '{spec}': use solar|tokyo|blade|emerald or RRGGBB-RRGGBB-RRGGBB"
            )
        }),
    }
}

pub fn run_entry() -> Result<(), String> {
    let cli = Cli::parse();

    match cli.command {
        None => launch_gui(GuiBootOptions {
            initial_media: cli.file,
            ..GuiBootOptions::default()
        }),
        Some(Commands::Studio {
            file,
            preset,
            look,
            theme,
            settings,
            color,
            tritone,
            charset,
            cols,
            oled_black,
            black_gate,
            transparent,
            wireframe,
            audio_reactive,
            split,
            zoom,
            bloom,
            live_motion,
            show_updates,
            exit_after_frames,
        }) => {
            let prefs = AppPreferences::load();
            let mut params = RenderParams {
                cols: prefs.default_cols.clamp(40, 320),
                charset: prefs.default_charset,
                color_mode: prefs.default_color_mode,
                oled_black: prefs.default_oled_black,
                ..RenderParams::default()
            };
            if let Some(code) = look.as_deref() {
                params.apply_look_code(code)?;
            }
            if let Some(c) = cols {
                params.cols = c;
            }
            if let Some(cs) = charset.as_deref().and_then(CharsetPreset::from_cli) {
                params.charset = cs;
            }
            if let Some(cm) = color.as_deref().and_then(ColorMode::from_cli) {
                params.color_mode = cm;
            }
            if let Some(tt) = tritone.as_deref() {
                params.tritone = parse_tritone_arg(tt)?;
                params.color_mode = ColorMode::TriTone;
            }
            if oled_black {
                params.oled_black = true;
                params.transparent_bg = false;
            }
            if let Some(bg) = black_gate {
                params.black_gate = bg.clamp(0.0, 0.35);
            }
            if transparent {
                params.transparent_bg = true;
                params.oled_black = false;
            }
            if wireframe {
                params.wireframe_only = true;
            }
            if let Some(ar) = audio_reactive {
                params.audio_reactive = ar.clamp(0.0, 2.0);
            }
            if bloom {
                params.crt_bloom = true;
            }

            let initial_preset = match preset.as_deref() {
                Some("synthwave") => Some(SamplePreset::SynthwaveHorizon),
                Some("orbs") => Some(SamplePreset::ChromaticSpheres),
                Some("cyber") => Some(SamplePreset::CyberSculpture),
                _ => None,
            };

            let theme_override = theme.as_deref().and_then(StudioTheme::from_cli);
            let open_settings_tab = settings
                .as_deref()
                .and_then(|s| SettingsTab::from_cli(s).or(Some(SettingsTab::Appearance)));

            launch_gui(GuiBootOptions {
                initial_media: file,
                initial_preset,
                initial_params: Some(params),
                theme_override,
                open_settings_tab,
                split_wipe: split,
                initial_zoom: zoom,
                live_motion,
                show_update_modal: show_updates,
                auto_exit_after_frames: exit_after_frames,
            })
        }
        Some(Commands::Image {
            input,
            output,
            look,
            cols,
            charset,
            color,
            tritone,
            oled_black,
            black_gate,
            transparent,
            wireframe,
            wireframe_sens,
            crunch,
            edge,
            dither,
            bloom,
            scanlines,
        }) => {
            let (rgba, w, h) = if let Some(rest) = input.strip_prefix("sample:") {
                let p = match rest {
                    "synthwave" => SamplePreset::SynthwaveHorizon,
                    "orbs" => SamplePreset::ChromaticSpheres,
                    _ => SamplePreset::CyberSculpture,
                };
                (generate_sample_rgba(p, 1280, 800, 0.0), 1280, 800)
            } else {
                let img = image::open(&input)
                    .map_err(|e| format!("Failed opening image '{input}': {e}"))?
                    .to_rgba8();
                let (w, h) = img.dimensions();
                (img.into_raw(), w, h)
            };

            let atlas = GlyphAtlas::default_embedded(20);
            let mut params = RenderParams::default();
            if let Some(code) = look.as_deref() {
                params.apply_look_code(code)?;
            }
            if let Some(c) = cols {
                params.cols = c;
            }
            if let Some(cs) = charset.as_deref().and_then(CharsetPreset::from_cli) {
                params.charset = cs;
            }
            if let Some(cm) = color.as_deref().and_then(ColorMode::from_cli) {
                params.color_mode = cm;
            }
            if let Some(tt) = tritone.as_deref() {
                params.tritone = parse_tritone_arg(tt)?;
                params.color_mode = ColorMode::TriTone;
            }
            if oled_black {
                params.oled_black = true;
                params.transparent_bg = false;
            }
            if let Some(bg) = black_gate {
                params.black_gate = bg.clamp(0.0, 0.35);
            }
            if transparent {
                params.transparent_bg = true;
                params.oled_black = false;
            }
            if wireframe {
                params.wireframe_only = true;
            }
            if let Some(ws) = wireframe_sens {
                params.wireframe_sensitivity = ws.clamp(0.2, 3.0);
            }
            if let Some(cr) = crunch {
                params.directional_crunch = cr;
            }
            if let Some(ew) = edge {
                params.edge_weight = ew;
            }
            if let Some(d) = dither {
                params.dither = d;
            }
            if bloom {
                params.crt_bloom = true;
            }
            if scanlines {
                params.scanlines = true;
            }

            let mut frame = AsciiFrame::new();
            convert_image_buffer(&rgba, w, h, 4, &atlas, &params, false, &mut frame);

            if let Some(out_path) = output {
                let ext = out_path
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("png")
                    .to_ascii_lowercase();
                match ext.as_str() {
                    "png" => {
                        let (pw, ph) = export_png(&frame, &atlas, 28, &params, &out_path)?;
                        eprintln!(
                            "Saved PNG {} ({pw}x{ph} px, {}x{} cells, {}/95 glyphs, {:.2} ms, RSS {})\nLook Code: {}",
                            out_path.display(),
                            frame.cols,
                            frame.rows,
                            frame.unique_glyphs,
                            frame.render_ms,
                            format_rss_mb(),
                            params.to_look_code()
                        );
                    }
                    "svg" => export_svg(&frame, &params, &out_path)?,
                    "html" | "htm" => export_html(&frame, &params, &out_path)?,
                    "ans" => export_ansi(&frame, &params, &out_path)?,
                    _ => export_txt(&frame, &out_path)?,
                }
            } else {
                println!("{}", frame_to_ansi_string(&frame, &params));
            }
            Ok(())
        }
        Some(Commands::Video {
            input,
            output,
            look,
            cols,
            fps,
            charset,
            color,
            tritone,
            oled_black,
            black_gate,
            transparent,
            wireframe,
            wireframe_sens,
            audio_reactive,
            stability,
            crunch,
            edge,
            bloom,
        }) => {
            let atlas = GlyphAtlas::default_embedded(20);
            let mut params = RenderParams::default();
            if let Some(code) = look.as_deref() {
                params.apply_look_code(code)?;
            }
            if let Some(c) = cols {
                params.cols = c;
            }
            if let Some(cs) = charset.as_deref().and_then(CharsetPreset::from_cli) {
                params.charset = cs;
            }
            if let Some(cm) = color.as_deref().and_then(ColorMode::from_cli) {
                params.color_mode = cm;
            }
            if let Some(tt) = tritone.as_deref() {
                params.tritone = parse_tritone_arg(tt)?;
                params.color_mode = ColorMode::TriTone;
            }
            if oled_black {
                params.oled_black = true;
                params.transparent_bg = false;
            }
            if let Some(bg) = black_gate {
                params.black_gate = bg.clamp(0.0, 0.35);
            }
            if transparent {
                params.transparent_bg = true;
                params.oled_black = false;
            }
            if wireframe {
                params.wireframe_only = true;
            }
            if let Some(ws) = wireframe_sens {
                params.wireframe_sensitivity = ws.clamp(0.2, 3.0);
            }
            if let Some(ar) = audio_reactive {
                params.audio_reactive = ar.clamp(0.0, 2.0);
            }
            if let Some(st) = stability {
                params.temporal_stability = st;
            }
            if let Some(cr) = crunch {
                params.directional_crunch = cr;
            }
            if let Some(ew) = edge {
                params.edge_weight = ew;
            }
            if bloom {
                params.crt_bloom = true;
            }

            let progress = TranscodeProgress::new(0);
            let monitor = Arc::clone(&progress);
            let reporter = std::thread::spawn(move || {
                while !monitor.finished.load(Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_millis(400));
                    let done = monitor.frames_done.load(Ordering::Relaxed);
                    if done > 0 && !monitor.finished.load(Ordering::Relaxed) {
                        eprint!(
                            "\rStreaming ASCII encode: {done} frames | RSS {}   ",
                            format_rss_mb()
                        );
                    }
                }
            });

            let summary =
                transcode_video_streaming(&input, &output, &atlas, &params, fps, progress)?;
            let _ = reporter.join();
            eprintln!(
                "\rCompleted: {} frames ({}x{} px) in {:.2}s ({:.1} FPS) | RSS {}\nLook Code: {}",
                summary.frames_rendered,
                summary.out_width,
                summary.out_height,
                summary.elapsed_secs,
                summary.avg_fps,
                format_rss_mb(),
                params.to_look_code()
            );
            Ok(())
        }
        Some(Commands::Look { code }) => {
            let mut params = RenderParams::default();
            if let Some(c) = code {
                params.apply_look_code(&c)?;
            }
            println!("{}", params.to_look_code());
            Ok(())
        }
        Some(Commands::BuildInfo) => {
            let ff = detect_ffmpeg_version();
            let rss = format_rss_mb();
            println!("{}", format_build_report(None, ff.as_deref(), &rss));
            Ok(())
        }
        Some(Commands::Bench { cols, iters }) => {
            let atlas = GlyphAtlas::default_embedded(20);
            let rgba = generate_sample_rgba(SamplePreset::CyberSculpture, 1280, 720, 0.25);
            let params = RenderParams {
                cols,
                ..RenderParams::default()
            };
            let mut frame = AsciiFrame::new();

            convert_image_buffer(&rgba, 1280, 720, 4, &atlas, &params, true, &mut frame);

            let t0 = Instant::now();
            for i in 0..iters {
                let use_temp = i > 0;
                convert_image_buffer(&rgba, 1280, 720, 4, &atlas, &params, use_temp, &mut frame);
            }
            let elapsed = t0.elapsed().as_secs_f64();
            let avg_ms = (elapsed * 1000.0) / (iters as f64);
            let fps = (iters as f64) / elapsed;
            let cells_per_frame = (frame.cols as u64) * (frame.rows as u64);
            let mcells_sec = ((cells_per_frame as f64) * fps) / 1_000_000.0;

            println!("LumaGlyph v{APP_VERSION} Engine Benchmark");
            println!(
                "  Grid Resolution : {}x{} ({cells_per_frame} cells)",
                frame.cols, frame.rows
            );
            println!(
                "  Active Glyphs   : {} / 95 printable ASCII used",
                frame.unique_glyphs
            );
            println!("  Avg Latency     : {avg_ms:.3} ms / frame");
            println!("  Engine Speed    : {fps:.1} FPS ({mcells_sec:.2} M cells/sec)");
            println!("  Memory (RSS)    : {}", format_rss_mb());
            Ok(())
        }
        Some(Commands::CheckUpdate {
            repo,
            download,
            dest,
            allow_custom_repo,
        }) => {
            let prefs = AppPreferences::load();
            let slug = repo.unwrap_or_else(|| {
                if download && !allow_custom_repo {
                    DEFAULT_GITHUB_REPO.to_string()
                } else {
                    prefs.github_repo
                }
            });

            if download {
                let target_dir = dest.unwrap_or_else(|| PathBuf::from("."));
                let out =
                    download_and_verify_release_blocking(&slug, &target_dir, !allow_custom_repo)?;
                println!("Verified release : {}", out.tag);
                println!("Archive          : {}", out.archive_name);
                println!("SHA-256 (matched): {}", out.sha256_hex);
                println!(
                    "Saved executable : {} ({:.2} MB)",
                    out.saved_path.display(),
                    (out.binary_size_bytes as f64) / (1024.0 * 1024.0)
                );
            } else {
                let out = check_latest_release_blocking(&slug)?;
                if out.update_available {
                    println!(
                        "New release available: {} (current v{APP_VERSION})\n{}",
                        out.latest_tag, out.html_url
                    );
                } else {
                    println!("Up to date. Latest GitHub release is {}.", out.latest_tag);
                }
            }
            Ok(())
        }
    }
}
