use dotenvy::dotenv;
use reqwest::Client;
use uuid::Uuid;
use chrono::Utc;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::time::sleep;
use regex::Regex;
use std::fs::OpenOptions;
use std::io::Write;

mod db;
mod models;
mod schema;
mod openai;
mod r2;

use models::OpenFoodFactsResponse;
use db::{establish_connection, barcode_exists, insert_product, insert_barcode, insert_product_image, NewProduct, NewBarcode, NewProductImage};
use r2::R2Client;
use serde::{Deserialize, Serialize};

include!("../../src/utils/block_list.rs");


fn clean_product_name(name: &str) -> String {
    let mut cleaned = name.replace("&quot;", "\"").replace("&amp;", "&");
    
    static RE_PARENS: OnceLock<Regex> = OnceLock::new();
    let re_parens = RE_PARENS.get_or_init(|| Regex::new(r"\(.*?\)").unwrap());
    cleaned = re_parens.replace_all(&cleaned, " ").to_string();
    
    static RE_YEARS: OnceLock<Regex> = OnceLock::new();
    let re_years = RE_YEARS.get_or_init(|| Regex::new(r"(?i)\b(?:aged\s+)?(\d+)\s*(?:years?(?:\s*old)?|y\.?o\.?)\b").unwrap());
    cleaned = re_years.replace_all(&cleaned, "${1} Years Old").to_string();
    
    static RE_ABV: OnceLock<Regex> = OnceLock::new();
    let re_abv = RE_ABV.get_or_init(|| Regex::new(r"(?i)\d+(\.\d+)?\s*%\s*(vol\.?)?").unwrap());
    cleaned = re_abv.replace_all(&cleaned, " ").to_string();
    
    static RE_MEASURE: OnceLock<Regex> = OnceLock::new();
    let re_measure = RE_MEASURE.get_or_init(|| Regex::new(r"(?i)\b\d+(\.\d+)?\s*(ml|cl|lt|l|liter|liters|litre|litres|g|kg|mg|oz|fl\.?\s*oz|lb|lbs)\b").unwrap());
    cleaned = re_measure.replace_all(&cleaned, " ").to_string();
    
    static RE_QTY: OnceLock<Regex> = OnceLock::new();
    let re_qty = RE_QTY.get_or_init(|| Regex::new(r"(?i)\b(x\s*\d+\s*(pcs|pack|packs|ea)?|\d+\s*(pcs|pack|packs|ea|bottles|cans))\b").unwrap());
    cleaned = re_qty.replace_all(&cleaned, " ").to_string();
    
    static RE_SPAM: OnceLock<Regex> = OnceLock::new();
    let re_spam = RE_SPAM.get_or_init(|| Regex::new(r"(?i)\b(empty|can only|no drink|used|aluminum|pull tab|beer can|from \d{4})\b").unwrap());
    cleaned = re_spam.replace_all(&cleaned, " ").to_string();
    
    static RE_SYMBOLS: OnceLock<Regex> = OnceLock::new();
    let re_symbols = RE_SYMBOLS.get_or_init(|| Regex::new(r"[-_—|/·,]").unwrap());
    cleaned = re_symbols.replace_all(&cleaned, " ").to_string();

    static RE_SPACES: OnceLock<Regex> = OnceLock::new();
    let re_spaces = RE_SPACES.get_or_init(|| Regex::new(r"\s{2,}").unwrap());
    cleaned = re_spaces.replace_all(&cleaned, " ").to_string();

    // 이름 끝에 남는 단독 숫자·개수·용량 패턴 반복 제거
    // 예: "6 pack", "x1", "1 LT", ", 5", ", 0" 등
    static RE_TRAILING: OnceLock<Regex> = OnceLock::new();
    let re_trailing = RE_TRAILING.get_or_init(|| {
        Regex::new(r"(?i)\s+(x\s*\d+|\d+\s*(pack|packs|lt|l|ml|cl|g|kg|oz|pcs|ea|bottles|cans)?|\d+)$").unwrap()
    });
    loop {
        let next = re_trailing.replace(&cleaned, "").to_string();
        let next = next.trim().trim_end_matches(&[',', '-', ' ', '.'][..]).trim().to_string();
        if next == cleaned.trim() {
            break;
        }
        cleaned = next;
    }

    // Remove trailing commas, hyphens, spaces, or dots
    let trimmed = cleaned.trim().trim_end_matches(&[',', '-', ' ', '.'][..]).trim().to_string();

    // Title Case 변환: 각 단어의 첫 글자를 대문자로
    trimmed
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn log_failed_page(page: i64) {
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open("failed_pages.txt") {
        let _ = writeln!(file, "{}", page);
    }
}

fn parse_category(tags: &Option<Vec<String>>) -> i16 {
    if let Some(tags) = tags {
        let tags_str = tags.join(" ").to_lowercase();
        if tags_str.contains("wine") { return 0; }
        if tags_str.contains("whisky") || tags_str.contains("whiskies") { return 1; }
        if tags_str.contains("beer") { return 2; }
        if tags_str.contains("soju") || tags_str.contains("sake") { return 3; }
        if tags_str.contains("liqueur") || tags_str.contains("liquor") || tags_str.contains("spirit") { return 4; }
        if tags_str.contains("beverage") { return 7; }
    }
    8
}

fn build_desc(brands: &Option<String>) -> Option<String> {
    brands.as_ref().filter(|b| !b.is_empty()).cloned()
}

async fn download_image(client: &Client, r2: &R2Client, url: &str, image_id: Uuid) -> Result<(), Box<dyn std::error::Error>> {
    let resp = client.get(url).send().await?;
    if resp.status().is_success() {
        let bytes = resp.bytes().await?;
        let key = format!("images/{}", image_id);
        r2.upload_image(&key, bytes.to_vec(), "image/jpeg").await?;
    }
    Ok(())
}

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
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Option<Vec<Candidate>>,
}

