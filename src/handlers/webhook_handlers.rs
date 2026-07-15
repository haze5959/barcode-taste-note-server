use crate::Pool;
use crate::diesel::{ExpressionMethods, QueryDsl, RunQueryDsl};
use crate::errors::CommonResponseError;
use crate::errors::handler_disel_error;
use crate::schema::users;
use actix_web::{web, HttpResponse, Error};
use base64::{engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD}, Engine as _};
use chrono::{DateTime, Utc, TimeZone};
use serde::Deserialize;
use uuid::Uuid;
use log::{info, error};

#[derive(Debug, Deserialize)]
pub struct AppStoreNotificationRequest {
    #[serde(rename = "signedPayload")]
    pub signed_payload: String,
}

#[derive(Debug, Deserialize)]
pub struct DecodedPayload {
    #[serde(rename = "notificationType")]
    pub notification_type: String,
    pub data: Option<NotificationData>,
}

#[derive(Debug, Deserialize)]
pub struct NotificationData {
    #[serde(rename = "appAccountToken")]
    pub app_account_token: Option<Uuid>,
    #[serde(rename = "signedTransactionInfo")]
    pub signed_transaction_info: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TransactionInfo {
    #[serde(rename = "productId")]
    pub product_id: String,
    #[serde(rename = "expiresDate")]
    pub expires_date: Option<i64>,
    #[serde(rename = "purchaseDate")]
    pub purchase_date: Option<i64>,
    /// 앱에서 구매 시 넣은 appAccountToken(=user_id).
    /// 주의: data 객체가 아니라 signedTransactionInfo(JWS) 페이로드 안에 들어있다.
    #[serde(rename = "appAccountToken")]
    pub app_account_token: Option<Uuid>,
}

/// Helper to decode JWS payload without signature verification
fn decode_unverified<T: for<'de> Deserialize<'de>>(signed_token: &str) -> Option<T> {
    let parts: Vec<&str> = signed_token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let payload = parts[1];
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&decoded).ok()
}

pub async fn handle_appstore_notification(
    db: web::Data<Pool>,
    payload: web::Json<AppStoreNotificationRequest>,
) -> Result<HttpResponse, Error> {
    let signed_payload = &payload.signed_payload;
    
    let decoded: DecodedPayload = match decode_unverified(signed_payload) {
        Some(d) => d,
        None => {
            error!("Failed to decode AppStore signedPayload");
            return Ok(HttpResponse::BadRequest().finish());
        }
    };

    info!("Received App Store Notification: {}", decoded.notification_type);

    let data = match decoded.data {
        Some(d) => d,
        None => return Ok(HttpResponse::Ok().finish()),
    };

    // 트랜잭션 정보(JWS)를 먼저 디코드한다 — appAccountToken 이 이 안에 들어있다
    let tx_info: Option<TransactionInfo> = data.signed_transaction_info
        .as_ref()
        .and_then(|info_str| decode_unverified(info_str));

    // appAccountToken(=user_id)은 data 객체가 아니라 signedTransactionInfo 페이로드에서 추출
    let user_id = match tx_info
        .as_ref()
        .and_then(|tx| tx.app_account_token)
        .or(data.app_account_token)
    {
        Some(uid) => uid,
        None => {
            info!("Notification skipped: No appAccountToken (user_id) found in transaction info");
            return Ok(HttpResponse::Ok().finish());
        }
    };

    let _ = web::block(move || {
        let conn = &mut db.get().unwrap();
        
        match decoded.notification_type.as_str() {
            "SUBSCRIBED" | "DID_RENEW" => {
                let expiry = if let Some(tx) = tx_info {
                    if let Some(ms) = tx.expires_date {
                        Utc.timestamp_millis_opt(ms).single()
                    } else {
                        // 만약 expiresDate가 없다면 구매 시간 기준으로 수동 계산
                        let base_time = tx.purchase_date
                            .and_then(|ms| Utc.timestamp_millis_opt(ms).single())
                            .unwrap_or_else(Utc::now);
                            
                        if tx.product_id == "com.barnote.subscription.yearly" {
                            Some(base_time + chrono::Duration::days(365)) // 단순 365일
                        } else {
                            Some(base_time + chrono::Duration::days(30)) // 단순 30일
                        }
                    }
                } else {
                    None
                };

                if let Some(expiry_date) = expiry {
                    diesel::update(users::table.find(user_id))
                        .set(users::premium_expire_at.eq(Some(expiry_date)))
                        .execute(conn)
                        .map_err(handler_disel_error)?;
                    info!("User {} premium extended to {:?}", user_id, expiry_date);

                    // 신규 구독(SUBSCRIBED)일 때만 메일 발송 (갱신 DID_RENEW 은 메일 생략)
                    if decoded.notification_type == "SUBSCRIBED" {
                        let email_body = format!("[App Store / iOS] 구독자 도착 🎉\nuser_id: {}\nexpiry_date: {:?}", user_id, expiry_date);
                        if let Ok(mut child) = std::process::Command::new("mail")
                            .arg("-s")
                            .arg("[Barnote] 새로운 프리미엄 구독 결제 발생 (App Store / iOS)")
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
                    }
                }
            },
            "EXPIRED" | "REFUND" | "REVOCATION" | "DID_FAIL_TO_RENEW" => {
                diesel::update(users::table.find(user_id))
                    .set(users::premium_expire_at.eq(None::<DateTime<Utc>>))
                    .execute(conn)
                    .map_err(handler_disel_error)?;
                info!("User {} premium expired/removed", user_id);
            },
            _ => {
                info!("Notification type {} not handled for DB update", decoded.notification_type);
            }
        }
        Ok::<(), CommonResponseError>(())
    }).await??;

    Ok(HttpResponse::Ok().finish())
}

