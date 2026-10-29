//! Command-line contract tests.
//!
//! These run the *built binary* rather than calling library functions, because the
//! CLI is what most users actually touch: scripts, batch jobs, and the README
//! examples all go through it.
//!
//! Each test states a contract a user would notice if it broke: exit codes, which
//! files appear, whether the output is valid, and whether bad input produces a
//! readable message instead of a panic or a stack trace.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn binary() -> PathBuf {
    // Cargo puts integration test binaries next to the crate binary.
    if let Ok(exe) = std::env::var("CARGO_BIN_EXE_lumaglyph") {
        return PathBuf::from(exe);
    }
    let mut path = std::env::current_exe().expect("test exe");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.join(if cfg!(windows) {
        "lumaglyph.exe"
    } else {
        "lumaglyph"
    })
}

/// Runs the binary and fails the test if it does not exit within `CLI_TIMEOUT`.
///
/// `Command::output()` waits forever, which is how a CLI test that accidentally
/// launched the workstation window hung a CI job for an hour. Every invocation now has
/// a hard deadline.
///
/// Both pipes are drained on their own threads rather than after the process exits: an
/// ANSI dump to stdout is several hundred kilobytes and would fill the pipe buffer,
/// blocking the child forever while the test waits for it to exit. That deadlock is
/// another way to hang CI, and it is not hypothetical - it happened while writing this
/// helper.
fn run(args: &[&str]) -> Output {
    use std::io::Read;

    const CLI_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

    let mut child = Command::new(binary())
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to run {}: {e}", binary().display()));

    let mut stdout_pipe = child.stdout.take().expect("piped stdout");
    let mut stderr_pipe = child.stderr.take().expect("piped stderr");
    let stdout_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        stdout_pipe.read_to_end(&mut buf).ok();
        buf
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        stderr_pipe.read_to_end(&mut buf).ok();
        buf
    });

    let start = std::time::Instant::now();
    let status = loop {
        match child.try_wait().expect("poll the child process") {
            Some(status) => break status,
            None if start.elapsed() > CLI_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                panic!(
                    "`lumaglyph {}` did not exit within {}s. It almost certainly opened \
                     the GUI window, which must never happen for a CLI invocation.",
                    args.join(" "),
                    CLI_TIMEOUT.as_secs()
                );
            }
            None => std::thread::sleep(std::time::Duration::from_millis(25)),
        }
    };

    Output {
        status,
        stdout: stdout_reader.join().expect("stdout reader"),
        stderr: stderr_reader.join().expect("stderr reader"),
    }
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lumaglyph_cli_{tag}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A deterministic test image written with the `image` crate.
fn write_test_image(dir: &Path, name: &str, w: u32, h: u32) -> PathBuf {
    let mut img = image::RgbImage::new(w, h);
    for (x, y, px) in img.enumerate_pixels_mut() {
        *px = image::Rgb([
            ((x * 255) / w.max(1)) as u8,
            ((y * 255) / h.max(1)) as u8,
            (((x + y) * 255) / (w + h).max(1)) as u8,
        ]);
    }
    let path = dir.join(name);
    img.save(&path).expect("write test image");
    path
}

// ---------------------------------------------------------------------------
// Basic contract
// ---------------------------------------------------------------------------

#[test]
fn version_flag_reports_the_crate_version() {
    let out = run(&["--version"]);
    assert!(out.status.success(), "exit status {:?}", out.status);
    let text = stdout_of(&out);
    assert!(
        text.contains(env!("CARGO_PKG_VERSION")),
        "unexpected version output: {text}"
    );
    assert!(text.to_lowercase().contains("lumaglyph"));
}

#[test]
fn help_lists_every_documented_subcommand() {
    let out = run(&["--help"]);
    assert!(out.status.success());
    let text = stdout_of(&out);
    for sub in [
        "studio",
        "image",
        "video",
        "look",
        "build-info",
        "bench",
        "check-update",
        "self-update",
    ] {
        assert!(text.contains(sub), "help does not mention `{sub}`");
    }
}

#[test]
fn every_subcommand_has_its_own_help() {
    for sub in [
        "studio",
        "image",
        "video",
        "look",
        "build-info",
        "bench",
        "check-update",
        "self-update",
    ] {
        let out = run(&[sub, "--help"]);
        assert!(
            out.status.success(),
            "`{sub} --help` failed: {}",
            stderr_of(&out)
        );
        assert!(
            !stdout_of(&out).trim().is_empty(),
            "`{sub} --help` printed nothing"
        );
    }
}

