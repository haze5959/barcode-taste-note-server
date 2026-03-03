use serde::{Deserialize, Serialize};
use std::env;
use std::path::PathBuf;
use tokio::fs;
use base64::{Engine as _, engine::general_purpose::STANDARD};

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
    pub category: String,
    pub image_url: Option<String>,
}

pub async fn analyze_image_with_gemini(image_id_str: &str) -> Result<GeminiProductAnalysis, String> {
    let api_key = env::var("GEMINI_API_KEY").map_err(|_| "GEMINI_API_KEY is missing".to_string())?;
    
    // Read local image
    let path = PathBuf::from("static").join("images").join(image_id_str);
    let image_bytes = fs::read(&path).await.map_err(|e| format!("Failed to read image at {:?}: {}", path, e))?;
    
    // Base64 encode
    let base64_image = STANDARD.encode(image_bytes);

    let prompt = "Analyze the provided image and correctly identify the precise alcoholic beverage product name (including any variants/editions). \
    Return its exact English name. Provide a brief English description strictly under 100 characters. \
    Identify the category as strictly one of: whisky, wine, beer, soju, sake, liqueur, spirit, cocktail, coffee, beverage. \
    Also, find and provide a direct URL to a representative public image of this specific product (low-resolution is preferred). If no appropriate image is found, you may omit the `image_url` field. \
    Return strictly in JSON format matching this structure: {\"name\": \"...\", \"description\": \"...\", \"category\": \"...\", \"image_url\": \"...\"} \
    The `name` and `category` fields are mandatory.";

    let request_body = GeminiRequest {
        contents: vec![Content {
            parts: vec![
                Part::Text { text: prompt.to_string() },
                Part::InlineData {
                    inline_data: InlineData {
                        mime_type: "image/jpeg".to_string(), // assuming jpeg for now, or could determine from magic bytes
                        data: base64_image,
                    }
                }
            ]
        }]
    };

    let url = format!("https://generativelanguage.googleapis.com/v1beta/models/gemini-1.5-flash:generateContent?key={}", api_key);
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

    // Parse specific JSON string blocks if markdown wrapping is used by Gemini
    let cleaned_text = text_output.trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let analysis: GeminiProductAnalysis = serde_json::from_str(cleaned_text)
        .map_err(|e| format!("Failed to decode JSON from Gemini: {} - {}", e, cleaned_text))?;

    Ok(analysis)
}