// ============================================================
// Google Play 구독 RDN (Real-time Developer Notifications via Cloud Pub/Sub)
// ============================================================

/// Cloud Pub/Sub push 요청 래퍼 (Pub/Sub가 이 형태로 POST 한다)
#[derive(Debug, Deserialize)]
pub struct PubSubPushRequest {
    pub message: PubSubMessage,
}

#[derive(Debug, Deserialize)]
pub struct PubSubMessage {
    /// base64(STANDARD)로 인코딩된 RDN 페이로드 (없을 수도 있어 Option)
    pub data: Option<String>,
}

/// Google Play RDN DeveloperNotification (필요한 필드만 매핑)
#[derive(Debug, Deserialize)]
pub struct DeveloperNotification {
    #[serde(rename = "packageName")]
    pub package_name: String,
    #[serde(rename = "subscriptionNotification")]
    pub subscription_notification: Option<SubscriptionNotification>,
}

#[derive(Debug, Deserialize)]
pub struct SubscriptionNotification {
    /// 1:RECOVERED 2:RENEWED 3:CANCELED 4:PURCHASED 5:ON_HOLD 6:IN_GRACE
    /// 7:RESTARTED 12:REVOKED 13:EXPIRED ...
    #[serde(rename = "notificationType")]
    pub notification_type: i32,
    #[serde(rename = "purchaseToken")]
    pub purchase_token: String,
    #[serde(rename = "subscriptionId")]
    pub subscription_id: Option<String>,
}

/// Play Developer API: purchases.subscriptionsv2.get 응답 (필요한 필드만)
#[derive(Debug, Deserialize)]
pub struct SubscriptionPurchaseV2 {
    #[serde(rename = "lineItems")]
    pub line_items: Option<Vec<SubscriptionLineItem>>,
    #[serde(rename = "externalAccountIdentifiers")]
    pub external_account_identifiers: Option<ExternalAccountIdentifiers>,
}

