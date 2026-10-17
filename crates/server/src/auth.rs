//! Request authorisation: `Host`/`Origin` checks for loopback servers and the session token.

use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};

use crate::Shared;

pub const COOKIE: &str = "mx_token";
pub const HEADER: &str = "x-mx-token";

fn host_allowed(allowed: &[String], host: &str) -> bool {
    allowed.iter().any(|h| h.eq_ignore_ascii_case(host))
}

fn cookie_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|c| c.trim().strip_prefix(COOKIE)?.strip_prefix('='))
}

fn query_token(query: &str) -> Option<&str> {
    query.split('&').find_map(|kv| kv.strip_prefix("token="))
}

fn query_without_token(query: &str) -> String {
    query
        .split('&')
        .filter(|kv| !kv.starts_with("token=") && !kv.is_empty())
        .collect::<Vec<_>>()
        .join("&")
}

fn set_cookie(token: &str) -> String {
    format!("{COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict")
}

pub async fn check(State(state): State<Shared>, req: Request, next: Next) -> Response {
    let headers = req.headers();
    if let Some(allowed) = &state.allowed_hosts {
        // DNS rebinding protection: the page must have been loaded from our own address.
        let host = headers
            .get(header::HOST)
            .and_then(|h| h.to_str().ok())
            .unwrap_or("");
        if !host_allowed(allowed, host) {
            return (StatusCode::FORBIDDEN, "invalid host").into_response();
        }
        // Cross-site requests (and WebSocket handshakes) carry a foreign Origin.
        if let Some(origin) = headers.get(header::ORIGIN).and_then(|o| o.to_str().ok()) {
            let origin_host = origin.split("://").nth(1).unwrap_or("");
            if !host_allowed(allowed, origin_host) {
                return (StatusCode::FORBIDDEN, "invalid origin").into_response();
            }
        }
    }
    let Some(token) = state.config.token.as_deref() else {
        return next.run(req).await;
    };
    let header_ok = headers.get(HEADER).and_then(|v| v.to_str().ok()) == Some(token);
    if header_ok || cookie_token(headers) == Some(token) {
        return next.run(req).await;
    }
    let query = req.uri().query().unwrap_or("");
    if query_token(query) == Some(token) {
        // First load with ?token=…: store it in a cookie and drop it from the URL.
        let path = req.uri().path();
        let rest = query_without_token(query);
        let location = if rest.is_empty() {
            path.to_owned()
        } else {
            format!("{path}?{rest}")
        };
        let mut resp = Redirect::to(&location).into_response();
        resp.headers_mut().insert(
            header::SET_COOKIE,
            set_cookie(token).parse().expect("valid header"),
        );
        return resp;
    }
    (StatusCode::UNAUTHORIZED, "missing or invalid token").into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cookie_and_query() {
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, "a=1; mx_token=abc; b=2".parse().unwrap());
        assert_eq!(cookie_token(&h), Some("abc"));
        assert_eq!(query_token("x=1&token=t0k"), Some("t0k"));
        assert_eq!(query_without_token("x=1&token=t0k&y=2"), "x=1&y=2");
        assert_eq!(query_without_token("token=t0k"), "");
        assert!(host_allowed(&["127.0.0.1:80".into()], "127.0.0.1:80"));
        assert!(!host_allowed(&["127.0.0.1:80".into()], "evil.com"));
    }
}
