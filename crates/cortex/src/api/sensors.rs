//! Physical sensor routes.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};

use crate::api::error;
use crate::state::SharedState;
use crate::types::{ApiError, SensorSample};

type ApiResult<T> = Result<Json<T>, (StatusCode, Json<ApiError>)>;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/sensors", get(list))
        .route("/sensors/:sensor_id", get(get_one))
}

async fn list(State(state): State<SharedState>) -> Json<Vec<SensorSample>> {
    Json(crate::sensors::latest(&state))
}

async fn get_one(
    State(state): State<SharedState>,
    Path(sensor_id): Path<String>,
) -> ApiResult<SensorSample> {
    crate::sensors::latest_one(&state, &sensor_id)
        .map(Json)
        .ok_or_else(|| {
            error::err(
                StatusCode::NOT_FOUND,
                "sensor_not_found",
                Some(format!("unknown sensor {sensor_id}")),
            )
        })
}
