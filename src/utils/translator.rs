use reqwest;
use serde::Deserialize;
use std::env;
use regex::Regex;
use std::sync::OnceLock;
use log::error;

#[derive(Deserialize)]
struct DeepLResponse {
    translations: Vec<DeepLTranslation>,
}

#[derive(Deserialize)]
struct DeepLTranslation {
    text: String,
}

pub async fn translate_to_english_if_cjk(text: &str) -> String {
    // 1. Check if string contains CJK characters using regex
    static RE_CJK: OnceLock<Regex> = OnceLock::new();
    let re_cjk = RE_CJK.get_or_init(|| Regex::new(r"[\p{Hangul}\p{Han}\p{Hiragana}\p{Katakana}]").unwrap());

    if !re_cjk.is_match(text) {
        // No CJK characters found, return original string to save API costs
        return text.to_string();
    }

    // 2. Read DeepL API Key from environment
    let api_key = match env::var("DEEPL_API_KEY") {
        Ok(k) if !k.is_empty() => k,
        _ => return text.to_string(), // If no key, fallback to original
    };

    // 3. Request DeepL API (assuming Free API endpoint)
    let url = "https://api-free.deepl.com/v2/translate";

    let client = reqwest::Client::new();
    let res = client.post(url)
        .header("Authorization", format!("DeepL-Auth-Key {}", api_key))
        .form(&[
            ("text", text),
            ("target_lang", "EN-US"),
        ])
        .send()
        .await;

    match res {
        Ok(response) if response.status().is_success() => {
            if let Ok(json) = response.json::<DeepLResponse>().await {
                if let Some(trans) = json.translations.first() {
                    println!(">>> [DeepL] 변환 결과: \"{}\" -> \"{}\"", text, trans.text);
                    return trans.text.clone();
                }
            }
        },
        Ok(response) => {
            error!("[DeepL Error] API returned status: {}", response.status());
        }
        Err(e) => {
            error!("[DeepL Request Error] {}", e);
        }
    }

    // Fallback if anything fails
    text.to_string()
}
