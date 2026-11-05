//! Local HTTP server for the OBS browser source.
//!
//! - `GET /`        overlay page (transparent background)
//! - `GET /api`     current state as JSON
//! - `GET /events`  the same JSON pushed as Server-Sent Events on every change

use std::convert::Infallible;
use std::net::SocketAddr;

use axum::Router;
use axum::extract::State as AxState;
use axum::http::header;
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
    let app = Router::new()
        .route("/", get(overlay))
        .route("/api", get(api))
        .route("/events", get(events))
        .with_state(rx);
    axum::serve(listener, app).await
}

async fn overlay() -> impl IntoResponse {
    ([(header::CACHE_CONTROL, "no-store")], axum::response::Html(OVERLAY_HTML))
}

async fn api(AxState(rx): AxState<Rx>) -> impl IntoResponse {
    let body = rx.borrow().to_json();
    ([(header::CACHE_CONTROL, "no-store")], Json(body))
}

async fn events(AxState(rx): AxState<Rx>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = WatchStream::new(rx).map(|state| Ok(Event::default().data(state.to_json().to_string())));
    Sse::new(stream).keep_alive(KeepAlive::default())
}
