use axum::{
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
                CourierError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
                CourierError::NotFound => StatusCode::NOT_FOUND,
                CourierError::Taken => StatusCode::BAD_REQUEST,
                CourierError::Provider(provider_error) => match provider_error {
                    courier::ProviderError::AlreadyExists => StatusCode::BAD_REQUEST,
                    courier::ProviderError::NotFound => StatusCode::NOT_FOUND,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                },
            },
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        self.status().into_response()
    }
}
