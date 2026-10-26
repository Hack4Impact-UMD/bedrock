use axum::{
    extract::{Request, State},
    http::{self, StatusCode},
    middleware::Next,
    response::Response,
};
use identity::IdentityError;

use crate::AppState;

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

    let project = state
        .identity
        .authenticate_api_key(key)
        .await
        .map_err(|err| match err {
            IdentityError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::UNAUTHORIZED,
        })?;

    req.extensions_mut().insert(project);
    Ok(next.run(req).await)
}
