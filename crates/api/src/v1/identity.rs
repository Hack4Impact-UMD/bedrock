use axum::{
    Extension, Json, Router,
    extract::State,
    http::StatusCode,
    routing::{delete, get},
};
use identity::{ApiKeyResponse, Project};

use crate::{AppState, middleware::AuthExtension};

async fn get_project(
    State(state): State<AppState>,
    Extension((project_id, _)): Extension<AuthExtension>,
) -> Result<Json<Project>, StatusCode> {
    let project = state
        .identity
        .get_project(project_id)
        .await
        .map_err(|err| match err {
            identity::IdentityError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::NOT_FOUND,
        })?;

    Ok(Json(project))
}

async fn revoke_key(
    State(state): State<AppState>,
    Extension((project_id, key_id)): Extension<AuthExtension>,
) -> Result<Json<ApiKeyResponse>, StatusCode> {
    let revoked = state
        .identity
        .revoke_api_key(project_id, key_id)
        .await
        .map_err(|err| match err {
            identity::IdentityError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::NOT_FOUND,
        })?;

    Ok(Json(ApiKeyResponse::from(revoked)))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/project", get(get_project))
        .route("/revoke", delete(revoke_key))
}
