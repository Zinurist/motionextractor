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

/// Canonical form of a path that may not exist yet (its parent must). Uses `dunce` so Windows paths
/// keep their usual form (no `\\?\` prefix).
fn canonical(path: &Path) -> std::io::Result<PathBuf> {
    match dunce::canonicalize(path) {
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
            .filter_map(|r| dunce::canonicalize(r).ok())
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
                .and_then(|r| dunce::canonicalize(r).ok()),
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
    /// Bytes available to this user on the directory's filesystem (null if unknown).
    available: Option<u64>,
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
        available: fs4::available_space(&dir).ok(),
    }))
}

/// `dir/name`, or the first of `name (2).ext`, `name (3).ext`, … that does not exist yet.
fn free_name(dir: &Path, name: &str) -> PathBuf {
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    std::iter::once(name.to_owned())
        .chain((2..).map(|n| format!("{stem} ({n}){ext}")))
        .map(|candidate| dir.join(candidate))
        .find(|p| !p.exists())
        .expect("an unused name exists")
}

/// Checks an uploaded file name: a plain, visible file name without path components.
fn valid_name(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty()
        && trimmed == name
        && !name.starts_with('.')
        && !name.contains(['/', '\\', ':', '\0'])
        && name.len() <= 255
}

#[derive(Deserialize)]
pub struct UploadQuery {
    /// Target directory (inside the roots in server mode).
    dir: String,
    /// File name (no path components).
    name: String,
    #[serde(default)]
    overwrite: bool,
    /// If the name is taken, use the first free "name (2).ext", "name (3).ext", ….
    #[serde(default)]
    unique: bool,
}

#[derive(Serialize)]
pub struct Uploaded {
    path: String,
    size: u64,
}

/// Stores the request body as a file. The data goes to a hidden `.name.part` file first and is
/// renamed when complete, so interrupted uploads never leave a truncated video behind.
pub async fn upload(
    State(state): State<Shared>,
    Query(q): Query<UploadQuery>,
    body: axum::body::Body,
) -> Result<Json<Uploaded>, ApiError> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    if !valid_name(&q.name) {
        return Err(ApiError::bad_request(format!(
            "invalid file name {:?}",
            q.name
        )));
    }
    let dir = state.resolve_path(&q.dir)?;
    if !dir.is_dir() {
        return Err(ApiError::bad_request(format!(
            "{} is not a directory",
            dir.display()
        )));
    }
    let mut target = dir.join(&q.name);
    if q.unique {
        target = free_name(&dir, &q.name);
    }
    if target.exists() && !q.overwrite {
        return Err(ApiError(
            axum::http::StatusCode::CONFLICT,
            format!("{} already exists", target.display()),
        ));
    }
    let file_name = target
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let part = dir.join(format!(".{file_name}.part"));
    let io = |e: std::io::Error| {
        ApiError(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("upload failed: {e}"),
        )
    };
    let mut file = tokio::fs::File::create(&part).await.map_err(io)?;
    let mut size = 0u64;
    let mut stream = body.into_data_stream();
    let result: Result<(), ApiError> = async {
        while let Some(chunk) = stream.next().await {
            let chunk =
                chunk.map_err(|e| ApiError::bad_request(format!("upload interrupted: {e}")))?;
            size += chunk.len() as u64;
            file.write_all(&chunk).await.map_err(io)?;
        }
        file.flush().await.map_err(io)?;
        drop(file);
        tokio::fs::rename(&part, &target).await.map_err(io)
    }
    .await;
    if let Err(e) = result {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(e);
    }
    Ok(Json(Uploaded {
        path: target.display().to_string(),
        size,
    }))
}

#[derive(Deserialize)]
pub struct DownloadQuery {
    path: String,
}

/// Sends a file as an attachment (with HTTP range support).
pub async fn download(
    State(state): State<Shared>,
    Query(q): Query<DownloadQuery>,
    req: axum::extract::Request,
) -> Result<axum::response::Response, ApiError> {
    use axum::response::IntoResponse;
    use tower::ServiceExt;

    let path = state.resolve_path(&q.path)?;
    if !path.is_file() {
        return Err(ApiError::bad_request(format!(
            "{} is not a file",
            path.display()
        )));
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "download".into());
    let mut resp = tower_http::services::ServeFile::new(&path)
        .oneshot(req)
        .await
        .map_err(|e| ApiError(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .into_response();
    // ASCII fallback plus the exact (UTF-8, percent-encoded) name.
    let ascii: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() && c != '"' && c != '\\' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let encoded: String = name
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect();
    let disposition = format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}");
    if let Ok(v) = disposition.parse() {
        resp.headers_mut()
            .insert(axum::http::header::CONTENT_DISPOSITION, v);
    }
    Ok(resp)
}

#[cfg(test)]
mod tests {
    use super::valid_name;

    #[test]
    fn free_names() {
        let dir = std::env::temp_dir().join(format!("mx-free-name-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(super::free_name(&dir, "clip.mp4"), dir.join("clip.mp4"));
        std::fs::write(dir.join("clip.mp4"), b"").unwrap();
        assert_eq!(super::free_name(&dir, "clip.mp4"), dir.join("clip (2).mp4"));
        std::fs::write(dir.join("clip (2).mp4"), b"").unwrap();
        assert_eq!(super::free_name(&dir, "clip.mp4"), dir.join("clip (3).mp4"));
        std::fs::write(dir.join("README"), b"").unwrap();
        assert_eq!(super::free_name(&dir, "README"), dir.join("README (2)"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn upload_names() {
        assert!(valid_name("clip.mp4"));
        assert!(valid_name("Mein Urlaub (2).mov"));
        for bad in [
            "",
            " ",
            ".hidden.mp4",
            "../x.mp4",
            "a/b.mp4",
            "a\\b.mp4",
            "c:x",
            " x.mp4",
        ] {
            assert!(!valid_name(bad), "{bad:?}");
        }
    }
}