#[test]
fn build_info_reports_platform_and_version() {
    let out = run(&["build-info"]);
    assert!(out.status.success());
    let text = stdout_of(&out);
    assert!(text.contains(env!("CARGO_PKG_VERSION")));
    assert!(text.contains("LumaGlyph"));
    // The report gets pasted into bug reports, so it must carry the target triple.
    assert!(text.contains("x86_64") || text.contains("aarch64"));
    // And it must not leak the machine that compiled it.
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            assert!(!text.contains(&home), "build-info leaked $HOME");
        }
    }
    assert!(!text.contains("/home/"), "build-info leaked a home path");
}

#[test]
fn unknown_subcommand_fails_with_a_message() {
    let out = run(&["definitely-not-a-command"]);
    assert!(
        !out.status.success(),
        "an unknown command should exit non-zero"
    );
    let err = stderr_of(&out);
    assert!(!err.trim().is_empty(), "no explanation for the failure");
    assert!(
        !err.contains("panicked"),
        "unknown command panicked instead of erroring"
    );
}

#[test]
fn a_missing_file_argument_never_opens_the_window() {
    // `lumaglyph some-typo` used to be parsed as a *file path*, not an unknown
    // subcommand, so the app opened the workstation on a file that did not exist. On a
    // headless runner it never returned. It must now fail immediately instead.
    let started = std::time::Instant::now();
    let out = run(&["definitely-not-a-command"]);
    let elapsed = started.elapsed();
    assert!(
        !out.status.success(),
        "a nonexistent file argument should exit non-zero"
    );
    let err = stderr_of(&out);
    assert!(
        err.to_lowercase().contains("no such file"),
        "unhelpful message for a nonexistent file: {err}"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(20),
        "took {elapsed:?} to reject a nonexistent file; it may have tried to open the window"
    );
}

#[test]
fn a_missing_input_file_reports_the_path_it_could_not_open() {
    let out = run(&["image", "/nonexistent/definitely_missing.png"]);
    assert!(!out.status.success());
    let err = stderr_of(&out).to_lowercase();
    assert!(
        err.contains("no such file") || err.contains("not found") || err.contains("failed"),
        "unhelpful error: {err}"
    );
    assert!(!err.contains("panicked"), "missing input panicked: {err}");
}

// ---------------------------------------------------------------------------
// Image conversion
// ---------------------------------------------------------------------------

