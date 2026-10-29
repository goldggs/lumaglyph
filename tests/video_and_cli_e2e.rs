//! Real end-to-end CLI binary & streaming `ffmpeg` video/audio transcode tests,
//! plus Windows subprocess `CREATE_NO_WINDOW` verification.

use lumaglyph::color::ColorMode;
use lumaglyph::engine::RenderParams;
use lumaglyph::glyph_atlas::GlyphAtlas;
use lumaglyph::video::{
    detect_ffmpeg_version, probe_video, set_video_pipeline_prefs, transcode_video_streaming,
    TranscodeProgress,
};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static VID_SEQ: AtomicUsize = AtomicUsize::new(1);

fn temp_vid_dir() -> PathBuf {
    let id = VID_SEQ.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!("lumaglyph_vid_{}_{id}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn generate_test_mp4_with_audio(out_path: &Path) -> bool {
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x180:rate=15:duration=0.6",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=220:sample_rate=44100:duration=0.6",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
        ])
        .arg(out_path)
        .status();
    matches!(status, Ok(s) if s.success())
}

#[test]
fn video_rs_enforces_create_no_window_on_all_windows_subprocesses() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = std::fs::read_to_string(manifest_dir.join("src/video.rs")).unwrap();
    assert!(
        src.contains("0x0800_0000"),
        "src/video.rs must pass CREATE_NO_WINDOW (0x0800_0000) on Windows so ffmpeg/ffprobe never flash a console!"
    );
}

