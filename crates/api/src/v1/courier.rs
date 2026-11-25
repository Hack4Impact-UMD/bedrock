use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use courier::{
    CourierError, Domain, DomainCheck, DomainOwner, Email, EmailId, OutgoingEmail, SendMode,
};
use email_address::EmailAddress;
use identity::ProjectId;
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    AppState,
    error::{ApiError, ErrorBody},
    middleware::AuthContext,
};

const TAG: &str = "courier";

const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 100;

/// Message content. Supply HTML, plain text, or both.
#[derive(Deserialize, Debug, ToSchema)]
#[serde(rename_all = "snake_case")]
enum EmailBody {
    Html(String),
    Text(String),
    Both { html: String, text: String },
}

#[derive(Deserialize, ToSchema)]
struct SendRequest {
    /// Sender address. Its domain must be one the project can send from.
    pub from: String,
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    #[serde(default)]
    pub bcc: Vec<String>,
    pub subject: String,
    pub body: EmailBody,
    /// Defaults to `live`. `test` records the email without delivering it.
    #[serde(default)]
    pub mode: Option<SendMode>,
}

#[derive(Deserialize, ToSchema)]
struct CreateDomainRequest {
    pub domain: String,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
struct Page {
    /// Page size, from 1 to 100. Defaults to 50.
    pub limit: Option<i64>,
    /// Return emails older than the email with this id.
    pub before: Option<Uuid>,
}

impl Page {
    fn limit(&self) -> i64 {
        self.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
    }

    fn before(&self) -> Option<EmailId> {
        self.before.map(EmailId)
    }
}

fn owned_by(domain: &Domain, project: ProjectId) -> bool {
    domain.owner() == DomainOwner::Project(project)
}

fn visible_to(domain: &Domain, project: ProjectId) -> bool {
    domain.owner() == DomainOwner::Shared || owned_by(domain, project)
}

/// List the domains this project can send from: its own, then shared ones.
#[utoipa::path(
    get,
    path = "/domains",
    tag = TAG,
    security(("api_key" = [])),
    responses(
        (status = 200, body = Vec<Domain>),
        (status = 401, body = ErrorBody),
    ),
)]
async fn get_domains(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
) -> Result<Json<Vec<Domain>>, ApiError> {
    let domains = state
        .courier
        .get_sender_identities_for_project(auth.project)
        .await?;

    Ok(Json(domains))
}

/// Register a sending domain for this project.
///
/// The response includes the DNS records to publish before the domain can be verified.
#[utoipa::path(
    post,
    path = "/domains",
    tag = TAG,
    security(("api_key" = [])),
    request_body = CreateDomainRequest,
    responses(
        (status = 201, body = Domain),
        (status = 401, body = ErrorBody),
        (status = 409, description = "Domain already registered", body = ErrorBody),
        (status = 422, description = "Invalid domain", body = ErrorBody),
    ),
)]
async fn create_domain(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(payload): Json<CreateDomainRequest>,
) -> Result<(StatusCode, Json<Domain>), ApiError> {
    let domain = state
        .courier
        .create_sender_identity(DomainOwner::Project(auth.project), &payload.domain)
        .await?;

    Ok((StatusCode::CREATED, Json(domain)))
}

/// Get a domain this project can send from.
#[utoipa::path(
    get,
    path = "/domains/{domain}",
    tag = TAG,
    security(("api_key" = [])),
    params(("domain" = String, Path, description = "Domain name, e.g. `mail.example.org`")),
    responses(
        (status = 200, body = Domain),
        (status = 401, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
)]
async fn get_domain(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(domain): Path<String>,
) -> Result<Json<Domain>, ApiError> {
    let domain = state.courier.get_sender_identity(&domain).await?;
    if !visible_to(&domain, auth.project) {
        return Err(CourierError::NotFound.into());
    }

    Ok(Json(domain))
}

/// Check a domain's verification status with the mail provider.
#[utoipa::path(
    get,
    path = "/domains/{domain}/check",
    tag = TAG,
    security(("api_key" = [])),
    params(("domain" = String, Path, description = "Domain name, e.g. `mail.example.org`")),
    responses(
        (status = 200, body = DomainCheck),
        (status = 401, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
)]
async fn check_domain(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(domain): Path<String>,
) -> Result<Json<DomainCheck>, ApiError> {
    let domain = state.courier.get_sender_identity(&domain).await?;
    if !visible_to(&domain, auth.project) {
        return Err(CourierError::NotFound.into());
    }
    let check = state.courier.check_domain_status(&domain.domain).await?;

    Ok(Json(check))
}

