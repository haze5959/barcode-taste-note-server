use crate::models::CommonResponse;
use actix_web::{HttpResponse, error::ResponseError};
use derive_more::Display;
use diesel::result::Error::{self, NotFound};
use serde::{Deserialize, Serialize};

#[derive(Debug, Display, Serialize)]
pub enum ServiceError {
    #[display(fmt = "Internal Server Error")]
    InternalServerError,

    #[display(fmt = "Internal DB Error")]
    InternalDBError,

    #[display(fmt = "BadRequest: {}", _0)]
    BadRequest(String),

    #[display(fmt = "Duplicated Error")]
    DuplicatedError,

    #[display(fmt = "JWKSFetchError")]
    JWKSFetchError,

    #[display(fmt = "ResponseError: {}", _0)]
    ResponseError(CommonResponseError),
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, Serialize, Deserialize)]
pub enum CommonResponseError {
    RecordNotFound = 100,
}

// impl ResponseError trait allows to convert our errors into http responses with appropriate data
impl ResponseError for ServiceError {
    fn error_response(&self) -> HttpResponse {
        match self {
            ServiceError::InternalServerError => {
                HttpResponse::InternalServerError().json("Internal Server Error, Please try later")
            }
            ServiceError::InternalDBError => {
                HttpResponse::InternalServerError().json("Internal DB Error, Please try later")
            }
            ServiceError::BadRequest(message) => HttpResponse::BadRequest().json(message),
            ServiceError::DuplicatedError => HttpResponse::InternalServerError().json("Duplicated"),
            ServiceError::ResponseError(error) => {
                let response: CommonResponse<Option<()>> = CommonResponse {
                    result: false,
                    data: None,
                    error: Some(*error as u8),
                };

                return HttpResponse::Ok().json(response);
            }
            ServiceError::JWKSFetchError => {
                HttpResponse::InternalServerError().json("Could not fetch JWKS")
            }
        }
    }
}

pub fn handler_disel_error(error: Error) -> ServiceError {
    return match error {
        NotFound => ServiceError::ResponseError(CommonResponseError::RecordNotFound),
        _ => {
            eprintln!("[DB Error] {}", error.to_string());
            ServiceError::InternalDBError
        }
    };
}
