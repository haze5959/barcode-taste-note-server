use serde::{Deserialize, Serialize};
use std::env;
use base64::{Engine as _, engine::general_purpose::STANDARD};
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
    pub category: String,
    pub details: Option<serde_json::Value>,
}

pub async fn analyze_image_with_gemini(r2: &crate::utils::r2::R2Client, image_id_str: &str) -> Result<GeminiProductAnalysis, String> {
    let key = format!("images/{}", image_id_str);
    
    let result: Result<(GeminiProductAnalysis, String), String> = async {
        let api_key = env::var("GEMINI_API_KEY").map_err(|_| "GEMINI_API_KEY is missing".to_string())?;
        
        let image_bytes = r2.get_image(&key).await.map_err(|e| format!("Failed to read image from R2 {}: {:?}", key, e))?;
        let base64_image = STANDARD.encode(image_bytes);

        let prompt = "Analyze image for F&B/alcohol. If NOT F&B, return: {\"error\":\"Not an F&B product\"}.
Even if there are multiple products in the image, select and analyze only the single most prominent product and return a single JSON object. DO NOT return a JSON array under any circumstances.
Name: Core English name ONLY. No promo/limited/seasonal/capacity info. No hyphens. Title Case. KEEP aging/vintage as \"X Years Old\" (e.g., 7YO/7yo/7 year old -> \"7 Years Old\"). If it is a wine, KEEP the vintage year in the name (e.g. \"2019\"). KEEP brand prefix if name alone is just a flavor/color/descriptor (e.g., \"Cherry Liqueur\" -> \"Quaglia Cherry\").
Desc: Professional factual English desc (<200 chars). No repeating name. Include production methods, flavor markers, market specs.
Category: wine, whisky, beer, soju, sake, liqueur, spirit, beverage.
Return JSON: {\"name\":\"...\",\"description\":\"...\",\"category\":\"...\",\"details\":{\"style\":<int>,\"manufacturer\":\"<str>\",\"country\":\"<2-letter_iso>\",\"alcohol\":<float>,\"grape\":<int>,\"ibu\":<int>}}
Rules for 'details': 'grape' ONLY if wine. 'ibu' ONLY if beer. Use null for any field you are not confident about.
STYLE: Wine(0:red,1:white,2:rose,3:sparkling,4:dessert,5:fortified,6:natural),Whisky(100:singleMalt,101:blended,102:singleGrain,103:bourbon,104:rye,105:tennessee,106:irish,107:japanese,108:canadian,109:other),Beer(200:lager,201:pilsner,202:paleAle,203:ipa,204:hazyIpa,205:stout,206:porter,207:wheat,208:sour,209:belgianAle,210:amber),Asian(300:soju,301:fruitSoju,302:junmai,303:junmaiGinjo,304:junmaiDaiginjo,305:ginjo,306:daiginjo,307:honjozo,308:nigori,309:cheongju,310:yakju,311:makgeolli),Spirits(400:vodka,401:gin,402:lightRum,403:darkRum,404:spicedRum,405:tequila,406:mezcal,407:brandy,408:cognac,409:armagnac,410:absinthe,411:baijiu,412:liqueur),Cocktail(500:classic,501:craft,502:tiki,503:sour,504:highball,505:frozen,506:mocktail),Coffee(600:espresso,601:americano,602:latte,603:cappuccino,604:macchiato,605:flatWhite,606:mocha,607:drip,608:pourOver,609:coldBrew,610:singleOrigin),Other(700:other)
GRAPE(Wine ONLY): Red(0:cabSauv,1:merlot,2:pinotNoir,3:syrah,4:malbec,5:sangiovese,6:tempranillo,7:nebbiolo,8:grenache,9:zinfandel,10:cabFranc,11:carmenere,12:gamay,13:montepulciano,14:petitVerdot),White(100:chardonnay,101:sauvBlanc,102:riesling,103:pinotGrigio,104:gewurztraminer,105:cheninBlanc,106:viognier,107:semillon,108:moscato,109:albarino,110:pinotBlanc),Other(200:redBlend,201:whiteBlend,299:other)";

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

        let url = format!("https://generativelanguage.googleapis.com/v1beta/models/gemini-3.1-flash-lite:generateContent?key={}", api_key);
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

    let (analysis_res, log_text, is_success) = match result {
        Ok((analysis, text)) => (Ok(analysis), text, true),
        Err(e) => (Err(e.clone()), format!("ERROR: {}", e), false),
    };

    crate::utils::logger::log_gemini_request(is_success, image_id_str, &log_text).await;

    analysis_res
}

#[derive(Deserialize, Debug)]
pub struct GeminiScrapeInfo {
    pub category: String,
    pub description: String,
    pub details: Option<serde_json::Value>,
}

pub async fn generate_product_info_with_gemini(product_name: &str) -> Option<GeminiScrapeInfo> {
    let api_key = std::env::var("GEMINI_API_KEY").ok()?;
    
    let prompt = format!(
        "Analyze '{}'. Provide factual, encyclopedia-style English desc (<200 chars). No repeating name. Include production info, flavor profile, market specs.
Identify 'category' from: wine, whisky, beer, soju, sake, liqueur, spirit, beverage.
Return JSON: {{\"category\":\"...\",\"description\":\"...\",\"details\":{{\"style\":<int>,\"manufacturer\":\"<str>\",\"country\":\"<2-letter_iso>\",\"alcohol\":<float>,\"grape\":<int>,\"ibu\":<int>}}}}
Rules for 'details': 'grape' ONLY if wine. 'ibu' ONLY if beer. Use null for any field you are not confident about.
STYLE: Wine(0:red,1:white,2:rose,3:sparkling,4:dessert,5:fortified,6:natural),Whisky(100:singleMalt,101:blended,102:singleGrain,103:bourbon,104:rye,105:tennessee,106:irish,107:japanese,108:canadian,109:other),Beer(200:lager,201:pilsner,202:paleAle,203:ipa,204:hazyIpa,205:stout,206:porter,207:wheat,208:sour,209:belgianAle,210:amber),Asian(300:soju,301:fruitSoju,302:junmai,303:junmaiGinjo,304:junmaiDaiginjo,305:ginjo,306:daiginjo,307:honjozo,308:nigori,309:cheongju,310:yakju,311:makgeolli),Spirits(400:vodka,401:gin,402:lightRum,403:darkRum,404:spicedRum,405:tequila,406:mezcal,407:brandy,408:cognac,409:armagnac,410:absinthe,411:baijiu,412:liqueur),Cocktail(500:classic,501:craft,502:tiki,503:sour,504:highball,505:frozen,506:mocktail),Coffee(600:espresso,601:americano,602:latte,603:cappuccino,604:macchiato,605:flatWhite,606:mocha,607:drip,608:pourOver,609:coldBrew,610:singleOrigin),Other(700:other)
GRAPE(Wine ONLY): Red(0:cabSauv,1:merlot,2:pinotNoir,3:syrah,4:malbec,5:sangiovese,6:tempranillo,7:nebbiolo,8:grenache,9:zinfandel,10:cabFranc,11:carmenere,12:gamay,13:montepulciano,14:petitVerdot),White(100:chardonnay,101:sauvBlanc,102:riesling,103:pinotGrigio,104:gewurztraminer,105:cheninBlanc,106:viognier,107:semillon,108:moscato,109:albarino,110:pinotBlanc),Other(200:redBlend,201:whiteBlend,299:other)",
        product_name
    );

    let request_body = GeminiRequest {
        contents: vec![Content {
            parts: vec![
                Part::Text { text: prompt }
            ]
        }]
    };

    let url = format!("https://generativelanguage.googleapis.com/v1beta/models/gemini-3.1-flash-lite:generateContent?key={}", api_key);
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
