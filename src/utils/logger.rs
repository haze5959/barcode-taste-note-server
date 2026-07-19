// 테스트용 Utils

use crate::models::CommonResponse;
use actix_web::http::header::HeaderMap;
use actix_web::web::Bytes;
use actix_web::{body::MessageBody, body::to_bytes, dev::ServiceResponse};
use serde::{Deserialize, Serialize};
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
        let mut child = match std::process::Command::new("mail")
            .arg("-s")
            .arg(&subject)
            .arg("barcodetastenote@gmail.com")
            .stdin(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                error!("[Mail] `mail` 실행 실패 (설치/PATH 확인 필요): {}", e);
                return;
            }
        };

        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            if let Err(e) = stdin.write_all(body.as_bytes()) {
                error!("[Mail] 메일 본문 쓰기 실패: {}", e);
            }
        }

        match child.wait_with_output() {
            Ok(out) if !out.status.success() => error!(
                "[Mail] 발송 실패 (exit {:?}): {}",
                out.status.code(),
                String::from_utf8_lossy(&out.stderr).trim()
            ),
            Err(e) => error!("[Mail] 프로세스 대기 실패: {}", e),
            _ => {}
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

pub async fn log_search_history(query: &str) {
    let now = chrono::Local::now();
    let time_str = now.format("%Y-%m-%d %H:%M:%S").to_string();
    let date_str = now.format("%Y%m%d").to_string();
    let dir_path = format!("logs/{}", date_str);
    let _ = tokio::fs::create_dir_all(&dir_path).await;

    let log_filename = "search_history.log";

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(format!("{}/{}", dir_path, log_filename)).await {
        let log_line = format!("{} : {}\n", time_str, query);
        let _ = file.write_all(log_line.as_bytes()).await;
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
// 포맷: { "<barcode>": { "updated_at": "YYYY-MM-DD HH:MM:SS", "fail_count": N }, ... }
//  - updated_at 은 fail_count 갱신 시각(로컬/KST), 최신순(내림차순)으로 정렬해 저장한다.
// ============================================================

const FAIL_BARCODES_PATH: &str = "logs/fail_barcodes.json";

/// 실패 바코드 1건의 값
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailBarcodeEntry {
    /// 마지막 실패 시각 (YYYY-MM-DD HH:MM:SS)
    pub updated_at: String,
    /// 누적 실패(접근) 횟수
    pub fail_count: i64,
}

lazy_static! {
    /// fail_barcodes.json 동시 접근(읽기-수정-쓰기) 직렬화용 락
    static ref FAIL_BARCODES_LOCK: Mutex<()> = Mutex::new(());
}

fn now_str() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// fail_barcodes.json 을 읽어 맵으로 반환 (파일이 없거나 깨졌으면 빈 맵)
fn read_fail_barcodes() -> HashMap<String, FailBarcodeEntry> {
    std::fs::read_to_string(FAIL_BARCODES_PATH)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// serde_json 의 Map 은 순서를 보존하지 않으므로, 정렬된 순서 그대로 JSON 객체로 직렬화하기 위한 래퍼.
struct OrderedFailBarcodes<'a>(&'a [(&'a String, &'a FailBarcodeEntry)]);

impl Serialize for OrderedFailBarcodes<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (k, v) in self.0 {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}

/// 맵을 updated_at 내림차순(최신 먼저)으로 정렬해 fail_barcodes.json 으로 저장.
/// updated_at 이 고정폭 형식이라 문자열 내림차순 정렬이 곧 시간 최신순이다.
fn write_fail_barcodes(map: &HashMap<String, FailBarcodeEntry>) {
    let _ = std::fs::create_dir_all("logs");
    let mut entries: Vec<(&String, &FailBarcodeEntry)> = map.iter().collect();
    entries.sort_by(|a, b| b.1.updated_at.cmp(&a.1.updated_at));

    if let Ok(json) = serde_json::to_string_pretty(&OrderedFailBarcodes(&entries)) {
        let _ = std::fs::write(FAIL_BARCODES_PATH, json);
    }
}

/// 바코드가 실패 목록에 이미 있으면 fail_count +1, updated_at 갱신 후 true 를 반환한다(=스크래핑 생략, 실패 응답).
/// 없으면 아무것도 바꾸지 않고 false 를 반환한다(=스크래핑 시도).
pub fn check_and_increment_fail_barcode(barcode: &str) -> bool {
    let _guard = FAIL_BARCODES_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut map = read_fail_barcodes();
    match map.get_mut(barcode) {
        Some(entry) => {
            entry.fail_count += 1;
            entry.updated_at = now_str();
            write_fail_barcodes(&map);
            true
        }
        None => false,
    }
}

/// 스크래핑까지 실패한 바코드를 실패 목록에 신규 추가한다(횟수 1, 이미 있으면 +1). updated_at 갱신.
pub fn record_fail_barcode(barcode: &str) {
    let _guard = FAIL_BARCODES_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut map = read_fail_barcodes();
    let entry = map
        .entry(barcode.to_string())
        .or_insert(FailBarcodeEntry { updated_at: String::new(), fail_count: 0 });
    entry.fail_count += 1;
    entry.updated_at = now_str();
    write_fail_barcodes(&map);
}

/// 조회 응답용: 실패 바코드 전체를 updated_at 최신순으로 담되, JSON 객체 키 순서를 보존해 직렬화한다.
pub struct FailBarcodesView(Vec<(String, FailBarcodeEntry)>);

impl Serialize for FailBarcodesView {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (k, v) in &self.0 {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}

/// fail_barcodes.json 전체를 updated_at 최신순(내림차순)으로 반환한다 (파일에 저장된 순서와 동일).
pub fn read_fail_barcodes_view() -> FailBarcodesView {
    let _guard = FAIL_BARCODES_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut entries: Vec<(String, FailBarcodeEntry)> = read_fail_barcodes().into_iter().collect();
    entries.sort_by(|a, b| b.1.updated_at.cmp(&a.1.updated_at));
    FailBarcodesView(entries)
}

/// fail_barcodes.json 에서 특정 barcode 키를 삭제한다. 있어서 삭제하면 true, 없었으면 false.
pub fn delete_fail_barcode(barcode: &str) -> bool {
    let _guard = FAIL_BARCODES_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut map = read_fail_barcodes();
    let existed = map.remove(barcode).is_some();
    if existed {
        write_fail_barcodes(&map);
    }
    existed
}

// ============================================================
// 성공 바코드 추적 (logs/success_barcodes.json)
// 포맷: { "<barcode>": { "updated_at": "YYYY-MM-DD HH:MM:SS", "success_count": N }, ... }
//  - updated_at 은 success_count 갱신 시각(로컬/KST), 최신순(내림차순)으로 정렬해 저장한다.
// ============================================================

const SUCCESS_BARCODES_PATH: &str = "logs/success_barcodes.json";

/// 성공 바코드 1건의 값
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuccessBarcodeEntry {
    /// 마지막 성공 시각 (YYYY-MM-DD HH:MM:SS)
    pub updated_at: String,
    /// 누적 성공(접근) 횟수
    pub success_count: i64,
}

lazy_static! {
    /// success_barcodes.json 동시 접근(읽기-수정-쓰기) 직렬화용 락
    static ref SUCCESS_BARCODES_LOCK: Mutex<()> = Mutex::new(());
}

/// success_barcodes.json 을 읽어 맵으로 반환 (파일이 없거나 깨졌으면 빈 맵)
fn read_success_barcodes() -> HashMap<String, SuccessBarcodeEntry> {
    std::fs::read_to_string(SUCCESS_BARCODES_PATH)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// serde_json 의 Map 은 순서를 보존하지 않으므로, 정렬된 순서 그대로 JSON 객체로 직렬화하기 위한 래퍼.
struct OrderedSuccessBarcodes<'a>(&'a [(&'a String, &'a SuccessBarcodeEntry)]);

impl Serialize for OrderedSuccessBarcodes<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (k, v) in self.0 {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}

/// 맵을 updated_at 내림차순(최신 먼저)으로 정렬해 success_barcodes.json 으로 저장.
fn write_success_barcodes(map: &HashMap<String, SuccessBarcodeEntry>) {
    let _ = std::fs::create_dir_all("logs");
    let mut entries: Vec<(&String, &SuccessBarcodeEntry)> = map.iter().collect();
    entries.sort_by(|a, b| b.1.updated_at.cmp(&a.1.updated_at));

    if let Ok(json) = serde_json::to_string_pretty(&OrderedSuccessBarcodes(&entries)) {
        let _ = std::fs::write(SUCCESS_BARCODES_PATH, json);
    }
}

/// 조회에 성공한 바코드를 성공 목록에 신규 추가한다(횟수 1, 이미 있으면 +1). updated_at 갱신.
pub fn record_success_barcode(barcode: &str) {
    let _guard = SUCCESS_BARCODES_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut map = read_success_barcodes();
    let entry = map
        .entry(barcode.to_string())
        .or_insert(SuccessBarcodeEntry { updated_at: String::new(), success_count: 0 });
    entry.success_count += 1;
    entry.updated_at = now_str();
    write_success_barcodes(&map);
}

/// 조회 응답용: 성공 바코드 전체를 updated_at 최신순으로 담되, JSON 객체 키 순서를 보존해 직렬화한다.
pub struct SuccessBarcodesView(Vec<(String, SuccessBarcodeEntry)>);

impl Serialize for SuccessBarcodesView {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (k, v) in &self.0 {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}

/// success_barcodes.json 전체를 updated_at 최신순(내림차순)으로 반환한다 (파일에 저장된 순서와 동일).
pub fn read_success_barcodes_view() -> SuccessBarcodesView {
    let _guard = SUCCESS_BARCODES_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut entries: Vec<(String, SuccessBarcodeEntry)> = read_success_barcodes().into_iter().collect();
    entries.sort_by(|a, b| b.1.updated_at.cmp(&a.1.updated_at));
    SuccessBarcodesView(entries)
}

pub async fn send_daily_search_history_report() -> Result<(), std::io::Error> {
    let yesterday = chrono::Local::now() - chrono::Duration::days(1);
    let yesterday_dir_str = yesterday.format("%Y%m%d").to_string();
    let yesterday_date_str = yesterday.format("%Y-%m-%d").to_string();
    
    let log_path = format!("logs/{}/search_history.log", yesterday_dir_str);
    
    let body = if tokio::fs::metadata(&log_path).await.is_ok() {
        match tokio::fs::read_to_string(&log_path).await {
            Ok(content) => {
                if content.trim().is_empty() {
                    "어제의 검색 기록이 비어 있습니다.".to_string()
                } else {
                    content
                }
            }
            Err(e) => format!("어제의 검색 기록 파일 읽기 실패: {:?}", e),
        }
    } else {
        "어제의 검색 기록이 없습니다.".to_string()
    };
    
    let subject = format!("[Barnote] {} 검색 히스토리 리포트", yesterday_date_str);
    send_operator_mail(subject, body);
    
    Ok(())
}

pub fn start_report_scheduler() {
    use chrono::TimeZone;
    actix_rt::spawn(async {
        loop {
            let now = chrono::Local::now();
            let target = if now.time() < chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap() {
                now.date_naive().and_hms_milli_opt(9, 0, 0, 0).unwrap()
            } else {
                (now.date_naive() + chrono::Duration::days(1)).and_hms_milli_opt(9, 0, 0, 0).unwrap()
            };
            
            if let Some(target_dt) = chrono::Local.from_local_datetime(&target).single() {
                let duration = target_dt.signed_duration_since(now);
                if let Ok(std_dur) = duration.to_std() {
                    tokio::time::sleep(std_dur).await;
                }
            } else {
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                continue;
            }
            
            let _ = send_daily_search_history_report().await;
            
            // Sleep for 60 seconds to avoid double execution due to timing drift
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        }
    });
}

