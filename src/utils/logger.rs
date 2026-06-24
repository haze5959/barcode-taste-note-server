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
use std::time::Duration;

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

/// 운영자 알림 메일 배치 간격: 첫 이벤트 시점부터 이 시간 동안 누적했다가,
/// 첫 이벤트로부터 이 시간이 지나면 누적분을 한 번에 발송한다. (이후 이벤트로 타이머가 리셋되지 않음)
const OPERATOR_MAIL_BATCH_INTERVAL: Duration = Duration::from_secs(600); // 10분

/// 운영자(barcodetastenote@gmail.com)에게 메일을 보내는 공통 함수.
/// `mail` 명령 실행 + wait 는 블로킹이므로 별도 스레드에서 처리해 async 실행기를 막지 않는다.
fn send_operator_mail(subject: String, body: String) {
    std::thread::spawn(move || {
        if let Ok(mut child) = std::process::Command::new("mail")
            .arg("-s")
            .arg(&subject)
            .arg("barcodetastenote@gmail.com")
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            use std::io::Write;
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(body.as_bytes());
            }
            let _ = child.wait();
        }
    });
}

/// 메일 배치 공통 상태: 이벤트가 들어올 때마다 pending 에 누적한다.
/// 첫 이벤트가 윈도우(타이머)를 열고, 첫 이벤트로부터 OPERATOR_MAIL_BATCH_INTERVAL 후 누적분을 flush 한다.
struct MailBatchState<T> {
    pending: T,
    scheduled: bool, // 현재 배치 윈도우(타이머)가 열려 있는지
}

lazy_static! {
    /// 바코드 검색 실패 누적 (barcode, product_name)
    static ref BARCODE_FAILURE_MAIL: Mutex<MailBatchState<Vec<(String, String)>>> =
        Mutex::new(MailBatchState { pending: Vec::new(), scheduled: false });

    /// 신규 가입자 수 누적
    static ref USER_REGISTER_MAIL: Mutex<MailBatchState<i64>> =
        Mutex::new(MailBatchState { pending: 0, scheduled: false });
}

/// 이벤트 1건을 누적한다(add). 윈도우가 닫혀 있으면(=첫 이벤트면) 타이머를 띄워,
/// 첫 이벤트로부터 OPERATOR_MAIL_BATCH_INTERVAL 후에 누적분으로 flush 를 호출한다.
/// 윈도우가 이미 열려 있으면 누적만 하고 타이머는 리셋하지 않는다.
fn record_batched_event<T, A, F>(lock: &'static Mutex<MailBatchState<T>>, add: A, flush: F)
where
    T: Default + Send + 'static,
    A: FnOnce(&mut T),
    F: Fn(T) + Send + 'static,
{
    let opened_window = {
        let mut state = lock.lock().unwrap_or_else(|p| p.into_inner());
        add(&mut state.pending);
        if state.scheduled {
            false // 이미 윈도우가 열려 있음 → 누적만
        } else {
            state.scheduled = true; // 첫 이벤트 → 윈도우 열기
            true
        }
    };

    if opened_window {
        // 첫 이벤트 시점부터 INTERVAL 후 1회 발송 (이후 들어오는 이벤트로 타이머가 리셋되지 않음)
        std::thread::spawn(move || {
            std::thread::sleep(OPERATOR_MAIL_BATCH_INTERVAL);
            let data = {
                let mut state = lock.lock().unwrap_or_else(|p| p.into_inner());
                state.scheduled = false;
                std::mem::take(&mut state.pending)
            };
            flush(data);
        });
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

    // 바코드 검색 실패 시 운영자에게 메일 알림 (첫 실패 시점부터 일정 시간 누적 후 한 번에 발송)
    if !is_success {
        let bc = barcode.to_string();
        let pn = product_name.unwrap_or("(없음)").to_string();
        record_batched_event(
            &BARCODE_FAILURE_MAIL,
            move |pending: &mut Vec<(String, String)>| pending.push((bc, pn)),
            |failures: Vec<(String, String)>| {
                if failures.is_empty() {
                    return;
                }
                let mins = OPERATOR_MAIL_BATCH_INTERVAL.as_secs() / 60;
                let mut body = format!("바코드 검색 실패 {}건 (최근 {}분간 누적) ❌\n\n", failures.len(), mins);
                for (i, (b, p)) in failures.iter().enumerate() {
                    body.push_str(&format!("{}. barcode: {} / product_name: {}\n", i + 1, b, p));
                }
                send_operator_mail(format!("[Barnote] 바코드 검색 실패 {}건", failures.len()), body);
            },
        );
    }
}

/// 신규 가입 성공 시 호출한다. 운영자에게 가입 알림 메일을 첫 가입 시점 기준 배치로 보낸다.
/// (첫 가입으로부터 OPERATOR_MAIL_BATCH_INTERVAL 동안 누적한 뒤, 그 시점까지의 가입자 수를 한 번에 발송)
pub fn notify_user_registered() {
    record_batched_event(
        &USER_REGISTER_MAIL,
        |count: &mut i64| *count += 1,
        |count: i64| {
            if count <= 0 {
                return;
            }
            let mins = OPERATOR_MAIL_BATCH_INTERVAL.as_secs() / 60;
            let body = format!("신규 가입자 {}명 🎉 (최근 {}분간 가입)", count, mins);
            send_operator_mail(format!("[Barnote] 신규 가입자 {}명", count), body);
        },
    );
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
