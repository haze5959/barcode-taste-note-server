// 테스트용 Utils

use crate::models::CommonResponse;
use actix_web::http::header::HeaderMap;
use actix_web::web::Bytes;
use actix_web::{body::MessageBody, body::to_bytes, dev::ServiceResponse};
use serde_json::Value;
use std::fmt::Debug;
use std::str;

pub fn print_response_model<T: Debug>(model: &CommonResponse<T>) {
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
    print_json_log(&body_bytes);
}

pub fn print_json_log(bytes: &Bytes) {
    let json: Value;
    match serde_json::from_slice(&bytes) {
        Ok(v) => {
            json = v;
        }
        Err(e) => {
            // 로그 찍기
            eprintln!(
                "[JSON Parse Error] {}\nRaw body: {}",
                e,
                str::from_utf8(&bytes).unwrap_or("<non-UTF8 body>")
            );
            // 패닉 발생
            panic!("Failed to parse response body as JSON: {}", e);
        }
    }

    println!("[JSON]: {}", json.to_string());
}
