//! The request gate: loopback `Host` allow-list, per-launch token (first visit via `?token=`, then an
//! HttpOnly cookie), and the WebSocket `Origin` check, so other local users and other websites cannot
//! drive the daemon through this server.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};

use crate::AppState;

pub(crate) const COOKIE_NAME: &str = "bh_web_token";

fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn cookie_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find_map(|(name, value)| (name == COOKIE_NAME).then_some(value))
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

fn query_token(query: Option<&str>) -> Option<&str> {
    query?
        .split('&')
        .find_map(|pair| pair.strip_prefix("token="))
}

pub(crate) async fn gate(State(state): State<Arc<AppState>>, request: Request, next: Next) -> Response {
    let headers = request.headers();
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
    if !host.is_some_and(|host| state.options.allowed_hosts.iter().any(|allowed| allowed == host)) {
        return (StatusCode::FORBIDDEN, "unexpected Host header").into_response();
    }
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        let origin_host = origin.split_once("://").map_or(origin, |(_, rest)| rest);
        if Some(origin_host) != host {
            return (StatusCode::FORBIDDEN, "cross-origin request refused").into_response();
        }
    }
    let token = &state.options.token;
    if request.uri().path() == "/" && query_token(request.uri().query()).is_some_and(|t| constant_time_eq(t, token)) {
        let cookie = format!("{COOKIE_NAME}={token}; HttpOnly; SameSite=Strict; Path=/");
        return ([(header::SET_COOKIE, cookie)], Redirect::to("/")).into_response();
    }
    let presented = cookie_token(headers).or_else(|| bearer_token(headers));
    if presented.is_some_and(|t| constant_time_eq(t, token)) {
        return next.run(request).await;
    }
    (
        StatusCode::UNAUTHORIZED,
        "Unauthorized. Open the URL printed by `beehive web` (it carries the access token).",
    )
        .into_response()
}
