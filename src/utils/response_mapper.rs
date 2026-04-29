// 테스트용 Utils

use crate::models::CommonResponse;
use actix_web::{body::MessageBody, body::to_bytes, dev::ServiceResponse, http::StatusCode};
use serde::de::DeserializeOwned;

use super::logger::print_json_log;

pub async fn response_to_model<Model: DeserializeOwned, B: MessageBody + 'static>(
    response: ServiceResponse<B>,
) -> CommonResponse<Model> {
    let response: ServiceResponse = response.map_into_boxed_body();
    let status = response.status();
    let body_bytes = to_bytes(response.into_body())
        .await
        .expect("Failed to read response body");
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        panic!("Auth failed with status: {}", status);
    } else if status == StatusCode::OK {
        print_json_log(&body_bytes);
        serde_json::from_slice::<CommonResponse<Model>>(&body_bytes)
            .expect("Failed to deserialize response body")
    } else {
        panic!("HTTP Req Fail with status: {}", status);
    }
}
