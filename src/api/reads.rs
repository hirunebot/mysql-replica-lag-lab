use std::sync::Arc;

use axum::{
    Json,
    extract::{
        Path, State,
        rejection::{JsonRejection, PathRejection},
    },
    http::StatusCode,
};
use serde::Deserialize;

use super::HttpError;
use crate::{
    contracts::{ManualRead, ReadPreset},
    run_manager::{ReadError, RunManager},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReadRequest {
    preset: ReadPreset,
}

pub(super) async fn post(
    State(manager): State<Arc<RunManager>>,
    path: Result<Path<String>, PathRejection>,
    body: Result<Json<ReadRequest>, JsonRejection>,
) -> Result<Json<ManualRead>, HttpError> {
    let Path(id) = path.map_err(|_| HttpError::missing())?;
    let Json(ReadRequest {
        preset: ReadPreset::MarkerSum,
    }) = body.map_err(|_| {
        HttpError::new(
            StatusCode::BAD_REQUEST,
            "invalid_read",
            "Expected only the marker_sum preset.",
            false,
        )
    })?;
    manager.read_marker_sum(&id).await.map(Json).map_err(|e| {
        let status = match e {
            ReadError::Missing => StatusCode::NOT_FOUND,
            ReadError::NotReady | ReadError::Busy | ReadError::Stale => StatusCode::CONFLICT,
            ReadError::DatabaseUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            ReadError::Timeout => StatusCode::GATEWAY_TIMEOUT,
        };
        HttpError(status, e.api_error())
    })
}
