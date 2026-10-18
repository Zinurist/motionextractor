//! End-to-end tests of the `motion-extractor` command line.

use std::path::Path;
use std::process::{Command, Output};

use mx_media::DecodeOptions;
use mx_media::testing::{MovingSquare, ffmpeg};

fn bin(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_motion-extractor"))
        .args(args)
        .output()
        .expect("run binary")
}

fn ok(args: &[&str]) -> String {
    let out = bin(args);
    assert!(
        out.status.success(),
        "{args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn fail(args: &[&str]) -> String {
    let out = bin(args);
    assert!(!out.status.success(), "{args:?} unexpectedly succeeded");
    String::from_utf8(out.stderr).unwrap()
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn setup(spec: MovingSquare) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let input = spec.generate(&ffmpeg(), dir.path());
    (dir, input)
}

fn frame(path: &Path, n: u64) -> mx_core::Frame {
    let ff = ffmpeg();
    let info = ff.probe(path).unwrap();
    let opts = DecodeOptions {
        start_frame: n,
        frame_count: Some(1),
        ..Default::default()
    };
    ff.decode(&info, &opts)
        .unwrap()
        .read_frame()
        .unwrap()
        .unwrap()
}

#[test]
fn probe_prints_json() {
    let (_dir, input) = setup(MovingSquare::default());
    let json: serde_json::Value = serde_json::from_str(&ok(&["probe", s(&input)])).unwrap();
    assert_eq!(json["width"], 160);
    assert_eq!(json["frame_count"], 30);
    assert_eq!(json["fps"]["num"], 30);
}

#[test]
fn render_defaults_to_h264_mp4() {
    let (dir, input) = setup(MovingSquare::default());
    let out = dir.path().join("out.mp4");
    ok(&["render", "-q", s(&input), s(&out)]);
    let info = ffmpeg().probe(&out).unwrap();
    assert_eq!(info.codec, "h264");
    assert_eq!(info.frame_count, 30);
}

#[test]
fn dry_run_shows_command() {
    let (dir, input) = setup(MovingSquare::default());
    let out = dir.path().join("out.webm");
    let cmd = ok(&[
        "render",
        s(&input),
        s(&out),
        "--dry-run",
        "--crf",
        "40",
        "--ffmpeg-arg=-tune",
        "--ffmpeg-arg=grain",
    ]);
    assert!(cmd.contains("libvpx-vp9"), "{cmd}");
    assert!(cmd.contains("-crf 40"), "{cmd}");
    assert!(cmd.contains("-tune grain"), "{cmd}");
    assert!(!out.exists());
}

#[test]
fn effect_options_and_fixed_reference() {
    let (dir, input) = setup(MovingSquare::default());
    let out = dir.path().join("out.mkv");
    // 0.1 s = frame 3 at 30 fps.
    ok(&[
        "render",
        "-q",
        s(&input),
        s(&out),
        "--codec",
        "ffv1",
        "--reference",
        "0.1s",
        "--color",
        "#ff0000",
    ]);
    let spec = MovingSquare::default();
    let f = frame(&out, 20);
    let y = spec.y + 8;
    assert_eq!(f.pixel(spec.x(3) + 8, y), [255, 0, 0], "reference position");
    assert_eq!(f.pixel(spec.x(20) + 8, y), [255, 0, 0], "current position");
    assert_eq!(f.pixel(spec.x(10), y), [0, 0, 0]);
}

#[test]
fn overlay_and_bloom() {
    let (dir, input) = setup(MovingSquare::default());
    let out = dir.path().join("out.mkv");
    ok(&[
        "render",
        "-q",
        s(&input),
        s(&out),
        "--codec",
        "ffv1",
        "--offset",
        "-2",
        "--overlay",
        "--color",
        "#00ff00",
        "--bloom",
        "--bloom-radius",
        "0.05",
    ]);
    let spec = MovingSquare::default();
    let f = frame(&out, 10);
    let y = spec.y + 8;
    // Leading edge of the square in frame 12 is only in the reference: motion colour.
    assert_eq!(f.pixel(spec.x(12) + spec.size - 1, y)[1], 255);
    // The square itself in both frames shows the original (white).
    assert_eq!(f.pixel(spec.x(12) + 1, y), [255, 255, 255]);
    // Glow spreads into the black background below the square.
    assert!(f.pixel(spec.x(10), spec.y + spec.size + 2)[1] > 0, "bloom");
}

#[test]
fn presets_save_load_and_override() {
    let (dir, input) = setup(MovingSquare::default());
    let preset = dir.path().join("p.json");
    let out = dir.path().join("a.mkv");
    ok(&[
        "render",
        "-q",
        s(&input),
        s(&out),
        "--codec",
        "ffv1",
        "--gain",
        "8",
        "--offset",
        "0.2s",
        "--save-preset",
        s(&preset),
    ]);
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&preset).unwrap()).unwrap();
    assert_eq!(json["effect"]["gain"], 8.0);
    assert_eq!(json["comparison"]["offset"]["unit"], "seconds");
    assert_eq!(json["render"]["codec"], "ffv1");

    let out2 = dir.path().join("b.mkv");
    let cmd = ok(&[
        "render",
        s(&input),
        s(&out2),
        "--preset",
        s(&preset),
        "--crf",
        "5",
        "--codec",
        "h264",
        "--dry-run",
    ]);
    assert!(cmd.contains("libx264") && cmd.contains("-crf 5"), "{cmd}");
}

