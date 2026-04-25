use serde_json::json;
use log::{error, info};
use actix_web::web;
use diesel::prelude::*;
use crate::schema::fcm_tokens;

pub async fn send_fcm_push(
    db: web::Data<crate::Pool>,
    token: &str,
    loc_key: &str,
    loc_args: Vec<String>,
    data_user_id: &str,
    data_type: &str,
) {
    let provider = match gcp_auth::provider().await {
        Ok(p) => p,
        Err(e) => {
            error!("Failed to create GCP provider: {}", e);
            return;
        }
    };

    let project_id = match provider.project_id().await {
        Ok(pid) => pid,
        Err(e) => {
            error!("Failed to get GCP project_id: {}", e);
            return;
        }
    };

    let auth_token = match provider.token(&["https://www.googleapis.com/auth/firebase.messaging"]).await {
        Ok(t) => t.as_str().to_string(),
        Err(e) => {
            error!("Failed to get GCP token: {}", e);
            return;
        }
    };

    let url = format!("https://fcm.googleapis.com/v1/projects/{}/messages:send", project_id);

    // FCM v1 JSON Payload
    let payload = json!({
        "message": {
            "token": token,
            "android": {
                "notification": {
                    "body_loc_key": loc_key,
                    "body_loc_args": loc_args
                }
            },
            "apns": {
                "payload": {
                    "aps": {
                        "alert": {
                            "loc-key": loc_key,
                            "loc-args": loc_args
                        }
                    }
                }
            },
            "data": {
                "user_id": data_user_id,
                "type": data_type
            }
        }
    });

    let client = reqwest::Client::new();
    let res = client.post(&url)
        .bearer_auth(auth_token)
        .json(&payload)
        .send()
        .await;

    match res {
        Ok(r) => {
            if !r.status().is_success() {
                let status = r.status();
                let err_text = r.text().await.unwrap_or_default();
                if status == 404 && err_text.contains("UNREGISTERED") {
                    info!("FCM Token UNREGISTERED. Auto-removed token from DB: {}", token);
                    let token_clone = token.to_string();
                    let _ = web::block(move || {
                        if let Ok(mut conn) = db.get() {
                            let _ = diesel::delete(fcm_tokens::table.filter(fcm_tokens::token.eq(token_clone)))
                                .execute(&mut conn);
                        }
                    }).await;
                } else {
                    error!("FCM Error {}: {}", status, err_text);
                }
            }
        }
        Err(e) => {
            error!("FCM Network Error: {}", e);
        }
    }
}
