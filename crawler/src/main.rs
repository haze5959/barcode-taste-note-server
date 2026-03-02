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

use models::OpenFoodFactsResponse;
use db::{establish_connection, barcode_exists, product_exists_by_name, insert_product, insert_barcode, insert_product_image, NewProduct, NewBarcode, NewProductImage};

fn clean_product_name(name: &str) -> String {
    let mut cleaned = name.replace("&quot;", "");
    
    static RE_PARENS: OnceLock<Regex> = OnceLock::new();
    let re_parens = RE_PARENS.get_or_init(|| Regex::new(r"\(.*?\)").unwrap());
    cleaned = re_parens.replace_all(&cleaned, " ").to_string();
    
    static RE_ABV: OnceLock<Regex> = OnceLock::new();
    let re_abv = RE_ABV.get_or_init(|| Regex::new(r"(?i)\d+(\.\d+)?\s*%\s*(vol\.?)?").unwrap());
    cleaned = re_abv.replace_all(&cleaned, " ").to_string();
    
    static RE_VOL: OnceLock<Regex> = OnceLock::new();
    let re_vol = RE_VOL.get_or_init(|| Regex::new(r"(?i)\b\d+(\.\d+)?\s*(ml|cl|l|liter|liters|litre|litres)\b").unwrap());
    cleaned = re_vol.replace_all(&cleaned, " ").to_string();
    
    static RE_SPACES: OnceLock<Regex> = OnceLock::new();
    let re_spaces = RE_SPACES.get_or_init(|| Regex::new(r"\s{2,}").unwrap());
    cleaned = re_spaces.replace_all(&cleaned, " ").to_string();
    
    // Remove trailing commas or hyphens
    cleaned.trim().trim_end_matches(&[',', '-', ' '][..]).trim().to_string()
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

async fn download_image(client: &Client, url: &str, image_id: Uuid) -> Result<(), Box<dyn std::error::Error>> {
    let resp = client.get(url).send().await?;
    if resp.status().is_success() {
        let bytes = resp.bytes().await?;
        let path = format!("../static/images/{}", image_id);
        std::fs::create_dir_all("../static/images")?;
        std::fs::write(path, bytes)?;
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    
    let mut conn = establish_connection();
    let client = Client::new();
    
    let failed_pages_str = std::fs::read_to_string("failed_pages.txt").unwrap_or_default();
    let pages_to_crawl: Vec<i64> = failed_pages_str
        .lines()
        .filter_map(|l| l.trim().parse::<i64>().ok())
        .collect();

    if pages_to_crawl.is_empty() {
        println!("No pages to crawl.");
        return Ok(());
    }

    // Clear failed_pages.txt to log only NEW failures
    let _ = std::fs::write("failed_pages.txt", "");

    let mut consecutive_fails = 0;
    
    for page in pages_to_crawl {
        println!("Crawling page: {}", page);
        let url = format!("https://world.openfoodfacts.org/category/alcoholic-beverages.json?page={}&page_size=50&fields=code,product_name,brands,categories_tags,image_url", page);
        
        let Ok(resp) = client.get(&url).send().await else {
            println!("Request error on page {}", page);
            log_failed_page(page);
            consecutive_fails += 1;
            if consecutive_fails > 20 { break; }
            sleep(Duration::from_secs(1)).await;
            continue;
        };

        if !resp.status().is_success() {
            println!("Failed to fetch page {}, status: {}", page, resp.status());
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
        
        for off_prod in api_response.products {
            let code = match off_prod.code {
                Some(c) if !c.is_empty() => c,
                _ => continue,
            };
            
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
            
            if barcode_exists(&mut conn, &code) {
                continue;
            }
            
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
                    if download_image(&client, &img_url, image_id).await.is_ok() {
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
        }
        
        sleep(Duration::from_secs(1)).await;
    }
    
    Ok(())
}
