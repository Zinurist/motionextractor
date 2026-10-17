//! HTTP + WebSocket API around an engine [`Session`], serving the embedded web UI.

mod api;
mod assets;
mod auth;
mod files;
mod frames;
mod ws;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};
use mx_engine::{Event, EventSink, Session};
use serde::Serialize;
use tokio::net::TcpListener;
use tokio::sync::broadcast;

pub use frames::{FRAME_HEADER_LEN, FRAME_MAGIC, FrameHeader};

/// How the server is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Inside the desktop app: native dialogs, unrestricted paths.
    Desktop,
    /// Headless: paths restricted to the configured roots.
    Server,
}

/// Native file dialogs, provided by the desktop shell.
pub trait Dialogs: Send + Sync {
    fn open_video(&self) -> Option<PathBuf>;
    fn save_video(&self, suggested_name: &str) -> Option<PathBuf>;
    fn open_preset(&self) -> Option<PathBuf>;
    fn save_preset(&self) -> Option<PathBuf>;
}

pub struct ServerConfig {
    pub mode: Mode,
    /// Required on every request when set (query `token` once, then cookie or `x-mx-token`).
    pub token: Option<String>,
    /// Allowed directories in server mode (empty: no file access).
    pub roots: Vec<PathBuf>,
    pub dialogs: Option<Arc<dyn Dialogs>>,
}

/// Creates the event sink to pass to [`Session::new`] and the channel the server reads from.
pub fn event_channel() -> (EventSink, broadcast::Sender<Event>) {
    let (tx, _) = broadcast::channel(256);
    let sender = tx.clone();
    let sink: EventSink = Arc::new(move |e| {
        // No receivers (no clients connected) is fine.
        let _ = sender.send(e);
    });
    (sink, tx)
}

pub(crate) struct AppState {
    pub session: Arc<Session>,
    pub events: broadcast::Sender<Event>,
    pub config: ServerConfig,
    /// Values accepted in the `Host` header (loopback binds only).
    pub allowed_hosts: Option<Vec<String>>,
}

pub(crate) type Shared = Arc<AppState>;

fn router(state: Shared) -> Router {
    let api = Router::new()
        .route("/info", get(api::info))
        .route("/session", get(api::session))
        .route("/open", post(api::open))
        .route(
            "/preset",
            get(api::get_preset)
                .put(api::put_preset)
                .patch(api::patch_preset),
        )
        .route("/preset/save", post(api::save_preset))
        .route("/preset/load", post(api::load_preset))
        .route("/settings", get(api::get_settings).put(api::put_settings))
        .route(
            "/render",
            get(api::render_status)
                .post(api::start_render)
                .delete(api::cancel_render),
        )
        .route("/render/command", post(api::render_command))
        .route("/frame/{index}", get(api::frame))
        .route("/fs/list", get(files::list))
        .route("/dialog/{kind}", post(api::dialog))
        .route("/ws", get(ws::upgrade));
    Router::new()
        .nest("/api", api)
        .fallback(assets::serve)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::check,
        ))
        .with_state(state)
}

/// Serves on `listener` until the future is dropped.
pub async fn serve(
    listener: TcpListener,
    session: Arc<Session>,
    events: broadcast::Sender<Event>,
    config: ServerConfig,
) -> std::io::Result<()> {
    let addr = listener.local_addr()?;
    let allowed_hosts = addr.ip().is_loopback().then(|| {
        let port = addr.port();
        vec![
            format!("127.0.0.1:{port}"),
            format!("localhost:{port}"),
            format!("[::1]:{port}"),
        ]
    });
    let state = Arc::new(AppState {
        session,
        events,
        config,
        allowed_hosts,
    });
    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
}
