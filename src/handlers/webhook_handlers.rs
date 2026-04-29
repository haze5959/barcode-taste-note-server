use crate::Pool;
use crate::diesel::{ExpressionMethods, QueryDsl, RunQueryDsl};
use crate::errors::CommonResponseError;
use crate::errors::handler_disel_error;
use crate::schema::users;
use actix_web::{web, HttpResponse, Error};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
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

    let user_id = match data.app_account_token {
        Some(uid) => uid,
        None => {
            info!("Notification skipped: No appAccountToken (user_id) found");
            return Ok(HttpResponse::Ok().finish());
        }
    };

    let tx_info: Option<TransactionInfo> = data.signed_transaction_info
        .as_ref()
        .and_then(|info_str| decode_unverified(info_str));

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
                    
                    // 이메일 전송 (mail 명령어 호출)
                    let email_body = format!("구독자 도착 🎉\nuser_id: {}\nexpiry_date: {:?}", user_id, expiry_date);
                    if let Ok(mut child) = std::process::Command::new("mail")
                        .arg("-s")
                        .arg("[Barnote] 새로운 프리미엄 구독 결제 발생")
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
