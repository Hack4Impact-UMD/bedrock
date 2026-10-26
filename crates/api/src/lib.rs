use axum::{Json, Router, routing::get};
use identity::PgIdentityService;
use serde::Serialize;
mod middleware;
mod v1;

#[derive(Clone)]
pub struct AppState {
    pub identity: PgIdentityService,
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/health",
            get(|| async {
                Json(HealthResponse {
                    status: String::from("OK"),
                })
            }),
        )
        .nest("/v1", v1::router(state.clone()))
        .with_state(state)
}
