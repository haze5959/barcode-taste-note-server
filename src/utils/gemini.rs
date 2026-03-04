use serde::{Deserialize, Serialize};
use std::env;
use std::path::PathBuf;
use tokio::fs;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::Local;
use tokio::io::AsyncWriteExt;

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
    let path = PathBuf::from("static").join("images").join(image_id_str);
    
    let result: Result<(GeminiProductAnalysis, String), String> = async {
        let api_key = env::var("GEMINI_API_KEY").map_err(|_| "GEMINI_API_KEY is missing".to_string())?;
        
        let image_bytes = fs::read(&path).await.map_err(|e| format!("Failed to read image at {:?}: {}", path, e))?;
        let base64_image = STANDARD.encode(image_bytes);

        let prompt = "Analyze the provided image and correctly identify the precise alcoholic beverage product name (including any variants/editions). \
        Return its exact English name. Provide a brief English description of the product itself (including brand, ABV/alcohol content, aging years if available, and key characteristics) strictly under 200 characters. \
        Identify the category as strictly one of: whisky, wine, beer, soju, sake, liqueur, spirit, cocktail, coffee, beverage. \
        Also, find and provide a direct public URL for a high-quality, professional image of this specific product. \
        IMPORTANT: Your `image_url` must be the most representative and high-quality image result as found on Google Image Search. Prioritize professional studio photography showing the bottle clearly. \
        If no appropriate image is found, you may omit the `image_url` field. \
        Return strictly in JSON format matching this structure: {\"name\": \"...\", \"description\": \"...\", \"category\": \"...\", \"image_url\": \"...\"} \
        The `name` and `category` fields are mandatory.";

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

    // Create log entry
    let time_str = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let log_line = format!("{} : {} : {}\n", time_str, image_id_str, log_text);
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open("gemini_requests.log").await {
        let _ = file.write_all(log_line.as_bytes()).await;
    }

    // Move image to static/images/deleted/ and ensure .jpeg extension
    let deleted_dir = PathBuf::from("static").join("images").join("deleted");
    let _ = fs::create_dir_all(&deleted_dir).await;
    let new_filename = format!("{}.jpeg", image_id_str.trim_end_matches(".jpeg").trim_end_matches(".jpg"));
    let new_path = deleted_dir.join(new_filename);
    
    if fs::metadata(&path).await.is_ok() {
        if fs::rename(&path, &new_path).await.is_err() {
            if fs::copy(&path, &new_path).await.is_ok() {
                let _ = fs::remove_file(&path).await;
            }
        }
    }

    analysis_res
}