#[derive(Debug, Deserialize)]
pub struct SubscriptionLineItem {
    /// 만료 시각 (RFC3339). 갱신마다 갱신된다.
    #[serde(rename = "expiryTime")]
    pub expiry_time: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ExternalAccountIdentifiers {
    /// 앱에서 구매 시 setObfuscatedAccountId 로 넣은 값 = 우리 user_id
    #[serde(rename = "obfuscatedExternalAccountId")]
    pub obfuscated_external_account_id: Option<String>,
}

/// purchaseToken 으로 Play Developer API(subscriptionsv2.get)를 호출해 구독 정보를 조회한다.
/// 서비스 계정(GOOGLE_APPLICATION_CREDENTIALS)으로 androidpublisher 스코프 토큰을 발급해 사용한다.
async fn fetch_play_subscription(
    package_name: &str,
    purchase_token: &str,
) -> Option<SubscriptionPurchaseV2> {
    let provider = match gcp_auth::provider().await {
        Ok(p) => p,
        Err(e) => {
            error!("[PlayStore] GCP provider 생성 실패: {}", e);
            return None;
        }
    };
    let token = match provider
        .token(&["https://www.googleapis.com/auth/androidpublisher"])
        .await
    {
        Ok(t) => t.as_str().to_string(),
        Err(e) => {
            error!("[PlayStore] GCP 토큰 발급 실패: {}", e);
            return None;
        }
    };

    let url = format!(
        "https://androidpublisher.googleapis.com/androidpublisher/v3/applications/{}/purchases/subscriptionsv2/tokens/{}",
        package_name, purchase_token
    );

    let client = reqwest::Client::new();
    let res = match client.get(&url).bearer_auth(token).send().await {
        Ok(r) => r,
        Err(e) => {
            error!("[PlayStore] 구독 조회 요청 실패: {}", e);
            return None;
        }
    };

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        error!("[PlayStore] 구독 조회 API 오류 ({}): {}", status, body);
        return None;
    }

    match res.json::<SubscriptionPurchaseV2>().await {
        Ok(v) => Some(v),
        Err(e) => {
            error!("[PlayStore] 구독 응답 파싱 실패: {}", e);
            None
        }
    }
}

