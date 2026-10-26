use axum::{Router, middleware::from_fn_with_state};

use crate::{AppState, middleware};

mod identity;

pub fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .nest("/identity", identity::router())
        .route_layer(from_fn_with_state(state.clone(), middleware::api_key_auth))
}
