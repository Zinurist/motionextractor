//! HTTP and WebSocket API tests against a real server and ffmpeg.

use std::path::PathBuf;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use mx_engine::Session;
use mx_media::testing::{MovingSquare, ffmpeg};
use mx_server::{FrameHeader, Mode, ServerConfig, event_channel, serve};
use reqwest::StatusCode;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

struct Server {
    base: String,
    addr: std::net::SocketAddr,
    dir: tempfile::TempDir,
    video: PathBuf,
    http: reqwest::Client,
}

async fn start(token: Option<&str>) -> Server {
    let dir = tempfile::tempdir().unwrap();
    let video = MovingSquare::default().generate(&ffmpeg(), dir.path());
    let (sink, events) = event_channel();
    let session = Session::new(ffmpeg(), Some(dir.path().join("settings.json")), sink).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let config = ServerConfig {
        mode: Mode::Server,
        token: token.map(String::from),
        roots: vec![dir.path().to_owned()],
        dialogs: None,
        on_connect: None,
    };
    tokio::spawn(serve(listener, session, events, config));
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    Server {
        base: format!("http://{addr}"),
        addr,
        dir,
        video,
        http,
    }
}

impl Server {
    async fn get(&self, path: &str) -> (StatusCode, Value) {
        let r = self
            .http
            .get(format!("{}{path}", self.base))
            .send()
            .await
            .unwrap();
        let status = r.status();
        (status, r.json().await.unwrap_or(Value::Null))
    }

    async fn send(&self, method: reqwest::Method, path: &str, body: Value) -> (StatusCode, Value) {
        let r = self
            .http
            .request(method, format!("{}{path}", self.base))
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = r.status();
        (status, r.json().await.unwrap_or(Value::Null))
    }

    async fn post(&self, path: &str, body: Value) -> (StatusCode, Value) {
        self.send(reqwest::Method::POST, path, body).await
    }

    async fn open(&self) -> Value {
        let (s, v) = self.post("/api/open", json!({ "path": self.video })).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        v
    }
}

#[tokio::test]
async fn info_and_session() {
    let s = start(None).await;
    let (status, info) = s.get("/api/info").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(info["mode"], "server");
    assert_eq!(info["ffmpeg"]["ok"], true);
    assert_eq!(info["dialogs"], false);
    let (_, session) = s.get("/api/session").await;
    assert_eq!(session["video"], Value::Null);
    assert_eq!(session["preset"]["effect"]["color"], "#ffffff");
    let opened = s.open().await;
    assert_eq!(opened["info"]["frame_count"], 30);
    let (_, session) = s.get("/api/session").await;
    assert_eq!(session["video"]["preview_width"], 160);
}

