use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use courier::CourierError;
use identity::IdentityError;

pub enum ApiError {
    Identity(IdentityError),
    Courier(CourierError),
}

impl From<IdentityError> for ApiError {
    fn from(err: IdentityError) -> Self {
        Self::Identity(err)
    }
}

impl From<CourierError> for ApiError {
    fn from(err: CourierError) -> Self {
        Self::Courier(err)
    }
}

impl ApiError {
    fn status(&self) -> StatusCode {
        match self {
            Self::Identity(err) => match err {
                IdentityError::InvalidKey | IdentityError::Revoked => StatusCode::UNAUTHORIZED,
                IdentityError::KeyNotFound | IdentityError::ProjectNotFound => {
                    StatusCode::NOT_FOUND
                }
                IdentityError::SlugTaken => StatusCode::CONFLICT,
                IdentityError::InvalidSlug => StatusCode::UNPROCESSABLE_ENTITY,
                IdentityError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            },
            ApiError::Courier(err) => match err {
                CourierError::InvalidAddress(_) => StatusCode::BAD_REQUEST,
                CourierError::InvalidDomain(_) => StatusCode::UNPROCESSABLE_ENTITY,
                CourierError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
                CourierError::NotFound => StatusCode::NOT_FOUND,
                CourierError::Taken => StatusCode::CONFLICT,
                CourierError::Provider(provider_error) => match provider_error {
                    courier::ProviderError::AlreadyExists => StatusCode::CONFLICT,
                    courier::ProviderError::Rejected(_) => StatusCode::UNPROCESSABLE_ENTITY,
                    courier::ProviderError::NotFound => StatusCode::NOT_FOUND,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                },
            },
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        // Server errors keep their details out of the response.
        if status.is_server_error() {
            return status.into_response();
        }

        let message = match &self {
            Self::Identity(err) => err.to_string(),
            Self::Courier(err) => err.to_string(),
        };
        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}
