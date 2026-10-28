use crate::charset::CharsetPreset;
use crate::color::ColorMode;
use crate::engine::{convert_image_buffer, AsciiFrame, RenderParams};
use crate::export::{
    export_ansi, export_html, export_png, export_svg, export_txt, frame_to_ansi_string,
};
use crate::glyph_atlas::GlyphAtlas;
use crate::gui::{launch_gui, GuiBootOptions};
use crate::samples::{generate_sample_rgba, SamplePreset};
use crate::telemetry::format_rss_mb;
use crate::updater::{check_latest_release_blocking, AppPreferences, APP_VERSION};
use crate::video::{transcode_video_streaming, TranscodeProgress};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(
    name = "lumaglyph",
    version,
    about = "Blazing-fast, low-memory genuine 95-character color ASCII art & video studio"
)]
pub struct Cli {
    /// Optional media file to open directly in the native GUI studio when no subcommand is given.
    pub file: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Launch the interactive Native GPU Studio (default when run with no subcommand).
    Studio {
        /// Optional image or video file to open on startup.
        file: Option<PathBuf>,
        #[arg(long)]
        preset: Option<String>,
        #[arg(long)]
        color: Option<String>,
        #[arg(long)]
        charset: Option<String>,
        #[arg(long)]
        cols: Option<u32>,
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
    /// Convert an image (PNG/JPG/WebP/GIF/BMP) to genuine 95-char ASCII (PNG/SVG/HTML/ANS/TXT or stdout).
    Image {
        /// Input image path (or `sample:cyber`, `sample:synthwave`, `sample:orbs`).
        input: String,
        /// Output file (.png, .svg, .html, .ans, .txt). Prints TrueColor ANSI to stdout if omitted.
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(long, default_value_t = 156)]
        cols: u32,
        #[arg(long, default_value = "genuine95")]
        charset: String,
        #[arg(long, default_value = "truecolor")]
        color: String,
        #[arg(long, default_value_t = 1.40)]
        crunch: f32,
        #[arg(long, default_value_t = 0.92)]
        edge: f32,
        #[arg(long, default_value_t = 0.08)]
        dither: f32,
        #[arg(long)]
        bloom: bool,
        #[arg(long)]
        scanlines: bool,
    },
    /// Stream-transcode a video file to ASCII video (.mp4 / .gif / .ans) with flat memory usage.
    Video {
        /// Input video path (.mp4, .mkv, .mov, .webm).
        input: PathBuf,
        /// Output video/animation path (.mp4, .gif, .ans).
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long, default_value_t = 156)]
        cols: u32,
        #[arg(long)]
        fps: Option<f64>,
        #[arg(long, default_value = "genuine95")]
        charset: String,
        #[arg(long, default_value = "truecolor")]
        color: String,
        #[arg(long, default_value_t = 0.16)]
        stability: f32,
        #[arg(long, default_value_t = 1.40)]
        crunch: f32,
        #[arg(long, default_value_t = 0.92)]
        edge: f32,
        #[arg(long)]
        bloom: bool,
    },
    /// Run a multi-core throughput & memory footprint benchmark.
    Bench {
        #[arg(long, default_value_t = 180)]
        cols: u32,
        #[arg(long, default_value_t = 120)]
        iters: u32,
    },
    /// Manually check GitHub Releases for a newer version (no background telemetry).
    CheckUpdate {
        #[arg(long)]
        repo: Option<String>,
    },
}

