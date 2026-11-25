use axum::{Extension, Json, extract::State};
use identity::{ApiKey, Project};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{
    AppState,
    error::{ApiError, ErrorBody},
    middleware::AuthContext,
};

const TAG: &str = "identity";

/// Get the project that owns the API key used in this request.
#[utoipa::path(
    get,
    path = "/project",
    tag = TAG,
    security(("api_key" = [])),
    responses(
        (status = 200, body = Project),
        (status = 401, body = ErrorBody),
    ),
)]
async fn get_project(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
) -> Result<Json<Project>, ApiError> {
    let project = state.identity.get_project(auth.project).await?;

    Ok(Json(project))
}

/// Revoke the API key used to make this request.
#[utoipa::path(
    delete,
    path = "/revoke",
    tag = TAG,
    security(("api_key" = [])),
    responses(
        (status = 200, description = "The revoked key", body = ApiKey),
        (status = 401, body = ErrorBody),
    ),
)]
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

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(get_project))
        .routes(routes!(revoke_key))
}
