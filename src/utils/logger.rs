use actix_web::http::header::HeaderMap;
use actix_web::{body::MessageBody, body::to_bytes, dev::ServiceResponse};
use serde_json::Value;
use std::fmt::Debug;

pub fn print_model<T: Debug>(model: &T) {
    println!("[Model]: {:?}", model);
}

pub fn print_header_log(header: &HeaderMap) {
    for (key, value) in header {
        println!("[Header]: {}: {:?}", key, value);
    }
}

pub async fn print_response_log<B: MessageBody + 'static>(response: ServiceResponse<B>) {
    let response: ServiceResponse = response.map_into_boxed_body();
    let body_bytes = to_bytes(response.into_body()).await.unwrap();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();
    println!("[Body]: {}", json.to_string());
}
