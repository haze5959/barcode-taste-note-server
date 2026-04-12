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
use db::{establish_connection, barcode_exists, product_exists_by_name, insert_product, insert_barcode, insert_product_image, NewProduct, NewBarcode, NewProductImage};
use r2::R2Client;

fn clean_product_name(name: &str) -> String {
    let mut cleaned = name.replace("&quot;", "\"").replace("&amp;", "&");
    
    static RE_PARENS: OnceLock<Regex> = OnceLock::new();
    let re_parens = RE_PARENS.get_or_init(|| Regex::new(r"\(.*?\)").unwrap());
    cleaned = re_parens.replace_all(&cleaned, " ").to_string();
    
    static RE_YEARS: OnceLock<Regex> = OnceLock::new();
    let re_years = RE_YEARS.get_or_init(|| Regex::new(r"(?i)\b(?:aged\s+)?(\d+)\s*years?(?:\s*old)?\b").unwrap());
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
        if tags_str.contains("whisky") || tags_str.contains("whiskies") { return 0; }
        if tags_str.contains("wine") || tags_str.contains("wines") { return 1; }
        if tags_str.contains("beer") || tags_str.contains("beers") { return 2; }
        if tags_str.contains("soju") || tags_str.contains("sake") { return 3; }
        if tags_str.contains("liqueur") || tags_str.contains("liqueurs") || tags_str.contains("spirit") || tags_str.contains("spirits") { return 4; }
        if tags_str.contains("cocktail") || tags_str.contains("cocktails") { return 5; }
        if tags_str.contains("coffee") || tags_str.contains("coffees") { return 6; }
        if tags_str.contains("beverage") || tags_str.contains("beverages") { return 7; }
    }
    8
}

fn build_desc(brands: &Option<String>) -> Option<String> {
    let mut desc_parts = Vec::new();
    if let Some(b) = brands {
        if !b.is_empty() {
            desc_parts.push(format!("Brand: {}", b));
        }
    }
    if desc_parts.is_empty() {
        None
    } else {
        Some(desc_parts.join(", "))
    }
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
            
            let name = match off_prod.product_name {
                Some(n) if !n.is_empty() => {
                    let cleaned = clean_product_name(&n);
                    if cleaned.is_empty() {
                        continue;
                    }
                    cleaned
                },
                _ => continue,
            };
            
            let type_ = parse_category(&off_prod.categories_tags);
            let desc = build_desc(&off_prod.brands);
            
            let product_id = if let Some(existing_pid) = product_exists_by_name(&mut conn, &name) {
                existing_pid
            } else {
                let embedding = match openai::get_embedding(&name).await {
                    Ok(vec) => Some(vec),
                    Err(e) => {
                        println!("Failed to get embedding for {}: {}", name, e);
                        None
                    }
                };

                let pid = Uuid::new_v4();
                let new_product = NewProduct {
                    id: pid,
                    name: &name,
                    desc: desc.as_deref(),
                    type_,
                    registered: Utc::now(),
                    embedding,
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
