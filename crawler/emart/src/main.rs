use dotenvy::dotenv;
use serde::{Deserialize, Serialize};
use std::env;
use std::time::Duration;
use tokio::time::sleep;

// ============================================================
// Emart Everyday API 응답 모델
// ============================================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EmartProduct {
    sku_code: Option<String>,
    product_name: Option<String>,
    category_mcode_name: Option<String>,
    product_thumbnail: Option<String>,
}

// ============================================================
// Gemini 요청/응답 모델
// ============================================================

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
}

#[derive(Serialize)]
struct GeminiContent {
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
struct GeminiPart {
    text: String,
}

#[derive(Deserialize, Debug)]
struct GeminiResponse {
    candidates: Option<Vec<GeminiCandidate>>,
}

#[derive(Deserialize, Debug)]
struct GeminiCandidate {
    content: Option<GeminiCandidateContent>,
}

#[derive(Deserialize, Debug)]
struct GeminiCandidateContent {
    parts: Option<Vec<GeminiCandidatePart>>,
}

#[derive(Deserialize, Debug)]
struct GeminiCandidatePart {
    text: Option<String>,
}

// ============================================================
// 출력 JSON 모델
// ============================================================

#[derive(Debug, Serialize, Deserialize)]
struct OutputProduct {
    barcode: String,
    product_name: String,
    desc: String,
    #[serde(rename = "type")]
    type_: String,
    image_url: Option<String>,
}

// ============================================================
// 카테고리 매핑
// ============================================================

fn map_category(category_name: &str) -> &'static str {
    match category_name {
        "맥주" => "beer",
        "소주/전통주" => "soju",
        "위스키" => "whisky",
        "보드카/사케/백주" => "sake",
        "레드와인" | "화이트와인" | "스파클링와인" => "wine",
        _ => "beverage",
    }
}

// ============================================================
// Gemini 텍스트 호출 (productName → product_name + desc)
// ============================================================

const GEMINI_PROMPT_TEMPLATE: &str = r#"Analyze the provided product name and identify the alcoholic beverage or F&B product.
Follow these rules strictly to generate the response:

1. VALIDATION: If the item is NOT a food, beverage, or alcoholic product, return strictly: {"error": "Not an F&B product"}.

2. PRODUCT NAME:
   - Identify the core English product name.
   - EXCLUDE promotional subtitles, limited edition markers, seasonal artwork names, or capacity (ml/L).
   - EXCLUDE any packaging or container descriptors such as Can, Bottle, Draft, Draught, Pack, Q Pack, Keg, Box, Pouch, Cup, PET, or similar terms.
   - REMOVE special characters/symbols (e.g., hyphens).
   - Use Title Case (e.g., 'Suntory Royal Blended Whisky').
   - Examples: 'Asahi Super Dry Draft Beer Can 340ml' → 'Asahi Super Dry', 'Cass Fresh Q Pack 1.6L' → 'Cass Fresh'.

3. DESCRIPTION:
   - Provide a professional English description.
   - Include brand, standard ABV, production methods, and key flavor markers.
   - MANDATORY: Keep it factual, encyclopedia-style, and STRICTLY UNDER 200 characters.

4. OUTPUT FORMAT:
   - Return strictly in JSON format with the following keys:
     {
       "product_name": "Core Product Name",
       "desc": "Professional factual description under 200 characters"
     }

DO NOT include any conversational text, markdown blocks (unless requested), or extra fields outside of this JSON structure.

Product name to analyze: "{}""#;

async fn call_gemini(client: &reqwest::Client, api_key: &str, product_name: &str) -> Result<(String, String), String> {
    let prompt = GEMINI_PROMPT_TEMPLATE.replace("{}", product_name);

    let request_body = GeminiRequest {
        contents: vec![GeminiContent {
            parts: vec![GeminiPart { text: prompt }],
        }],
    };

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent?key={}",
        api_key
    );

    let res = client
        .post(&url)
        .json(&request_body)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !res.status().is_success() {
        let err_text = res.text().await.unwrap_or_default();
        return Err(format!("Gemini API error: {}", err_text));
    }

    let gemini_resp: GeminiResponse = res
        .json()
        .await
        .map_err(|e| format!("Parsing failed: {}", e))?;

    let text_output = gemini_resp
        .candidates
        .and_then(|mut c| c.pop())
        .and_then(|c| c.content)
        .and_then(|mut content| content.parts.take())
        .and_then(|mut parts| parts.pop())
        .and_then(|p| p.text)
        .ok_or_else(|| "Empty Gemini response".to_string())?;

    // JSON 블록 정리 (```json ... ``` 제거)
    let cleaned = text_output
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    // error 케이스 확인
    if cleaned.contains("\"error\"") {
        return Err(format!("Gemini returned error for: {}", product_name));
    }

    // product_name, desc 파싱
    let parsed: serde_json::Value = serde_json::from_str(cleaned)
        .map_err(|e| format!("JSON parse failed: {} | raw: {}", e, cleaned))?;

    let name = parsed["product_name"]
        .as_str()
        .ok_or_else(|| "Missing product_name field".to_string())?
        .to_string();
    let desc = parsed["desc"]
        .as_str()
        .ok_or_else(|| "Missing desc field".to_string())?
        .to_string();

    Ok((name, desc))
}

