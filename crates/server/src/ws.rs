//! WebSocket: preview frame requests (latest wins) and server events.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use mx_engine::{Session, View, ViewFrame};
use serde::Deserialize;
use tokio::sync::{broadcast, mpsc, watch};

use crate::Shared;
use crate::frames::{FLAG_EXACT, FLAG_ORIGINAL, FORMAT_JPEG, FORMAT_RGB, FrameHeader, jpeg};

/// How long a request waits for its frames before a stand-in is sent.
const QUICK_WAIT: Duration = Duration::from_millis(15);
const POLL_WAIT: Duration = Duration::from_millis(100);
const GIVE_UP: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMessage {
    View { seq: u32, frame: u64, view: View },
}

#[derive(Debug, Clone, Copy)]
struct Request {
    seq: u32,
    frame: u64,
}

pub async fn upgrade(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(st): State<Shared>,
) -> Response {
    ws.on_upgrade(move |socket| client(socket, addr, st))
}

async fn client(socket: WebSocket, addr: SocketAddr, st: Shared) {
    if let Some(on_connect) = &st.config.on_connect {
        on_connect();
    }
    let (mut sink, mut stream) = socket.split();
    let (out_tx, mut out_rx) = mpsc::channel::<Message>(8);
    let writer = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            if sink.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Server events as JSON text messages.
    let mut events = st.events.subscribe();
    let event_tx = out_tx.clone();
    let event_task = tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(e) => {
                    let text = serde_json::to_string(&e).expect("serialisable");
                    if event_tx.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::debug!("websocket client lagged by {n} events");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // One latest-wins slot per view, each served by its own task.
    let remote = !addr.ip().is_loopback();
    let quality = st.session.settings().jpeg_quality;
    let mut slots = Vec::new();
    let mut tasks = Vec::new();
    for view in [View::Output, View::Original] {
        let (tx, rx) = watch::channel(None::<Request>);
        let latest = Arc::new(AtomicU32::new(0));
        slots.push((view, tx, latest.clone()));
        let sender = Sender {
            out: out_tx.clone(),
            remote,
            quality,
        };
        tasks.push(tokio::spawn(serve_view(
            st.session.clone(),
            view,
            rx,
            latest,
            sender,
        )));
    }

    while let Some(Ok(msg)) = stream.next().await {
        let Message::Text(text) = msg else {
            if matches!(msg, Message::Close(_)) {
                break;
            }
            continue;
        };
        match serde_json::from_str::<ClientMessage>(&text) {
            Ok(ClientMessage::View { seq, frame, view }) => {
                if let Some((_, tx, latest)) = slots.iter().find(|(v, _, _)| *v == view) {
                    latest.store(seq, Ordering::SeqCst);
                    let _ = tx.send(Some(Request { seq, frame }));
                }
            }
            Err(e) => tracing::debug!("bad websocket message {text:?}: {e}"),
        }
    }
    drop(slots);
    for t in tasks {
        t.abort();
    }
    event_task.abort();
    writer.abort();
}

#[derive(Clone)]
struct Sender {
    out: mpsc::Sender<Message>,
    remote: bool,
    quality: u8,
}

impl Sender {
    fn send(&self, seq: u32, v: &ViewFrame) -> bool {
        let (format, payload) = if self.remote {
            (FORMAT_JPEG, jpeg(&v.frame, self.quality))
        } else {
            (FORMAT_RGB, v.frame.data().to_vec())
        };
        let mut flags = 0;
        if v.exact {
            flags |= FLAG_EXACT;
        }
        if v.view == View::Original {
            flags |= FLAG_ORIGINAL;
        }
        let header = FrameHeader {
            seq,
            index: v.index as u32,
            width: v.frame.width(),
            height: v.frame.height(),
            format,
            flags,
            rev: v.rev as u32,
        };
        self.out
            .blocking_send(Message::Binary(header.encode(&payload).into()))
            .is_ok()
    }

    fn error(&self, message: &str) {
        let text = serde_json::json!({ "type": "error", "message": message }).to_string();
        let _ = self.out.blocking_send(Message::Text(text.into()));
    }
}

async fn serve_view(
    session: Arc<Session>,
    view: View,
    mut rx: watch::Receiver<Option<Request>>,
    latest: Arc<AtomicU32>,
    sender: Sender,
) {
    while rx.changed().await.is_ok() {
        let Some(req) = *rx.borrow_and_update() else {
            continue;
        };
        let (session, latest, sender) = (session.clone(), latest.clone(), sender.clone());
        let done =
            tokio::task::spawn_blocking(move || answer(&session, view, req, &latest, &sender))
                .await;
        if done.is_err() {
            break;
        }
    }
}

/// Sends the requested frame, or a stand-in first if it is not decoded yet. Gives up as soon as a
/// newer request arrives.
fn answer(session: &Session, view: View, req: Request, latest: &AtomicU32, sender: &Sender) {
    let stale = || latest.load(Ordering::SeqCst) != req.seq;
    let result = (|| {
        if let Some(v) = session.view(req.frame, view, QUICK_WAIT, &stale)? {
            sender.send(req.seq, &v);
            return Ok(());
        }
        if stale() {
            return Ok(());
        }
        if let Some(v) = session.approximate_view(req.frame, view)? {
            sender.send(req.seq, &v);
        }
        let start = Instant::now();
        while !stale() && start.elapsed() < GIVE_UP {
            if let Some(v) = session.view(req.frame, view, POLL_WAIT, &stale)? {
                sender.send(req.seq, &v);
                break;
            }
        }
        Ok::<_, mx_engine::SessionError>(())
    })();
    if let Err(e) = result {
        sender.error(&e.to_string());
    }
}
