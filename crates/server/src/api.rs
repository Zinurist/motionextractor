//! REST handlers.

use std::path::PathBuf;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use mx_engine::{
    CacheState, JobStatus, Preset, PresetError, RenderError, SessionError, Settings, VideoState,
    View,
};
use mx_media::MediaError;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::files::home_dir;
use crate::frames::png;
use crate::{Mode, Shared};

/// An error response: `{"error": "…"}` with a status code.
#[derive(Debug)]
pub struct ApiError(pub StatusCode, pub String);

impl ApiError {
    pub fn bad_request(m: impl Into<String>) -> Self {
        Self(StatusCode::BAD_REQUEST, m.into())
    }

    pub fn forbidden(m: impl Into<String>) -> Self {
        Self(StatusCode::FORBIDDEN, m.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<SessionError> for ApiError {
    fn from(e: SessionError) -> Self {
        let status = match &e {
            SessionError::NoVideo | SessionError::Busy | SessionError::Exists(_) => {
                StatusCode::CONFLICT
            }
            SessionError::Media(MediaError::Probe(_) | MediaError::NoVideoStream(_)) => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            SessionError::Param(_)
            | SessionError::Effect(_)
            | SessionError::Settings(_)
            | SessionError::Render(RenderError::InvalidRange { .. } | RenderError::Param(_)) => {
                StatusCode::BAD_REQUEST
            }
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self(status, e.to_string())
    }
}

impl From<PresetError> for ApiError {
    fn from(e: PresetError) -> Self {
        Self::bad_request(e.to_string())
    }
}

type ApiResult<T> = Result<Json<T>, ApiError>;

/// Runs blocking engine work off the async runtime.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, ApiError> + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
}

#[derive(Serialize)]
pub struct FfmpegInfo {
    ok: bool,
    version: Option<String>,
    error: Option<String>,
}

pub async fn info(State(st): State<Shared>) -> ApiResult<Value> {
    let ff = st.session.ffmpeg().clone();
    let ffmpeg = blocking(move || {
        Ok(match ff.version() {
            Ok(v) => FfmpegInfo {
                ok: true,
                version: Some(v),
                error: None,
            },
            Err(e) => FfmpegInfo {
                ok: false,
                version: None,
                error: Some(e.to_string()),
            },
        })
    })
    .await?;
    let roots: Vec<String> = match st.config.mode {
        Mode::Server => st
            .config
            .roots
            .iter()
            .map(|r| r.display().to_string())
            .collect(),
        Mode::Desktop => home_dir()
            .into_iter()
            .map(|h| h.display().to_string())
            .collect(),
    };
    Ok(Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "mode": st.config.mode,
        "ffmpeg": ffmpeg,
        "roots": roots,
        "dialogs": st.config.dialogs.is_some(),
    })))
}

#[derive(Serialize)]
pub struct SessionInfo {
    video: Option<VideoState>,
    preset: Preset,
    rev: u64,
    cache: CacheState,
    render: Option<JobStatus>,
    settings: Settings,
}

pub async fn session(State(st): State<Shared>) -> ApiResult<SessionInfo> {
    let s = &st.session;
    let (preset, rev) = s.preset();
    Ok(Json(SessionInfo {
        video: s.video(),
        preset,
        rev,
        cache: s.cache_state(),
        render: s.render_status(),
        settings: s.settings(),
    }))
}

#[derive(Deserialize)]
pub struct PathBody {
    path: String,
}

pub async fn open(State(st): State<Shared>, Json(body): Json<PathBody>) -> ApiResult<VideoState> {
    let path = st.resolve_path(&body.path)?;
    let session = st.session.clone();
    Ok(Json(blocking(move || Ok(session.open(&path)?)).await?))
}

#[derive(Serialize)]
pub struct PresetResponse {
    preset: Preset,
    rev: u64,
}

pub async fn get_preset(State(st): State<Shared>) -> ApiResult<PresetResponse> {
    let (preset, rev) = st.session.preset();
    Ok(Json(PresetResponse { preset, rev }))
}

pub async fn put_preset(
    State(st): State<Shared>,
    Json(body): Json<Value>,
) -> ApiResult<PresetResponse> {
    let preset = Preset::from_json(&body.to_string())?;
    let rev = st.session.set_preset(preset.clone())?;
    Ok(Json(PresetResponse { preset, rev }))
}

/// JSON merge patch (RFC 7386) of the current preset.
pub async fn patch_preset(
    State(st): State<Shared>,
    Json(patch): Json<Value>,
) -> ApiResult<PresetResponse> {
    let (current, _) = st.session.preset();
    let mut value = serde_json::to_value(&current).expect("serialisable");
    merge_patch(&mut value, &patch);
    put_preset(State(st), Json(value)).await
}

fn merge_patch(target: &mut Value, patch: &Value) {
    match (target, patch) {
        (Value::Object(t), Value::Object(p)) => {
            for (k, v) in p {
                if v.is_null() && !matches!(t.get(k), Some(Value::Null) | None) {
                    // Nullable settings (e.g. "crf") are reset by null; keep the key.
                    t.insert(k.clone(), Value::Null);
                } else {
                    merge_patch(t.entry(k.clone()).or_insert(Value::Null), v);
                }
            }
        }
        (t, p) => *t = p.clone(),
    }
}