pub fn run_entry() -> Result<(), String> {
    // Windows GUI subsystem note: when double-clicked with zero CLI arguments, launch straight into GUI
    let cli = Cli::parse();

    match cli.command {
        None => launch_gui(GuiBootOptions {
            initial_media: cli.file,
            ..GuiBootOptions::default()
        }),
        Some(Commands::Studio {
            file,
            preset,
            color,
            charset,
            cols,
            split,
            zoom,
            bloom,
            live_motion,
            show_updates,
            exit_after_frames,
        }) => {
            let mut params = RenderParams::default();
            if let Some(c) = cols {
                params.cols = c;
            }
            if let Some(cs) = charset.as_deref().and_then(CharsetPreset::from_cli) {
                params.charset = cs;
            }
            if let Some(cm) = color.as_deref().and_then(ColorMode::from_cli) {
                params.color_mode = cm;
            }
            params.crt_bloom = bloom;

            let initial_preset = match preset.as_deref() {
                Some("synthwave") => Some(SamplePreset::SynthwaveHorizon),
                Some("orbs") => Some(SamplePreset::ChromaticSpheres),
                Some("cyber") => Some(SamplePreset::CyberSculpture),
                _ => None,
            };

            launch_gui(GuiBootOptions {
                initial_media: file,
                initial_preset,
                initial_params: Some(params),
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
            cols,
            charset,
            color,
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
            let mut params = RenderParams {
                cols,
                directional_crunch: crunch,
                edge_weight: edge,
                dither,
                crt_bloom: bloom,
                scanlines,
                ..RenderParams::default()
            };
            if let Some(cs) = CharsetPreset::from_cli(&charset) {
                params.charset = cs;
            }
            if let Some(cm) = ColorMode::from_cli(&color) {
                params.color_mode = cm;
            }

            let mut frame = AsciiFrame::new();
            convert_image_buffer(&rgba, w, h, 4, &atlas, &params, false, &mut frame);

            if let Some(out_path) = output {
                let ext = out_path
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("png")
                    .to_ascii_lowercase();
                let inc_bg = params.color_mode == ColorMode::DualColor;
                match ext.as_str() {
                    "png" => {
                        let (pw, ph) = export_png(
                            &frame,
                            &atlas,
                            28,
                            params.crt_bloom,
                            params.scanlines,
                            &out_path,
                        )?;
                        eprintln!(
                            "Saved PNG {} ({pw}x{ph}px, {}x{} cells, {}/95 unique glyphs, {:.2} ms, RSS {})",
                            out_path.display(),
                            frame.cols,
                            frame.rows,
                            frame.unique_glyphs,
                            frame.render_ms,
                            format_rss_mb()
                        );
                    }
                    "svg" => export_svg(&frame, inc_bg, &out_path)?,
                    "html" | "htm" => export_html(&frame, inc_bg, &out_path)?,
                    "ans" => export_ansi(&frame, inc_bg, &out_path)?,
                    _ => export_txt(&frame, &out_path)?,
                }
            } else {
                let inc_bg = params.color_mode == ColorMode::DualColor;
                println!("{}", frame_to_ansi_string(&frame, inc_bg));
            }
            Ok(())
        }
        Some(Commands::Video {
            input,
            output,
            cols,
            fps,
            charset,
            color,
            stability,
            crunch,
            edge,
            bloom,
        }) => {
            let atlas = GlyphAtlas::default_embedded(20);
            let mut params = RenderParams {
                cols,
                temporal_stability: stability,
                directional_crunch: crunch,
                edge_weight: edge,
                crt_bloom: bloom,
                ..RenderParams::default()
            };
            if let Some(cs) = CharsetPreset::from_cli(&charset) {
                params.charset = cs;
            }
            if let Some(cm) = ColorMode::from_cli(&color) {
                params.color_mode = cm;
            }

            let progress = TranscodeProgress::new(0);
            let monitor = Arc::clone(&progress);
            let reporter = std::thread::spawn(move || {
                while !monitor.finished.load(Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_millis(400));
                    let done = monitor.frames_done.load(Ordering::Relaxed);
                    if done > 0 && !monitor.finished.load(Ordering::Relaxed) {
                        eprint!(
                            "\rStreaming ASCII encode: {done} frames · RSS {}   ",
                            format_rss_mb()
                        );
                    }
                }
            });

            let summary =
                transcode_video_streaming(&input, &output, &atlas, &params, fps, progress)?;
            let _ = reporter.join();
            eprintln!(
                "\rDone! Rendered {} frames ({}x{}px) in {:.2}s ({:.1} FPS) · Peak/Final RSS {}",
                summary.frames_rendered,
                summary.out_width,
                summary.out_height,
                summary.elapsed_secs,
                summary.avg_fps,
                format_rss_mb()
            );
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

            // Warmup pass
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
        Some(Commands::CheckUpdate { repo }) => {
            let prefs = AppPreferences::load();
            let slug = repo.unwrap_or(prefs.github_repo);
            let out = check_latest_release_blocking(&slug)?;
            if out.update_available {
                println!(
                    "New release available: {} (current v{APP_VERSION})\n{}",
                    out.latest_tag, out.html_url
                );
            } else {
                println!(
                    "Up to date (latest release: {}, installed: v{APP_VERSION})",
                    out.latest_tag
                );
            }
            Ok(())
        }
    }
}