#[test]
fn range_and_height() {
    let (dir, input) = setup(MovingSquare::default());
    let out = dir.path().join("out.mp4");
    ok(&[
        "render",
        "-q",
        s(&input),
        s(&out),
        "--start",
        "5",
        "--end",
        "0.5s",
        "--height",
        "60",
    ]);
    let info = ffmpeg().probe(&out).unwrap();
    assert_eq!(info.frame_count, 10);
    assert_eq!((info.width, info.height), (80, 60));
}

#[test]
fn png_sequence() {
    let (dir, input) = setup(MovingSquare {
        frames: 5,
        ..Default::default()
    });
    let out = dir.path().join("frames");
    ok(&["render", "-q", s(&input), s(&out), "--codec", "png"]);
    assert_eq!(std::fs::read_dir(&out).unwrap().count(), 5);
    assert!(out.join("frame_000001.png").exists());
    assert!(fail(&["render", s(&input), s(&out), "--codec", "png"]).contains("already exists"));
}

#[test]
fn refuses_overwrite_without_flag() {
    let (dir, input) = setup(MovingSquare::default());
    let out = dir.path().join("out.mp4");
    std::fs::write(&out, b"keep me").unwrap();
    assert!(fail(&["render", s(&input), s(&out)]).contains("already exists"));
    assert_eq!(std::fs::read(&out).unwrap(), b"keep me");
    ok(&["render", "-q", "-y", s(&input), s(&out)]);
    assert_eq!(ffmpeg().probe(&out).unwrap().frame_count, 30);
}

#[test]
fn reports_errors() {
    let (dir, input) = setup(MovingSquare::default());
    let out = dir.path().join("out.mp4");
    assert!(fail(&["render", s(&input), s(&out), "--offset", "0"]).contains("offset"));
    assert!(fail(&["render", s(&input), s(&out), "--threshold", "2"]).contains("threshold"));
    assert!(fail(&["render", s(&input), s(&out), "--color", "red"]).contains("color"));
    assert!(fail(&["render", "missing.mp4", s(&out)]).contains("ffprobe"));
    assert!(fail(&["render", s(&input), s(&out), "--start", "40"]).contains("invalid frame range"));
    assert!(
        fail(&[
            "render",
            s(&input),
            s(&out),
            "--ffmpeg-arg=-c:v",
            "--ffmpeg-arg=bogus"
        ])
        .contains("bogus")
    );
    assert!(!out.exists());
}

/// Minimal HTTP GET (keeps the test free of an HTTP client dependency).
fn http_get(addr: &str, path: &str) -> String {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(addr).unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

#[test]
fn version_is_major_minor() {
    let out = ok(&["--version"]);
    let version = out.trim().strip_prefix("motion-extractor ").unwrap();
    let expected = env!("CARGO_PKG_VERSION").strip_suffix(".0").unwrap();
    assert_eq!(version, expected, "X.Y, crate version X.Y.0");
}

#[test]
fn serve_prints_url_and_answers() {
    use std::io::{BufRead, BufReader};
    let (dir, input) = setup(MovingSquare::default());
    let mut child = Command::new(env!("CARGO_BIN_EXE_motion-extractor"))
        .args([
            "serve",
            "--port",
            "0",
            "--root",
            s(dir.path()),
            "--settings",
            s(&dir.path().join("settings.json")),
            s(&input),
        ])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let url = line.trim().rsplit(' ').next().unwrap().to_owned();
    assert!(url.starts_with("http://127.0.0.1:"), "{line}");
    let addr = url.trim_start_matches("http://").trim_end_matches('/');
    let session = http_get(addr, "/api/session");
    assert!(session.starts_with("HTTP/1.1 200"), "{session}");
    assert!(
        session.contains("\"frame_count\":30"),
        "video opened at startup: {session}"
    );
    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
fn serve_on_all_interfaces_needs_no_token() {
    use std::io::{BufRead, BufReader};
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_motion-extractor"))
        .args([
            "serve",
            "--bind",
            "0.0.0.0",
            "--port",
            "0",
            "--root",
            s(dir.path()),
            "--settings",
            s(&dir.path().join("settings.json")),
        ])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let url = line.trim().rsplit(' ').next().unwrap().to_owned();
    assert!(url.starts_with("http://0.0.0.0:"), "{line}");
    assert!(!url.contains("token"), "{line}");
    let port = url.trim_end_matches('/').rsplit(':').next().unwrap();
    let info = http_get(&format!("127.0.0.1:{port}"), "/api/info");
    assert!(info.starts_with("HTTP/1.1 200"), "{info}");
    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
fn background_color_and_revealed_original() {
    let (dir, input) = setup(MovingSquare::default());
    let out = dir.path().join("out.mkv");
    ok(&[
        "render",
        "-q",
        s(&input),
        s(&out),
        "--codec",
        "ffv1",
        "--background-color",
        "#102030",
        "--motion-fill",
        "original",
    ]);
    let spec = MovingSquare::default();
    let f = frame(&out, 10);
    let y = spec.y + 8;
    assert_eq!(f.pixel(2, 2), [16, 32, 48], "background color");
    // The square's leading edge (only in frame 10) shows the original: white.
    assert_eq!(f.pixel(spec.x(10) + spec.size - 1, y), [255, 255, 255]);
    // Its trailing edge (only in frame 9) reveals what frame 10 has there: black.
    assert_eq!(f.pixel(spec.x(9), y), [0, 0, 0]);
}