/// Google Play RDN 수신 핸들러 (Cloud Pub/Sub push).
/// 노티엔 purchaseToken만 오므로, 이를 이용해 API로 obfuscatedExternalAccountId(=user_id)와
/// 만료 시각을 조회한 뒤 handle_appstore_notification 과 동일하게 프리미엄 상태를 갱신한다.
///
/// Pub/Sub은 2xx 응답을 ack로 간주하므로, 처리 실패는 로깅만 하고 200을 반환한다(무한 재전송 방지).
pub async fn handle_playstore_notification(
    db: web::Data<Pool>,
    payload: web::Json<PubSubPushRequest>,
) -> Result<HttpResponse, Error> {
    // 1. Pub/Sub 메시지 → base64 디코드 → RDN 파싱
    let data_b64 = match &payload.message.data {
        Some(d) => d,
        None => {
            error!("[PlayStore] Pub/Sub 메시지에 data 필드 없음");
            return Ok(HttpResponse::Ok().finish());
        }
    };
    let decoded = match STANDARD.decode(data_b64) {
        Ok(b) => b,
        Err(e) => {
            error!("[PlayStore] base64 디코드 실패: {}", e);
            return Ok(HttpResponse::Ok().finish());
        }
    };
    // 수신 확인용: RDN 원문 로깅 (테스트 메시지 포함)
    info!("[PlayStore] Pub/Sub 메시지 수신: {}", String::from_utf8_lossy(&decoded));

    let notification: DeveloperNotification = match serde_json::from_slice(&decoded) {
        Ok(n) => n,
        Err(e) => {
            error!("[PlayStore] RDN 파싱 실패: {}", e);
            return Ok(HttpResponse::Ok().finish());
        }
    };

    // 2. 구독 노티만 처리 (test/oneTimeProduct/voidedPurchase 등은 무시하고 ack)
    let sub_noti = match notification.subscription_notification {
        Some(s) => s,
        None => {
            info!("[PlayStore] 구독 알림 아님(테스트/일회성 등) → ack만 하고 종료");
            return Ok(HttpResponse::Ok().finish());
        }
    };
    info!(
        "[PlayStore] 구독 노티 수신: type={}, subscriptionId={:?}",
        sub_noti.notification_type, sub_noti.subscription_id
    );

    // 3. purchaseToken으로 구독 정보 조회 → user_id, 만료 시각
    let subscription =
        match fetch_play_subscription(&notification.package_name, &sub_noti.purchase_token).await {
            Some(s) => s,
            None => return Ok(HttpResponse::Ok().finish()),
        };

    let user_id = match subscription
        .external_account_identifiers
        .as_ref()
        .and_then(|e| e.obfuscated_external_account_id.as_deref())
        .and_then(|s| Uuid::parse_str(s).ok())
    {
        Some(uid) => uid,
        None => {
            error!("[PlayStore] obfuscatedExternalAccountId 없음/형식오류 → 스킵 (앱 구매 시 setObfuscatedAccountId 설정 필요)");
            return Ok(HttpResponse::Ok().finish());
        }
    };

    // lineItems 중 가장 늦은 만료 시각
    let expiry = subscription.line_items.as_ref().and_then(|items| {
        items
            .iter()
            .filter_map(|li| li.expiry_time.as_deref())
            .filter_map(|t| DateTime::parse_from_rfc3339(t).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .max()
    });

    // 4. 이후는 handle_appstore_notification 과 동일하게 처리
    let notification_type = sub_noti.notification_type;
    let subscription_id = sub_noti.subscription_id.unwrap_or_default();

    let block_result = web::block(move || {
        let conn = &mut db.get().unwrap();

        match notification_type {
            // RECOVERED(1) / RENEWED(2) / PURCHASED(4) / RESTARTED(7) → 구독 활성
            1 | 2 | 4 | 7 => {
                if let Some(expiry_date) = expiry {
                    diesel::update(users::table.find(user_id))
                        .set(users::premium_expire_at.eq(Some(expiry_date)))
                        .execute(conn)
                        .map_err(handler_disel_error)?;
                    info!("User {} premium extended to {:?} (Play)", user_id, expiry_date);

                    // 신규 구매(PURCHASED=4)일 때만 메일 발송 (갱신/복구/재시작은 메일 생략)
                    if notification_type == 4 {
                        let email_body = format!(
                            "[Play Store / Android] 구독자 도착 🎉\nuser_id: {}\nsubscription_id: {}\nexpiry_date: {:?}",
                            user_id, subscription_id, expiry_date
                        );
                        if let Ok(mut child) = std::process::Command::new("mail")
                            .arg("-s")
                            .arg("[Barnote] 새로운 프리미엄 구독 결제 발생 (Play Store / Android)")
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
                    }
                } else {
                    info!("[PlayStore] 활성 노티지만 만료 시각 없음 user={}", user_id);
                }
            }
            // REVOKED(12) / EXPIRED(13) → 프리미엄 해제
            12 | 13 => {
                diesel::update(users::table.find(user_id))
                    .set(users::premium_expire_at.eq(None::<DateTime<Utc>>))
                    .execute(conn)
                    .map_err(handler_disel_error)?;
                info!("User {} premium expired/removed (Play)", user_id);
            }
            other => {
                info!("[PlayStore] notificationType {} 미처리", other);
            }
        }
        Ok::<(), CommonResponseError>(())
    })
    .await;

    // Pub/Sub ack: 처리 실패해도 200 반환(영구 오류 무한 재전송 방지), 단 로깅
    match block_result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => error!("[PlayStore] DB 처리 오류: {:?}", e),
        Err(e) => error!("[PlayStore] 블로킹 실행 오류: {:?}", e),
    }

    Ok(HttpResponse::Ok().finish())
}
