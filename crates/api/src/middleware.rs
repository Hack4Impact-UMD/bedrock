use axum::{
    extract::{Request, State},
    http,
    middleware::Next,
    response::Response,
};
use identity::{ApiKeyId, IdentityError, ProjectId};

use crate::{AppState, error::ApiError};

#[derive(Clone, Copy)]
pub struct AuthContext {
    pub project: ProjectId,
    pub key: ApiKeyId,
}

pub async fn api_key_auth(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let key = req
        .headers()
        .get(http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .ok_or(IdentityError::InvalidKey)?;

    let (project, key) = state.identity.authenticate_api_key(key).await?;

    req.extensions_mut().insert(AuthContext { project, key });
    Ok(next.run(req).await)
}
