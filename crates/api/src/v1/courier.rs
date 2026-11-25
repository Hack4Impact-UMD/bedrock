use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use courier::{
    CourierError, Domain, DomainCheck, DomainOwner, Email, EmailId, OutgoingEmail, SendMode,
};
use email_address::EmailAddress;
use identity::ProjectId;
use serde::Deserialize;
use uuid::Uuid;

use crate::{AppState, error::ApiError, middleware::AuthContext};

const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 100;

#[derive(Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
enum EmailBody {
    Html(String),
    Text(String),
    Both { html: String, text: String },
}

#[derive(Deserialize)]
struct SendRequest {
    pub from: String,
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    #[serde(default)]
    pub bcc: Vec<String>,
    pub subject: String,
    pub body: EmailBody,
    /// Defaults to live; `test` records the email without sending it.
    #[serde(default)]
    pub mode: Option<SendMode>,
}

#[derive(Deserialize)]
struct CreateDomainRequest {
    pub domain: String,
}

#[derive(Deserialize)]
struct Page {
    pub limit: Option<i64>,
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
        .list_email_from_domain(&domain.domain, page.limit(), page.before())
        .await?;

    Ok(Json(emails))
}

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

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/domains", get(get_domains).post(create_domain))
        .route("/domains/{domain}", get(get_domain).delete(delete_domain))
        .route("/domains/{domain}/check", get(check_domain))
        .route("/domains/{domain}/mail", get(list_domain_mail))
        .route("/mail", get(list_mail))
        .route("/mail/send", post(send_mail))
        .route("/mail/{id}", get(get_mail))
}