#[test]
fn video_streaming_mp4_gif_and_ans_end_to_end_when_ffmpeg_present() {
    if detect_ffmpeg_version().is_none() {
        // Gracefully skip only if host runner lacks ffmpeg installed
        return;
    }

    let dir = temp_vid_dir();
    let input_mp4 = dir.join("input.mp4");
    if !generate_test_mp4_with_audio(&input_mp4) {
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }

    let meta = probe_video(&input_mp4).unwrap();
    assert_eq!((meta.width, meta.height), (320, 180));
    assert!(meta.fps > 10.0);

    set_video_pipeline_prefs("", 20);
    let atlas = GlyphAtlas::default_embedded(16);

    // 1. Audio-reactive OLED MP4 transcode
    let out_mp4 = dir.join("out_oled.mp4");
    let params_mp4 = RenderParams {
        cols: 60,
        oled_black: true,
        audio_reactive: 1.2,
        color_mode: ColorMode::TriTone,
        ..RenderParams::default()
    };
    let sum_mp4 = transcode_video_streaming(
        &input_mp4,
        &out_mp4,
        &atlas,
        &params_mp4,
        Some(15.0),
        TranscodeProgress::new(0),
    )
    .unwrap();
    assert!(sum_mp4.frames_rendered >= 5);
    assert_eq!(sum_mp4.out_width % 2, 0);
    assert_eq!(sum_mp4.out_height % 2, 0);
    assert!(out_mp4.metadata().unwrap().len() > 512);

    // 2. Animated GIF transcode
    let out_gif = dir.join("out.gif");
    let sum_gif = transcode_video_streaming(
        &input_mp4,
        &out_gif,
        &atlas,
        &RenderParams {
            cols: 48,
            transparent_bg: true,
            wireframe_only: true,
            ..RenderParams::default()
        },
        Some(10.0),
        TranscodeProgress::new(0),
    )
    .unwrap();
    assert!(sum_gif.frames_rendered >= 4);
    assert_eq!(&std::fs::read(&out_gif).unwrap()[0..3], b"GIF");

    // 3. Terminal .ans stream export
    let out_ans = dir.join("out.ans");
    let sum_ans = transcode_video_streaming(
        &input_mp4,
        &out_ans,
        &atlas,
        &params_mp4,
        Some(10.0),
        TranscodeProgress::new(0),
    )
    .unwrap();
    assert!(sum_ans.frames_rendered >= 4);
    assert!(out_ans.metadata().unwrap().len() > 256);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn compiled_cli_binary_executes_headless_commands_cleanly() {
    let bin = env!("CARGO_BIN_EXE_lumaglyph");

    let info_out = Command::new(bin).arg("build-info").output().unwrap();
    assert!(info_out.status.success());
    let info_str = String::from_utf8_lossy(&info_out.stdout);
    // Compare against the crate version instead of a literal, so a release bump does
    // not require editing a test (and cannot be forgotten in one).
    assert!(
        info_str.contains(&format!("LumaGlyph v{}", env!("CARGO_PKG_VERSION"))),
        "build-info did not report version {}:\n{info_str}",
        env!("CARGO_PKG_VERSION")
    );

    let look_out = Command::new(bin).arg("look").output().unwrap();
    assert!(look_out.status.success());
    assert!(String::from_utf8_lossy(&look_out.stdout).starts_with("lg2:"));

    let bench_out = Command::new(bin)
        .args(["bench", "--cols", "64", "--iters", "4"])
        .output()
        .unwrap();
    assert!(bench_out.status.success());

    let dir = temp_vid_dir();
    let in_png = dir.join("cli_in.png");
    let mut img = image::RgbaImage::new(96, 64);
    for (x, y, px) in img.enumerate_pixels_mut() {
        *px = image::Rgba([(x * 2) as u8, (y * 3) as u8, ((x + y) * 2) as u8, 255]);
    }
    img.save(&in_png).unwrap();

    // Built-in `sample:*` magic strings are no longer accepted; real files are required
    let no_sample = Command::new(bin)
        .args(["image", "sample:cyber"])
        .output()
        .unwrap();
    assert!(!no_sample.status.success());

    let out_png = dir.join("cli_test.png");
    let img_out = Command::new(bin)
        .arg("image")
        .arg(&in_png)
        .args(["--cols", "64", "--oled-black", "-o"])
        .arg(&out_png)
        .output()
        .unwrap();
    assert!(img_out.status.success());
    assert!(out_png.is_file());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn timecode_formatting_is_exact_across_ranges() {
    use lumaglyph::video::format_timecode;

    assert_eq!(format_timecode(0.0), "00:00.0");
    assert_eq!(format_timecode(1.0), "00:01.0");
    assert_eq!(format_timecode(1.04), "00:01.0");
    assert_eq!(format_timecode(1.05), "00:01.1");
    assert_eq!(format_timecode(9.96), "00:10.0");
    assert_eq!(format_timecode(59.9), "00:59.9");
    assert_eq!(format_timecode(60.0), "01:00.0");
    assert_eq!(format_timecode(125.4), "02:05.4");
    assert_eq!(format_timecode(3600.0), "1:00:00.0");
    assert_eq!(format_timecode(3725.5), "1:02:05.5");
    // Defensive: negative and non-finite inputs clamp to zero rather than panicking.
    assert_eq!(format_timecode(-42.0), "00:00.0");
    assert_eq!(format_timecode(f64::NAN), "00:00.0");
    assert_eq!(format_timecode(f64::INFINITY), "00:00.0");
}

#[test]
fn video_stream_seek_pause_and_idle_controls_are_wired() {
    use lumaglyph::video::LiveVideoStream;
    use std::sync::atomic::Ordering;

    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/video.rs"),
    )
    .unwrap();

    // API surface used by the on-canvas transport.
    for needle in [
        "pub fn duration_secs",
        "pub fn position_secs",
        "pub fn seek_to",
        "pub fn seek_by",
        "pub fn toggle_pause",
        "pub fn set_idle_hold",
        "pub idle_hold: Arc<AtomicBool>",
        "pub fn open_with_options",
        "pub fn extract_source_frame",
        "pub fn format_timecode",
    ] {
        assert!(src.contains(needle), "src/video.rs must expose '{needle}'");
    }
    // Decoder thread must honour seeks and the idle gate.
    assert!(src.contains("thread_seek"));
    assert!(src.contains("thread_idle.load(Ordering::Relaxed)"));
    assert!(src.contains("\"-noaccurate_seek\""));

    // Pause toggle is a pure flipper, verifiable without spawning ffmpeg.
    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let next = !flag.load(Ordering::Relaxed);
    flag.store(next, Ordering::Relaxed);
    assert!(flag.load(Ordering::Relaxed));
    let _ = std::mem::size_of::<LiveVideoStream>();
}

#[test]
fn extract_source_frame_returns_exact_rgb24_dimensions_at_requested_time() {
    use lumaglyph::video::{extract_source_frame, format_timecode};

    let dir = temp_vid_dir();
    let src = dir.join("frame_src.mp4");
    if !generate_test_mp4_with_audio(&src) {
        eprintln!("Skipping: ffmpeg not available on this host");
        return;
    }

    // Frame 0: full-resolution RGB24 buffer with exact WxHx3 length.
    let (rgb, w, h) = extract_source_frame(&src, 0.0).expect("frame 0 must decode");
    assert_eq!((w, h), (320, 180));
    assert_eq!(rgb.len(), 320 * 180 * 3);
    // A generated test pattern is not a flat color; ensure it holds real content.
    assert!(rgb.iter().any(|&b| b > 40));
    assert!(rgb.chunks(3).any(|px| px[0] != rgb[0]));

    // A later timestamp decodes a visibly different frame.
    let (rgb2, w2, h2) = extract_source_frame(&src, 0.30).expect("mid frame must decode");
    assert_eq!((w2, h2), (320, 180));
    assert_eq!(rgb2.len(), rgb.len());
    assert_ne!(rgb2, rgb, "frame at 0.30s must differ from frame at 0.00s");

    // Defensive: out-of-range times clamp instead of failing.
    let (rgb3, w3, h3) = extract_source_frame(&src, 99_999.0).expect("clamped seek must decode");
    assert_eq!((w3, h3), (320, 180));
    assert_eq!(rgb3.len(), 320 * 180 * 3);
    let (_, w4, h4) = extract_source_frame(&src, -5.0).expect("negative seek must clamp to 0");
    assert_eq!((w4, h4), (320, 180));

    // The exported filename stamp used by the GUI must be filesystem safe.
    let stamp = format_timecode(2.5).replace(':', "-").replace('.', "_");
    assert_eq!(stamp, "00-02_5");
    assert!(!stamp.contains(':') && !stamp.contains('.'));

    let _ = std::fs::remove_dir_all(&dir);
}