#[derive(Deserialize)]
struct Candidate {
    content: Option<CandidateContent>,
}

#[derive(Deserialize)]
struct CandidateContent {
    parts: Option<Vec<CandidatePart>>,
}

#[derive(Deserialize)]
struct CandidatePart {
    text: Option<String>,
}

#[derive(Deserialize, Debug)]
struct OutputProduct {
    name: Option<String>,
    description: Option<String>,
    category: Option<String>,
    details: Option<serde_json::Value>,
    error: Option<String>,
}

const GEMINI_PROMPT_TEMPLATE: &str = r#"Analyze F&B product name. If NOT F&B, return: {"error":"Not an F&B product"}.
Name: Core English name ONLY. EXCLUDE promo/limited/seasonal/capacity/containers(Can,Bottle,Box,etc)/category suffixes(Whisky,Wine,Beer,etc). No hyphens. Title Case. KEEP aging/vintage as "X Years Old" (e.g., 7YO/7yo/7 year old → "7 Years Old"). If it is a wine, KEEP the vintage year in the name (e.g. "2019"). KEEP brand prefix if name alone is just a flavor/color/descriptor (e.g., "Cherry Liqueur" → "Quaglia Cherry", "Pistachio Cream" → "Cellini Crema Di Pistacchio").
Desc: Professional factual English desc (<200 chars). No repeating name. Include production methods, flavor markers, market specs.
Return JSON: {"name":"...","description":"...","category":"...","details":{"style":<int>,"manufacturer":"<str>","country":"<2-letter_iso>","alcohol":<float>,"grape":<int>,"ibu":<int>}}
Rules for 'details': 'grape' ONLY if wine. 'ibu' ONLY if beer. Use null for any field you are not confident about.
STYLE: Wine(0:red,1:white,2:rose,3:sparkling,4:dessert,5:fortified,6:natural),Whisky(100:singleMalt,101:blended,102:singleGrain,103:bourbon,104:rye,105:tennessee,106:irish,107:japanese,108:canadian,109:other),Beer(200:lager,201:pilsner,202:paleAle,203:ipa,204:hazyIpa,205:stout,206:porter,207:wheat,208:sour,209:belgianAle,210:amber),Asian(300:soju,301:fruitSoju,302:junmai,303:junmaiGinjo,304:junmaiDaiginjo,305:ginjo,306:daiginjo,307:honjozo,308:nigori,309:cheongju,310:yakju,311:makgeolli),Spirits(400:vodka,401:gin,402:lightRum,403:darkRum,404:spicedRum,405:tequila,406:mezcal,407:brandy,408:cognac,409:armagnac,410:absinthe,411:baijiu,412:liqueur),Cocktail(500:classic,501:craft,502:tiki,503:sour,504:highball,505:frozen,506:mocktail),Coffee(600:espresso,601:americano,602:latte,603:cappuccino,604:macchiato,605:flatWhite,606:mocha,607:drip,608:pourOver,609:coldBrew,610:singleOrigin),Other(700:other)
GRAPE(Wine ONLY): Red(0:cabSauv,1:merlot,2:pinotNoir,3:syrah,4:malbec,5:sangiovese,6:tempranillo,7:nebbiolo,8:grenache,9:zinfandel,10:cabFranc,11:carmenere,12:gamay,13:montepulciano,14:petitVerdot),White(100:chardonnay,101:sauvBlanc,102:riesling,103:pinotGrigio,104:gewurztraminer,105:cheninBlanc,106:viognier,107:semillon,108:moscato,109:albarino,110:pinotBlanc),Other(200:redBlend,201:whiteBlend,299:other)
Product name: "{}"
Brand: "{}" "#;

