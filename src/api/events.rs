use std::{convert::Infallible, sync::Arc, time::Duration};

use axum::{
    extract::{OriginalUri, Path, Query, State, rejection::PathRejection},
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse,
        sse::{Event, KeepAlive, Sse},
    },
};
use futures_util::{StreamExt, stream};
use serde::Deserialize;

use super::HttpError;
use crate::run_manager::RunManager;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EventQuery {
    after: Option<String>,
}

pub(super) async fn get(
    State(manager): State<Arc<RunManager>>,
    path: Result<Path<String>, PathRejection>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
) -> Result<impl IntoResponse, HttpError> {
    let Path(run_id) = path.map_err(|_| HttpError::missing())?;
    let after = if let Some(last) = headers.get("last-event-id") {
        let (_, sequence) = last
            .to_str()
            .ok()
            .and_then(|last| last.rsplit_once(':'))
            .filter(|(id, _)| *id == run_id)
            .ok_or_else(invalid_cursor)?;
        Some(parse_sequence(sequence)?)
    } else {
        let Query(query) = Query::<EventQuery>::try_from_uri(&uri).map_err(|_| invalid_cursor())?;
        query.after.as_deref().map(parse_sequence).transpose()?
    };
    let subscription = manager
        .subscribe(run_id, after)
        .ok_or_else(HttpError::missing)?;
    let frames = stream::unfold(subscription, |mut subscription| async move {
        let frame = subscription.next().await?;
        let event = Event::default()
            .id(&frame.id)
            .event(frame.name)
            .data(&frame.json);
        Some((Ok::<_, Infallible>(event), subscription))
    });
    // 通知のないrunでもヘッダーをすぐ転送できるよう、最初のコメントを送る。
    let connected =
        stream::once(async { Ok::<_, Infallible>(Event::default().comment("connected")) });
    Ok(Sse::new(connected.chain(frames))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

fn parse_sequence(value: &str) -> Result<u64, HttpError> {
    value
        .parse::<u64>()
        .ok()
        .filter(|number| number.to_string() == value)
        .ok_or_else(invalid_cursor)
}

fn invalid_cursor() -> HttpError {
    HttpError::new(
        StatusCode::BAD_REQUEST,
        "invalid_cursor",
        "Expected a valid event sequence for this run.",
        false,
    )
}
