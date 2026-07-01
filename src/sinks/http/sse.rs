use crate::sinks::http::api::state_json;
use crate::sinks::http::AppState;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::stream::Stream;
use std::convert::Infallible;
use std::time::Duration;

/// Emit the current state immediately, then on every hub broadcast, plus a
/// periodic tick so the UI clock/sun position refreshes even without provider updates.
pub async fn stream_handler(
    State(s): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = async_stream::stream! {
        let mut rx = s.hub.subscribe();
        let v = state_json(&s.hub, &s.cfg).await;
        yield Ok(Event::default().event("state").data(v.to_string()));
        let mut ticker = tokio::time::interval(Duration::from_secs(5));
        loop {
            tokio::select! {
                _ = rx.recv() => {},
                _ = ticker.tick() => {},
            }
            let v = state_json(&s.hub, &s.cfg).await;
            yield Ok(Event::default().event("state").data(v.to_string()));
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}
