use dotenvy::dotenv;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::env;
use std::time::Duration;
use tokio::time::sleep;

// ============================================================
// 크롤링 대상 컬렉션 목록 및 타입 하드코딩
// ============================================================

struct CollectionConfig {
    path: &'static str,
    type_: &'static str,
}

const COLLECTIONS: &[CollectionConfig] = &[
    // 완료
    // CollectionConfig { path: "tequila", type_: "spirit" },
    CollectionConfig { path: "vodka", type_: "spirit" },

    // 대기
    // CollectionConfig { path: "whisky", type_: "whisky" },
    // CollectionConfig { path: "red-wine", type_: "wine" },
    // CollectionConfig { path: "white-wine", type_: "wine" },
    // CollectionConfig { path: "beer", type_: "beer" },
    // CollectionConfig { path: "gin", type_: "spirit" },
    // CollectionConfig { path: "rum", type_: "spirit" },
    // CollectionConfig { path: "liqueur", type_: "spirit" },
    // CollectionConfig { path: "cognac-brandy", type_: "spirit" },
    // CollectionConfig { path: "coffee", type_: "coffee" },
];

// ============================================================
// Shopify meta 변수에서 파싱할 제품 모델
// ============================================================

#[derive(Debug, Deserialize)]
struct ShopifyMeta {
    products: Vec<ShopifyProduct>,
}

#[derive(Debug, Deserialize)]
struct ShopifyProduct {
    variants: Vec<ShopifyVariant>,
}

#[derive(Debug, Deserialize)]
struct ShopifyVariant {
    name: String,
    sku: Option<String>,
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
   - EXCLUDE category names from the end of the product name, such as Whiskey, Whisky, Wine, Sake, Soju, Beer, etc.
   - Use Title Case.
   - Examples: 'Jack Daniel's Fire Whiskey 0.7L (5099873006504)' → 'Jack Daniel's Fire', 'Jim Beam Whiskey' → 'Jim Beam'.

3. DESCRIPTION:
   - Provide a professional English description WITHOUT repeating or starting with the product name.
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
// HTML 파싱: meta 변수에서 제품 정보 추출
// ============================================================

fn parse_meta_products(html: &str) -> Vec<ShopifyProduct> {
    // var meta = {"products":[...]}; 패턴을 찾아서 JSON 파싱
    let re = Regex::new(r#"var\s+meta\s*=\s*(\{.*?"products"\s*:\s*\[.*?\]\s*\})"#).unwrap();

    // 위 regex는 greedy하므로 대신 시작점 찾고 수동 파싱
    if let Some(start_idx) = html.find("var meta = {") {
        let json_start = start_idx + "var meta = ".len();
        // { 이후 중괄호 매칭으로 끝 찾기
        let bytes = html.as_bytes();
        let mut depth = 0;
        let mut json_end = json_start;
        for (i, &b) in bytes[json_start..].iter().enumerate() {
            match b {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        json_end = json_start + i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }

        let json_str = &html[json_start..json_end];
        match serde_json::from_str::<ShopifyMeta>(json_str) {
            Ok(meta) => return meta.products,
            Err(e) => {
                eprintln!("  meta JSON 파싱 실패: {}", e);
            }
        }
    }

    // 대안: regex로 products 배열만 추출
    let _ = re;
    vec![]
}

// ============================================================
// HTML 파싱: data-src에서 이미지 URL 추출
// ============================================================

fn parse_image_urls(html: &str) -> Vec<String> {
    let re = Regex::new(
        r#"data-src="(//spades\.com\.mt/cdn/shop/[^"]*\{width\}x\.[^"]*?)""#
    ).unwrap();

    let width_re = Regex::new(r#"\{width\}"#).unwrap();
    let v_param_re = Regex::new(r#"\?v=\d+"#).unwrap();

    re.captures_iter(html)
        .map(|cap| {
            let raw = cap[1].to_string();
            // {width} → 500
            let url = width_re.replace(&raw, "500").to_string();
            // ?v=... 제거
            let url = v_param_re.replace(&url, "").to_string();
            // // → https://
            if url.starts_with("//") {
                format!("https:{}", url)
            } else {
                url
            }
        })
        .collect()
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
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36")
        .default_headers({
            let mut h = reqwest::header::HeaderMap::new();
            h.insert(reqwest::header::ACCEPT, "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8".parse().unwrap());
            h.insert(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9".parse().unwrap());
            h
        })
        .build()?;

    let mut all_results: Vec<OutputProduct> = Vec::new();

    for collection in COLLECTIONS {
        println!("\n========================================");
        println!("컬렉션 크롤링 시작: {} (type: {})", collection.path, collection.type_);
        println!("========================================");

        let base_url = format!("https://spades.com.mt/collections/{}", collection.path);
        let mut page_num: u32 = 1;
        let mut consecutive_empty = 0;

        loop {
            let url = format!("{}?page={}", base_url, page_num);
            println!("\n[Page {}] Fetching: {}", page_num, url);

            let resp = match client.get(&url).send().await {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("[Page {}] Request error: {}", page_num, e);
                    consecutive_empty += 1;
                    if consecutive_empty >= 3 {
                        println!("3 consecutive errors. Stopping collection.");
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

            let html = resp.text().await.unwrap_or_default();

            // meta 변수에서 제품 정보 추출
            let products = parse_meta_products(&html);
            // data-src에서 이미지 URL 추출
            let image_urls = parse_image_urls(&html);

            if products.is_empty() {
                println!("[Page {}] 제품 없음 → 다음 컬렉션으로", page_num);
                consecutive_empty += 1;
                if consecutive_empty >= 2 { break; }
                page_num += 1;
                continue;
            }

            consecutive_empty = 0;
            println!("[Page {}] {}개 제품, {}개 이미지 발견", page_num, products.len(), image_urls.len());

            for (i, prod) in products.iter().enumerate() {
                let variant = match prod.variants.first() {
                    Some(v) => v,
                    None => {
                        println!("  → variant 없음, 스킵");
                        continue;
                    }
                };

                let raw_name = &variant.name;
                if raw_name.is_empty() {
                    println!("  → 제품명 없음, 스킵");
                    continue;
                }

                let barcode = match &variant.sku {
                    Some(s) if !s.is_empty() => s.clone(),
                    _ => {
                        println!("  → 바코드(SKU) 없음, 스킵: {}", raw_name);
                        continue;
                    }
                };

                // 이미지 URL 매칭 (순서 동일)
                let image_url = image_urls.get(i).cloned();

                let type_ = collection.type_.to_string();

                // Gemini 호출
                match call_gemini(&client, &api_key, raw_name).await {
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