pub async fn save_preset(State(st): State<Shared>, Json(body): Json<PathBody>) -> ApiResult<Value> {
    let path = st.resolve_path(&body.path)?;
    st.session.preset().0.save(&path)?;
    Ok(Json(json!({ "path": path })))
}

pub async fn load_preset(
    State(st): State<Shared>,
    Json(body): Json<PathBody>,
) -> ApiResult<PresetResponse> {
    let path = st.resolve_path(&body.path)?;
    let preset = Preset::load(&path)?;
    let rev = st.session.set_preset(preset.clone())?;
    Ok(Json(PresetResponse { preset, rev }))
}

pub async fn get_settings(State(st): State<Shared>) -> ApiResult<Settings> {
    Ok(Json(st.session.settings()))
}

pub async fn put_settings(
    State(st): State<Shared>,
    Json(s): Json<Settings>,
) -> ApiResult<Settings> {
    let session = st.session.clone();
    let saved = s.clone();
    blocking(move || Ok(session.set_settings(s)?)).await?;
    Ok(Json(saved))
}

pub async fn render_status(State(st): State<Shared>) -> ApiResult<Option<JobStatus>> {
    Ok(Json(st.session.render_status()))
}

#[derive(Deserialize)]
pub struct RenderBody {
    output: String,
    #[serde(default)]
    overwrite: bool,
}

pub async fn start_render(
    State(st): State<Shared>,
    Json(body): Json<RenderBody>,
) -> ApiResult<JobStatus> {
    let output = st.resolve_path(&body.output)?;
    let session = st.session.clone();
    Ok(Json(
        blocking(move || Ok(session.start_render(output, body.overwrite)?)).await?,
    ))
}

pub async fn cancel_render(State(st): State<Shared>) -> ApiResult<Value> {
    Ok(Json(json!({ "cancelled": st.session.cancel_render() })))
}

#[derive(Deserialize)]
pub struct CommandBody {
    output: String,
}

pub async fn render_command(
    State(st): State<Shared>,
    Json(body): Json<CommandBody>,
) -> ApiResult<Value> {
    // The path is only displayed, so it is not checked against the roots.
    let output = PathBuf::from(if body.output.is_empty() {
        "output.mp4"
    } else {
        &body.output
    });
    Ok(Json(
        json!({ "command": st.session.render_command(output)? }),
    ))
}

#[derive(Deserialize)]
pub struct FrameQuery {
    #[serde(default = "default_view")]
    view: View,
    /// Decode at full resolution instead of using the preview cache.
    #[serde(default)]
    full: bool,
}

fn default_view() -> View {
    View::Output
}

pub async fn frame(
    State(st): State<Shared>,
    Path(index): Path<u64>,
    Query(q): Query<FrameQuery>,
) -> Result<Response, ApiError> {
    let session = st.session.clone();
    let image = blocking(move || {
        let frame = if q.full {
            session.full_frame(index, q.view)?
        } else {
            let v = session
                .view(index, q.view, Duration::from_secs(30), &|| false)?
                .ok_or_else(|| {
                    ApiError(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "frame not available".into(),
                    )
                })?;
            (*v.frame).clone()
        };
        Ok(png(&frame))
    })
    .await?;
    Ok(([(header::CONTENT_TYPE, "image/png")], image).into_response())
}

#[derive(Deserialize, Default)]
pub struct DialogBody {
    #[serde(default)]
    suggested: String,
}

pub async fn dialog(
    State(st): State<Shared>,
    Path(kind): Path<String>,
    body: Option<Json<DialogBody>>,
) -> ApiResult<Value> {
    let dialogs = st.config.dialogs.clone().ok_or_else(|| {
        ApiError(
            StatusCode::NOT_IMPLEMENTED,
            "no native dialogs in server mode".into(),
        )
    })?;
    let suggested = body.map(|b| b.0.suggested).unwrap_or_default();
    let path = blocking(move || {
        Ok(match kind.as_str() {
            "open-video" => dialogs.open_video(),
            "save-video" => dialogs.save_video(&suggested),
            "open-preset" => dialogs.open_preset(),
            "save-preset" => dialogs.save_preset(),
            _ => {
                return Err(ApiError(
                    StatusCode::NOT_FOUND,
                    format!("unknown dialog {kind}"),
                ));
            }
        })
    })
    .await?;
    Ok(Json(json!({ "path": path })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_patch_semantics() {
        let mut v = json!({"a": {"b": 1, "c": 2}, "crf": 5, "list": [1]});
        merge_patch(
            &mut v,
            &json!({"a": {"b": 3}, "crf": null, "list": [2, 3], "new": true}),
        );
        assert_eq!(
            v,
            json!({"a": {"b": 3, "c": 2}, "crf": null, "list": [2, 3], "new": true})
        );
    }
}
