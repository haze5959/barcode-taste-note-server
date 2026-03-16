use serde::{Deserialize, Serialize};
use std::env;
use tokio::fs;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use tokio::io::AsyncWriteExt;
use log::error;

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<Content>,
}

#[derive(Serialize)]
struct Content {
    parts: Vec<Part>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum Part {
    Text { text: String },
    InlineData { inline_data: InlineData },
}

#[derive(Serialize)]
struct InlineData {
    mime_type: String,
    data: String,
}

#[derive(Deserialize, Debug)]
pub struct GeminiResponse {
    candidates: Option<Vec<Candidate>>,
}

#[derive(Deserialize, Debug)]
pub struct Candidate {
    content: Option<CandidateContent>,
}

#[derive(Deserialize, Debug)]
pub struct CandidateContent {
    parts: Option<Vec<CandidatePart>>,
}

#[derive(Deserialize, Debug)]
pub struct CandidatePart {
    text: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct GeminiProductAnalysis {
    pub name: String,
    pub description: String,
    pub category: String
}

pub async fn analyze_image_with_gemini(r2: &crate::utils::r2::R2Client, image_id_str: &str) -> Result<GeminiProductAnalysis, String> {
    let key = format!("images/{}", image_id_str);
    
    let result: Result<(GeminiProductAnalysis, String), String> = async {
        let api_key = env::var("GEMINI_API_KEY").map_err(|_| "GEMINI_API_KEY is missing".to_string())?;
        
        let image_bytes = r2.get_image(&key).await.map_err(|e| format!("Failed to read image from R2 {}: {:?}", key, e))?;
        let base64_image = STANDARD.encode(image_bytes);

        let prompt = "Analyze the provided image and identify the alcoholic beverage or F&B product. \
        IMPORTANT RULES: \
        1. If the item in the image is clearly NOT a food, beverage, or alcoholic product, you MUST stop and return strictly this JSON: {\"error\": \"Not an F&B product\"}. \
        2. For the `name`, determine the core English product name ONLY. You MUST EXCLUDE any promotional subtitles, limited edition markers, seasonal artwork edition names, or capacity variants (e.g., if it is 'Suntory Royal Blended Whisky Sakura Blossom Limited Edition', return strictly 'Suntory Royal Blended Whisky'). \
        3. Provides a professional English description of the product using your extensive knowledge base. Include the brand, standard ABV, specific production methods (e.g., first press malt, aging types), and key flavor markers. \
        STRICTLY UNDER 200 characters. USE factual, encyclopedia-style language. \
        Avoid empty fillers or speculative hedges. For well-known products, you MUST include standard market specifications. \
        4. Identify the category as strictly one of: whisky, wine, beer, soju, sake, liqueur, spirit, cocktail, coffee, beverage. \
        Return strictly in JSON format matching this structure: {\"name\": \"...\", \"description\": \"...\", \"category\": \"...\"} \
        Unless Rule 1 applies, the `name`, `description`, and `category` fields are mandatory.";

        let request_body = GeminiRequest {
            contents: vec![Content {
                parts: vec![
                    Part::Text { text: prompt.to_string() },
                    Part::InlineData {
                        inline_data: InlineData {
                            mime_type: "image/jpeg".to_string(),
                            data: base64_image,
                        }
                    }
                ]
            }]
        };

        let url = format!("https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent?key={}", api_key);
        let client = reqwest::Client::new();
        let res = client.post(&url)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        if !res.status().is_success() {
            let err_text = res.text().await.unwrap_or_default();
            return Err(format!("Gemini API responded with error: {}", err_text));
        }

        let gemini_resp: GeminiResponse = res.json().await.map_err(|e| format!("Parsing failed: {}", e))?;
        let text_output = gemini_resp.candidates
            .and_then(|mut c| c.pop())
            .and_then(|c| c.content)
            .and_then(|mut content| content.parts.take())
            .and_then(|mut parts| parts.pop())
            .and_then(|p| p.text)
            .ok_or_else(|| "No text returned from Gemini".to_string())?;

        let cleaned_text = text_output.trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim()
            .to_string();

        let analysis: GeminiProductAnalysis = serde_json::from_str(&cleaned_text)
            .map_err(|e| format!("Failed to decode JSON from Gemini: {} - {}", e, cleaned_text))?;

        Ok((analysis, cleaned_text))
    }.await;

    let (analysis_res, log_text) = match result {
        Ok((analysis, text)) => (Ok(analysis), text),
        Err(e) => (Err(e.clone()), format!("ERROR: {}", e)),
    };

    // Create log entry, ensuring text is flat without newlines
    let time_str = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let flat_log_text = log_text.replace("\n", " ").replace("\r", "");
    let log_line = format!("{} : {} : {}\n", time_str, image_id_str, flat_log_text);
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open("gemini_requests.log").await {
        let _ = file.write_all(log_line.as_bytes()).await;
    }

    analysis_res
}

#[derive(Deserialize, Debug)]
pub struct GeminiScrapeInfo {
    pub category: String,
    pub description: String,
}

pub async fn generate_product_info_with_gemini(product_name: &str) -> Option<GeminiScrapeInfo> {
    let api_key = std::env::var("GEMINI_API_KEY").ok()?;
    
    let prompt = format!(
        "Analyze the product name '{}'. Provide a professional and detailed English description using your extensive knowledge. \
        Include the official brand, standard ABV, specific production characteristics (e.g., ingredients, aging, filtration), and key flavor profile markers. \
        STRICTLY UNDER 200 characters. Use factual, encyclopedia-style language. \
        For well-known alcoholic beverages, you MUST include standard market specifications rather than generic phrases. \
        Also identify the category as strictly one of: whisky, wine, beer, soju, sake, liqueur, spirit, cocktail, coffee, beverage. \
        Return strictly in JSON format matching this structure: {{\"category\": \"...\", \"description\": \"...\"}}",
        product_name
    );

    let request_body = GeminiRequest {
        contents: vec![Content {
            parts: vec![
                Part::Text { text: prompt }
            ]
        }]
    };

    let url = format!("https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent?key={}", api_key);
    let client = reqwest::Client::new();
    let res = client.post(&url)
        .json(&request_body)
        .send()
        .await
        .ok()?;

    if !res.status().is_success() {
        let err_text = res.text().await.unwrap_or_default();
        error!("[Scraper Gemini Error] {}", err_text);
        return None;
    }

    let gemini_resp: GeminiResponse = res.json().await.ok()?;
    let text_output = gemini_resp.candidates
        .and_then(|mut c| c.pop())
        .and_then(|c| c.content)
        .and_then(|mut content| content.parts.take())
        .and_then(|mut parts| parts.pop())
        .and_then(|p| p.text)?;

    let cleaned_text = text_output.trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim()
        .to_string();

    serde_json::from_str::<GeminiScrapeInfo>(&cleaned_text).ok()
}
