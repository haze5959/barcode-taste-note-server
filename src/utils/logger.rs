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
use std::collections::HashMap;
use lazy_static::lazy_static;
use std::sync::Mutex;
use std::time::{Duration, Instant};

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

/// 바코드 검색 실패 메일 발송 간격: 마지막 발송 후 이 시간이 지나야 다시 보낸다.
const BARCODE_FAILURE_MAIL_INTERVAL: Duration = Duration::from_secs(600); // 10분

/// 실패 메일 throttle 전역 상태
struct BarcodeFailureMailState {
    /// 마지막으로 메일을 발송한 시각 (None = 아직 한 번도 발송 안 함)
    last_sent_at: Option<Instant>,
    /// 아직 보내지 않고 쌓아둔 실패 목록 (barcode, product_name)
    pending: Vec<(String, String)>,
}

lazy_static! {
    static ref BARCODE_FAILURE_MAIL: Mutex<BarcodeFailureMailState> =
        Mutex::new(BarcodeFailureMailState { last_sent_at: None, pending: Vec::new() });
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

    // 바코드 검색 실패 시 운영자에게 메일 알림.
    // 마지막 발송 후 10분이 지나야 보내며, 그 전에 들어온 실패는 버퍼에 쌓아두었다가
    // 10분 경과 후 들어온 실패에서 한 번에 모아 보낸다. (webhook_handlers.rs의 `mail` 전송 방식 참고)
    if !is_success {
        // 락은 버퍼 갱신/발송 여부 판단만 짧게 잡고, 블로킹 메일 전송 전에 해제한다.
        let to_send: Option<Vec<(String, String)>> = {
            let mut state = match BARCODE_FAILURE_MAIL.lock() {
                Ok(s) => s,
                Err(poisoned) => poisoned.into_inner(),
            };
            state.pending.push((barcode.to_string(), product_name.unwrap_or("(없음)").to_string()));

            let should_send = match state.last_sent_at {
                None => true, // 첫 실패는 즉시 발송
                Some(t) => t.elapsed() >= BARCODE_FAILURE_MAIL_INTERVAL,
            };

            if should_send {
                state.last_sent_at = Some(Instant::now());
                Some(std::mem::take(&mut state.pending)) // 쌓인 실패 전부 비우면서 가져옴
            } else {
                None // 아직 10분 미경과 → 누적만 하고 발송 보류
            }
        };

        if let Some(failures) = to_send {
            // `mail` 실행 + wait는 블로킹이므로 별도 스레드에서 처리해 async 실행기를 막지 않는다
            std::thread::spawn(move || {
                let mut email_body = format!("바코드 검색 실패 {}건 (최근 10분 누적) ❌\n\n", failures.len());
                for (i, (bc, pn)) in failures.iter().enumerate() {
                    email_body.push_str(&format!("{}. barcode: {} / product_name: {}\n", i + 1, bc, pn));
                }
                let subject = format!("[Barnote] 바코드 검색 실패 {}건", failures.len());
                if let Ok(mut child) = std::process::Command::new("mail")
                    .arg("-s")
                    .arg(&subject)
                    .arg("barcodetastenote@gmail.com")
                    .stdin(std::process::Stdio::piped())
                    .spawn()
                {
                    use std::io::Write;
                    if let Some(mut stdin) = child.stdin.take() {
                        let _ = stdin.write_all(email_body.as_bytes());
                    }
                    let _ = child.wait();
                }
            });
        }
    }
}

// ============================================================
// 스크래핑 실패 바코드 추적 (logs/fail_barcodes.json)
// 포맷: { "<barcode>": <실패 횟수>, ... }
// ============================================================

const FAIL_BARCODES_PATH: &str = "logs/fail_barcodes.json";

lazy_static! {
    /// fail_barcodes.json 동시 접근(읽기-수정-쓰기) 직렬화용 락
    static ref FAIL_BARCODES_LOCK: Mutex<()> = Mutex::new(());
}

/// fail_barcodes.json 을 읽어 맵으로 반환 (파일이 없거나 깨졌으면 빈 맵)
fn read_fail_barcodes() -> HashMap<String, i64> {
    std::fs::read_to_string(FAIL_BARCODES_PATH)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// 맵을 fail_barcodes.json 으로 저장
fn write_fail_barcodes(map: &HashMap<String, i64>) {
    let _ = std::fs::create_dir_all("logs");
    if let Ok(json) = serde_json::to_string_pretty(map) {
        let _ = std::fs::write(FAIL_BARCODES_PATH, json);
    }
}

/// 바코드가 실패 목록에 이미 있으면 실패 횟수를 +1 하고 true 를 반환한다(=스크래핑 생략, 실패 응답).
/// 없으면 아무것도 바꾸지 않고 false 를 반환한다(=스크래핑 시도).
pub fn check_and_increment_fail_barcode(barcode: &str) -> bool {
    let _guard = FAIL_BARCODES_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut map = read_fail_barcodes();
    match map.get_mut(barcode) {
        Some(count) => {
            *count += 1;
            write_fail_barcodes(&map);
            true
        }
        None => false,
    }
}

/// 스크래핑까지 실패한 바코드를 실패 목록에 신규 추가한다(횟수 1, 이미 있으면 +1).
pub fn record_fail_barcode(barcode: &str) {
    let _guard = FAIL_BARCODES_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut map = read_fail_barcodes();
    *map.entry(barcode.to_string()).or_insert(0) += 1;
    write_fail_barcodes(&map);
}
