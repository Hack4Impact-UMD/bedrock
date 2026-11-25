use axum::middleware::from_fn_with_state;
use utoipa_axum::router::OpenApiRouter;

use crate::{AppState, middleware};

mod courier;
mod identity;

pub fn router(state: AppState) -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .nest("/identity", identity::router())
        .nest("/courier", courier::router())
        .route_layer(from_fn_with_state(state.clone(), middleware::api_key_auth))
}