// ============================================================
// 이마트 에브리데이 API 호출 (productThumbnail -> 이미지 URL)
// ============================================================

const EMART_IMAGE_BASE: &str = "https://emile.emarteveryday.co.kr/product";

fn build_image_url(thumbnail: &str) -> String {
    // thumbnail 예: "/product/6901035603232/mwkk57jUL3fKnqc6.png"
    // → "https://emile.emarteveryday.co.kr/product/6901035603232/mwkk57jUL3fKnqc6.png"
    if thumbnail.starts_with("http") {
        thumbnail.to_string()
    } else {
        format!("{}{}", EMART_IMAGE_BASE.trim_end_matches("/product"), thumbnail)
    }
}

// ============================================================
// MAIN
// ============================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();

    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    let base_url = "https://emile.emarteveryday.co.kr/api/v1/product/ProductList?strCode=4006&lCode=025&mCode=&rowsPerPage=20&sort=best";

    let mut all_results: Vec<OutputProduct> = Vec::new();
    let mut page_num: u32 = 0;
    let mut consecutive_empty = 0;

    loop {
        let url = format!("{}&pageNum={}", base_url, page_num);
        println!("[Page {}] Fetching: {}", page_num, url);

        let resp = match client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[Page {}] Request error: {}", page_num, e);
                consecutive_empty += 1;
                if consecutive_empty >= 3 {
                    println!("3 consecutive errors. Stopping.");
                    break;
                }
                sleep(Duration::from_secs(2)).await;
                page_num += 1;
                continue;
            }
        };

        if !resp.status().is_success() {
            eprintln!("[Page {}] HTTP error: {}", page_num, resp.status());
            consecutive_empty += 1;
            if consecutive_empty >= 3 { break; }
            sleep(Duration::from_secs(2)).await;
            page_num += 1;
            continue;
        }

        // 응답이 배열인지 오브젝트인지 모두 처리
        let raw_text = resp.text().await.unwrap_or_default();
        let products: Vec<EmartProduct> = if raw_text.trim_start().starts_with('[') {
            serde_json::from_str(&raw_text).unwrap_or_default()
        } else {
            // wrapper 오브젝트 형태일 경우 data 필드 추출
            match serde_json::from_str::<serde_json::Value>(&raw_text) {
                Ok(v) => serde_json::from_value(v["data"].clone()).unwrap_or_default(),
                Err(_) => vec![],
            }
        };

        if products.is_empty() {
            println!("[Page {}] 빈 페이지 → 크롤링 종료", page_num);
            consecutive_empty += 1;
            if consecutive_empty >= 2 { break; }
            page_num += 1;
            continue;
        }

        consecutive_empty = 0;
        println!("[Page {}] {}개 제품 처리 중...", page_num, products.len());

        for prod in products {
            let barcode = match prod.sku_code {
                Some(ref c) if !c.is_empty() => c.clone(),
                _ => {
                    println!("  → 바코드 없음, 스킵");
                    continue;
                }
            };

            let raw_name = match prod.product_name {
                Some(ref n) if !n.is_empty() => n.clone(),
                _ => {
                    println!("  → 제품명 없음, 스킵");
                    continue;
                }
            };

            let type_ = prod.category_mcode_name
                .as_deref()
                .map(map_category)
                .unwrap_or("beverage")
                .to_string();

            let image_url = prod.product_thumbnail
                .as_deref()
                .filter(|t| !t.is_empty())
                .map(build_image_url);

            // Gemini 호출
            match call_gemini(&client, &api_key, &raw_name).await {
                Ok((product_name, desc)) => {
                    println!("  ✓ {} → {}", raw_name, product_name);
                    all_results.push(OutputProduct {
                        barcode,
                        product_name,
                        desc,
                        type_,
                        image_url,
                    });
                }
                Err(e) => {
                    println!("  ✗ {} 실패: {}", raw_name, e);
                }
            }

            // API 부하 방지
            sleep(Duration::from_millis(500)).await;
        }

        // 결과 중간 저장 (각 페이지 완료 후)
        save_output(&all_results);

        page_num += 1;
        sleep(Duration::from_secs(1)).await;
    }

    println!("\n완료. 총 {}개 제품 저장됨.", all_results.len());
    save_output(&all_results);

    Ok(())
}

fn save_output(products: &[OutputProduct]) {
    let json_str = match serde_json::to_string_pretty(products) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("JSON 직렬화 실패: {}", e);
            return;
        }
    };

    if let Err(e) = std::fs::write("output/new_product.json", &json_str) {
        // output 폴더 없으면 생성 후 재시도
        let _ = std::fs::create_dir_all("output");
        if let Err(e2) = std::fs::write("output/new_product.json", &json_str) {
            eprintln!("파일 저장 실패: {} / {}", e, e2);
        }
    }
}
