use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use identity::IdentityError;

pub enum ApiError {
    Identity(IdentityError),
}

impl From<IdentityError> for ApiError {
    fn from(err: IdentityError) -> Self {
        Self::Identity(err)
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
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        self.status().into_response()
    }
}
