//! Local HTTP server for OBS browser sources.
//!
//! - `GET /widget/bpm`, `/widget/chart`, `/widget/stats` — overlay widgets
//!   (transparent background, styled from the app's Overlay settings); `/` is the BPM widget
//! - `GET /api`     current state as JSON (`?history=1` adds the samples)
//! - `GET /events`  the same JSON pushed as Server-Sent Events on every change

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::extract::{Path, Query, Request, State as AxState};
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Json, Response};
use axum::routing::get;
use futures_lite::Stream;
use serde::Deserialize;
use tokio_stream::StreamExt as _;
use tokio_stream::wrappers::WatchStream;

use crate::state::Rx;

const WIDGET_HTML: &str = include_str!("../web/widget.html");

/// Widget kinds, as used in `/widget/<kind>`.
pub const WIDGETS: [&str; 3] = ["bpm", "chart", "stats"];

pub async fn bind(addr: SocketAddr) -> std::io::Result<tokio::net::TcpListener> {
    tokio::net::TcpListener::bind(addr).await
}

pub async fn serve(listener: tokio::net::TcpListener, rx: Rx) -> std::io::Result<()> {
    let port = listener.local_addr()?.port();
    axum::serve(listener, router(rx, port)).await
}

fn router(rx: Rx, port: u16) -> Router {
    let allowed: Arc<[String]> = [format!("127.0.0.1:{port}"), format!("localhost:{port}")].into();
    Router::new()
        .route("/", get(|| widget(Path("bpm".to_owned()))))
        .route("/widget/{kind}", get(widget))
        .route("/api", get(api))
        .route("/events", get(events))
        .with_state(rx)
        .layer(middleware::from_fn(move |req: Request, next: Next| {
            let allowed = allowed.clone();
            async move {
                if host_allowed(&req, &allowed) {
                    next.run(req).await
                } else {
                    StatusCode::FORBIDDEN.into_response()
                }
            }
        }))
}

/// Rejects DNS-rebinding: a page on `evil.example` that resolves to 127.0.0.1 still
/// sends `Host: evil.example`, so only requests addressed to loopback by name get through.
fn host_allowed(req: &Request, allowed: &[String]) -> bool {
    req.headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|host| allowed.iter().any(|a| a.eq_ignore_ascii_case(host)))
}

async fn widget(Path(kind): Path<String>) -> Response {
    if !WIDGETS.contains(&kind.as_str()) {
        return StatusCode::NOT_FOUND.into_response();
    }
    ([(header::CACHE_CONTROL, "no-store")], Html(WIDGET_HTML)).into_response()
}

#[derive(Deserialize, Default)]
struct Opts {
    #[serde(default)]
    history: Option<String>,
}

impl Opts {
    fn history(&self) -> bool {
        matches!(self.history.as_deref(), Some("1" | "true"))
    }
}

async fn api(AxState(rx): AxState<Rx>, Query(opts): Query<Opts>) -> impl IntoResponse {
    let body = rx.borrow().to_json(opts.history());
    ([(header::CACHE_CONTROL, "no-store")], Json(body))
}

async fn events(
    AxState(rx): AxState<Rx>,
    Query(opts): Query<Opts>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let history = opts.history();
    let mut last = String::new();
    // The state also changes for things widgets don't show (e.g. the nearby list);
    // skip events whose payload is identical to the previous one.
    let stream = WatchStream::new(rx).filter_map(move |state| {
        let data = state.to_json(history).to_string();
        if data == last {
            return None;
        }
        last.clone_from(&data);
        Some(Ok(Event::default().data(data)))
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;
    use axum::body::Body;
    use tower::ServiceExt;

    async fn get(path: &str, host: Option<&str>) -> (StatusCode, String) {
        let (_tx, rx) = crate::state::channel(Settings::default());
        let mut req = Request::builder().uri(path);
        if let Some(h) = host {
            req = req.header(header::HOST, h);
        }
        let res = router(rx, 3030)
            .oneshot(req.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let body = axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    #[tokio::test]
    async fn widgets_and_api() {
        for path in ["/", "/widget/bpm", "/widget/chart", "/widget/stats"] {
            let (status, body) = get(path, Some("127.0.0.1:3030")).await;
            assert_eq!(status, StatusCode::OK, "{path}");
            assert!(body.contains("<html"), "{path}");
        }
        assert_eq!(
            get("/widget/nope", Some("localhost:3030")).await.0,
            StatusCode::NOT_FOUND
        );

        let (status, body) = get("/api?history=1", Some("localhost:3030")).await;
        assert_eq!(status, StatusCode::OK);
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(json["history"].is_array());
        assert_eq!(json["widgets"]["chart"]["minutes"], 5);
    }

    #[tokio::test]
    async fn rejects_foreign_hosts() {
        for host in [
            None,
            Some("evil.example:3030"),
            Some("127.0.0.1:3030.evil.example"),
        ] {
            assert_eq!(get("/api", host).await.0, StatusCode::FORBIDDEN, "{host:?}");
        }
        assert_eq!(get("/api", Some("LOCALHOST:3030")).await.0, StatusCode::OK);
    }
}
