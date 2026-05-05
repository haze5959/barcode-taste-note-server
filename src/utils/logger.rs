// 테스트용 Utils

use crate::models::CommonResponse;
use actix_web::http::header::HeaderMap;
use actix_web::web::Bytes;
use actix_web::{body::MessageBody, body::to_bytes, dev::ServiceResponse};
use serde_json::Value;
use std::fmt::Debug;
use log::error;
use tokio::io::{AsyncBufReadExt, BufReader, AsyncWriteExt};
use tokio::fs::OpenOptions;
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
            error!(
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

pub async fn count_lines_in_file(path: &str) -> i64 {
    if let Ok(file) = tokio::fs::File::open(path).await {
        let reader = BufReader::new(file);
        let mut count = 0;
        let mut lines = reader.lines();
        while let Ok(Some(_)) = lines.next_line().await {
            count += 1;
        }
        count
    } else {
        0
    }
}

pub async fn log_gemini_request(is_success: bool, image_id_str: &str, log_text: &str) {
    let now = chrono::Local::now();
    let time_str = now.format("%Y-%m-%d %H:%M:%S").to_string();
    let date_str = now.format("%Y%m%d").to_string();
    let flat_log_text = log_text.replace("\n", " ").replace("\r", "");
    let log_line = format!("{} : {} : {}\n", time_str, image_id_str, flat_log_text);
    
    let dir_path = format!("logs/{}", date_str);
    let _ = tokio::fs::create_dir_all(&dir_path).await;

    let log_filename = if is_success {
        "gemini_requests_success.log"
    } else {
        "gemini_requests_failure.log"
    };

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(format!("{}/{}", dir_path, log_filename)).await {
        let _ = file.write_all(log_line.as_bytes()).await;
    }
}

pub async fn log_barcode_request(is_success: bool, barcode: &str, product_name: Option<&str>) {
    let now = chrono::Local::now();
    let time_str = now.format("%Y-%m-%d %H:%M:%S").to_string();
    let date_str = now.format("%Y%m%d").to_string();
    let dir_path = format!("logs/{}", date_str);
    let _ = tokio::fs::create_dir_all(&dir_path).await;

    let log_filename = if is_success {
        "barcode_requests_success.log"
    } else {
        "barcode_requests_failure.log"
    };

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(format!("{}/{}", dir_path, log_filename)).await {
        let log_line = if is_success {
            format!("{} : {} : {}\n", time_str, barcode, product_name.unwrap_or(""))
        } else {
            format!("{} : {}\n", time_str, barcode)
        };
        let _ = file.write_all(log_line.as_bytes()).await;
    }
}
