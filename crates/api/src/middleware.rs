use axum::{
    extract::{Request, State},
    http::{self, StatusCode},
    middleware::Next,
    response::Response,
};
use identity::{ApiKeyId, IdentityError, ProjectId};

use crate::AppState;

pub type AuthExtension = (ProjectId, ApiKeyId);

pub async fn api_key_auth(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let key = req
        .headers()
        .get(http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let auth = state
        .identity
        .authenticate_api_key(key)
        .await
        .map_err(|err| match err {
            IdentityError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::UNAUTHORIZED,
        })?;

    req.extensions_mut().insert(auth);
    Ok(next.run(req).await)
}
