//! File system access for the UI: path checks and a simple directory browser.

use std::path::{Component, Path, PathBuf};

use axum::Json;
use axum::extract::{Query, State};
use serde::{Deserialize, Serialize};

use crate::api::ApiError;
use crate::{AppState, Mode, Shared};

/// The user's home directory (start of the file browser in desktop mode).
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// Canonical form of a path that may not exist yet (its parent must).
fn canonical(path: &Path) -> std::io::Result<PathBuf> {
    match path.canonicalize() {
        Ok(p) => Ok(p),
        Err(e) => {
            let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
                return Err(e);
            };
            let parent = if parent.as_os_str().is_empty() {
                Path::new(".")
            } else {
                parent
            };
            Ok(canonical(parent)?.join(name))
        }
    }
}

impl AppState {
    /// Checks a path from a client: in server mode it must lie inside one of the roots.
    /// Relative paths are relative to the first root.
    pub fn resolve_path(&self, raw: &str) -> Result<PathBuf, ApiError> {
        if raw.trim().is_empty() {
            return Err(ApiError::bad_request("empty path"));
        }
        let path = PathBuf::from(raw);
        if self.config.mode == Mode::Desktop {
            return Ok(path);
        }
        let path = match (path.is_relative(), self.config.roots.first()) {
            (true, Some(root)) => root.join(path),
            _ => path,
        };
        if path.components().any(|c| c == Component::ParentDir) && !path.exists() {
            return Err(ApiError::forbidden("path outside the allowed directories"));
        }
        let full = canonical(&path).map_err(|e| ApiError::bad_request(format!("{raw}: {e}")))?;
        let allowed = self
            .config
            .roots
            .iter()
            .filter_map(|r| r.canonicalize().ok())
            .any(|r| full.starts_with(r));
        if allowed {
            Ok(full)
        } else {
            Err(ApiError::forbidden("path outside the allowed directories"))
        }
    }

    pub fn start_dir(&self) -> Option<PathBuf> {
        match self.config.mode {
            Mode::Desktop => home_dir().or_else(|| std::env::current_dir().ok()),
            Mode::Server => self
                .config
                .roots
                .first()
                .and_then(|r| r.canonicalize().ok()),
        }
    }
}

#[derive(Deserialize)]
pub struct ListQuery {
    path: Option<String>,
}

#[derive(Serialize)]
pub struct Entry {
    name: String,
    path: String,
    dir: bool,
    size: u64,
}

#[derive(Serialize)]
pub struct Listing {
    path: String,
    parent: Option<String>,
    entries: Vec<Entry>,
}

pub async fn list(
    State(state): State<Shared>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Listing>, ApiError> {
    let dir = match q.path.as_deref().filter(|p| !p.is_empty()) {
        Some(p) => state.resolve_path(p)?,
        None => state
            .start_dir()
            .ok_or_else(|| ApiError::forbidden("no directory available"))?,
    };
    let read = std::fs::read_dir(&dir)
        .map_err(|e| ApiError::bad_request(format!("{}: {e}", dir.display())))?;
    let mut entries: Vec<Entry> = read
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }
            let meta = e.metadata().ok()?;
            Some(Entry {
                path: e.path().display().to_string(),
                name,
                dir: meta.is_dir(),
                size: meta.len(),
            })
        })
        .collect();
    entries.sort_by(|a, b| {
        b.dir
            .cmp(&a.dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    let parent = dir
        .parent()
        .filter(|p| state.resolve_path(&p.display().to_string()).is_ok())
        .map(|p| p.display().to_string());
    Ok(Json(Listing {
        path: dir.display().to_string(),
        parent,
        entries,
    }))
}
