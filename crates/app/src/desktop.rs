//! The desktop app: the local server plus a native window (Tauri) showing the web UI.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result};
use mx_engine::Session;
use mx_server::{Dialogs, Mode, ServerConfig, event_channel};
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, DragDropEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_dialog::{DialogExt, FilePath};

use crate::serve::{default_settings_path, ffmpeg_for};

const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mov", "mkv", "webm", "avi", "m4v", "ts", "mts", "m2ts", "mpg", "mpeg", "wmv", "flv",
    "gif", "ogv", "3gp",
];

/// Each extension in lower and upper case: GTK matches filter patterns case-sensitively, so `*.mov`
/// would hide `IMG_0001.MOV`.
fn both_cases(extensions: &[&str]) -> Vec<String> {
    let mut out = Vec::with_capacity(extensions.len() * 2);
    for e in extensions {
        out.push(e.to_lowercase());
        out.push(e.to_uppercase());
    }
    out
}

/// Native file dialogs through the Tauri dialog plugin (called from server worker threads; the
/// plugin runs them on the UI thread).
#[derive(Default)]
struct NativeDialogs(OnceLock<AppHandle>);

impl NativeDialogs {
    fn pick(&self, save: bool, filter: (&str, &[&str]), suggested: &str) -> Option<PathBuf> {
        let app = self.0.get()?;
        let extensions = both_cases(filter.1);
        let extensions: Vec<&str> = extensions.iter().map(String::as_str).collect();
        let mut dialog = app
            .dialog()
            .file()
            .add_filter(filter.0, &extensions)
            .add_filter("All files", &["*"]);
        if !suggested.is_empty() {
            let path = PathBuf::from(suggested);
            if let Some(dir) = path.parent().filter(|d| d.is_dir()) {
                dialog = dialog.set_directory(dir);
            }
            if let Some(name) = path.file_name() {
                dialog = dialog.set_file_name(name.to_string_lossy());
            }
        }
        let picked = if save {
            dialog.blocking_save_file()
        } else {
            dialog.blocking_pick_file()
        };
        match picked? {
            FilePath::Path(p) => Some(p),
            FilePath::Url(u) => u.to_file_path().ok(),
        }
    }
}

impl Dialogs for NativeDialogs {
    fn open_video(&self) -> Option<PathBuf> {
        self.pick(false, ("Videos", VIDEO_EXTENSIONS), "")
    }

    fn save_video(&self, suggested: &str) -> Option<PathBuf> {
        self.pick(true, ("Videos", VIDEO_EXTENSIONS), suggested)
    }

    fn open_preset(&self) -> Option<PathBuf> {
        self.pick(false, ("Presets", &["json"]), "")
    }

    fn save_preset(&self) -> Option<PathBuf> {
        self.pick(true, ("Presets", &["json"]), "preset.json")
    }
}

/// Opens a file in the background, reporting failures to the UI.
fn open_in_background(session: &Arc<Session>, path: PathBuf) {
    let session = session.clone();
    std::thread::spawn(move || {
        if let Err(e) = session.open(&path) {
            session.report_error(format!("Cannot open {}: {e}", path.display()));
        }
    });
}

fn smoke_test_passed() {
    println!("smoke test: UI connected");
    std::process::exit(0);
}

pub fn run(file: Option<PathBuf>, smoke_test: bool) -> Result<()> {
    let settings_path = default_settings_path();
    let ff = ffmpeg_for(settings_path.as_deref());
    let (sink, events) = event_channel();
    let session = Session::new(ff, settings_path, sink)?;
    if let Some(file) = file {
        open_in_background(&session, file);
    }

    let rt = tokio::runtime::Runtime::new()?;
    let listener = rt
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .context("cannot start the local server")?;
    let port = listener.local_addr()?.port();
    let token = random_token();
    let dialogs = Arc::new(NativeDialogs::default());
    // The smoke test passes once the UI's JavaScript has connected to the server.
    let on_connect: Option<Arc<dyn Fn() + Send + Sync>> =
        smoke_test.then(|| Arc::new(smoke_test_passed) as Arc<dyn Fn() + Send + Sync>);
    let config = ServerConfig {
        mode: Mode::Desktop,
        token: Some(token.clone()),
        roots: Vec::new(),
        dialogs: Some(dialogs.clone()),
        on_connect,
    };
    rt.spawn(mx_server::serve(listener, session.clone(), events, config));
    let url: tauri::Url = format!("http://127.0.0.1:{port}/?token={token}").parse()?;
    tracing::info!("desktop UI at http://127.0.0.1:{port}/");

    if smoke_test {
        // Fail if the UI does not come up in time.
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(60));
            eprintln!("smoke test: timed out");
            std::process::exit(1);
        });
    }

    let drop_session = session.clone();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let _ = dialogs.0.set(app.handle().clone());
            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
                .title("Motion Extractor")
                .inner_size(1400.0, 900.0)
                .min_inner_size(900.0, 600.0)
                .on_page_load(move |_webview, payload| {
                    if smoke_test && payload.event() == PageLoadEvent::Finished {
                        println!("smoke test: page loaded ({})", payload.url());
                    }
                })
                .build()?;
            Ok(())
        })
        .on_window_event(move |_window, event| {
            if let WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }) = event
                && let Some(path) = paths.first()
            {
                open_in_background(&drop_session, path.clone());
            }
        })
        .run(tauri::generate_context!())
        .context("cannot start the desktop window")?;
    drop(rt);
    Ok(())
}

/// Session token for the desktop window, so other local processes cannot drive the app.
fn random_token() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("system random number generator");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_match_both_cases() {
        assert_eq!(both_cases(&["mov", "mp4"]), ["mov", "MOV", "mp4", "MP4"]);
    }
}