async fn call_gemini(client: &reqwest::Client, api_key: &str, product_name: &str, brands: &str) -> Result<OutputProduct, String> {
    let prompt = GEMINI_PROMPT_TEMPLATE
        .replace("{}", product_name)
        .replacen("{}", brands, 1);

    let request_body = GeminiRequest {
        contents: vec![Content {
            parts: vec![Part::Text { text: prompt }],
        }],
    };

    let url = format!("https://generativelanguage.googleapis.com/v1beta/models/gemini-3.1-flash-lite:generateContent?key={}", api_key);
    let res = client.post(&url)
        .json(&request_body)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !res.status().is_success() {
        return Err(format!("Status {}", res.status()));
    }

    let gemini_resp: GeminiResponse = res.json().await.map_err(|e| e.to_string())?;
    let text_output = gemini_resp.candidates
        .and_then(|mut c| c.pop())
        .and_then(|c| c.content)
        .and_then(|mut content| content.parts.take())
        .and_then(|mut parts| parts.pop())
        .and_then(|p| p.text)
        .ok_or("No text in response")?;

    let cleaned_text = text_output.trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim()
        .to_string();

    serde_json::from_str::<OutputProduct>(&cleaned_text).map_err(|e| e.to_string())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    
    let mut conn = establish_connection();
    let client = Client::new();
    let r2 = R2Client::new().await;

    // .env의 OFW_SESSION_COOKIE 값으로 session 쿠키를 구성합니다.
    // 값이 없으면 기존 방식(쿠키 없음)으로 진행됩니다.
    let session_value = std::env::var("OFW_SESSION_COOKIE").unwrap_or_default();
    let cookie_header = if session_value.is_empty() {
        None
    } else {
        Some(format!("session={}", session_value))
    };
    
    let mut consecutive_fails = 0;
    
    for page in 1.. {
        println!("Crawling page: {}", page);
        let url = format!("https://world.openfoodfacts.org/category/alcoholic-beverages.json?sort_by=created_t&page={}&page_size=50&fields=code,product_name,brands,categories_tags,image_url", page);
        
        let mut req = client.get(&url);
        if let Some(ref cookie) = cookie_header {
            req = req.header("Cookie", cookie);
        }
        let Ok(resp) = req.send().await else {
            println!("Request error on page {}", page);
            log_failed_page(page);
            consecutive_fails += 1;
            if consecutive_fails > 20 { break; }
            sleep(Duration::from_secs(1)).await;
            continue;
        };

        if !resp.status().is_success() {
            println!("Failed to fetch page {}, status: {}", page, resp.status());
            if resp.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE {
                println!("Service Unavailable (503). Stopping crawler.");
                break;
            }
            log_failed_page(page);
            consecutive_fails += 1;
            if consecutive_fails > 20 { break; }
            sleep(Duration::from_secs(1)).await;
            continue;
        }
        
        let Ok(api_response) = resp.json::<OpenFoodFactsResponse>().await else {
            println!("JSON parse error on page {}", page);
            log_failed_page(page);
            consecutive_fails += 1;
            if consecutive_fails > 20 { break; }
            sleep(Duration::from_secs(1)).await;
            continue;
        };
        
        consecutive_fails = 0;
        
        if api_response.products.is_empty() {
            println!("No products found at page {}.", page);
            continue;
        }

        let mut consecutive_exists_count = 0;
        
        for off_prod in api_response.products {
            let code = match off_prod.code {
                Some(c) if !c.is_empty() => c,
                _ => continue,
            };

            if barcode_exists(&mut conn, &code) {
                consecutive_exists_count += 1;
                if consecutive_exists_count >= 2 {
                    println!("Found 2 consecutive existing barcodes. Stopping crawler.");
                    return Ok(());
                }
                continue;
            }
            
            // Reset count when we find a new barcode
            consecutive_exists_count = 0;
            
            let mut name = match off_prod.product_name {
                Some(n) if !n.is_empty() => {
                    let cleaned = clean_product_name(&n);
                    if cleaned.is_empty() {
                        continue;
                    }
                    if PRODUCT_BLOCK_LIST.contains(&cleaned.to_lowercase().as_str()) {
                        println!("Skipping blocked product: {}", cleaned);
                        continue;
                    }
                    cleaned
                },
                _ => continue,
            };
            
            let type_ = parse_category(&off_prod.categories_tags);
            let brands = build_desc(&off_prod.brands);

            // brands가 없으면 스킵
            let brands_str = match brands.as_deref() {
                Some(b) if !b.is_empty() => b.to_string(),
                _ => {
                    println!("Skipping product (no brand info): {}", name);
                    continue;
                }
            };

            // 와인일 경우 상품명 앞에 브랜드를 붙여준다. 
            if type_ == 0 {
                if let Some(ref b) = off_prod.brands {
                    if !b.is_empty() {
                        name = format!("{} {}", b, name);
                    }
                }
            }
            
            let mut final_name = name.clone();
            let mut final_desc = Some(brands_str.clone());
            let mut final_type = type_;
            let mut final_details = None;

            if let Ok(api_key) = std::env::var("GEMINI_API_KEY") {
                if let Ok(output) = call_gemini(&client, &api_key, &name, &brands_str).await {
                    if output.error.is_none() {
                        if let Some(gn) = output.name {
                            if !gn.is_empty() { final_name = gn; }
                        }
                        if let Some(gd) = output.description {
                            if !gd.is_empty() { final_desc = Some(gd); }
                        }
                        if let Some(gc) = output.category {
                            let tags = Some(vec![gc]);
                            final_type = parse_category(&tags);
                        }
                        final_details = output.details;
                    }
                } else {
                    println!("Failed to call Gemini for: {}", name);
                }
            }
            
            let product_id = {
                let embedding = match openai::get_embedding(&final_name).await {
                    Ok(vec) => Some(vec),
                    Err(e) => {
                        println!("Failed to get embedding for {}: {}", final_name, e);
                        None
                    }
                };

                let pid = Uuid::new_v4();
                let new_product = NewProduct {
                    id: pid,
                    name: &final_name,
                    desc: final_desc.as_deref(),
                    type_: final_type,
                    registered: Utc::now(),
                    embedding,
                    details: final_details,
                };
                
                if insert_product(&mut conn, &new_product).is_err() {
                    continue;
                }
                
                if let Some(img_url) = off_prod.image_url {
                    let image_id = Uuid::new_v4();
                    if download_image(&client, &r2, &img_url, image_id).await.is_ok() {
                        let new_image = NewProductImage {
                            id: image_id,
                            product_id: Some(pid),
                            registered: Utc::now(),
                        };
                        let _ = insert_product_image(&mut conn, &new_image);
                    }
                }
                
                pid
            };
            
            let new_barcode = NewBarcode {
                id: Uuid::new_v4(),
                barcode_id: &code,
                product_id,
            };
            let _ = insert_barcode(&mut conn, &new_barcode);

            println!("added product: {}", name);
        }
        
        sleep(Duration::from_secs(1)).await;
    }
    
    Ok(())
}
