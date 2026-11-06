//! HTTPの公開境界。要求・応答にはステップ1の型をそのまま使う。
use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit, Path, State,
        rejection::{JsonRejection, PathRejection},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};

use crate::{
    config::Config,
    contracts::{ApiError, DashboardState, RunConfig, RunSnapshot},
    run_manager::{RunManager, StartError},
};

mod reads;

/// DBに接続せずHTTPルーターを構成する。到達確認は状態取得・実行時に行う。
pub fn router(config: Config) -> anyhow::Result<Router> {
    Ok(router_with(RunManager::new(config)?))
}

fn router_with(manager: Arc<RunManager>) -> Router {
    Router::new()
        .route("/api/state", get(state))
        .route("/api/runs", post(start).layer(DefaultBodyLimit::max(4096)))
        .route("/api/runs/{id}", get(run))
        .route(
            "/api/runs/{id}/reads",
            post(reads::post).layer(DefaultBodyLimit::max(256)),
        )
        .fallback(|| async { HttpError::missing() })
        .with_state(manager)
}

async fn state(State(manager): State<Arc<RunManager>>) -> Json<DashboardState> {
    Json(manager.state().await)
}

async fn start(
    State(manager): State<Arc<RunManager>>,
    input: Result<Json<RunConfig>, JsonRejection>,
) -> Result<(StatusCode, Json<RunSnapshot>), HttpError> {
    let Json(config) = input
        .map_err(|_| HttpError::invalid("Expected JSON with only the four workload settings."))?;
    let snapshot = manager.start_run(config).map_err(|e| match e {
        StartError::Invalid(message) => HttpError::invalid(&message),
        StartError::Busy => HttpError::new(
            StatusCode::CONFLICT,
            "run_in_progress",
            "A run or manual read is active, or database work has not been confirmed stopped.",
            true,
        ),
    })?;
    Ok((StatusCode::ACCEPTED, Json(snapshot)))
}

async fn run(
    State(manager): State<Arc<RunManager>>,
    path: Result<Path<String>, PathRejection>,
) -> Result<Json<RunSnapshot>, HttpError> {
    let Path(id) = path.map_err(|_| HttpError::missing())?;
    manager.run(&id).map(Json).ok_or_else(HttpError::missing)
}

struct HttpError(StatusCode, ApiError);
impl HttpError {
    fn new(status: StatusCode, code: &str, message: &str, retryable: bool) -> Self {
        Self(
            status,
            ApiError {
                code: code.into(),
                message: message.into(),
                retryable,
            },
        )
    }
    fn invalid(message: &str) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_config", message, false)
    }
    fn missing() -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "run_not_found",
            "No matching run or endpoint was found.",
            false,
        )
    }
}
impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        (self.0, Json(self.1)).into_response()
    }
}

#[cfg(test)]
mod tests;
