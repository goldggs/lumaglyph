use crate::color::ColorMode;
use crate::engine::{convert_image_buffer, rasterize_rgb24_even, AsciiFrame, RenderParams};
use crate::export::frame_to_ansi_string;
use crate::glyph_atlas::GlyphAtlas;
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::Arc;
use std::time::{Duration, Instant};

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

/// Resolve `ffmpeg` or `ffprobe` binary: checks alongside the executable first, then `PATH`.
pub fn resolve_tool_binary(tool: &str) -> PathBuf {
    let exe_name = if cfg!(windows) {
        format!("{tool}.exe")
    } else {
        tool.to_string()
    };
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
    Some(first.chars().take(48).collect())
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

/// Stream a video file frame-by-frame into an ASCII video (`.mp4`/`.mkv`/`.webm`),
/// looping `.gif`, or playable `.ans` terminal animation while maintaining flat RAM usage.
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

    // Pre-scale decoded frames to ~4x6 pixels per character cell so even 4K inputs
    // use <1.5 MB per frame buffer without losing sub-cell shape accuracy.
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
    let mut out_rgb = Vec::new();
    let mut frames_rendered = 0u64;
    let mut out_dims = (0usize, 0usize);

    if ext == "ans" {
        let file = File::create(output_path)
            .map_err(|e| format!("Failed creating ANSI stream file: {e}"))?;
        let mut writer = BufWriter::new(file);
        let include_bg = params.color_mode == ColorMode::DualColor;
        writer
            .write_all(b"\x1b[2J\x1b[?25l")
            .map_err(|e| e.to_string())?;

        while read_exact_or_eof(&mut dec_stdout, &mut in_rgb)? {
            if progress.cancel_requested.load(Ordering::Relaxed) {
                break;
            }
            convert_image_buffer(
                &in_rgb,
                dec_w,
                dec_h,
                3,
                atlas,
                params,
                true,
                &mut ascii_frame,
            );
            out_dims = (ascii_frame.cols as usize, ascii_frame.rows as usize);
            writer.write_all(b"\x1b[H").map_err(|e| e.to_string())?;
            let ans = frame_to_ansi_string(&ascii_frame, include_bg);
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
            convert_image_buffer(
                &in_rgb,
                dec_w,
                dec_h,
                3,
                atlas,
                params,
                true,
                &mut ascii_frame,
            );
            let (ow, oh) = rasterize_rgb24_even(
                &ascii_frame,
                atlas,
                params.crt_bloom,
                params.scanlines,
                &mut out_rgb,
            );
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
                let mut gframe = gif::Frame::from_rgb_speed(ow as u16, oh as u16, &out_rgb, 15);
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
        let mut enc_child: Option<std::process::Child> = None;
        let mut enc_stdin: Option<std::process::ChildStdin> = None;

        while read_exact_or_eof(&mut dec_stdout, &mut in_rgb)? {
            if progress.cancel_requested.load(Ordering::Relaxed) {
                break;
            }
            convert_image_buffer(
                &in_rgb,
                dec_w,
                dec_h,
                3,
                atlas,
                params,
                true,
                &mut ascii_frame,
            );
            let (ow, oh) = rasterize_rgb24_even(
                &ascii_frame,
                atlas,
                params.crt_bloom,
                params.scanlines,
                &mut out_rgb,
            );
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
                    "rgb24",
                    "-s",
                    &format!("{ow}x{oh}"),
                    "-r",
                    &format!("{fps:.4}"),
                    "-i",
                    "pipe:0",
                    "-i",
                ])
                .arg(input_path)
                .args([
                    "-map",
                    "0:v:0",
                    "-map",
                    "1:a?",
                    "-c:v",
                    "libx264",
                    "-preset",
                    "fast",
                    "-crf",
                    "18",
                    "-pix_fmt",
                    "yuv420p",
                    "-c:a",
                    "aac",
                    "-b:a",
                    "192k",
                    "-movflags",
                    "+faststart",
                    "-shortest",
                ])
                .arg(output_path)
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
                    .write_all(&out_rgb)
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

/// Bounded 2-frame live preview video streamer for smooth GUI playback with flat RAM.
pub struct LiveVideoStream {
    pub path: PathBuf,
    pub meta: VideoMetadata,
    pub dec_w: u32,
    pub dec_h: u32,
    rx: Receiver<Vec<u8>>,
    stop_flag: Arc<AtomicBool>,
    pub paused: Arc<AtomicBool>,
}

impl LiveVideoStream {
    pub fn open(path: &Path) -> Result<Self, String> {
        let meta = probe_video(path)?;
        let scale = (640.0 / (meta.width.max(meta.height) as f64)).min(1.0);
        let dec_w = (((meta.width as f64 * scale).round() as u32).max(64) + 1) & !1;
        let dec_h = (((meta.height as f64 * scale).round() as u32).max(64) + 1) & !1;

        let (tx, rx): (SyncSender<Vec<u8>>, Receiver<Vec<u8>>) = mpsc::sync_channel(2);
        let stop_flag = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(false));

        let thread_stop = Arc::clone(&stop_flag);
        let thread_paused = Arc::clone(&paused);
        let pbuf = path.to_path_buf();
        let fps = meta.fps.clamp(5.0, 60.0);

        std::thread::Builder::new()
            .name("lumaglyph-video-preview".to_string())
            .spawn(move || {
                let frame_dur = Duration::from_secs_f64(1.0 / fps);
                let frame_bytes = (dec_w as usize) * (dec_h as usize) * 3;

                while !thread_stop.load(Ordering::Relaxed) {
                    let mut cmd = Command::new(resolve_tool_binary("ffmpeg"));
                    cmd.args(["-v", "error", "-i"])
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
                    loop {
                        if thread_stop.load(Ordering::Relaxed) {
                            break;
                        }
                        while thread_paused.load(Ordering::Relaxed)
                            && !thread_stop.load(Ordering::Relaxed)
                        {
                            std::thread::sleep(Duration::from_millis(25));
                            next_tick = Instant::now();
                        }

                        let mut buf = vec![0u8; frame_bytes];
                        match read_exact_or_eof(&mut stdout, &mut buf) {
                            Ok(true) => {
                                if tx.send(buf).is_err() {
                                    let _ = child.kill();
                                    let _ = child.wait();
                                    return;
                                }
                                next_tick += frame_dur;
                                let now = Instant::now();
                                if next_tick > now {
                                    std::thread::sleep(next_tick - now);
                                } else {
                                    next_tick = now;
                                }
                            }
                            _ => break, // Loop back to start seamlessly
                        }
                    }
                    let _ = child.kill();
                    let _ = child.wait();
                }
            })
            .map_err(|e| format!("Failed spawning video preview thread: {e}"))?;

        Ok(Self {
            path: path.to_path_buf(),
            meta,
            dec_w,
            dec_h,
            rx,
            stop_flag,
            paused,
        })
    }

    /// Non-blocking drain to latest decoded frame so the GUI never lags behind.
    pub fn poll_latest_frame(&self) -> Option<Vec<u8>> {
        let mut latest = None;
        while let Ok(f) = self.rx.try_recv() {
            latest = Some(f);
        }
        latest
    }
}

impl Drop for LiveVideoStream {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }
}
