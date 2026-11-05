//! Local HTTP server for the OBS browser source.
//!
//! - `GET /`        overlay page (transparent background)
//! - `GET /api`     current state as JSON
//! - `GET /events`  the same JSON pushed as Server-Sent Events on every change

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::extract::{Request, State as AxState};
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Json};
use axum::routing::get;
use futures_lite::Stream;
use tokio_stream::StreamExt as _;
use tokio_stream::wrappers::WatchStream;

use crate::state::Rx;

const OVERLAY_HTML: &str = include_str!("../web/overlay.html");

pub async fn bind(addr: SocketAddr) -> std::io::Result<tokio::net::TcpListener> {
    tokio::net::TcpListener::bind(addr).await
}

pub async fn serve(listener: tokio::net::TcpListener, rx: Rx) -> std::io::Result<()> {
    let port = listener.local_addr()?.port();
    let allowed: Arc<[String]> = [format!("127.0.0.1:{port}"), format!("localhost:{port}")].into();

    let app = Router::new()
        .route("/", get(overlay))
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
        }));
    axum::serve(listener, app).await
}

/// Rejects DNS-rebinding: a page on `evil.example` that resolves to 127.0.0.1 still
/// sends `Host: evil.example`, so only requests addressed to loopback by name get through.
fn host_allowed(req: &Request, allowed: &[String]) -> bool {
    req.headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|host| allowed.iter().any(|a| a.eq_ignore_ascii_case(host)))
}

async fn overlay() -> impl IntoResponse {
    (
        [(header::CACHE_CONTROL, "no-store")],
        axum::response::Html(OVERLAY_HTML),
    )
}

async fn api(AxState(rx): AxState<Rx>) -> impl IntoResponse {
    let body = rx.borrow().to_json();
    ([(header::CACHE_CONTROL, "no-store")], Json(body))
}

async fn events(AxState(rx): AxState<Rx>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream =
        WatchStream::new(rx).map(|state| Ok(Event::default().data(state.to_json().to_string())));
    Sse::new(stream).keep_alive(KeepAlive::default())
}