#[test]
fn image_to_stdout_prints_ansi_text() {
    let dir = temp_dir("stdout");
    let input = write_test_image(&dir, "in.png", 64, 48);
    let out = run(&["image", input.to_str().unwrap(), "--cols", "40"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let text = stdout_of(&out);
    assert!(!text.trim().is_empty(), "no ASCII was printed");
    assert!(
        text.contains('\u{1b}'),
        "stdout mode should emit ANSI colour"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn image_exports_every_supported_output_format() {
    let dir = temp_dir("formats");
    let input = write_test_image(&dir, "in.png", 64, 48);
    for ext in ["png", "svg", "html", "txt", "ans"] {
        let out_path = dir.join(format!("out.{ext}"));
        let out = run(&[
            "image",
            input.to_str().unwrap(),
            "-o",
            out_path.to_str().unwrap(),
            "--cols",
            "40",
        ]);
        assert!(
            out.status.success(),
            "{ext} export failed: {}",
            stderr_of(&out)
        );
        assert!(out_path.exists(), "{ext} export produced no file");
        let size = std::fs::metadata(&out_path).unwrap().len();
        assert!(size > 0, "{ext} export produced an empty file");
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn png_export_is_a_real_decodable_image() {
    let dir = temp_dir("png_valid");
    let input = write_test_image(&dir, "in.png", 80, 60);
    let out_path = dir.join("out.png");
    let out = run(&[
        "image",
        input.to_str().unwrap(),
        "-o",
        out_path.to_str().unwrap(),
        "--cols",
        "50",
    ]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let img = image::open(&out_path).expect("exported PNG is not decodable");
    assert!(img.width() > 0 && img.height() > 0);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn txt_export_is_plain_ascii_with_no_escape_codes() {
    let dir = temp_dir("txt_plain");
    let input = write_test_image(&dir, "in.png", 64, 48);
    let out_path = dir.join("out.txt");
    let out = run(&[
        "image",
        input.to_str().unwrap(),
        "-o",
        out_path.to_str().unwrap(),
        "--cols",
        "30",
    ]);
    assert!(out.status.success());
    let text = std::fs::read_to_string(&out_path).unwrap();
    assert!(!text.contains('\u{1b}'), "txt export contains ANSI escapes");
    assert!(text.is_ascii(), "txt export contains non-ASCII characters");
    assert!(!text.trim().is_empty(), "txt export is empty");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn column_count_is_honoured_exactly() {
    let dir = temp_dir("cols");
    let input = write_test_image(&dir, "in.png", 120, 90);
    for cols in [20u32, 40, 80, 156, 200] {
        let out_path = dir.join(format!("out_{cols}.txt"));
        let out = run(&[
            "image",
            input.to_str().unwrap(),
            "-o",
            out_path.to_str().unwrap(),
            "--cols",
            &cols.to_string(),
        ]);
        assert!(out.status.success(), "cols={cols}: {}", stderr_of(&out));
        let text = std::fs::read_to_string(&out_path).unwrap();
        let width = text.lines().next().unwrap().chars().count();
        assert_eq!(
            width as u32, cols,
            "--cols {cols} produced {width} characters"
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn absurd_column_counts_are_clamped_rather_than_crashing() {
    let dir = temp_dir("cols_clamp");
    let input = write_test_image(&dir, "in.png", 32, 32);
    for cols in ["0", "1", "100000"] {
        let out = run(&[
            "image",
            input.to_str().unwrap(),
            "-o",
            dir.join("x.png").to_str().unwrap(),
            "--cols",
            cols,
        ]);
        assert!(
            !stderr_of(&out).contains("panicked"),
            "--cols {cols} panicked: {}",
            stderr_of(&out)
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn every_color_mode_and_charset_is_accepted_by_the_cli() {
    let dir = temp_dir("modes");
    let input = write_test_image(&dir, "in.png", 48, 48);
    for mode in [
        "truecolor",
        "dual",
        "tritone",
        "amber",
        "green",
        "cyberpunk",
        "ansi16",
        "xterm256",
        "mono",
    ] {
        let out = run(&[
            "image",
            input.to_str().unwrap(),
            "-o",
            dir.join("m.txt").to_str().unwrap(),
            "--color",
            mode,
            "--cols",
            "32",
        ]);
        assert!(
            out.status.success(),
            "color mode {mode} failed: {}",
            stderr_of(&out)
        );
    }
    for charset in ["genuine95", "structural", "dense", "retro"] {
        let out = run(&[
            "image",
            input.to_str().unwrap(),
            "-o",
            dir.join("c.txt").to_str().unwrap(),
            "--charset",
            charset,
            "--cols",
            "32",
        ]);
        assert!(
            out.status.success(),
            "charset {charset} failed: {}",
            stderr_of(&out)
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_invalid_color_mode_is_rejected_with_a_message() {
    let dir = temp_dir("bad_mode");
    let input = write_test_image(&dir, "in.png", 32, 32);
    let out = run(&[
        "image",
        input.to_str().unwrap(),
        "-o",
        dir.join("x.txt").to_str().unwrap(),
        "--color",
        "not-a-colour",
    ]);
    assert!(!out.status.success(), "an invalid colour mode was accepted");
    assert!(!stderr_of(&out).contains("panicked"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_corrupt_image_fails_cleanly() {
    let dir = temp_dir("corrupt");
    let bogus = dir.join("broken.png");
    std::fs::write(&bogus, b"this is not a PNG file at all").unwrap();
    let out = run(&[
        "image",
        bogus.to_str().unwrap(),
        "-o",
        dir.join("x.png").to_str().unwrap(),
    ]);
    assert!(
        !out.status.success(),
        "a corrupt image was treated as valid"
    );
    let err = stderr_of(&out).to_lowercase();
    assert!(!err.contains("panicked"), "corrupt image panicked: {err}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_empty_file_fails_cleanly() {
    let dir = temp_dir("empty_file");
    let bogus = dir.join("empty.png");
    std::fs::write(&bogus, b"").unwrap();
    let out = run(&["image", bogus.to_str().unwrap()]);
    assert!(!out.status.success());
    assert!(!stderr_of(&out).contains("panicked"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_look_code_from_the_command_line_is_applied() {
    let dir = temp_dir("look_cli");
    let input = write_test_image(&dir, "in.png", 64, 64);
    let out_path = dir.join("looked.txt");
    let out = run(&[
        "image",
        input.to_str().unwrap(),
        "-o",
        out_path.to_str().unwrap(),
        "--look",
        "lg2:cols=48,cs=retro,cm=mono",
    ]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let text = std::fs::read_to_string(&out_path).unwrap();
    assert_eq!(text.lines().next().unwrap().chars().count(), 48);
    // The retro set is 17 characters; nothing outside it should appear.
    let unique: std::collections::HashSet<char> = text.chars().filter(|c| *c != '\n').collect();
    assert!(
        unique.len() <= 17,
        "retro charset produced {} distinct glyphs",
        unique.len()
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_one_to_five_megapixel_range_converts_without_stalling() {
    let dir = temp_dir("big");
    let input = write_test_image(&dir, "big.png", 1200, 800);
    let out = run(&[
        "image",
        input.to_str().unwrap(),
        "-o",
        dir.join("big.png").to_str().unwrap(),
        "--cols",
        "160",
    ]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_single_pixel_image_still_produces_output() {
    let dir = temp_dir("tiny");
    // A bright pixel, not a black one: an all-black source legitimately renders
    // as spaces, which would make this test assert the wrong thing.
    let path = dir.join("one.png");
    image::RgbImage::from_pixel(1, 1, image::Rgb([255, 255, 255]))
        .save(&path)
        .unwrap();
    let input = path;
    let out = run(&[
        "image",
        input.to_str().unwrap(),
        "-o",
        dir.join("one.txt").to_str().unwrap(),
        "--cols",
        "16",
    ]);
    assert!(
        out.status.success(),
        "1x1 image failed: {}",
        stderr_of(&out)
    );
    let text = std::fs::read_to_string(dir.join("one.txt")).unwrap();
    assert!(
        text.chars().any(|c| c != ' ' && c != '\n'),
        "a white pixel rendered as nothing at all"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_very_wide_image_does_not_underflow_the_grid() {
    let dir = temp_dir("wide");
    let input = write_test_image(&dir, "wide.png", 2000, 4);
    let out = run(&[
        "image",
        input.to_str().unwrap(),
        "-o",
        dir.join("wide.txt").to_str().unwrap(),
        "--cols",
        "80",
    ]);
    assert!(
        out.status.success(),
        "wide image failed: {}",
        stderr_of(&out)
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// Look codes
// ---------------------------------------------------------------------------

#[test]
fn look_subcommand_prints_a_valid_code() {
    let out = run(&["look"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let text = stdout_of(&out).trim().to_string();
    assert!(
        text.starts_with("lg2:") || text.contains("lg2:"),
        "look did not print a code: {text}"
    );
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

#[test]
fn bench_runs_a_small_job_and_reports_throughput() {
    let out = run(&["bench", "--cols", "40", "--iters", "2"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let text = stdout_of(&out).to_lowercase();
    assert!(text.contains("fps") || text.contains("ms") || text.contains("frame"));
}

// ---------------------------------------------------------------------------
// Safety net
// ---------------------------------------------------------------------------

#[test]
fn no_invocation_anywhere_produces_a_panic_or_a_stack_trace() {
    // A Rust panic in release is a `panicked at` line plus SIGABRT. None of these
    // inputs should ever reach it.
    let dir = temp_dir("fuzz_cli");
    let good = write_test_image(&dir, "in.png", 32, 32);
    let good_s = good.to_str().unwrap();
    let cases: Vec<Vec<&str>> = vec![
        vec!["image"],
        vec!["image", ""],
        vec!["image", good_s, "--cols", "abc"],
        vec!["image", good_s, "--cols", "-1"],
        vec!["image", good_s, "--black-gate", "99"],
        vec!["image", good_s, "--tritone", "../../etc"],
        vec!["video"],
        vec!["video", "/nonexistent.mp4"],
        vec!["look"],
        vec!["build-info", "--extra-unknown-flag"],
        vec!["bench", "--cols", "0"],
    ];
    for args in cases {
        let out = run(&args);
        let combined = format!("{}{}", stdout_of(&out), stderr_of(&out));
        assert!(
            !combined.contains("panicked at"),
            "args {args:?} panicked:\n{combined}"
        );
        assert!(
            !combined.contains("stack backtrace"),
            "args {args:?} printed a backtrace"
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}
