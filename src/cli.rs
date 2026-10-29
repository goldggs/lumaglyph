use crate::charset::CharsetPreset;
use crate::color::{ColorMode, TriTonePalette};
use crate::engine::{convert_image_buffer, AsciiFrame, RenderParams};
use crate::export::{
    export_ansi, export_html, export_png, export_svg, export_txt, frame_to_ansi_string,
};
use crate::glyph_atlas::GlyphAtlas;
use crate::gui::{launch_gui, GuiBootOptions, SettingsTab};
use crate::telemetry::format_rss_mb;
use crate::theme::StudioTheme;
use crate::updater::{
    check_latest_release_blocking, download_and_verify_release_blocking, format_build_report,
    relaunch_executable, update_and_prepare_relaunch_blocking, AppPreferences, APP_VERSION,
    DEFAULT_GITHUB_REPO,
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
        zoom: Option<f32>,
        #[arg(long)]
        bloom: bool,
        #[arg(long)]
        show_updates: bool,
        #[arg(long, hide = true)]
        exit_after_frames: Option<u32>,
    },
    /// Convert an image (PNG/JPG/WebP/GIF/BMP) to 95-character ASCII (PNG/SVG/HTML/ANS/TXT or stdout).
    Image {
        /// Input image file path (PNG, JPG, WebP, GIF, or BMP).
        input: PathBuf,
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
    /// Manually check GitHub Releases, download a verified binary, or install & relaunch in-place.
    CheckUpdate {
        #[arg(long)]
        repo: Option<String>,
        /// Download, SHA-256 verify, inspect native binary header, and extract the latest release binary.
        #[arg(long)]
        download: bool,
        /// Rebuild (if inside a clean Cargo checkout) or install the verified release binary in-place.
        #[arg(long)]
        install: bool,
        /// Automatically launch the updated binary immediately after `--install`.
        #[arg(long)]
        relaunch: bool,
        /// Force rebuilding from the verified GitHub release tag in a local Cargo/Git checkout.
        #[arg(long)]
        from_source: bool,
        /// Destination directory when `--download` is set (defaults to current working directory).
        #[arg(long)]
        dest: Option<PathBuf>,
        /// Permit `--download` / `--install` from a custom `--repo` other than the canonical repository.
        #[arg(long)]
        allow_custom_repo: bool,
    },
    /// Rebuild or install the latest verified GitHub release in-place and automatically launch it.
    #[command(visible_alias = "update")]
    SelfUpdate {
        #[arg(long)]
        repo: Option<String>,
        /// Force rebuilding from the verified GitHub release tag in a local Cargo/Git checkout.
        #[arg(long)]
        from_source: bool,
        /// Skip automatically launching the newly installed binary after updating.
        #[arg(long)]
        no_relaunch: bool,
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
        "image"
            | "video"
            | "look"
            | "build-info"
            | "info"
            | "bench"
            | "check-update"
            | "self-update"
            | "update"
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

/// Rejects a positional file argument that does not exist.
///
/// Without this, `lumaglyph some-typo` is parsed as a *file path* rather than an
/// unknown subcommand, and the app dutifully opens the workstation on a file that is
/// not there. That is a bad enough experience on a desktop (a window appears and does
/// nothing); in a headless environment it hangs forever, because there is no window
/// server to fail against. Failing before the window is created fixes both.
fn reject_missing_media(path: Option<PathBuf>) -> Result<Option<PathBuf>, String> {
    match path {
        Some(path) if !path.exists() => Err(format!(
            "no such file: {}\n\
             If you meant a different file, check the path. Run `lumaglyph --help` for the list of commands.",
            path.display()
        )),
        other => Ok(other),
    }
}

pub fn run_entry() -> Result<(), String> {
    let cli = Cli::parse();

    match cli.command {
        None => launch_gui(GuiBootOptions {
            initial_media: reject_missing_media(cli.file)?,
            ..GuiBootOptions::default()
        }),
        Some(Commands::Studio {
            file,
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
            zoom,
            bloom,
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
            // An unrecognised value is an error, not a silent fallback: a typo in a
            // batch script would otherwise produce a whole folder of wrong-looking
            // exports with exit code 0.
            if let Some(cs) = charset.as_deref() {
                params.charset = CharsetPreset::from_cli(cs)
                    .ok_or_else(|| format!("unknown character set '{cs}'"))?;
            }
            if let Some(cm) = color.as_deref() {
                params.color_mode =
                    ColorMode::from_cli(cm).ok_or_else(|| format!("unknown color mode '{cm}'"))?;
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

            let theme_override = theme.as_deref().and_then(StudioTheme::from_cli);
            let open_settings_tab = settings
                .as_deref()
                .and_then(|s| SettingsTab::from_cli(s).or(Some(SettingsTab::Appearance)));

            launch_gui(GuiBootOptions {
                initial_media: reject_missing_media(file)?,
                initial_params: Some(params),
                theme_override,
                open_settings_tab,
                initial_zoom: zoom,
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
            let img = image::open(&input)
                .map_err(|e| format!("Failed opening image '{}': {e}", input.display()))?
                .to_rgba8();
            let (w, h) = img.dimensions();
            let rgba = img.into_raw();

            let atlas = GlyphAtlas::default_embedded(20);
            let mut params = RenderParams::default();
            if let Some(code) = look.as_deref() {
                params.apply_look_code(code)?;
            }
            if let Some(c) = cols {
                params.cols = c;
            }
            // An unrecognised value is an error, not a silent fallback: a typo in a
            // batch script would otherwise produce a whole folder of wrong-looking
            // exports with exit code 0.
            if let Some(cs) = charset.as_deref() {
                params.charset = CharsetPreset::from_cli(cs)
                    .ok_or_else(|| format!("unknown character set '{cs}'"))?;
            }
            if let Some(cm) = color.as_deref() {
                params.color_mode =
                    ColorMode::from_cli(cm).ok_or_else(|| format!("unknown color mode '{cm}'"))?;
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
            // An unrecognised value is an error, not a silent fallback: a typo in a
            // batch script would otherwise produce a whole folder of wrong-looking
            // exports with exit code 0.
            if let Some(cs) = charset.as_deref() {
                params.charset = CharsetPreset::from_cli(cs)
                    .ok_or_else(|| format!("unknown character set '{cs}'"))?;
            }
            if let Some(cm) = color.as_deref() {
                params.color_mode =
                    ColorMode::from_cli(cm).ok_or_else(|| format!("unknown color mode '{cm}'"))?;
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
            let bw = 1280u32;
            let bh = 720u32;
            let mut rgba = vec![0u8; (bw * bh * 4) as usize];
            for y in 0..bh {
                let ny = (y as f32 - 360.0) / 360.0;
                for x in 0..bw {
                    let nx = (x as f32 - 640.0) / 360.0;
                    let r2 = (nx * nx + ny * ny).sqrt();
                    let spoke = if (ny.atan2(nx) * 7.0).sin() > 0.12 {
                        1.0f32
                    } else {
                        0.08
                    };
                    let ring = if (r2 > 0.2 && r2 < 0.5) || (r2 > 0.65 && r2 < 0.92) {
                        1.0f32
                    } else {
                        0.05
                    };
                    let idx = ((y * bw + x) * 4) as usize;
                    rgba[idx] = ((ring * spoke * 245.0).clamp(0.0, 255.0)) as u8;
                    rgba[idx + 1] = ((((nx + 1.5) * 0.33) * 230.0).clamp(0.0, 255.0)) as u8;
                    rgba[idx + 2] =
                        (((1.0 - (r2 * 0.6).min(1.0)) * spoke * 245.0).clamp(0.0, 255.0)) as u8;
                    rgba[idx + 3] = 255;
                }
            }
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
            install,
            relaunch,
            from_source,
            dest,
            allow_custom_repo,
        }) => {
            let prefs = AppPreferences::load();
            let slug = repo.unwrap_or_else(|| {
                if (download || install) && !allow_custom_repo {
                    DEFAULT_GITHUB_REPO.to_string()
                } else {
                    prefs.github_repo
                }
            });

            if install {
                let out =
                    update_and_prepare_relaunch_blocking(&slug, from_source, !allow_custom_repo)?;
                println!("Updated release  : {} ({})", out.tag, out.method);
                println!("SHA-256          : {}", out.sha256_hex);
                println!(
                    "Installed binary : {} ({:.2} MB)",
                    out.installed_exe.display(),
                    (out.binary_size_bytes as f64) / (1024.0 * 1024.0)
                );
                if relaunch {
                    let pid = relaunch_executable(&out.installed_exe, &[])?;
                    println!("Launched updated LumaGlyph (PID {pid}).");
                }
            } else if download {
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
        Some(Commands::SelfUpdate {
            repo,
            from_source,
            no_relaunch,
            allow_custom_repo,
        }) => {
            let slug = repo.unwrap_or_else(|| DEFAULT_GITHUB_REPO.to_string());
            let out = update_and_prepare_relaunch_blocking(&slug, from_source, !allow_custom_repo)?;
            println!("Updated release  : {} ({})", out.tag, out.method);
            println!("SHA-256          : {}", out.sha256_hex);
            println!(
                "Installed binary : {} ({:.2} MB)",
                out.installed_exe.display(),
                (out.binary_size_bytes as f64) / (1024.0 * 1024.0)
            );
            if !no_relaunch {
                let pid = relaunch_executable(&out.installed_exe, &[])?;
                println!("Launched updated LumaGlyph (PID {pid}).");
            }
            Ok(())
        }
    }
}
