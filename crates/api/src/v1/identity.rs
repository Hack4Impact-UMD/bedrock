use axum::{
    Extension, Json, Router,
    body::Body,
    extract::{Request, State},
    http::StatusCode,
    response::Response,
    routing::get,
};
use identity::{Project, ProjectId};

use crate::AppState;

async fn get_project(
    State(state): State<AppState>,
    Extension(project_id): Extension<ProjectId>,
) -> Result<Json<Project>, StatusCode> {
    let project = state
        .identity
        .get_project(project_id)
        .await
        .map_err(|err| match err {
            identity::IdentityError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::NOT_FOUND,
        })?;

    return Ok(Json(project));
}

pub fn router() -> Router<AppState> {
    Router::new().route("/hi", get(|| async { "hello from identity" }))
}
