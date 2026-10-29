use crate::engine::{
    convert_image_buffer_with_audio, rasterize_rgb24_even, rasterize_rgba, AsciiFrame, RenderParams,
};
use crate::export::frame_to_ansi_string;
use crate::glyph_atlas::GlyphAtlas;
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

static VIDEO_CRF: AtomicU8 = AtomicU8::new(18);
static FFMPEG_CUSTOM_DIR: OnceLock<Mutex<String>> = OnceLock::new();

pub fn set_video_pipeline_prefs(custom_dir: &str, crf: u8) {
    VIDEO_CRF.store(crf.clamp(12, 34), Ordering::Relaxed);
    let cell = FFMPEG_CUSTOM_DIR.get_or_init(|| Mutex::new(String::new()));
    if let Ok(mut guard) = cell.lock() {
        *guard = custom_dir.trim().to_string();
    }
}

/// Apply `CREATE_NO_WINDOW` on Windows so background `ffmpeg` pipes never flash a console window.
fn configure_no_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = cmd;
}

/// Resolve `ffmpeg` or `ffprobe` binary: checks custom Settings directory, alongside executable, then `PATH`.
pub fn resolve_tool_binary(tool: &str) -> PathBuf {
    let exe_name = if cfg!(windows) {
        format!("{tool}.exe")
    } else {
        tool.to_string()
    };
    if let Some(cell) = FFMPEG_CUSTOM_DIR.get() {
        if let Ok(guard) = cell.lock() {
            if !guard.is_empty() {
                let custom = Path::new(guard.as_str());
                let candidate = if custom.is_file() {
                    custom.with_file_name(&exe_name)
                } else {
                    custom.join(&exe_name)
                };
                if candidate.is_file() {
                    return candidate;
                }
            }
        }
    }
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(dir) = current_exe.parent() {
            let candidate = dir.join(&exe_name);
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    PathBuf::from(tool)
}

/// Returns short `ffmpeg` version headline if available.
pub fn detect_ffmpeg_version() -> Option<String> {
    let mut cmd = Command::new(resolve_tool_binary("ffmpeg"));
    cmd.arg("-version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    configure_no_window(&mut cmd);
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let first = text.lines().next()?.trim();
    let clean = first.split(" Copyright").next().unwrap_or(first).trim();
    Some(clean.chars().take(48).collect())
}

#[derive(Debug, Clone)]
pub struct VideoMetadata {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration_secs: Option<f64>,
}

/// Probe video dimensions, frame rate, and duration via `ffprobe` (with safe defaults).
pub fn probe_video(path: &Path) -> Result<VideoMetadata, String> {
    let mut cmd = Command::new(resolve_tool_binary("ffprobe"));
    cmd.args([
        "-v",
        "error",
        "-select_streams",
        "v:0",
        "-show_entries",
        "stream=width,height,r_frame_rate:format=duration",
        "-of",
        "default=noprint_wrappers=1",
    ])
    .arg(path)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    configure_no_window(&mut cmd);

    let out = cmd.output().map_err(|e| {
        format!(
            "Could not run ffprobe/ffmpeg ({e}). Install ffmpeg on PATH or place it next to lumaglyph."
        )
    })?;

    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("ffprobe failed: {}", err.trim()));
    }

    let text = String::from_utf8_lossy(&out.stdout);
    let mut width = 0u32;
    let mut height = 0u32;
    let mut fps = 30.0f64;
    let mut duration_secs = None;

    for line in text.lines() {
        if let Some((k, v)) = line.split_once('=') {
            match k.trim() {
                "width" => width = v.trim().parse().unwrap_or(width),
                "height" => height = v.trim().parse().unwrap_or(height),
                "r_frame_rate" => {
                    if let Some((num, den)) = v.trim().split_once('/') {
                        let n: f64 = num.parse().unwrap_or(30.0);
                        let d: f64 = den.parse().unwrap_or(1.0);
                        if d > 0.0 && n > 0.0 {
                            fps = (n / d).clamp(1.0, 120.0);
                        }
                    } else if let Ok(f) = v.trim().parse::<f64>() {
                        fps = f.clamp(1.0, 120.0);
                    }
                }
                "duration" => {
                    if let Ok(d) = v.trim().parse::<f64>() {
                        if d.is_finite() && d > 0.0 {
                            duration_secs = Some(d);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    if width == 0 || height == 0 {
        return Err("No valid video stream dimensions detected".to_string());
    }

    Ok(VideoMetadata {
        width,
        height,
        fps,
        duration_secs,
    })
}

/// Stream mono 8 kHz `s16le` PCM from `ffmpeg` in tiny 1-frame chunks to compute a crisp
/// per-frame beat/onset envelope (`0.0..=1.25`) with near-zero memory overhead and zero extra crates.
/// Gracefully returns an empty `Vec` if the input file has no audio track.
pub fn extract_audio_envelope(input_path: &Path, target_fps: f64) -> Vec<f32> {
    let fps = target_fps.clamp(1.0, 120.0);
    let sample_rate = 8000usize;
    let samples_per_frame = ((sample_rate as f64) / fps).round().max(32.0) as usize;
    let chunk_bytes = samples_per_frame * 2;

    let mut cmd = Command::new(resolve_tool_binary("ffmpeg"));
    cmd.args(["-v", "error", "-nostdin", "-i"])
        .arg(input_path)
        .args(["-vn", "-ac", "1", "-ar", "8000", "-f", "s16le", "pipe:1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    configure_no_window(&mut cmd);

    let Ok(mut child) = cmd.spawn() else {
        return Vec::new();
    };
    let Some(mut stdout) = child.stdout.take() else {
        let _ = child.kill();
        return Vec::new();
    };

    let mut raw_energy: Vec<f32> = Vec::with_capacity(1024);
    let mut pcm_buf = vec![0u8; chunk_bytes];
    let mut slow_ema = 0.0f32;

    loop {
        let mut got = 0usize;
        while got < chunk_bytes {
            match stdout.read(&mut pcm_buf[got..]) {
                Ok(0) => break,
                Ok(n) => got += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    got = 0;
                    break;
                }
            }
        }
        if got < 2 {
            break;
        }
        let sample_count = got / 2;
        let mut sum_sq = 0.0f64;
        for pair in pcm_buf[..sample_count * 2].chunks_exact(2) {
            let s = i16::from_le_bytes([pair[0], pair[1]]) as f64 / 32768.0;
            sum_sq += s * s;
        }
        let rms = (sum_sq / (sample_count as f64)).sqrt() as f32;
        let onset = (rms - slow_ema).max(0.0);
        slow_ema = slow_ema * 0.82 + rms * 0.18;
        // Blend sustained RMS body with snappy transient flux for punchy kick/snare reaction
        raw_energy.push(rms * 0.55 + onset * 2.15);
    }

    let _ = child.wait();
    if raw_energy.is_empty() {
        return raw_energy;
    }

    let mut sorted = raw_energy.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let p95 = sorted[(sorted.len() * 95 / 100).min(sorted.len() - 1)];
    if p95 < 1e-4 {
        raw_energy.fill(0.0);
        return raw_energy;
    }

    // Light attack/release smoothing + 95th-percentile normalization
    let mut smooth = 0.0f32;
    for val in &mut raw_energy {
        let norm = (*val / p95).clamp(0.0, 1.35);
        if norm > smooth {
            smooth = smooth * 0.25 + norm * 0.75; // fast attack
        } else {
            smooth = smooth * 0.62 + norm * 0.38; // musical release
        }
        *val = if smooth < 0.03 { 0.0 } else { smooth };
    }

    raw_energy
}

/// Live progress state shared lock-free between worker thread and GUI/CLI.
pub struct TranscodeProgress {
    pub frames_done: AtomicU64,
    pub total_frames_est: u64,
    pub cancel_requested: AtomicBool,
    pub finished: AtomicBool,
}

impl TranscodeProgress {
    pub fn new(total_frames_est: u64) -> Arc<Self> {
        Arc::new(Self {
            frames_done: AtomicU64::new(0),
            total_frames_est,
            cancel_requested: AtomicBool::new(false),
            finished: AtomicBool::new(false),
        })
    }
}

#[derive(Debug, Clone)]
pub struct TranscodeSummary {
    pub frames_rendered: u64,
    pub elapsed_secs: f64,
    pub avg_fps: f64,
    pub out_width: usize,
    pub out_height: usize,
}

/// Stream a video file frame-by-frame into an ASCII video (`.mp4`/`.mkv`/`.webm`/`.mov`),
/// looping `.gif`, or playable `.ans` terminal animation while maintaining flat RAM usage.
/// Supports Audio-Reactive beat modulation and Transparent Alpha video overlays (`.webm`/`.mov`/`.gif`).
pub fn transcode_video_streaming(
    input_path: &Path,
    output_path: &Path,
    atlas: &GlyphAtlas,
    params: &RenderParams,
    target_fps_override: Option<f64>,
    progress: Arc<TranscodeProgress>,
) -> Result<TranscodeSummary, String> {
    let meta = probe_video(input_path)?;
    let fps = target_fps_override.unwrap_or(meta.fps).clamp(1.0, 120.0);

    let audio_env = if params.audio_reactive > 0.001 {
        extract_audio_envelope(input_path, fps)
    } else {
        Vec::new()
    };

    let cols = params.cols.clamp(16, 400);
    let est_rows = ((cols as f32) * (meta.height as f32 / meta.width as f32) * atlas.aspect)
        .round()
        .max(1.0) as u32;
    let dec_w = ((cols * 4).min(meta.width).max(64) + 1) & !1;
    let dec_h = ((est_rows * 6).min(meta.height).max(64) + 1) & !1;

    let mut dec_cmd = Command::new(resolve_tool_binary("ffmpeg"));
    dec_cmd
        .args(["-v", "error", "-i"])
        .arg(input_path)
        .args([
            "-vf",
            &format!("fps={fps:.4},scale={dec_w}:{dec_h}:flags=fast_bilinear"),
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "pipe:1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_no_window(&mut dec_cmd);

    let mut dec_child = dec_cmd
        .spawn()
        .map_err(|e| format!("Failed to start ffmpeg decoder: {e}"))?;
    let mut dec_stdout = dec_child
        .stdout
        .take()
        .ok_or_else(|| "Missing decoder stdout pipe".to_string())?;

    let ext = output_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("mp4")
        .to_ascii_lowercase();

    let t0 = Instant::now();
    let frame_in_bytes = (dec_w as usize) * (dec_h as usize) * 3;
    let mut in_rgb = vec![0u8; frame_in_bytes];
    let mut ascii_frame = AsciiFrame::new();
    let mut out_buf = Vec::new();
    let mut frames_rendered = 0u64;
    let mut out_dims = (0usize, 0usize);

    if ext == "ans" {
        let file = File::create(output_path)
            .map_err(|e| format!("Failed creating ANSI stream file: {e}"))?;
        let mut writer = BufWriter::new(file);
        writer
            .write_all(b"\x1b[2J\x1b[?25l")
            .map_err(|e| e.to_string())?;

        while read_exact_or_eof(&mut dec_stdout, &mut in_rgb)? {
            if progress.cancel_requested.load(Ordering::Relaxed) {
                break;
            }
            let audio_e = audio_env
                .get(frames_rendered as usize)
                .copied()
                .unwrap_or(0.0);
            convert_image_buffer_with_audio(
                &in_rgb,
                dec_w,
                dec_h,
                3,
                atlas,
                params,
                true,
                audio_e,
                &mut ascii_frame,
            );
            out_dims = (ascii_frame.cols as usize, ascii_frame.rows as usize);
            writer.write_all(b"\x1b[H").map_err(|e| e.to_string())?;
            let ans = frame_to_ansi_string(&ascii_frame, params);
            writer
                .write_all(ans.as_bytes())
                .map_err(|e| e.to_string())?;
            frames_rendered += 1;
            progress
                .frames_done
                .store(frames_rendered, Ordering::Relaxed);
        }
        let _ = writer.write_all(b"\x1b[0m\x1b[?25h\n");
        let _ = writer.flush();
    } else if ext == "gif" {
        let mut gif_enc: Option<gif::Encoder<BufWriter<File>>> = None;
        let delay_cs = ((100.0 / fps).round() as u16).max(2);

        while read_exact_or_eof(&mut dec_stdout, &mut in_rgb)? {
            if progress.cancel_requested.load(Ordering::Relaxed) {
                break;
            }
            let audio_e = audio_env
                .get(frames_rendered as usize)
                .copied()
                .unwrap_or(0.0);
            let bloom_boost = audio_e * params.audio_reactive * 0.65;
            convert_image_buffer_with_audio(
                &in_rgb,
                dec_w,
                dec_h,
                3,
                atlas,
                params,
                true,
                audio_e,
                &mut ascii_frame,
            );

            let (ow, oh) = if params.transparent_bg {
                rasterize_rgba(&ascii_frame, atlas, params, bloom_boost, &mut out_buf)
            } else {
                rasterize_rgb24_even(&ascii_frame, atlas, params, bloom_boost, &mut out_buf)
            };
            out_dims = (ow, oh);

            if gif_enc.is_none() {
                let f = File::create(output_path)
                    .map_err(|e| format!("Failed creating GIF file: {e}"))?;
                let mut enc = gif::Encoder::new(BufWriter::new(f), ow as u16, oh as u16, &[])
                    .map_err(|e| format!("GIF init error: {e}"))?;
                enc.set_repeat(gif::Repeat::Infinite)
                    .map_err(|e| format!("GIF repeat error: {e}"))?;
                gif_enc = Some(enc);
            }
            if let Some(enc) = gif_enc.as_mut() {
                let mut gframe = if params.transparent_bg {
                    let mut rgba_copy = out_buf.clone();
                    let mut gf =
                        gif::Frame::from_rgba_speed(ow as u16, oh as u16, &mut rgba_copy, 15);
                    gf.dispose = gif::DisposalMethod::Background;
                    gf
                } else {
                    gif::Frame::from_rgb_speed(ow as u16, oh as u16, &out_buf, 15)
                };
                gframe.delay = delay_cs;
                enc.write_frame(&gframe)
                    .map_err(|e| format!("GIF write frame error: {e}"))?;
            }
            frames_rendered += 1;
            progress
                .frames_done
                .store(frames_rendered, Ordering::Relaxed);
        }
    } else {
        // MP4 / MKV / MOV / WebM streaming encode via ffmpeg stdin while preserving audio!
        // Supports True Alpha Video (`.webm` VP9 yuva420p or `.mov` RGBA PNG) when `transparent_bg` is enabled.
        let alpha_video = params.transparent_bg && (ext == "webm" || ext == "mov");
        let mut enc_child: Option<std::process::Child> = None;
        let mut enc_stdin: Option<std::process::ChildStdin> = None;

        while read_exact_or_eof(&mut dec_stdout, &mut in_rgb)? {
            if progress.cancel_requested.load(Ordering::Relaxed) {
                break;
            }
            let audio_e = audio_env
                .get(frames_rendered as usize)
                .copied()
                .unwrap_or(0.0);
            let bloom_boost = audio_e * params.audio_reactive * 0.65;
            convert_image_buffer_with_audio(
                &in_rgb,
                dec_w,
                dec_h,
                3,
                atlas,
                params,
                true,
                audio_e,
                &mut ascii_frame,
            );

            let (ow, oh) = if alpha_video {
                rasterize_rgba(&ascii_frame, atlas, params, bloom_boost, &mut out_buf)
            } else {
                rasterize_rgb24_even(&ascii_frame, atlas, params, bloom_boost, &mut out_buf)
            };
            out_dims = (ow, oh);

            if enc_child.is_none() {
                let mut ecmd = Command::new(resolve_tool_binary("ffmpeg"));
                ecmd.args([
                    "-y",
                    "-v",
                    "error",
                    "-f",
                    "rawvideo",
                    "-pix_fmt",
                    if alpha_video { "rgba" } else { "rgb24" },
                    "-s",
                    &format!("{ow}x{oh}"),
                    "-r",
                    &format!("{fps:.4}"),
                    "-i",
                    "pipe:0",
                    "-i",
                ])
                .arg(input_path)
                .args(["-map", "0:v:0", "-map", "1:a?"]);

                if alpha_video && ext == "webm" {
                    ecmd.args([
                        "-c:v",
                        "libvpx-vp9",
                        "-pix_fmt",
                        "yuva420p",
                        "-b:v",
                        "0",
                        "-crf",
                        "24",
                        "-deadline",
                        "realtime",
                        "-cpu-used",
                        "4",
                        "-auto-alt-ref",
                        "0",
                        "-c:a",
                        "libopus",
                        "-b:a",
                        "160k",
                        "-shortest",
                    ]);
                } else if alpha_video && ext == "mov" {
                    ecmd.args([
                        "-c:v",
                        "png",
                        "-pix_fmt",
                        "rgba",
                        "-c:a",
                        "aac",
                        "-b:a",
                        "192k",
                        "-shortest",
                    ]);
                } else if ext == "webm" {
                    ecmd.args([
                        "-c:v",
                        "libvpx-vp9",
                        "-pix_fmt",
                        "yuv420p",
                        "-b:v",
                        "0",
                        "-crf",
                        "24",
                        "-deadline",
                        "realtime",
                        "-cpu-used",
                        "4",
                        "-c:a",
                        "libopus",
                        "-b:a",
                        "160k",
                        "-shortest",
                    ]);
                } else {
                    let crf_s = VIDEO_CRF.load(Ordering::Relaxed).to_string();
                    ecmd.args([
                        "-c:v",
                        "libx264",
                        "-preset",
                        "fast",
                        "-crf",
                        &crf_s,
                        "-pix_fmt",
                        "yuv420p",
                        "-c:a",
                        "aac",
                        "-b:a",
                        "192k",
                        "-movflags",
                        "+faststart",
                        "-shortest",
                    ]);
                }

                ecmd.arg(output_path)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::piped());
                configure_no_window(&mut ecmd);

                let mut child = ecmd
                    .spawn()
                    .map_err(|e| format!("Failed starting ffmpeg encoder: {e}"))?;
                enc_stdin = child.stdin.take();
                enc_child = Some(child);
            }

            if let Some(stdin) = enc_stdin.as_mut() {
                stdin
                    .write_all(&out_buf)
                    .map_err(|e| format!("Encoder pipe write error: {e}"))?;
            }

            frames_rendered += 1;
            progress
                .frames_done
                .store(frames_rendered, Ordering::Relaxed);
        }

        drop(enc_stdin);
        if let Some(mut child) = enc_child {
            let status = child
                .wait()
                .map_err(|e| format!("Encoder wait failed: {e}"))?;
            if !status.success() && !progress.cancel_requested.load(Ordering::Relaxed) {
                let mut err_s = String::new();
                if let Some(mut se) = child.stderr.take() {
                    let _ = se.read_to_string(&mut err_s);
                }
                return Err(format!(
                    "ffmpeg encoder exited with error: {}",
                    err_s.trim()
                ));
            }
        }
    }

    let _ = dec_child.kill();
    let _ = dec_child.wait();
    progress.finished.store(true, Ordering::Relaxed);

    let elapsed_secs = t0.elapsed().as_secs_f64().max(1e-4);
    Ok(TranscodeSummary {
        frames_rendered,
        elapsed_secs,
        avg_fps: (frames_rendered as f64) / elapsed_secs,
        out_width: out_dims.0,
        out_height: out_dims.1,
    })
}

fn read_exact_or_eof(r: &mut impl Read, buf: &mut [u8]) -> Result<bool, String> {
    let mut read_total = 0usize;
    while read_total < buf.len() {
        match r.read(&mut buf[read_total..]) {
            Ok(0) => {
                return Ok(false);
            }
            Ok(n) => read_total += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(format!("Pipe read error: {e}")),
        }
    }
    Ok(true)
}

/// Bounded 2-frame live preview video streamer for smooth GUI playback with flat RAM
/// and synchronized per-frame audio-reactive beat envelope.
///
/// Supports arbitrary timeline seeking (`seek_to`), continuous position reporting
/// (`position_secs`) for the on-canvas scrub bar, and a cooperative `idle_hold`
/// flag so the decoder thread parks itself when the workstation window is unfocused.
pub struct LiveVideoStream {
    pub path: PathBuf,
    pub meta: VideoMetadata,
    pub dec_w: u32,
    pub dec_h: u32,
    pub audio_envelope: Vec<f32>,
    pub current_audio_energy: f32,
    rx: Receiver<(usize, Vec<u8>)>,
    stop_flag: Arc<AtomicBool>,
    pub paused: Arc<AtomicBool>,
    /// Cooperative background-idle gate (set when the OS window loses focus).
    pub idle_hold: Arc<AtomicBool>,
    /// Pending seek target in seconds (`None` = no seek requested).
    seek_target: Arc<Mutex<Option<f64>>>,
    /// Current playback position in seconds, stored as raw `f64` bits.
    position_bits: Arc<AtomicU64>,
}

impl LiveVideoStream {
    /// Opens a streaming preview at the workstation's configured decode resolution cap.
    pub fn open(path: &Path) -> Result<Self, String> {
        Self::open_with_options(path, 640, true)
    }

    /// Opens a streaming preview with an explicit maximum decode dimension and optional
    /// audio-envelope analysis (disabled saves a full pre-pass over the file).
    pub fn open_with_options(
        path: &Path,
        max_dim: u32,
        analyze_audio: bool,
    ) -> Result<Self, String> {
        let meta = probe_video(path)?;
        let cap = f64::from(max_dim.clamp(160, 4096));
        let scale = (cap / (meta.width.max(meta.height) as f64)).min(1.0);
        let dec_w = (((meta.width as f64 * scale).round() as u32).max(64) + 1) & !1;
        let dec_h = (((meta.height as f64 * scale).round() as u32).max(64) + 1) & !1;
        let fps = meta.fps.clamp(5.0, 60.0);
        let audio_envelope = if analyze_audio {
            extract_audio_envelope(path, fps)
        } else {
            Vec::new()
        };
        let duration = meta.duration_secs.unwrap_or(0.0).max(0.0);

        let (tx, rx) = mpsc::sync_channel::<(usize, Vec<u8>)>(2);
        let stop_flag = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(false));
        let idle_hold = Arc::new(AtomicBool::new(false));
        let seek_target: Arc<Mutex<Option<f64>>> = Arc::new(Mutex::new(None));
        let position_bits = Arc::new(AtomicU64::new(0.0f64.to_bits()));

        let thread_stop = Arc::clone(&stop_flag);
        let thread_paused = Arc::clone(&paused);
        let thread_idle = Arc::clone(&idle_hold);
        let thread_seek = Arc::clone(&seek_target);
        let thread_pos = Arc::clone(&position_bits);
        let pbuf = path.to_path_buf();

        std::thread::Builder::new()
            .name("lumaglyph-video-preview".to_string())
            .spawn(move || {
                let frame_dur = Duration::from_secs_f64(1.0 / fps);
                let frame_bytes = (dec_w as usize) * (dec_h as usize) * 3;
                while !thread_stop.load(Ordering::Relaxed) {
                    // Pick up any pending timeline seek before (re)spawning the decoder.
                    let seek_from = thread_seek.lock().ok().and_then(|mut g| g.take());
                    let start_at = seek_from.map_or(0.0, |t| t.clamp(0.0, duration.max(0.0)));
                    // Align the restart to a frame boundary so audio energy stays in sync.
                    let start_frame = (start_at * fps).round().max(0.0) as usize;
                    let start_secs = start_frame as f64 / fps;
                    // Monotonic index across seeks so the audio envelope lookup stays aligned.
                    let mut global_idx: usize = start_frame;

                    let mut cmd = Command::new(resolve_tool_binary("ffmpeg"));
                    cmd.args(["-v", "error"]);
                    if start_secs > 0.001 {
                        cmd.args(["-ss", &format!("{start_secs:.3}"), "-noaccurate_seek"]);
                    }
                    cmd.arg("-i")
                        .arg(&pbuf)
                        .args([
                            "-vf",
                            &format!("fps={fps:.3},scale={dec_w}:{dec_h}:flags=fast_bilinear"),
                            "-f",
                            "rawvideo",
                            "-pix_fmt",
                            "rgb24",
                            "pipe:1",
                        ])
                        .stdin(Stdio::null())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::null());
                    configure_no_window(&mut cmd);

                    let Ok(mut child) = cmd.spawn() else {
                        break;
                    };
                    let Some(mut stdout) = child.stdout.take() else {
                        let _ = child.kill();
                        break;
                    };

                    let mut next_tick = Instant::now();
                    let mut restart = false;
                    loop {
                        if thread_stop.load(Ordering::Relaxed) {
                            break;
                        }
                        // A new seek request always abandons the current decoder pipe.
                        if thread_seek.lock().map(|g| g.is_some()).unwrap_or(false) {
                            restart = true;
                            break;
                        }
                        while (thread_paused.load(Ordering::Relaxed)
                            || thread_idle.load(Ordering::Relaxed))
                            && !thread_stop.load(Ordering::Relaxed)
                        {
                            std::thread::sleep(Duration::from_millis(20));
                            next_tick = Instant::now();
                        }

                        let mut buf = vec![0u8; frame_bytes];
                        match read_exact_or_eof(&mut stdout, &mut buf) {
                            Ok(true) => {
                                let pos = global_idx as f64 / fps;
                                thread_pos.store(pos.to_bits(), Ordering::Relaxed);
                                if tx.send((global_idx, buf)).is_err() {
                                    let _ = child.kill();
                                    let _ = child.wait();
                                    return;
                                }
                                global_idx += 1;
                                next_tick += frame_dur;
                                let now = Instant::now();
                                if next_tick > now {
                                    std::thread::sleep(next_tick - now);
                                } else {
                                    next_tick = now;
                                }
                            }
                            _ => break, // End of stream: loop back to the beginning seamlessly
                        }
                    }
                    let _ = child.kill();
                    let _ = child.wait();
                    if !restart {
                        // Natural end of file: rewind to the start on the next pass.
                        if let Ok(mut g) = thread_seek.lock() {
                            *g = Some(0.0);
                        }
                    }
                }
            })
            .map_err(|e| format!("Failed spawning video preview thread: {e}"))?;

        Ok(Self {
            path: path.to_path_buf(),
            meta,
            dec_w,
            dec_h,
            audio_envelope,
            current_audio_energy: 0.0,
            rx,
            stop_flag,
            paused,
            idle_hold,
            seek_target,
            position_bits,
        })
    }

    /// Total timeline duration in seconds (0.0 when unknown).
    pub fn duration_secs(&self) -> f64 {
        self.meta.duration_secs.unwrap_or(0.0).max(0.0)
    }

    /// Current playback position in seconds.
    pub fn position_secs(&self) -> f64 {
        f64::from_bits(self.position_bits.load(Ordering::Relaxed))
    }

    /// Requests a timeline seek to `secs`, clamped to the media duration.
    pub fn seek_to(&self, secs: f64) {
        let clamped = secs.clamp(0.0, self.duration_secs());
        if let Ok(mut g) = self.seek_target.lock() {
            *g = Some(clamped);
        }
        self.position_bits
            .store(clamped.to_bits(), Ordering::Relaxed);
    }

    /// Relative seek helper used by the on-canvas jump buttons and arrow keys.
    pub fn seek_by(&self, delta_secs: f64) {
        self.seek_to(self.position_secs() + delta_secs);
    }

    /// Flips play/pause and returns the new paused state.
    pub fn toggle_pause(&self) -> bool {
        let next = !self.paused.load(Ordering::Relaxed);
        self.paused.store(next, Ordering::Relaxed);
        next
    }

    /// Parks the decoder thread while the OS window is unfocused/minimized.
    pub fn set_idle_hold(&self, idle: bool) {
        self.idle_hold.store(idle, Ordering::Relaxed);
    }

    /// Non-blocking drain to latest decoded frame and its synchronized audio beat energy.
    pub fn poll_latest_frame(&mut self) -> Option<(Vec<u8>, f32)> {
        let mut latest: Option<(usize, Vec<u8>)> = None;
        while let Ok(item) = self.rx.try_recv() {
            latest = Some(item);
        }
        if let Some((idx, buf)) = latest {
            let e = self.audio_envelope.get(idx).copied().unwrap_or(0.0);
            self.current_audio_energy = e;
            Some((buf, e))
        } else {
            None
        }
    }
}

/// Formats a timeline position as a professional `MM:SS.d` (or `H:MM:SS.d`) timecode.
pub fn format_timecode(secs: f64) -> String {
    let clamped = if secs.is_finite() { secs.max(0.0) } else { 0.0 };
    let total_tenths = (clamped * 10.0).round() as u64;
    let tenths = total_tenths % 10;
    let total_secs = total_tenths / 10;
    let s = total_secs % 60;
    let m = (total_secs / 60) % 60;
    let h = total_secs / 3600;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}.{tenths}")
    } else {
        format!("{m:02}:{s:02}.{tenths}")
    }
}

/// Decodes exactly one full-resolution RGB24 frame at `secs` for on-demand frame export.
/// Uses input seeking plus `-frames:v 1`, so cost is bounded regardless of file length.
///
/// Robust against end-of-stream edges: if the requested timestamp lands past the final frame
/// (or on a seek point the decoder cannot satisfy), progressively earlier candidates are tried,
/// ending at the very first frame, so an export never fails just because of rounding.
pub fn extract_source_frame(path: &Path, secs: f64) -> Result<(Vec<u8>, u32, u32), String> {
    let meta = probe_video(path)?;
    if meta.width == 0 || meta.height == 0 {
        return Err("Could not determine video frame dimensions".to_string());
    }
    let duration = meta.duration_secs.unwrap_or(0.0).max(0.0);
    let frame_dur = if meta.fps > 0.5 { 1.0 / meta.fps } else { 0.04 };
    let requested = if secs.is_finite() { secs.max(0.0) } else { 0.0 };

    let mut candidates: Vec<f64> = Vec::with_capacity(5);
    let upper = if duration > 0.0 {
        (duration - frame_dur * 1.5).max(0.0)
    } else {
        requested
    };
    candidates.push(requested.min(upper));
    candidates.push((requested - frame_dur * 2.0).max(0.0).min(upper));
    candidates.push(upper);
    candidates.push(0.0);
    candidates.dedup_by(|a, b| (*a - *b).abs() < 1e-4);

    let expected = (meta.width as usize) * (meta.height as usize) * 3;
    let mut last_err = String::from("no candidate timestamp produced a frame");

    for seek in candidates {
        let mut cmd = Command::new(resolve_tool_binary("ffmpeg"));
        cmd.args(["-v", "error"]);
        if seek > 0.001 {
            cmd.args(["-ss", &format!("{seek:.3}")]);
        }
        cmd.arg("-i")
            .arg(path)
            .args([
                "-frames:v",
                "1",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgb24",
                "pipe:1",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        configure_no_window(&mut cmd);

        let out = match cmd.output() {
            Ok(o) => o,
            Err(e) => {
                last_err = format!("Failed launching ffmpeg for frame export: {e}");
                continue;
            }
        };
        if !out.status.success() {
            last_err = format!(
                "ffmpeg could not decode the frame at {seek:.2}s (exit code {:?})",
                out.status.code()
            );
            continue;
        }
        if out.stdout.len() < expected {
            last_err = format!(
                "Decoded frame at {seek:.2}s is truncated ({} of {expected} bytes)",
                out.stdout.len()
            );
            continue;
        }
        let mut rgb = out.stdout;
        rgb.truncate(expected);
        return Ok((rgb, meta.width, meta.height));
    }

    Err(last_err)
}

impl Drop for LiveVideoStream {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }
}
