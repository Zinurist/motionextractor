//! `serve`: the web UI and API without a window.

use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Args;
use mx_engine::{Session, Settings};
use mx_media::Ffmpeg;
use mx_server::{Mode, ServerConfig, event_channel};

#[derive(Debug, Args)]
pub struct ServeArgs {
    /// Address to listen on. Non-loopback addresses require a token (one is generated if needed).
    #[arg(long, default_value = "127.0.0.1")]
    bind: IpAddr,
    /// Port (0 = any free port).
    #[arg(long, default_value_t = 8090)]
    port: u16,
    /// Directory the UI may read videos from and write renders to (repeatable; default: the
    /// current directory).
    #[arg(long = "root", value_name = "DIR")]
    roots: Vec<PathBuf>,
    /// Access token required by the server.
    #[arg(long)]
    token: Option<String>,
    /// Settings file (default: the per-user config directory).
    #[arg(long, value_name = "FILE")]
    settings: Option<PathBuf>,
    /// Video to open at startup.
    file: Option<PathBuf>,
}

/// Default location of the settings file.
pub fn default_settings_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "motion-extractor")
        .map(|d| d.config_dir().join("settings.json"))
}

/// ffmpeg from the settings, else the usual search. A missing ffmpeg is reported by the UI.
pub fn ffmpeg_for(settings_path: Option<&Path>) -> Ffmpeg {
    let settings = settings_path
        .and_then(|p| Settings::load(p).ok())
        .unwrap_or_default();
    let located = Ffmpeg::locate().ok();
    let pick = |custom: Option<PathBuf>, found: Option<PathBuf>, name: &str| {
        custom.or(found).unwrap_or_else(|| PathBuf::from(name))
    };
    Ffmpeg::new(
        pick(
            settings.ffmpeg_path,
            located.as_ref().map(|f| f.ffmpeg.clone()),
            "ffmpeg",
        ),
        pick(settings.ffprobe_path, located.map(|f| f.ffprobe), "ffprobe"),
    )
}

pub fn random_token() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("system random number generator");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn run(args: ServeArgs) -> Result<()> {
    let settings_path = args.settings.or_else(default_settings_path);
    let ff = ffmpeg_for(settings_path.as_deref());
    let (sink, events) = event_channel();
    let session = Session::new(ff, settings_path, sink)?;
    if let Some(file) = &args.file {
        session
            .open(file)
            .with_context(|| format!("cannot open {}", file.display()))?;
    }
    let roots = if args.roots.is_empty() {
        vec![std::env::current_dir()?]
    } else {
        args.roots
    };
    let token = match args.token {
        Some(t) => Some(t),
        None if !args.bind.is_loopback() => Some(random_token()),
        None => None,
    };
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind(SocketAddr::new(args.bind, args.port))
            .await
            .with_context(|| format!("cannot listen on {}:{}", args.bind, args.port))?;
        let addr = listener.local_addr()?;
        let query = token
            .as_ref()
            .map(|t| format!("?token={t}"))
            .unwrap_or_default();
        let host = if addr.ip().is_unspecified() {
            "localhost".to_owned()
        } else {
            addr.ip().to_string()
        };
        println!(
            "Motion Extractor listening on http://{host}:{}/{query}",
            addr.port()
        );
        let config = ServerConfig {
            mode: Mode::Server,
            token,
            roots,
            dialogs: None,
        };
        mx_server::serve(listener, Arc::clone(&session), events, config).await?;
        Ok(())
    })
}
