use axum::{Json, Router};
use courier::PgCourierService;
use identity::PgIdentityService;
use serde::Serialize;
use utoipa::{
    Modify, OpenApi, ToSchema,
    openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme},
};
use utoipa_axum::{router::OpenApiRouter, routes};
use utoipa_swagger_ui::SwaggerUi;
mod error;
mod middleware;
mod v1;

#[derive(Clone)]
pub struct AppState {
    pub identity: PgIdentityService,
    pub courier: PgCourierService,
}

#[derive(OpenApi)]
#[openapi(
    info(title = "bedrock", description = "Shared foundational services for Hack4Impact-UMD projects."),
    modifiers(&SecurityAddon),
    tags(
        (name = "health", description = "Service health"),
        (name = "identity", description = "Projects and API keys"),
        (name = "courier", description = "Sending domains and transactional email"),
    ),
)]
struct ApiDoc;

/// Registers the bearer API key scheme that the `/v1` routes require.
struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "api_key",
            SecurityScheme::Http(HttpBuilder::new().scheme(HttpAuthScheme::Bearer).build()),
        );
    }
}

#[derive(Serialize, ToSchema)]
struct HealthResponse {
    status: String,
}

#[utoipa::path(
    get,
    path = "/health",
    tag = "health",
    responses((status = 200, body = HealthResponse)),
)]
async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: String::from("OK"),
    })
}

pub fn router(state: AppState) -> Router {
    let (router, api) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(health))
        .nest("/v1", v1::router(state.clone()))
        .split_for_parts();

    router
        .merge(SwaggerUi::new("/").url("/openapi.json", api))
        .with_state(state)
}
