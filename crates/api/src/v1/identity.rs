use axum::{
    Extension, Json, Router,
    extract::State,
    routing::{delete, get},
};
use identity::{ApiKey, Project};

use crate::{AppState, error::ApiError, middleware::AuthContext};

async fn get_project(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
) -> Result<Json<Project>, ApiError> {
    let project = state.identity.get_project(auth.project).await?;

    Ok(Json(project))
}

async fn revoke_key(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
) -> Result<Json<ApiKey>, ApiError> {
    let revoked = state
        .identity
        .revoke_api_key(auth.project, auth.key)
        .await?;

    Ok(Json(revoked))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/project", get(get_project))
        .route("/revoke", delete(revoke_key))
}
