use crate::models::CommonResponse;
use actix_web::{body::MessageBody, body::to_bytes, dev::ServiceResponse};
use serde::de::DeserializeOwned;

pub async fn response_to_model<Model: DeserializeOwned, B: MessageBody + 'static>(
    response: ServiceResponse<B>,
) -> CommonResponse<Model> {
    let response: ServiceResponse = response.map_into_boxed_body();
    let body_bytes = to_bytes(response.into_body())
        .await
        .expect("Failed to read response body");

    serde_json::from_slice::<CommonResponse<Model>>(&body_bytes)
        .expect("Failed to deserialize response body")
}