/// Delete a domain owned by this project.
#[utoipa::path(
    delete,
    path = "/domains/{domain}",
    tag = TAG,
    security(("api_key" = [])),
    params(("domain" = String, Path, description = "Domain name, e.g. `mail.example.org`")),
    responses(
        (status = 204, description = "Domain deleted"),
        (status = 401, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
)]
async fn delete_domain(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(domain): Path<String>,
) -> Result<StatusCode, ApiError> {
    let identity = state.courier.get_sender_identity(&domain).await?;
    if !owned_by(&identity, auth.project) {
        return Err(CourierError::NotFound.into());
    }

    let deleted = state
        .courier
        .delete_sender_identity(DomainOwner::Project(auth.project), &domain)
        .await?;

    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(CourierError::NotFound.into())
    }
}

/// List emails sent from a domain owned by this project, newest first.
#[utoipa::path(
    get,
    path = "/domains/{domain}/mail",
    tag = TAG,
    security(("api_key" = [])),
    params(("domain" = String, Path, description = "Domain name, e.g. `mail.example.org`"), Page),
    responses(
        (status = 200, body = Vec<Email>),
        (status = 401, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
)]
async fn list_domain_mail(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(domain): Path<String>,
    Query(page): Query<Page>,
) -> Result<Json<Vec<Email>>, ApiError> {
    let domain = state.courier.get_sender_identity(&domain).await?;
    if !owned_by(&domain, auth.project) {
        return Err(CourierError::NotFound.into());
    }

    let emails = state
        .courier
        .list_email_from_domain_id(domain.into(), page.limit(), page.before())
        .await?;

    Ok(Json(emails))
}

/// List emails sent by this project, newest first.
#[utoipa::path(
    get,
    path = "/mail",
    tag = TAG,
    security(("api_key" = [])),
    params(Page),
    responses(
        (status = 200, body = Vec<Email>),
        (status = 401, body = ErrorBody),
    ),
)]
async fn list_mail(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(page): Query<Page>,
) -> Result<Json<Vec<Email>>, ApiError> {
    let emails = state
        .courier
        .list_email(auth.project, page.limit(), page.before())
        .await?;

    Ok(Json(emails))
}

/// Get an email sent by this project.
#[utoipa::path(
    get,
    path = "/mail/{id}",
    tag = TAG,
    security(("api_key" = [])),
    params(("id" = Uuid, Path, description = "Email id")),
    responses(
        (status = 200, body = Email),
        (status = 401, body = ErrorBody),
        (status = 404, body = ErrorBody),
    ),
)]
async fn get_mail(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<Uuid>,
) -> Result<Json<Email>, ApiError> {
    let email = state.courier.get_email(EmailId(id)).await?;
    if email.project_id != auth.project.0 {
        return Err(CourierError::NotFound.into());
    }

    Ok(Json(email))
}

/// Send an email.
///
/// Returns the recorded email; check its `status` to see whether the provider accepted it.
#[utoipa::path(
    post,
    path = "/mail/send",
    tag = TAG,
    security(("api_key" = [])),
    request_body = SendRequest,
    responses(
        (status = 200, body = Email),
        (status = 400, description = "Invalid address", body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 404, description = "Sender domain not found", body = ErrorBody),
        (status = 422, description = "Rejected by the mail provider", body = ErrorBody),
    ),
)]
async fn send_mail(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(payload): Json<SendRequest>,
) -> Result<Json<Email>, ApiError> {
    let body = match &payload.body {
        EmailBody::Html(t) => courier::EmailBody::Html(t),
        EmailBody::Text(t) => courier::EmailBody::Text(t),
        EmailBody::Both { html, text } => courier::EmailBody::Both { html, text },
    };

    let from = payload
        .from
        .parse::<EmailAddress>()
        .map_err(CourierError::from)?;
    let domain = state.courier.get_sender_identity(from.domain()).await?;
    if !visible_to(&domain, auth.project) {
        return Err(CourierError::NotFound.into());
    }

    let email = state
        .courier
        .send_email(
            auth.project,
            &OutgoingEmail {
                from: &payload.from,
                to: &payload.to,
                cc: &payload.cc,
                bcc: &payload.bcc,
                subject: &payload.subject,
                body,
            },
            payload.mode.unwrap_or(SendMode::Live),
        )
        .await?;

    Ok(Json(email))
}

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(get_domains, create_domain))
        .routes(routes!(get_domain, delete_domain))
        .routes(routes!(check_domain))
        .routes(routes!(list_domain_mail))
        .routes(routes!(list_mail))
        .routes(routes!(send_mail))
        .routes(routes!(get_mail))
}
