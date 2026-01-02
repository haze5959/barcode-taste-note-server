use crate::models::CommonResponse;
use actix_web::{HttpResponse, error::ResponseError};
use derive_more::Display;
use diesel::result::Error::{self, NotFound};
use serde::{Deserialize, Serialize};

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, Serialize, Deserialize)]
pub enum CommonResponseError {
    InternalServerError = 100,
    InternalDBError= 101,
    AuthValidationFail= 102,
    DuplicatedError= 103,
    JWKSFetchError= 104,
    RecordNotFound = 105,
    Unknown = 255
}

impl ResponseError for CommonResponseError {
    fn error_response(&self) -> HttpResponse {
        let response: CommonResponse<Option<()>> = CommonResponse {
            result: false,
            data: None,
            error: Some(*self as u8),
        };

        return HttpResponse::Ok().json(response);
    }
}

pub fn handler_disel_error(error: Error) -> CommonResponseError {
    return match error {
        NotFound => CommonResponseError::RecordNotFound,
        _ => {
            eprintln!("[DB Error] {}", error.to_string());
            CommonResponseError::InternalDBError
        }
    };
}