#[tokio::test]
async fn open_errors_and_path_restrictions() {
    let s = start(None).await;
    let (status, _) = s
        .post(
            "/api/open",
            json!({ "path": std::env::temp_dir().join("outside.mp4") }),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = s
        .post("/api/open", json!({ "path": "../../etc/passwd" }))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    std::fs::write(s.dir.path().join("bad.mp4"), b"nope").unwrap();
    let (status, body) = s.post("/api/open", json!({ "path": "bad.mp4" })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["error"].as_str().unwrap().contains("ffprobe"));
    let (status, _) = s.get("/api/frame/0").await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn presets_and_frames() {
    let s = start(None).await;
    s.open().await;
    let (status, body) = s
        .send(
            reqwest::Method::PATCH,
            "/api/preset",
            json!({ "effect": { "color": "#ff0000" }, "render": { "crf": 30 } }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["preset"]["effect"]["color"], "#ff0000");
    assert_eq!(body["preset"]["effect"]["gain"], 4.0);
    assert_eq!(body["preset"]["render"]["crf"], 30);
    assert_eq!(body["rev"], 1);
    let (status, body) = s
        .send(
            reqwest::Method::PATCH,
            "/api/preset",
            json!({ "effect": { "threshold": 7 } }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    for query in ["", "?view=original", "?full=true"] {
        let r = s
            .http
            .get(format!("{}/api/frame/10{query}", s.base))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(r.headers()["content-type"], "image/png");
        assert_eq!(&r.bytes().await.unwrap()[1..4], b"PNG");
    }

    let path = s.dir.path().join("p.json");
    let (status, _) = s.post("/api/preset/save", json!({ "path": path })).await;
    assert_eq!(status, StatusCode::OK);
    s.send(reqwest::Method::PUT, "/api/preset", json!({})).await;
    let (_, body) = s.post("/api/preset/load", json!({ "path": path })).await;
    assert_eq!(body["preset"]["effect"]["color"], "#ff0000");
}

#[tokio::test]
async fn render_through_api() {
    let s = start(None).await;
    s.open().await;
    let (_, cmd) = s
        .post("/api/render/command", json!({ "output": "x.webm" }))
        .await;
    assert!(cmd["command"].as_str().unwrap().contains("libx264"));
    let (status, job) = s.post("/api/render", json!({ "output": "out.mp4" })).await;
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["state"], "running");
    let mut state = Value::Null;
    for _ in 0..500 {
        let (_, job) = s.get("/api/render").await;
        state = job["state"].clone();
        if state != "running" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(state, "finished");
    assert!(s.dir.path().join("out.mp4").exists());
    let (status, _) = s.post("/api/render", json!({ "output": "out.mp4" })).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = s
        .post(
            "/api/render",
            json!({ "output": std::env::temp_dir().join("elsewhere.mp4") }),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (_, body) = s
        .send(reqwest::Method::DELETE, "/api/render", json!({}))
        .await;
    assert_eq!(body["cancelled"], false);
}

#[tokio::test]
async fn settings_files_and_dialogs() {
    let s = start(None).await;
    let (_, settings) = s.get("/api/settings").await;
    assert_eq!(settings["preview_height"], 540);
    let mut changed = settings.clone();
    changed["preview_height"] = json!(240);
    let (status, _) = s.send(reqwest::Method::PUT, "/api/settings", changed).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(s.get("/api/settings").await.1["preview_height"], 240);
    let mut bad = settings.clone();
    bad["decode_workers"] = json!(0);
    let (status, _) = s.send(reqwest::Method::PUT, "/api/settings", bad).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    std::fs::create_dir(s.dir.path().join("sub")).unwrap();
    let (status, list) = s.get("/api/fs/list").await;
    assert_eq!(status, StatusCode::OK);
    let entries = list["entries"].as_array().unwrap();
    assert_eq!(entries[0]["name"], "sub");
    assert_eq!(entries[0]["dir"], true);
    assert!(
        entries
            .iter()
            .any(|e| e["name"].as_str().unwrap().ends_with(".mkv"))
    );
    assert_eq!(list["parent"], Value::Null, "cannot leave the root");
    let (status, _) = s.get("/api/fs/list?path=/").await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = s.post("/api/dialog/open-video", json!({})).await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
}

#[tokio::test]
async fn token_and_host_checks() {
    let s = start(Some("secret")).await;
    let (status, _) = s.get("/api/info").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let r = s
        .http
        .get(format!("{}/?token=secret", s.base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::SEE_OTHER);
    assert_eq!(r.headers()["location"], "/");
    let cookie = r.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let r = s
        .http
        .get(format!("{}/api/info", s.base))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let r = s
        .http
        .get(format!("{}/api/info", s.base))
        .header("x-mx-token", "secret")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let r = s
        .http
        .get(format!("{}/api/info", s.base))
        .header("x-mx-token", "wrong")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);

    let r = s
        .http
        .get(format!("{}/api/info", s.base))
        .header("x-mx-token", "secret")
        .header("host", "evil.example:80")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    let r = s
        .http
        .get(format!("{}/api/info", s.base))
        .header("x-mx-token", "secret")
        .header("origin", "http://evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn static_ui() {
    let s = start(None).await;
    let r = s.http.get(format!("{}/", s.base)).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert!(
        r.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/html")
    );
    let r = s
        .http
        .get(format!("{}/some/client/route", s.base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let r = s
        .http
        .get(format!("{}/api/nope", s.base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn websocket_frames_and_events() {
    let s = start(None).await;
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{}/api/ws", s.addr))
        .await
        .unwrap();
    s.open().await;

    // Burst of requests: the last one must be answered exactly.
    for seq in 1..=5u32 {
        let msg = json!({ "type": "view", "seq": seq, "frame": seq * 4, "view": "output" });
        ws.send(Message::Text(msg.to_string().into()))
            .await
            .unwrap();
    }
    let orig = json!({ "type": "view", "seq": 1, "frame": 3, "view": "original" });
    ws.send(Message::Text(orig.to_string().into()))
        .await
        .unwrap();

    let mut saw_opened = false;
    let mut exact_output = None;
    let mut exact_original = None;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while exact_output.is_none() || exact_original.is_none() || !saw_opened {
        let msg = tokio::time::timeout_at(deadline, ws.next())
            .await
            .expect("timeout")
            .unwrap()
            .unwrap();
        match msg {
            Message::Text(t) => {
                let v: Value = serde_json::from_str(&t).unwrap();
                if v["type"] == "opened" {
                    saw_opened = true;
                }
            }
            Message::Binary(b) => {
                let (h, payload) = FrameHeader::decode(&b).unwrap();
                assert_eq!((h.width, h.height), (160, 120));
                assert_eq!(payload.len(), 160 * 120 * 3, "raw RGB on loopback");
                if h.flags & 1 == 1 {
                    if h.flags & 2 == 2 {
                        exact_original = Some(h);
                    } else if h.seq == 5 {
                        exact_output = Some(h);
                    }
                }
            }
            _ => {}
        }
    }
    assert_eq!(exact_output.unwrap().index, 20);
    assert_eq!(exact_original.unwrap().index, 3);

    // Parameter changes are broadcast.
    s.send(
        reqwest::Method::PATCH,
        "/api/preset",
        json!({ "effect": { "gain": 2 } }),
    )
    .await;
    loop {
        let msg = tokio::time::timeout_at(deadline, ws.next())
            .await
            .expect("timeout")
            .unwrap()
            .unwrap();
        if let Message::Text(t) = msg {
            let v: Value = serde_json::from_str(&t).unwrap();
            if v["type"] == "preset" {
                assert_eq!(v["preset"]["effect"]["gain"], 2.0);
                break;
            }
        }
    }
}

#[tokio::test]
async fn audio_for_playback() {
    let s = start(None).await;
    let (status, _) = s.get("/api/audio").await;
    assert_eq!(status, StatusCode::CONFLICT, "no video yet");
    let video = MovingSquare {
        audio: true,
        ..Default::default()
    }
    .generate(&ffmpeg(), s.dir.path());
    let (status, _) = s.post("/api/open", json!({ "path": video })).await;
    assert_eq!(status, StatusCode::OK);
    let r = s
        .http
        .get(format!("{}/api/audio", s.base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(r.headers()["content-type"], "audio/wav");
    assert_eq!(r.headers()["accept-ranges"], "bytes");
    let full = r.bytes().await.unwrap();
    assert_eq!(&full[8..12], b"WAVE");
    // Range requests let the player seek.
    let r = s
        .http
        .get(format!("{}/api/audio", s.base))
        .header("range", "bytes=100-199")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(&r.bytes().await.unwrap()[..], &full[100..200]);
    // Videos without audio.
    s.open().await;
    let (status, body) = s.get("/api/audio").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}
